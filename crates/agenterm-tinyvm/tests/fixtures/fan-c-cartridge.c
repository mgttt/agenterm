/* Freestanding third-party authoring fixture: ordinary C -> standard Wasm. */

typedef unsigned char u8;
typedef unsigned int u32;

#define WIDTH 32u
#define HEIGHT 16u
#define PALETTE_COUNT 3u
#define PIXEL_OFFSET (16u + PALETTE_COUNT * 4u)
#define FRAME_BYTES (PIXEL_OFFSET + WIDTH * HEIGHT)

__attribute__((import_module("tinyarcade:core/v1"), import_name("input_bits")))
extern int input_bits(void);
__attribute__((import_module("tinyarcade:core/v1"), import_name("indexed2d_version")))
extern int indexed2d_version(void);
__attribute__((import_module("tinyarcade:core/v1"), import_name("submit_render")))
extern int submit_render(const u8 *pointer, u32 length);
__attribute__((import_module("tinyarcade:core/v1"), import_name("save_state")))
extern int save_state(const u8 *pointer, u32 length);
__attribute__((import_module("tinyarcade:core/v1"), import_name("load_state")))
extern int load_state(u8 *pointer, u32 capacity);

static u8 frame[FRAME_BYTES];
static u32 dot_x;

static void put_u16(u32 at, u32 value) {
    frame[at] = (u8)value;
    frame[at + 1u] = (u8)(value >> 8u);
}

__attribute__((export_name("game_abi_version")))
int game_abi_version(void) {
    return 1;
}

__attribute__((export_name("game_init")))
int game_init(void) {
    if (indexed2d_version() != 1) {
        return 1;
    }
    frame[0] = 'T';
    frame[1] = 'A';
    frame[2] = 'I';
    frame[3] = '2';
    put_u16(4u, 1u);
    put_u16(6u, 16u);
    put_u16(8u, WIDTH);
    put_u16(10u, HEIGHT);
    put_u16(12u, PALETTE_COUNT);
    put_u16(14u, 0u);
    frame[16] = 8;
    frame[17] = 12;
    frame[18] = 20;
    frame[19] = 255;
    frame[20] = 46;
    frame[21] = 196;
    frame[22] = 182;
    frame[23] = 255;
    frame[24] = 244;
    frame[25] = 214;
    frame[26] = 94;
    frame[27] = 255;
    dot_x = WIDTH / 2u;
    return 0;
}

__attribute__((export_name("game_tick")))
int game_tick(void) {
    u32 index;
    int buttons = input_bits();
    if ((buttons & 1) != 0 && dot_x > 0u) {
        dot_x -= 1u;
    }
    if ((buttons & 2) != 0 && dot_x + 1u < WIDTH) {
        dot_x += 1u;
    }
    for (index = 0; index < WIDTH * HEIGHT; index += 1u) {
        frame[PIXEL_OFFSET + index] = (index / WIDTH == HEIGHT / 2u) ? 1u : 0u;
    }
    frame[PIXEL_OFFSET + (HEIGHT / 2u) * WIDTH + dot_x] = 2u;
    return submit_render(frame, FRAME_BYTES);
}

__attribute__((export_name("game_suspend")))
int game_suspend(void) {
    return save_state((const u8 *)&dot_x, 4u);
}

__attribute__((export_name("game_resume")))
int game_resume(void) {
    return load_state((u8 *)&dot_x, 4u) == 4 ? 0 : 1;
}
