#ifndef TINYARCADE_GUEST_V1_H
#define TINYARCADE_GUEST_V1_H

/* Freestanding C authoring declarations for a standard Wasm cartridge.
 * This header contributes no runtime, allocator, libc or private bytecode. */

#if !defined(__wasm32__)
#error "tinyarcade_guest_v1.h requires a wasm32 compilation target"
#endif

#if !defined(__clang__)
#error "tinyarcade_guest_v1.h currently requires Clang-compatible Wasm attributes"
#endif

typedef unsigned char tinyarcade_u8_v1;
typedef unsigned int tinyarcade_u32_v1;
_Static_assert(sizeof(tinyarcade_u8_v1) == 1, "TinyArcade u8 ABI");
_Static_assert(sizeof(tinyarcade_u32_v1) == 4, "TinyArcade u32 ABI");

#define TINYARCADE_IMPORT_V1(field)                                           \
    __attribute__((import_module("tinyarcade:core/v1"), import_name(field)))
#define TINYARCADE_EXPORT_V1(field) __attribute__((export_name(field)))

TINYARCADE_IMPORT_V1("input_bits") int tinyarcade_input_bits_v1(void);
TINYARCADE_IMPORT_V1("clock_ms") int tinyarcade_clock_ms_v1(void);
TINYARCADE_IMPORT_V1("random_u32") int tinyarcade_random_u32_v1(void);
TINYARCADE_IMPORT_V1("indexed2d_version") int tinyarcade_indexed2d_version_v1(void);
TINYARCADE_IMPORT_V1("grid3d_version") int tinyarcade_grid3d_version_v1(void);
TINYARCADE_IMPORT_V1("tones_version") int tinyarcade_tones_version_v1(void);
TINYARCADE_IMPORT_V1("submit_render") int tinyarcade_submit_render_v1(
    const tinyarcade_u8_v1 *pointer,
    tinyarcade_u32_v1 length);
TINYARCADE_IMPORT_V1("submit_audio") int tinyarcade_submit_audio_v1(
    const tinyarcade_u8_v1 *pointer,
    tinyarcade_u32_v1 length);
TINYARCADE_IMPORT_V1("save_state") int tinyarcade_save_state_v1(
    const tinyarcade_u8_v1 *pointer,
    tinyarcade_u32_v1 length);
TINYARCADE_IMPORT_V1("load_state") int tinyarcade_load_state_v1(
    tinyarcade_u8_v1 *pointer,
    tinyarcade_u32_v1 capacity);

#endif
