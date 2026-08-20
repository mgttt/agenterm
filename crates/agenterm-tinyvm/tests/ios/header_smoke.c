#include "tinyarcade.h"

_Static_assert(TINYARCADE_ABI_VERSION == 0x00010000u, "ABI version drift");
_Static_assert(sizeof(tinyarcade_config_v1) == 40, "config layout drift");

static void typecheck(void) {
    tinyarcade_runtime_v1* runtime = 0;
    tinyarcade_config_v1 config;
    (void)tinyarcade_v1_default_config(&config);
    (void)tinyarcade_v1_open(0, 0, &config, &runtime);
    (void)tinyarcade_v1_tick(runtime, 0, 0);
    (void)tinyarcade_v1_suspend(runtime);
    (void)tinyarcade_v1_resume(runtime, 0, 0);
    (void)tinyarcade_v1_close(runtime);
}

int main(void) {
    typecheck();
    return 0;
}
