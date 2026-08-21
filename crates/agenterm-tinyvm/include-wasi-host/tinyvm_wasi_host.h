#ifndef TINYVM_WASI_HOST_H
#define TINYVM_WASI_HOST_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TINYVM_WASI_HOST_ABI_MAJOR 1u
#define TINYVM_WASI_HOST_ABI_MINOR 0u

typedef enum tinyvm_wasi_host_status_v1 {
    TINYVM_WASI_HOST_OK = 0,
    TINYVM_WASI_HOST_INVALID_ARGUMENT = 1,
    TINYVM_WASI_HOST_DECODE_ERROR = 2,
    TINYVM_WASI_HOST_GUEST_TRAP = 3,
    TINYVM_WASI_HOST_STORAGE_ERROR = 4,
    TINYVM_WASI_HOST_PANIC = 5
} tinyvm_wasi_host_status_v1;

typedef struct tinyvm_wasi_host_config_v1 {
    uint32_t struct_size;
    uint32_t max_table_elems;
    uint32_t max_memory_pages;
    uint32_t max_call_depth;
    uint32_t max_activation_slots;
    uint32_t max_host_handles;
    uint32_t max_guest_descriptors;
    uint64_t max_steps;
} tinyvm_wasi_host_config_v1;

tinyvm_wasi_host_status_v1 tinyvm_wasi_host_v1_default_config(
    tinyvm_wasi_host_config_v1* output);

/* Runs the standard WASI `_start` export once. `preopen_path` is an App-owned
 * iOS container directory borrowed only for this call and exposed to the guest
 * solely as `/save`. `did_exit` is one only when accepted `proc_exit` ended the
 * command; a normal empty `_start` return reports zero. */
tinyvm_wasi_host_status_v1 tinyvm_wasi_host_v1_run(
    const uint8_t* wasm,
    size_t wasm_len,
    const uint8_t* preopen_path,
    size_t preopen_path_len,
    const tinyvm_wasi_host_config_v1* config,
    uint32_t* did_exit,
    uint32_t* exit_code);

#ifdef __cplusplus
}
#endif

#endif
