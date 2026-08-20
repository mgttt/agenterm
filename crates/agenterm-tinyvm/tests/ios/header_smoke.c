#include "tinyarcade.h"

_Static_assert(TINYARCADE_ABI_VERSION == 0x00010001u, "ABI version drift");
_Static_assert(sizeof(tinyarcade_config_v1) == 40, "config layout drift");

static void typecheck(void) {
    tinyarcade_runtime_v1* runtime = 0;
    tinyarcade_trust_store_v1* trust = 0;
    tinyarcade_catalog_entry_v1 entry = {0};
    tinyarcade_config_v1 config;
    (void)tinyarcade_v1_default_config(&config);
    (void)tinyarcade_v1_open(0, 0, &config, &runtime);
    (void)tinyarcade_v1_open_private(0, 0, &config, &runtime);
    (void)tinyarcade_v1_trust_store_create(&trust);
    (void)tinyarcade_v1_trust_store_add_key(trust, 0, 0, 0, 0);
    (void)tinyarcade_v1_trust_store_revoke_key(trust, 0, 0);
    (void)tinyarcade_v1_trust_store_revoke_content(trust, 0, 0);
    (void)tinyarcade_v1_open_reviewed(0, 0, &entry, trust, &config, &runtime);
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
