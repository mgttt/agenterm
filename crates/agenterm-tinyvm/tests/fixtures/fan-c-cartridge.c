/* Freestanding third-party authoring fixture: ordinary C -> standard Wasm. */

#include "tinyarcade_guest_v1.h"

typedef tinyarcade_u8_v1 u8;
typedef tinyarcade_u32_v1 u32;

#define WIDTH 32u
#define HEIGHT 16u
#define PALETTE_COUNT 3u
#define PIXEL_OFFSET (16u + PALETTE_COUNT * 4u)
#define FRAME_BYTES (PIXEL_OFFSET + WIDTH * HEIGHT)

static u8 frame[FRAME_BYTES];
static u32 dot_x;

static void put_u16(u32 at, u32 value) {
    frame[at] = (u8)value;
    frame[at + 1u] = (u8)(value >> 8u);
}

TINYARCADE_EXPORT_V1("game_abi_version")
int game_abi_version(void) {
    return 1;
}

TINYARCADE_EXPORT_V1("game_init")
int game_init(void) {
    if (tinyarcade_indexed2d_version_v1() != 1) {
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

TINYARCADE_EXPORT_V1("game_tick")
int game_tick(void) {
    u32 index;
    int buttons = tinyarcade_input_bits_v1();
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
    return tinyarcade_submit_render_v1(frame, FRAME_BYTES);
}

TINYARCADE_EXPORT_V1("game_suspend")
int game_suspend(void) {
    return tinyarcade_save_state_v1((const u8 *)&dot_x, 4u);
}

TINYARCADE_EXPORT_V1("game_resume")
int game_resume(void) {
    return tinyarcade_load_state_v1((u8 *)&dot_x, 4u) == 4 ? 0 : 1;
}
