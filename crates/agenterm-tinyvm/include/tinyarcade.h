#ifndef TINYARCADE_RUNTIME_H
#define TINYARCADE_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TINYARCADE_ABI_MAJOR 1u
#define TINYARCADE_ABI_MINOR 1u
#define TINYARCADE_ABI_VERSION 0x00010001u

typedef struct tinyarcade_runtime_v1 tinyarcade_runtime_v1;
typedef struct tinyarcade_trust_store_v1 tinyarcade_trust_store_v1;

typedef enum tinyarcade_status_v1 {
    TINYARCADE_OK = 0,
    TINYARCADE_INVALID_ARGUMENT = 1,
    TINYARCADE_DECODE_ERROR = 2,
    TINYARCADE_GUEST_TRAP = 3,
    TINYARCADE_BUFFER_TOO_SMALL = 4,
    TINYARCADE_WRONG_THREAD = 5,
    TINYARCADE_FAILED_INSTANCE = 6,
    TINYARCADE_PANIC = 7,
    TINYARCADE_TRUST_ERROR = 8
} tinyarcade_status_v1;

typedef enum tinyarcade_cartridge_origin_v1 {
    TINYARCADE_ORIGIN_BUNDLED = 0,
    TINYARCADE_ORIGIN_OFFICIAL_REVIEWED = 1,
    TINYARCADE_ORIGIN_PRIVATE_USER = 2
} tinyarcade_cartridge_origin_v1;

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

/* Pointer fields are borrowed only for the duration of open_reviewed. The
 * signature is the canonical detached Ed25519 signature described by the
 * TinyArcade signed catalog v1 contract. */
typedef struct tinyarcade_catalog_entry_v1 {
    uint32_t struct_size;
    const uint8_t* game_id;
    size_t game_id_len;
    const uint8_t* game_version;
    size_t game_version_len;
    uint32_t abi_version;
    uint32_t state_version;
    uint64_t wasm_length;
    const uint8_t* wasm_sha256;
    size_t wasm_sha256_len;
    const uint8_t* signing_key_id;
    size_t signing_key_id_len;
    const uint8_t* signature;
    size_t signature_len;
} tinyarcade_catalog_entry_v1;

uint32_t tinyarcade_v1_abi_version(void);
tinyarcade_status_v1 tinyarcade_v1_default_config(tinyarcade_config_v1* config);

/* Trust stores are mutable, single-thread-owned policy objects. Public keys
 * are exact 32-byte Ed25519 keys; content hashes are exact 32-byte SHA-256. */
tinyarcade_status_v1 tinyarcade_v1_trust_store_create(
    tinyarcade_trust_store_v1** output);
tinyarcade_status_v1 tinyarcade_v1_trust_store_close(
    tinyarcade_trust_store_v1* trust);
tinyarcade_status_v1 tinyarcade_v1_trust_store_add_key(
    tinyarcade_trust_store_v1* trust,
    const uint8_t* key_id,
    size_t key_id_len,
    const uint8_t* public_key,
    size_t public_key_len);
tinyarcade_status_v1 tinyarcade_v1_trust_store_revoke_key(
    tinyarcade_trust_store_v1* trust,
    const uint8_t* key_id,
    size_t key_id_len);
tinyarcade_status_v1 tinyarcade_v1_trust_store_revoke_content(
    tinyarcade_trust_store_v1* trust,
    const uint8_t* sha256,
    size_t sha256_len);

/* Runtime handles have strict single-thread ownership. Every operation,
 * including close, must run on the thread that successfully called open.
 * The library copies the WASM bytes during open and never retains caller
 * pointers. On failure, *output is NULL. */
tinyarcade_status_v1 tinyarcade_v1_open(
    const uint8_t* wasm,
    size_t wasm_len,
    const tinyarcade_config_v1* config,
    tinyarcade_runtime_v1** output);
/* Private imports get only tinyarcade:core/v1. This entry point never grants
 * official catalog provenance or a native capability registry. */
tinyarcade_status_v1 tinyarcade_v1_open_private(
    const uint8_t* wasm,
    size_t wasm_len,
    const tinyarcade_config_v1* config,
    tinyarcade_runtime_v1** output);
/* Reviewed opening verifies the exact catalog signature, key/content
 * revocation, length/hash and embedded manifest before runtime creation. */
tinyarcade_status_v1 tinyarcade_v1_open_reviewed(
    const uint8_t* wasm,
    size_t wasm_len,
    const tinyarcade_catalog_entry_v1* entry,
    tinyarcade_trust_store_v1* trust,
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
tinyarcade_status_v1 tinyarcade_v1_origin(
    tinyarcade_runtime_v1* runtime,
    uint32_t* output);

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
