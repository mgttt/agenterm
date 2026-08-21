#![no_std]

use core::cell::UnsafeCell;

const WIDTH: usize = 160;
const HEIGHT: usize = 120;
const PALETTE_COUNT: usize = 8;
const PIXEL_OFFSET: usize = 16 + PALETTE_COUNT * 4;
const FRAME_BYTES: usize = PIXEL_OFFSET + WIDTH * HEIGHT;
const STATE_BYTES: usize = 64;
const SCALE: i32 = 256;

const LEFT: u32 = 1 << 0;
const RIGHT: u32 = 1 << 1;
const PRIMARY: u32 = 1 << 4;

const PADDLE_Y: i32 = 108;
const PADDLE_WIDTH: i32 = 24;
const PADDLE_HEIGHT: i32 = 3;
const BALL_SIZE: i32 = 3;
const INNER_LEFT: i32 = 4;
const INNER_RIGHT: i32 = 156;
const INNER_TOP: i32 = 10;
const BRICK_COLUMNS: usize = 8;
const BRICK_ROWS: usize = 5;
const BRICK_COUNT: usize = BRICK_COLUMNS * BRICK_ROWS;
const ALL_BRICKS: u64 = (1u64 << BRICK_COUNT) - 1;

const READY: u8 = 0;
const PLAYING: u8 = 1;
const GAME_OVER: u8 = 2;

#[link(wasm_import_module = "tinyarcade:core/v1")]
unsafe extern "C" {
    fn input_bits() -> i32;
    fn clock_ms() -> i32;
    fn random_u32() -> i32;
    fn indexed2d_version() -> i32;
    fn tones_version() -> i32;
    fn submit_render(pointer: *const u8, length: u32) -> i32;
    fn submit_audio(pointer: *const u8, length: u32) -> i32;
    fn save_state(pointer: *const u8, length: u32) -> i32;
    fn load_state(pointer: *mut u8, capacity: u32) -> i32;
}

#[used]
#[unsafe(link_section = "tinyarcade.manifest.v1")]
static MANIFEST: [u8; 50] =
    *b"TAM1\x01\0\0\0\x01\0\0\0\x1b\0com.partnernet.paddle-guard\x05\x000.1.0\0\0";

struct Shared<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

struct Game {
    paddle_x: i32,
    ball_x: i32,
    ball_y: i32,
    velocity_x: i32,
    velocity_y: i32,
    previous_buttons: u32,
    last_tick_ms: u32,
    score: u32,
    bricks: u64,
    lives: u8,
    level: u8,
    phase: u8,
    launch_right: bool,
}

impl Game {
    const fn empty() -> Self {
        Self {
            paddle_x: 68 * SCALE,
            ball_x: 78 * SCALE,
            ball_y: 104 * SCALE,
            velocity_x: 0,
            velocity_y: 0,
            previous_buttons: 0,
            last_tick_ms: 0,
            score: 0,
            bricks: ALL_BRICKS,
            lives: 3,
            level: 1,
            phase: READY,
            launch_right: true,
        }
    }
}

static GAME: Shared<Game> = Shared(UnsafeCell::new(Game::empty()));
static FRAME: Shared<[u8; FRAME_BYTES]> = Shared(UnsafeCell::new([0; FRAME_BYTES]));
static AUDIO: Shared<[u8; 16]> = Shared(UnsafeCell::new([0; 16]));
static SNAPSHOT: Shared<[u8; STATE_BYTES]> = Shared(UnsafeCell::new([0; STATE_BYTES]));

#[unsafe(no_mangle)]
pub extern "C" fn game_abi_version() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn game_init() -> i32 {
    if unsafe { indexed2d_version() } != 1 || unsafe { tones_version() } != 1 {
        return 1;
    }
    let game = game_mut();
    *game = Game::empty();
    game.last_tick_ms = host_clock();
    game.launch_right = host_random() & 1 != 0;
    dock_ball(game);
    rebuild_frame(game);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn game_tick() -> i32 {
    let game = game_mut();
    erase_dynamic(game);
    let buttons = host_input();
    let pressed = buttons & !game.previous_buttons;
    game.previous_buttons = buttons;
    let now = host_clock();
    let elapsed = now.wrapping_sub(game.last_tick_ms).min(50);
    game.last_tick_ms = now;

    move_paddle(game, buttons, elapsed);
    let mut sound = 0;
    match game.phase {
        READY => {
            dock_ball(game);
            if pressed & PRIMARY != 0 {
                launch(game);
                sound = 1;
            }
        }
        PLAYING => {
            sound = advance_ball(game, elapsed);
        }
        _ => {
            if pressed & PRIMARY != 0 {
                reset_game(game);
                sound = 2;
            }
        }
    }
    draw_dynamic(game);

    if unsafe { submit_render(FRAME.0.get().cast::<u8>(), FRAME_BYTES as u32) } != 0 {
        return 2;
    }
    if sound != 0 && emit_sound(sound) != 0 {
        return 3;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn game_suspend() -> i32 {
    let snapshot = snapshot_mut();
    encode_state(game_mut(), snapshot);
    unsafe { save_state(snapshot.as_ptr(), STATE_BYTES as u32) }
}

#[unsafe(no_mangle)]
pub extern "C" fn game_resume() -> i32 {
    let snapshot = snapshot_mut();
    if unsafe { load_state(snapshot.as_mut_ptr(), STATE_BYTES as u32) } != STATE_BYTES as i32 {
        return 1;
    }
    if !decode_state(snapshot, game_mut()) {
        return 2;
    }
    rebuild_frame(game_mut());
    draw_dynamic(game_mut());
    0
}

fn game_mut() -> &'static mut Game {
    // The ABI and host enforce one single-thread-owned instance and no re-entry.
    unsafe { &mut *GAME.0.get() }
}

fn frame_mut() -> &'static mut [u8; FRAME_BYTES] {
    unsafe { &mut *FRAME.0.get() }
}

fn snapshot_mut() -> &'static mut [u8; STATE_BYTES] {
    unsafe { &mut *SNAPSHOT.0.get() }
}

fn host_input() -> u32 {
    unsafe { input_bits() as u32 }
}

fn host_clock() -> u32 {
    unsafe { clock_ms() as u32 }
}

fn host_random() -> u32 {
    unsafe { random_u32() as u32 }
}

fn reset_game(game: &mut Game) {
    game.score = 0;
    game.lives = 3;
    game.level = 1;
    game.bricks = ALL_BRICKS;
    game.phase = READY;
    game.launch_right = host_random() & 1 != 0;
    game.paddle_x = 68 * SCALE;
    dock_ball(game);
    rebuild_frame(game);
}

fn reset_level(game: &mut Game) {
    game.level = game.level.saturating_add(1).min(99);
    game.score = game.score.wrapping_add(500 * u32::from(game.level));
    game.bricks = ALL_BRICKS;
    game.phase = READY;
    game.launch_right = !game.launch_right;
    dock_ball(game);
    rebuild_frame(game);
}

fn dock_ball(game: &mut Game) {
    game.ball_x = game.paddle_x + ((PADDLE_WIDTH - BALL_SIZE) * SCALE / 2);
    game.ball_y = (PADDLE_Y - BALL_SIZE - 1) * SCALE;
    game.velocity_x = 0;
    game.velocity_y = 0;
}

fn launch(game: &mut Game) {
    let horizontal = (44 + i32::from(game.level).min(10) * 2) * SCALE;
    let vertical = (62 + i32::from(game.level).min(10) * 3) * SCALE;
    game.velocity_x = if game.launch_right {
        horizontal
    } else {
        -horizontal
    };
    game.velocity_y = -vertical;
    game.phase = PLAYING;
    draw_status(game);
}

fn move_paddle(game: &mut Game, buttons: u32, elapsed: u32) {
    let direction = match (buttons & LEFT != 0, buttons & RIGHT != 0) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    };
    let movement = direction * 105 * SCALE * elapsed as i32 / 1_000;
    game.paddle_x =
        (game.paddle_x + movement).clamp(INNER_LEFT * SCALE, (INNER_RIGHT - PADDLE_WIDTH) * SCALE);
}

fn advance_ball(game: &mut Game, elapsed: u32) -> u8 {
    if elapsed == 0 {
        return 0;
    }
    let steps = elapsed.div_ceil(8).max(1);
    let base = elapsed / steps;
    let extra = elapsed % steps;
    let mut sound = 0;
    let mut step = 0;
    while step < steps && game.phase == PLAYING {
        let milliseconds = base + u32::from(step < extra);
        let event = advance_substep(game, milliseconds);
        if event != 0 {
            sound = event;
        }
        step += 1;
    }
    sound
}

fn advance_substep(game: &mut Game, milliseconds: u32) -> u8 {
    let old_x = game.ball_x;
    let old_y = game.ball_y;
    let mut next_x = old_x + game.velocity_x * milliseconds as i32 / 1_000;
    let mut next_y = old_y + game.velocity_y * milliseconds as i32 / 1_000;
    let mut sound = 0;

    if next_x < INNER_LEFT * SCALE {
        next_x = INNER_LEFT * SCALE;
        game.velocity_x = game.velocity_x.abs();
        sound = 1;
    } else if next_x + BALL_SIZE * SCALE > INNER_RIGHT * SCALE {
        next_x = (INNER_RIGHT - BALL_SIZE) * SCALE;
        game.velocity_x = -game.velocity_x.abs();
        sound = 1;
    }
    if next_y < INNER_TOP * SCALE {
        next_y = INNER_TOP * SCALE;
        game.velocity_y = game.velocity_y.abs();
        sound = 1;
    }

    let paddle_top = PADDLE_Y * SCALE;
    if game.velocity_y > 0
        && old_y + BALL_SIZE * SCALE <= paddle_top
        && next_y + BALL_SIZE * SCALE >= paddle_top
        && overlaps(
            next_x / SCALE,
            BALL_SIZE,
            game.paddle_x / SCALE,
            PADDLE_WIDTH,
        )
    {
        next_y = (PADDLE_Y - BALL_SIZE) * SCALE;
        game.velocity_y = -game.velocity_y.abs();
        let ball_center = next_x / SCALE + BALL_SIZE / 2;
        let paddle_center = game.paddle_x / SCALE + PADDLE_WIDTH / 2;
        let angled = (ball_center - paddle_center) * 7 * SCALE;
        game.velocity_x = angled.clamp(-92 * SCALE, 92 * SCALE);
        if game.velocity_x.abs() < 24 * SCALE {
            game.velocity_x = if game.launch_right {
                24 * SCALE
            } else {
                -24 * SCALE
            };
        }
        game.launch_right = game.velocity_x > 0;
        sound = 1;
    }

    if let Some(brick) = hit_brick(game, next_x / SCALE, next_y / SCALE) {
        game.bricks &= !(1u64 << brick);
        game.score = game.score.wrapping_add(10 * u32::from(game.level).max(1));
        clear_brick(brick);
        draw_hud(game);
        game.velocity_y = -game.velocity_y;
        next_y = old_y + game.velocity_y * milliseconds as i32 / 1_000;
        sound = 1;
        if game.bricks == 0 {
            reset_level(game);
            return 2;
        }
    }

    game.ball_x = next_x;
    game.ball_y = next_y;
    if game.ball_y >= HEIGHT as i32 * SCALE {
        game.lives = game.lives.saturating_sub(1);
        if game.lives == 0 {
            game.phase = GAME_OVER;
        } else {
            game.phase = READY;
            game.launch_right = !game.launch_right;
            dock_ball(game);
        }
        draw_hud(game);
        draw_status(game);
        return 3;
    }
    sound
}

fn hit_brick(game: &Game, ball_x: i32, ball_y: i32) -> Option<usize> {
    let mut brick = 0;
    while brick < BRICK_COUNT {
        if game.bricks & (1u64 << brick) != 0 {
            let (x, y) = brick_origin(brick);
            if overlaps(ball_x, BALL_SIZE, x, 16) && overlaps(ball_y, BALL_SIZE, y, 5) {
                return Some(brick);
            }
        }
        brick += 1;
    }
    None
}

fn overlaps(a: i32, a_size: i32, b: i32, b_size: i32) -> bool {
    a < b + b_size && b < a + a_size
}

fn brick_origin(brick: usize) -> (i32, i32) {
    let column = brick % BRICK_COLUMNS;
    let row = brick / BRICK_COLUMNS;
    (6 + column as i32 * 19, 18 + row as i32 * 8)
}

fn rebuild_frame(game: &Game) {
    let frame = frame_mut();
    frame.fill(0);
    frame[..4].copy_from_slice(b"TAI2");
    put_u16(frame, 4, 1);
    put_u16(frame, 6, 16);
    put_u16(frame, 8, WIDTH as u16);
    put_u16(frame, 10, HEIGHT as u16);
    put_u16(frame, 12, PALETTE_COUNT as u16);
    put_u16(frame, 14, 0);
    let palette = [
        [10, 14, 24, 255],
        [88, 224, 255, 255],
        [255, 184, 72, 255],
        [245, 248, 255, 255],
        [248, 92, 120, 255],
        [173, 105, 255, 255],
        [81, 225, 152, 255],
        [255, 220, 88, 255],
    ];
    for (index, color) in palette.iter().enumerate() {
        let at = 16 + index * 4;
        frame[at..at + 4].copy_from_slice(color);
    }
    fill_rect(2, 8, 2, HEIGHT as i32 - 8, 1);
    fill_rect(INNER_RIGHT, 8, 2, HEIGHT as i32 - 8, 1);
    fill_rect(2, 8, INNER_RIGHT, 2, 1);
    let mut brick = 0;
    while brick < BRICK_COUNT {
        if game.bricks & (1u64 << brick) != 0 {
            draw_brick(brick);
        }
        brick += 1;
    }
    draw_hud(game);
    draw_status(game);
}

fn draw_brick(brick: usize) {
    let (x, y) = brick_origin(brick);
    let color = 4 + (brick / BRICK_COLUMNS) as u8 % 4;
    fill_rect(x, y, 16, 5, color);
}

fn clear_brick(brick: usize) {
    let (x, y) = brick_origin(brick);
    fill_rect(x, y, 16, 5, 0);
}

fn erase_dynamic(game: &Game) {
    fill_rect(
        game.paddle_x / SCALE,
        PADDLE_Y,
        PADDLE_WIDTH,
        PADDLE_HEIGHT,
        0,
    );
    if game.phase != GAME_OVER {
        fill_rect(
            game.ball_x / SCALE,
            game.ball_y / SCALE,
            BALL_SIZE,
            BALL_SIZE,
            0,
        );
    }
}

fn draw_dynamic(game: &Game) {
    fill_rect(
        game.paddle_x / SCALE,
        PADDLE_Y,
        PADDLE_WIDTH,
        PADDLE_HEIGHT,
        2,
    );
    if game.phase != GAME_OVER {
        fill_rect(
            game.ball_x / SCALE,
            game.ball_y / SCALE,
            BALL_SIZE,
            BALL_SIZE,
            3,
        );
    }
}

fn draw_hud(game: &Game) {
    fill_rect(7, 1, 56, 6, 0);
    draw_number(7, 1, game.score, 6, 1);
    fill_rect(126, 2, 27, 4, 0);
    let mut life = 0;
    while life < game.lives {
        fill_rect(126 + i32::from(life) * 9, 2, 6, 3, 2);
        life += 1;
    }
    fill_rect(78, 1, 15, 6, 0);
    draw_number(78, 1, u32::from(game.level), 2, 7);
}

fn draw_status(game: &Game) {
    fill_rect(65, 91, 30, 8, 0);
    match game.phase {
        READY => {
            fill_rect(72, 93, 10, 3, 6);
            fill_rect(83, 92, 2, 5, 6);
            fill_rect(86, 91, 2, 7, 6);
        }
        GAME_OVER => {
            fill_rect(68, 92, 24, 2, 4);
            fill_rect(68, 96, 24, 2, 4);
        }
        _ => {}
    }
}

const DIGITS: [u16; 10] = [
    0x7b6f, 0x2492, 0x73e7, 0x73cf, 0x5bc9, 0x79cf, 0x79ef, 0x7249, 0x7bef, 0x7bcf,
];

fn draw_number(x: i32, y: i32, value: u32, digits: usize, color: u8) {
    let mut divisor = 1u32;
    let mut at = 1;
    while at < digits {
        divisor = divisor.saturating_mul(10);
        at += 1;
    }
    let mut position = 0;
    while position < digits {
        let digit = (value / divisor) % 10;
        draw_digit(x + position as i32 * 4, y, digit as usize, color);
        divisor = (divisor / 10).max(1);
        position += 1;
    }
}

fn draw_digit(x: i32, y: i32, digit: usize, color: u8) {
    let glyph = DIGITS[digit];
    let mut row = 0;
    while row < 5 {
        let mut column = 0;
        while column < 3 {
            let bit = 14 - (row * 3 + column);
            if glyph & (1 << bit) != 0 {
                set_pixel(x + column, y + row, color);
            }
            column += 1;
        }
        row += 1;
    }
}

fn fill_rect(x: i32, y: i32, width: i32, height: i32, color: u8) {
    let mut row = 0;
    while row < height {
        let mut column = 0;
        while column < width {
            set_pixel(x + column, y + row, color);
            column += 1;
        }
        row += 1;
    }
}

fn set_pixel(x: i32, y: i32, color: u8) {
    if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
        frame_mut()[PIXEL_OFFSET + y as usize * WIDTH + x as usize] = color;
    }
}

fn emit_sound(kind: u8) -> i32 {
    let audio = unsafe { &mut *AUDIO.0.get() };
    audio[..4].copy_from_slice(b"TAT1");
    put_u16(audio, 4, 1);
    put_u16(audio, 6, 1);
    audio[8] = kind;
    audio[9] = 0;
    let (frequency, duration, amplitude) = match kind {
        1 => (420, 45, 480),
        2 => (960, 220, 650),
        _ => (110, 360, 600),
    };
    put_u16(audio, 10, frequency);
    put_u16(audio, 12, duration);
    put_u16(audio, 14, amplitude);
    unsafe { submit_audio(audio.as_ptr(), audio.len() as u32) }
}

fn encode_state(game: &Game, out: &mut [u8; STATE_BYTES]) {
    out.fill(0);
    out[..4].copy_from_slice(b"PGS1");
    out[4] = 1;
    out[5] = game.phase;
    out[6] = game.lives;
    out[7] = game.level;
    put_i32(out, 8, game.paddle_x);
    put_i32(out, 12, game.ball_x);
    put_i32(out, 16, game.ball_y);
    put_i32(out, 20, game.velocity_x);
    put_i32(out, 24, game.velocity_y);
    put_u32(out, 28, game.previous_buttons);
    put_u32(out, 32, game.last_tick_ms);
    put_u32(out, 36, game.score);
    out[40..48].copy_from_slice(&game.bricks.to_le_bytes());
    out[48] = game.launch_right as u8;
}

fn decode_state(input: &[u8; STATE_BYTES], game: &mut Game) -> bool {
    if &input[..4] != b"PGS1"
        || input[4] != 1
        || input[5] > GAME_OVER
        || input[6] > 3
        || input[7] == 0
        || input[7] > 99
        || input[48] > 1
        || input[49..].iter().any(|byte| *byte != 0)
    {
        return false;
    }
    let bricks = read_u64(input, 40);
    let paddle_x = read_i32(input, 8);
    let ball_x = read_i32(input, 12);
    let ball_y = read_i32(input, 16);
    let velocity_x = read_i32(input, 20);
    let velocity_y = read_i32(input, 24);
    if bricks & !ALL_BRICKS != 0
        || !(INNER_LEFT * SCALE..=(INNER_RIGHT - PADDLE_WIDTH) * SCALE).contains(&paddle_x)
        || !(-BALL_SIZE * SCALE..=WIDTH as i32 * SCALE).contains(&ball_x)
        || !(-BALL_SIZE * SCALE..=HEIGHT as i32 * SCALE).contains(&ball_y)
        || velocity_x.abs() > 120 * SCALE
        || velocity_y.abs() > 120 * SCALE
        || (input[5] == GAME_OVER && input[6] != 0)
    {
        return false;
    }
    game.paddle_x = paddle_x;
    game.ball_x = ball_x;
    game.ball_y = ball_y;
    game.velocity_x = velocity_x;
    game.velocity_y = velocity_y;
    game.previous_buttons = 0;
    game.last_tick_ms = read_u32(input, 32);
    game.score = read_u32(input, 36);
    game.bricks = bricks;
    game.lives = input[6];
    game.level = input[7];
    game.phase = input[5];
    game.launch_right = input[48] != 0;
    true
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(bytes: &mut [u8], at: usize, value: i32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_i32(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
