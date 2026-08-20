#ifndef TINYARCADE_RUNTIME_H
#define TINYARCADE_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TINYARCADE_ABI_MAJOR 1u
#define TINYARCADE_ABI_MINOR 0u
#define TINYARCADE_ABI_VERSION 0x00010000u

typedef struct tinyarcade_runtime_v1 tinyarcade_runtime_v1;

typedef enum tinyarcade_status_v1 {
    TINYARCADE_OK = 0,
    TINYARCADE_INVALID_ARGUMENT = 1,
    TINYARCADE_DECODE_ERROR = 2,
    TINYARCADE_GUEST_TRAP = 3,
    TINYARCADE_BUFFER_TOO_SMALL = 4,
    TINYARCADE_WRONG_THREAD = 5,
    TINYARCADE_FAILED_INSTANCE = 6,
    TINYARCADE_PANIC = 7
} tinyarcade_status_v1;

typedef struct tinyarcade_config_v1 {
    uint32_t struct_size;
    uint32_t max_table_elems;
    uint32_t max_memory_pages;
    uint64_t max_steps;
    uint32_t max_render_bytes;
    uint32_t max_audio_bytes;
    uint32_t max_state_bytes;
    uint32_t rng_seed;
} tinyarcade_config_v1;

uint32_t tinyarcade_v1_abi_version(void);
tinyarcade_status_v1 tinyarcade_v1_default_config(tinyarcade_config_v1* config);

/* Runtime handles have strict single-thread ownership. Every operation,
 * including close, must run on the thread that successfully called open.
 * The library copies the WASM bytes during open and never retains caller
 * pointers. On failure, *output is NULL. */
tinyarcade_status_v1 tinyarcade_v1_open(
    const uint8_t* wasm,
    size_t wasm_len,
    const tinyarcade_config_v1* config,
    tinyarcade_runtime_v1** output);
tinyarcade_status_v1 tinyarcade_v1_close(tinyarcade_runtime_v1* runtime);

tinyarcade_status_v1 tinyarcade_v1_tick(
    tinyarcade_runtime_v1* runtime,
    uint32_t buttons,
    uint32_t clock_ms);

/* All copy calls use the same two-stage protocol. *output_len always receives
 * the required byte count. NULL/0 is a size query and returns
 * TINYARCADE_BUFFER_TOO_SMALL when the value is non-empty. Bytes are not
 * NUL-terminated. Frame bytes stay valid inside the handle until next tick. */
tinyarcade_status_v1 tinyarcade_v1_copy_render(
    tinyarcade_runtime_v1* runtime,
    uint8_t* output,
    size_t capacity,
    size_t* output_len);
tinyarcade_status_v1 tinyarcade_v1_copy_audio(
    tinyarcade_runtime_v1* runtime,
    uint8_t* output,
    size_t capacity,
    size_t* output_len);

/* suspend runs the guest exactly once and stores one snapshot in the handle;
 * copy_snapshot may then be called repeatedly without running guest code. */
tinyarcade_status_v1 tinyarcade_v1_suspend(tinyarcade_runtime_v1* runtime);
tinyarcade_status_v1 tinyarcade_v1_copy_snapshot(
    tinyarcade_runtime_v1* runtime,
    uint8_t* output,
    size_t capacity,
    size_t* output_len);
tinyarcade_status_v1 tinyarcade_v1_resume(
    tinyarcade_runtime_v1* runtime,
    const uint8_t* snapshot,
    size_t snapshot_len);

tinyarcade_status_v1 tinyarcade_v1_copy_game_id(
    tinyarcade_runtime_v1* runtime,
    uint8_t* output,
    size_t capacity,
    size_t* output_len);
tinyarcade_status_v1 tinyarcade_v1_copy_game_version(
    tinyarcade_runtime_v1* runtime,
    uint8_t* output,
    size_t capacity,
    size_t* output_len);
tinyarcade_status_v1 tinyarcade_v1_is_failed(
    tinyarcade_runtime_v1* runtime,
    int32_t* output);

/* Per-thread diagnostic for the preceding tinyarcade call. This accessor does
 * not clear the stored message and uses the same two-stage byte protocol. */
tinyarcade_status_v1 tinyarcade_v1_last_error(
    uint8_t* output,
    size_t capacity,
    size_t* output_len);

#ifdef __cplusplus
}
#endif

#endif
