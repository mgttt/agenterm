#![no_std]

use core::cell::UnsafeCell;

const WIDTH: i32 = 5;
const DEPTH: i32 = 5;
const HEIGHT: i32 = 10;
const BOARD_CELLS: usize = (WIDTH * DEPTH * HEIGHT) as usize;
const PIECE_CELLS: usize = 4;
const FRAME_CAPACITY: usize = 32 + (BOARD_CELLS + PIECE_CELLS * 2) * 8;
const STATE_BYTES: usize = 288;

const LEFT: u32 = 1 << 0;
const RIGHT: u32 = 1 << 1;
const UP: u32 = 1 << 2;
const DOWN: u32 = 1 << 3;
const ROTATE_X: u32 = 1 << 4;
const ROTATE_Y: u32 = 1 << 5;
const ROTATE_Z: u32 = 1 << 6;
const HARD_DROP: u32 = 1 << 7;

#[link(wasm_import_module = "tinyarcade:core/v1")]
unsafe extern "C" {
    fn input_bits() -> i32;
    fn clock_ms() -> i32;
    fn random_u32() -> i32;
    fn submit_render(pointer: *const u8, length: u32) -> i32;
    fn submit_audio(pointer: *const u8, length: u32) -> i32;
    fn save_state(pointer: *const u8, length: u32) -> i32;
    fn load_state(pointer: *mut u8, capacity: u32) -> i32;
}

// Ordinary WebAssembly custom section consumed by the cartridge loader.
#[used]
#[unsafe(link_section = "tinyarcade.manifest.v1")]
static MANIFEST: [u8; 48] =
    *b"TAM1\x01\0\0\0\x01\0\0\0\x19\0com.partnernet.depth-well\x05\x000.1.0\0\0";

struct Shared<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

#[derive(Clone, Copy)]
struct Cell {
    x: i8,
    y: i8,
    z: i8,
}

const ZERO_CELL: Cell = Cell { x: 0, y: 0, z: 0 };

struct Game {
    board: [u8; BOARD_CELLS],
    piece: [Cell; PIECE_CELLS],
    bag: [u8; 5],
    bag_at: u8,
    x: i8,
    y: i8,
    z: i8,
    previous_buttons: u32,
    last_drop_ms: u32,
    score: u32,
    cleared_decks: u32,
    level: u32,
    game_over: bool,
}

impl Game {
    const fn empty() -> Self {
        Self {
            board: [0; BOARD_CELLS],
            piece: [ZERO_CELL; PIECE_CELLS],
            bag: [0, 1, 2, 3, 4],
            bag_at: 5,
            x: 2,
            y: 2,
            z: 9,
            previous_buttons: 0,
            last_drop_ms: 0,
            score: 0,
            cleared_decks: 0,
            level: 1,
            game_over: false,
        }
    }
}

static GAME: Shared<Game> = Shared(UnsafeCell::new(Game::empty()));
static FRAME: Shared<[u8; FRAME_CAPACITY]> = Shared(UnsafeCell::new([0; FRAME_CAPACITY]));
static AUDIO: Shared<[u8; 16]> = Shared(UnsafeCell::new([0; 16]));
static SNAPSHOT: Shared<[u8; STATE_BYTES]> = Shared(UnsafeCell::new([0; STATE_BYTES]));

#[unsafe(no_mangle)]
pub extern "C" fn game_abi_version() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn game_init() -> i32 {
    let game = game_mut();
    *game = Game::empty();
    game.last_drop_ms = host_clock();
    refill_bag(game);
    spawn(game);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn game_tick() -> i32 {
    let game = game_mut();
    let buttons = host_input();
    let pressed = buttons & !game.previous_buttons;
    game.previous_buttons = buttons;
    let now = host_clock();
    let mut sound = 0;

    if !game.game_over {
        if pressed & LEFT != 0 {
            try_move(game, -1, 0, 0);
        }
        if pressed & RIGHT != 0 {
            try_move(game, 1, 0, 0);
        }
        if pressed & UP != 0 {
            try_move(game, 0, 1, 0);
        }
        if pressed & DOWN != 0 {
            try_move(game, 0, -1, 0);
        }
        if pressed & ROTATE_X != 0 {
            try_rotate(game, 0);
        }
        if pressed & ROTATE_Y != 0 {
            try_rotate(game, 1);
        }
        if pressed & ROTATE_Z != 0 {
            try_rotate(game, 2);
        }
        if pressed & HARD_DROP != 0 {
            while fits(game, game.x, game.y, game.z - 1, &game.piece) {
                game.z -= 1;
                game.score = game.score.wrapping_add(2);
            }
            sound = lock_piece(game);
            game.last_drop_ms = now;
        } else if now.wrapping_sub(game.last_drop_ms) >= drop_interval(game.level) {
            game.last_drop_ms = now;
            if fits(game, game.x, game.y, game.z - 1, &game.piece) {
                game.z -= 1;
            } else {
                sound = lock_piece(game);
            }
        }
    }

    if emit_frame(game) != 0 {
        return 1;
    }
    if sound != 0 && emit_sound(sound) != 0 {
        return 2;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn game_suspend() -> i32 {
    let snapshot = snapshot_mut();
    encode_state(game_mut(), snapshot);
    host_save(snapshot)
}

#[unsafe(no_mangle)]
pub extern "C" fn game_resume() -> i32 {
    let snapshot = snapshot_mut();
    if host_load(snapshot) != STATE_BYTES as i32 {
        return 1;
    }
    if decode_state(snapshot, game_mut()) {
        0
    } else {
        2
    }
}

fn game_mut() -> &'static mut Game {
    // The ABI and host enforce one single-thread-owned instance and no re-entry.
    unsafe { &mut *GAME.0.get() }
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

fn host_save(bytes: &[u8]) -> i32 {
    unsafe { save_state(bytes.as_ptr(), bytes.len() as u32) }
}

fn host_load(bytes: &mut [u8]) -> i32 {
    unsafe { load_state(bytes.as_mut_ptr(), bytes.len() as u32) }
}

fn refill_bag(game: &mut Game) {
    game.bag = [0, 1, 2, 3, 4];
    let mut at = 4;
    while at > 0 {
        let other = host_random() as usize % (at + 1);
        game.bag.swap(at, other);
        at -= 1;
    }
    game.bag_at = 0;
}

fn spawn(game: &mut Game) {
    if game.bag_at >= 5 {
        refill_bag(game);
    }
    let kind = game.bag[game.bag_at as usize];
    game.bag_at += 1;
    game.piece = match kind {
        0 => cells([(-1, 0, 0), (0, 0, 0), (1, 0, 0), (2, 0, 0)]),
        1 => cells([(-1, 0, 0), (0, 0, 0), (1, 0, 0), (1, 1, 0)]),
        2 => cells([(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]),
        3 => cells([(-1, 0, 0), (0, 0, 0), (1, 0, 0), (0, 1, 0)]),
        _ => cells([(-1, 0, 0), (0, 0, 0), (0, 1, 0), (1, 1, 0)]),
    };
    game.x = 2;
    game.y = 2;
    game.z = 9;
    if !fits(game, game.x, game.y, game.z, &game.piece) {
        game.game_over = true;
    }
}

const fn cells(raw: [(i8, i8, i8); 4]) -> [Cell; 4] {
    [
        Cell {
            x: raw[0].0,
            y: raw[0].1,
            z: raw[0].2,
        },
        Cell {
            x: raw[1].0,
            y: raw[1].1,
            z: raw[1].2,
        },
        Cell {
            x: raw[2].0,
            y: raw[2].1,
            z: raw[2].2,
        },
        Cell {
            x: raw[3].0,
            y: raw[3].1,
            z: raw[3].2,
        },
    ]
}

fn index(x: i32, y: i32, z: i32) -> usize {
    ((z * DEPTH + y) * WIDTH + x) as usize
}

fn fits(game: &Game, x: i8, y: i8, z: i8, piece: &[Cell; 4]) -> bool {
    for cell in piece {
        let px = i32::from(x) + i32::from(cell.x);
        let py = i32::from(y) + i32::from(cell.y);
        let pz = i32::from(z) + i32::from(cell.z);
        if !(0..WIDTH).contains(&px)
            || !(0..DEPTH).contains(&py)
            || !(0..HEIGHT).contains(&pz)
            || game.board[index(px, py, pz)] != 0
        {
            return false;
        }
    }
    true
}

fn try_move(game: &mut Game, dx: i8, dy: i8, dz: i8) {
    let x = game.x + dx;
    let y = game.y + dy;
    let z = game.z + dz;
    if fits(game, x, y, z, &game.piece) {
        game.x = x;
        game.y = y;
        game.z = z;
    }
}

fn try_rotate(game: &mut Game, axis: u8) {
    let mut rotated = game.piece;
    for cell in &mut rotated {
        let (x, y, z) = (cell.x, cell.y, cell.z);
        match axis {
            0 => {
                cell.y = -z;
                cell.z = y;
            }
            1 => {
                cell.x = z;
                cell.z = -x;
            }
            _ => {
                cell.x = -y;
                cell.y = x;
            }
        }
    }
    if fits(game, game.x, game.y, game.z, &rotated) {
        game.piece = rotated;
    } else {
        // One-cell wall kick keeps rotations predictable near the well edge.
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if fits(game, game.x + dx, game.y + dy, game.z, &rotated) {
                game.x += dx;
                game.y += dy;
                game.piece = rotated;
                break;
            }
        }
    }
}

fn lock_piece(game: &mut Game) -> u8 {
    for cell in game.piece {
        let x = i32::from(game.x) + i32::from(cell.x);
        let y = i32::from(game.y) + i32::from(cell.y);
        let z = i32::from(game.z) + i32::from(cell.z);
        game.board[index(x, y, z)] = 1;
    }
    game.score = game.score.wrapping_add(10);
    let cleared = clear_full_decks(game);
    let sound = if cleared == 0 { 1 } else { 2 };
    spawn(game);
    if game.game_over { 3 } else { sound }
}

fn clear_full_decks(game: &mut Game) -> u32 {
    let mut z = 0;
    let mut cleared = 0;
    while z < HEIGHT {
        let mut full = true;
        let mut y = 0;
        while y < DEPTH {
            let mut x = 0;
            while x < WIDTH {
                if game.board[index(x, y, z)] == 0 {
                    full = false;
                }
                x += 1;
            }
            y += 1;
        }
        if full {
            let mut above = z;
            while above + 1 < HEIGHT {
                let mut y2 = 0;
                while y2 < DEPTH {
                    let mut x2 = 0;
                    while x2 < WIDTH {
                        let to = index(x2, y2, above);
                        let from = index(x2, y2, above + 1);
                        game.board[to] = game.board[from];
                        x2 += 1;
                    }
                    y2 += 1;
                }
                above += 1;
            }
            let top = (HEIGHT - 1) as usize * (WIDTH * DEPTH) as usize;
            game.board[top..top + (WIDTH * DEPTH) as usize].fill(0);
            cleared += 1;
        } else {
            z += 1;
        }
    }
    if cleared != 0 {
        game.cleared_decks += cleared;
        game.level = 1 + game.cleared_decks / 5;
        game.score = game
            .score
            .wrapping_add(250 * cleared * cleared * game.level);
    }
    cleared
}

fn drop_interval(level: u32) -> u32 {
    1_440u32
        .saturating_sub(level.saturating_sub(1).saturating_mul(90))
        .max(360)
}

fn emit_frame(game: &Game) -> i32 {
    let frame = unsafe { &mut *FRAME.0.get() };
    frame[..4].copy_from_slice(b"TAG3");
    put_u16(frame, 4, 1);
    put_u16(frame, 6, 32);
    put_u16(frame, 8, WIDTH as u16);
    put_u16(frame, 10, DEPTH as u16);
    put_u16(frame, 12, HEIGHT as u16);
    put_u32(frame, 16, game.score);
    put_u32(frame, 20, game.cleared_decks);
    put_u32(frame, 24, game.level);
    put_u32(frame, 28, game.game_over as u32);
    let mut count = 0usize;
    for z in 0..HEIGHT {
        for y in 0..DEPTH {
            for x in 0..WIDTH {
                if game.board[index(x, y, z)] != 0 {
                    write_cell(frame, count, x as u8, y as u8, z as u8, 1, 0x4dd5_ffff);
                    count += 1;
                }
            }
        }
    }
    if !game.game_over {
        let mut ghost_z = game.z;
        while fits(game, game.x, game.y, ghost_z - 1, &game.piece) {
            ghost_z -= 1;
        }
        for cell in game.piece {
            write_piece_cell(frame, count, game.x, game.y, ghost_z, cell, 3, 0x8f78_7850);
            count += 1;
        }
        for cell in game.piece {
            write_piece_cell(frame, count, game.x, game.y, game.z, cell, 2, 0xffb3_3dff);
            count += 1;
        }
    }
    put_u16(frame, 14, count as u16);
    unsafe { submit_render(frame.as_ptr(), (32 + count * 8) as u32) }
}

fn write_piece_cell(
    frame: &mut [u8],
    at: usize,
    x: i8,
    y: i8,
    z: i8,
    cell: Cell,
    kind: u8,
    rgba: u32,
) {
    write_cell(
        frame,
        at,
        (x + cell.x) as u8,
        (y + cell.y) as u8,
        (z + cell.z) as u8,
        kind,
        rgba,
    );
}

fn write_cell(frame: &mut [u8], at: usize, x: u8, y: u8, z: u8, kind: u8, rgba: u32) {
    let offset = 32 + at * 8;
    frame[offset..offset + 4].copy_from_slice(&[x, y, z, kind]);
    put_u32(frame, offset + 4, rgba);
}

fn emit_sound(kind: u8) -> i32 {
    let audio = unsafe { &mut *AUDIO.0.get() };
    audio[..4].copy_from_slice(b"TAT1");
    put_u16(audio, 4, 1);
    put_u16(audio, 6, 1);
    audio[8] = kind;
    audio[9] = 0;
    let (frequency, duration) = match kind {
        1 => (180, 55),
        2 => (880, 180),
        _ => (90, 500),
    };
    put_u16(audio, 10, frequency);
    put_u16(audio, 12, duration);
    put_u16(audio, 14, 650);
    unsafe { submit_audio(audio.as_ptr(), audio.len() as u32) }
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn encode_state(game: &Game, out: &mut [u8; STATE_BYTES]) {
    out.fill(0);
    out[..4].copy_from_slice(b"DWS1");
    out[4..254].copy_from_slice(&game.board);
    let mut at = 254;
    for cell in game.piece {
        out[at] = cell.x as u8;
        out[at + 1] = cell.y as u8;
        out[at + 2] = cell.z as u8;
        at += 3;
    }
    out[266..271].copy_from_slice(&game.bag);
    out[271] = game.bag_at;
    out[272] = game.x as u8;
    out[273] = game.y as u8;
    out[274] = game.z as u8;
    out[275] = game.game_over as u8;
    out[276..280].copy_from_slice(&game.last_drop_ms.to_le_bytes());
    out[280..284].copy_from_slice(&game.score.to_le_bytes());
    out[284..288].copy_from_slice(&game.cleared_decks.to_le_bytes());
}

fn decode_state(input: &[u8; STATE_BYTES], game: &mut Game) -> bool {
    if &input[..4] != b"DWS1" || input[271] > 5 || input[275] > 1 {
        return false;
    }
    game.board.copy_from_slice(&input[4..254]);
    let mut at = 254;
    for cell in &mut game.piece {
        cell.x = input[at] as i8;
        cell.y = input[at + 1] as i8;
        cell.z = input[at + 2] as i8;
        at += 3;
    }
    game.bag.copy_from_slice(&input[266..271]);
    game.bag_at = input[271];
    game.x = input[272] as i8;
    game.y = input[273] as i8;
    game.z = input[274] as i8;
    game.game_over = input[275] != 0;
    game.last_drop_ms = u32::from_le_bytes(input[276..280].try_into().unwrap_or([0; 4]));
    game.score = u32::from_le_bytes(input[280..284].try_into().unwrap_or([0; 4]));
    game.cleared_decks = u32::from_le_bytes(input[284..288].try_into().unwrap_or([0; 4]));
    game.level = 1 + game.cleared_decks / 5;
    game.previous_buttons = 0;
    let bag_is_permutation =
        (0..5).all(|piece| game.bag.iter().filter(|value| **value == piece).count() == 1);
    game.board.iter().all(|cell| *cell <= 1)
        && bag_is_permutation
        && game.piece.iter().all(|cell| {
            (-2..=2).contains(&cell.x) && (-2..=2).contains(&cell.y) && (-2..=2).contains(&cell.z)
        })
        && (0..WIDTH).contains(&i32::from(game.x))
        && (0..DEPTH).contains(&i32::from(game.y))
        && (0..HEIGHT).contains(&i32::from(game.z))
        && (game.game_over || fits(game, game.x, game.y, game.z, &game.piece))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
