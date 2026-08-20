#include "tinyarcade.h"

_Static_assert(TINYARCADE_ABI_VERSION == 0x00010003u, "ABI version drift");
_Static_assert(sizeof(tinyarcade_config_v1) == 40, "config layout drift");

static int32_t native_callback(
    void* context,
    const int32_t* params,
    size_t n_params,
    int32_t* results,
    size_t n_results,
    uint8_t* memory,
    size_t memory_len) {
    (void)context;
    (void)params;
    (void)n_params;
    (void)results;
    (void)n_results;
    (void)memory;
    (void)memory_len;
    return 0;
}

static void typecheck(void) {
    tinyarcade_runtime_v1* runtime = 0;
    tinyarcade_trust_store_v1* trust = 0;
    tinyarcade_catalog_entry_v1 entry = {0};
    tinyarcade_native_function_v1 native = {
        .struct_size = sizeof(tinyarcade_native_function_v1),
        .module = (const uint8_t*)"fan:physics/v1",
        .module_len = 14,
        .field = (const uint8_t*)"step_world",
        .field_len = 10,
        .n_params = 2,
        .n_results = 1,
        .max_calls_per_lifecycle = 1,
        .callback = native_callback,
        .context = 0,
    };
    tinyarcade_config_v1 config;
    (void)tinyarcade_v1_default_config(&config);
    (void)tinyarcade_v1_open(0, 0, &config, &runtime);
    (void)tinyarcade_v1_open_with_native_modules(0, 0, &native, 1, &config, &runtime);
    (void)tinyarcade_v1_open_private(0, 0, &config, &runtime);
    (void)tinyarcade_v1_trust_store_create(&trust);
    (void)tinyarcade_v1_trust_store_add_key(trust, 0, 0, 0, 0);
    (void)tinyarcade_v1_trust_store_revoke_key(trust, 0, 0);
    (void)tinyarcade_v1_trust_store_revoke_content(trust, 0, 0);
    (void)tinyarcade_v1_open_reviewed(0, 0, &entry, trust, &config, &runtime);
    (void)tinyarcade_v1_open_reviewed_with_native_modules(
        0, 0, &entry, trust, &native, 1, &config, &runtime);
    (void)tinyarcade_v1_tick(runtime, 0, 0);
    (void)tinyarcade_v1_suspend(runtime);
    (void)tinyarcade_v1_resume(runtime, 0, 0);
    (void)tinyarcade_v1_origin(runtime, 0);
    (void)tinyarcade_v1_close(runtime);
    (void)tinyarcade_v1_trust_store_close(trust);
}

int main(void) {
    typecheck();
    return 0;
}
