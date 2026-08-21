#![no_std]

use core::cell::UnsafeCell;

const WIDTH: usize = 160;
const HEIGHT: usize = 120;
const PALETTE_COUNT: usize = 8;
const PIXEL_OFFSET: usize = 16 + PALETTE_COUNT * 4;
const STATE_BYTES: usize = 64;
const PIXEL_END: usize = PIXEL_OFFSET + WIDTH * HEIGHT;
const METADATA_OFFSET: usize = PIXEL_END + 12;
const FRAME_BYTES: usize = METADATA_OFFSET + STATE_BYTES;
const STATE_METADATA_SCHEMA: u32 = 0x3147_4c53;
const SECTORS: u8 = 8;

const OUTER_LEFT: u32 = 1 << 0;
const OUTER_RIGHT: u32 = 1 << 1;
const MIDDLE_LEFT: u32 = 1 << 2;
const MIDDLE_RIGHT: u32 = 1 << 3;
const CORE_LEFT: u32 = 1 << 4;
const CORE_RIGHT: u32 = 1 << 5;
const RESTART: u32 = 1 << 7;

#[link(wasm_import_module = "tinyarcade:core/v1")]
unsafe extern "C" {
    fn input_bits() -> i32;
    fn clock_ms() -> i32;
    fn random_u32() -> i32;
    fn indexed2d_version() -> i32;
    fn indexed2d_metadata_version() -> i32;
    fn tones_version() -> i32;
    fn submit_render(pointer: *const u8, length: u32) -> i32;
    fn submit_audio(pointer: *const u8, length: u32) -> i32;
    fn save_state(pointer: *const u8, length: u32) -> i32;
    fn load_state(pointer: *mut u8, capacity: u32) -> i32;
}

#[used]
#[unsafe(link_section = "tinyarcade.manifest.v1")]
static MANIFEST: [u8; 49] =
    *b"TAM1\x01\0\0\0\x01\0\0\0\x1a\0com.partnernet.signal-lock\x05\x000.1.0\0\0";

struct Shared<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

struct Game {
    rings: [u8; 3],
    route: [u8; 3],
    ticks_remaining: u8,
    lives: u8,
    streak: u8,
    game_over: bool,
    par_turns: u8,
    turns_used: u16,
    round: u16,
    score: u32,
    previous_buttons: u32,
    last_sweep_ms: u32,
}

impl Game {
    const fn empty() -> Self {
        Self {
            rings: [0; 3],
            route: [0; 3],
            ticks_remaining: 48,
            lives: 3,
            streak: 0,
            game_over: false,
            par_turns: 1,
            turns_used: 0,
            round: 1,
            score: 0,
            previous_buttons: 0,
            last_sweep_ms: 0,
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
    if unsafe { indexed2d_version() } != 1
        || unsafe { indexed2d_metadata_version() } != 1
        || unsafe { tones_version() } != 1
    {
        return 1;
    }
    reset(game_mut());
    rebuild_frame(game_mut());
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn game_tick() -> i32 {
    let game = game_mut();
    let buttons = unsafe { input_bits() as u32 };
    let pressed = buttons & !game.previous_buttons;
    game.previous_buttons = buttons;
    let now = unsafe { clock_ms() as u32 };
    let mut sound = 0;

    if game.game_over {
        if pressed & RESTART != 0 {
            reset(game);
            sound = 2;
        }
    } else {
        for (mask, ring, direction) in [
            (OUTER_LEFT, 0, -1),
            (OUTER_RIGHT, 0, 1),
            (MIDDLE_LEFT, 1, -1),
            (MIDDLE_RIGHT, 1, 1),
            (CORE_LEFT, 2, -1),
            (CORE_RIGHT, 2, 1),
        ] {
            if pressed & mask != 0 && !game.game_over {
                sound = rotate(game, ring, direction).max(sound);
            }
        }
        if now.wrapping_sub(game.last_sweep_ms) >= 250 {
            game.last_sweep_ms = now;
            sound = sweep(game).max(sound);
        }
    }

    rebuild_frame(game);
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

fn random_sector() -> u8 {
    unsafe { random_u32() as u32 as u8 % SECTORS }
}

fn distinct_sector(used: &[u8]) -> u8 {
    let mut attempts = 0;
    while attempts < 16 {
        let candidate = random_sector();
        if !used.contains(&candidate) {
            return candidate;
        }
        attempts += 1;
    }
    let mut candidate = 0;
    while used.contains(&candidate) {
        candidate += 1;
    }
    candidate
}

fn reset(game: &mut Game) {
    game.route[0] = distinct_sector(&[]);
    game.route[1] = distinct_sector(&game.route[..1]);
    game.route[2] = distinct_sector(&game.route[..2]);
    game.rings = [random_sector(), random_sector(), random_sector()];
    if aligned_count(game) == 3 {
        game.rings[0] = (game.rings[0] + 1) % SECTORS;
    }
    game.ticks_remaining = ticks_allowed(1);
    game.lives = 3;
    game.streak = 0;
    game.game_over = false;
    game.turns_used = 0;
    game.round = 1;
    game.score = 0;
    game.previous_buttons = 0;
    game.last_sweep_ms = unsafe { clock_ms() as u32 };
    game.par_turns = minimum_turns(game);
}

fn rotate(game: &mut Game, ring: usize, direction: i8) -> u8 {
    game.rings[ring] = if direction < 0 {
        (game.rings[ring] + SECTORS - 1) % SECTORS
    } else {
        (game.rings[ring] + 1) % SECTORS
    };
    game.turns_used = game.turns_used.saturating_add(1);
    if aligned_count(game) == 3 {
        complete_round(game);
        2
    } else {
        1
    }
}

fn complete_round(game: &mut Game) {
    let extra = game.turns_used.saturating_sub(u16::from(game.par_turns));
    let perfect = extra == 0;
    game.streak = if perfect {
        game.streak.saturating_add(1)
    } else {
        0
    };
    let route_bonus = 80u32.saturating_sub(u32::from(extra) * 16);
    let perfect_bonus = if perfect { 40 } else { 0 };
    let gain = 120
        + u32::from(game.ticks_remaining) * 3
        + route_bonus
        + perfect_bonus
        + u32::from(game.streak.min(10)) * 15;
    game.score = game.score.saturating_add(gain);
    game.round = game.round.saturating_add(1).max(1);
    advance_route(game);
}

fn sweep(game: &mut Game) -> u8 {
    game.ticks_remaining = game.ticks_remaining.saturating_sub(1);
    if game.ticks_remaining != 0 {
        return 0;
    }
    game.lives = game.lives.saturating_sub(1);
    game.streak = 0;
    if game.lives == 0 {
        game.game_over = true;
        3
    } else {
        advance_route(game);
        3
    }
}

fn advance_route(game: &mut Game) {
    let next = distinct_sector(&game.route[1..]);
    game.route = [game.route[1], game.route[2], next];
    if aligned_count(game) == 3 {
        game.route[2] = distinct_sector(&game.route[..2]);
    }
    game.ticks_remaining = ticks_allowed(game.round);
    game.turns_used = 0;
    game.par_turns = minimum_turns(game);
}

fn aligned_count(game: &Game) -> u8 {
    let mut count = 0;
    let mut ring = 0;
    while ring < 3 {
        if game.rings[ring] == game.route[ring] {
            count += 1;
        }
        ring += 1;
    }
    count
}

fn minimum_turns(game: &Game) -> u8 {
    let mut total = 0;
    let mut ring = 0;
    while ring < 3 {
        let difference = game.rings[ring].abs_diff(game.route[ring]);
        total += difference.min(SECTORS - difference);
        ring += 1;
    }
    total
}

fn ticks_allowed(round: u16) -> u8 {
    let reduction = round.saturating_sub(1).saturating_mul(2).min(26) as u8;
    48 - reduction
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
    put_u16(frame, 14, 1);
    let palette = [
        [7, 13, 25, 255],
        [31, 53, 74, 255],
        [78, 219, 255, 255],
        [255, 184, 68, 255],
        [86, 230, 151, 255],
        [244, 248, 255, 255],
        [247, 87, 110, 255],
        [143, 105, 255, 255],
    ];
    for (index, color) in palette.iter().enumerate() {
        let at = 16 + index * 4;
        frame[at..at + 4].copy_from_slice(color);
    }

    fill_rect(0, 0, WIDTH as i32, 9, 1);
    draw_number(4, 2, game.score, 6, 5);
    draw_number(76, 2, u32::from(game.round), 3, 3);
    let mut life = 0;
    while life < game.lives {
        fill_rect(132 + i32::from(life) * 8, 3, 5, 3, 4);
        life += 1;
    }

    let radii = [39, 27, 15];
    let mut ring = 0;
    while ring < 3 {
        let mut sector = 0;
        while sector < SECTORS {
            let (x, y) = sector_point(radii[ring], sector);
            fill_rect(x - 1, y - 1, 3, 3, 1);
            sector += 1;
        }
        let (target_x, target_y) = sector_point(radii[ring], game.route[ring]);
        fill_rect(target_x - 3, target_y - 3, 7, 7, 3);
        fill_rect(target_x - 1, target_y - 1, 3, 3, 0);
        let (ring_x, ring_y) = sector_point(radii[ring], game.rings[ring]);
        let color = if game.rings[ring] == game.route[ring] {
            4
        } else {
            2
        };
        fill_rect(ring_x - 2, ring_y - 2, 5, 5, color);
        ring += 1;
    }
    let outer = sector_point(radii[0], game.route[0]);
    let middle = sector_point(radii[1], game.route[1]);
    let core = sector_point(radii[2], game.route[2]);
    draw_line(outer.0, outer.1, middle.0, middle.1, 7);
    draw_line(middle.0, middle.1, core.0, core.1, 7);
    fill_rect(77, 53, 7, 7, if aligned_count(game) == 3 { 4 } else { 5 });

    fill_rect(12, 103, 136, 4, 1);
    let time_width =
        136 * i32::from(game.ticks_remaining) / i32::from(ticks_allowed(game.round).max(1));
    fill_rect(
        12,
        103,
        time_width,
        4,
        if game.ticks_remaining < 10 { 6 } else { 3 },
    );
    draw_number(12, 111, u32::from(game.turns_used), 3, 2);
    draw_number(42, 111, u32::from(game.par_turns), 2, 4);
    draw_number(125, 111, u32::from(game.streak), 2, 7);
    if game.game_over {
        fill_rect(51, 49, 58, 17, 0);
        fill_rect(53, 51, 54, 13, 6);
        fill_rect(56, 54, 48, 7, 0);
    }
    frame[PIXEL_END..PIXEL_END + 4].copy_from_slice(b"TAM1");
    put_u32(frame, PIXEL_END + 4, STATE_METADATA_SCHEMA);
    put_u16(frame, PIXEL_END + 8, STATE_BYTES as u16);
    put_u16(frame, PIXEL_END + 10, 0);
    let state = snapshot_mut();
    encode_state(game, state);
    frame[METADATA_OFFSET..].copy_from_slice(state);
}

fn sector_point(radius: i32, sector: u8) -> (i32, i32) {
    const DIRECTIONS: [(i32, i32); 8] = [
        (0, -100),
        (71, -71),
        (100, 0),
        (71, 71),
        (0, 100),
        (-71, 71),
        (-100, 0),
        (-71, -71),
    ];
    let (x, y) = DIRECTIONS[sector as usize];
    (80 + radius * x / 100, 56 + radius * y / 100)
}

fn draw_line(mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u8) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        set_pixel(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = error * 2;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
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
        1 => (360, 35, 400),
        2 => (880, 180, 650),
        _ => (120, 300, 600),
    };
    put_u16(audio, 10, frequency);
    put_u16(audio, 12, duration);
    put_u16(audio, 14, amplitude);
    unsafe { submit_audio(audio.as_ptr(), audio.len() as u32) }
}

fn encode_state(game: &Game, out: &mut [u8; STATE_BYTES]) {
    out.fill(0);
    out[..4].copy_from_slice(b"SLG1");
    out[4] = 1;
    out[5] = game.lives;
    out[6] = game.streak;
    out[7] = game.game_over as u8;
    out[8..11].copy_from_slice(&game.rings);
    out[11..14].copy_from_slice(&game.route);
    out[14] = game.ticks_remaining;
    out[15] = game.par_turns;
    put_u16(out, 16, game.turns_used);
    put_u16(out, 18, game.round);
    put_u32(out, 20, game.score);
    put_u32(out, 24, game.previous_buttons);
    put_u32(out, 28, game.last_sweep_ms);
}

fn decode_state(input: &[u8; STATE_BYTES], game: &mut Game) -> bool {
    let rings = [input[8], input[9], input[10]];
    let route = [input[11], input[12], input[13]];
    let round = read_u16(input, 18);
    let lives = input[5];
    let game_over = input[7] != 0;
    if &input[..4] != b"SLG1"
        || input[4] != 1
        || lives > 3
        || input[7] > 1
        || rings.iter().any(|sector| *sector >= SECTORS)
        || route.iter().any(|sector| *sector >= SECTORS)
        || route[0] == route[1]
        || route[0] == route[2]
        || route[1] == route[2]
        || input[14] > ticks_allowed(round)
        || input[15] == 0
        || input[15] > 12
        || round == 0
        || game_over != (lives == 0)
        || (game_over && input[14] != 0)
        || input[32..].iter().any(|byte| *byte != 0)
    {
        return false;
    }
    game.rings = rings;
    game.route = route;
    game.ticks_remaining = input[14];
    game.lives = lives;
    game.streak = input[6];
    game.game_over = game_over;
    game.par_turns = input[15];
    game.turns_used = read_u16(input, 16);
    game.round = round;
    game.score = read_u32(input, 20);
    game.previous_buttons = 0;
    game.last_sweep_ms = read_u32(input, 28);
    true
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
