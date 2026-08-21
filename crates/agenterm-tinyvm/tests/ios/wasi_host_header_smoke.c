#include "tinyvm_wasi_host.h"

_Static_assert(TINYVM_WASI_HOST_ABI_MAJOR == 1u, "WASI host ABI major");
_Static_assert(sizeof(tinyvm_wasi_host_config_v1) == 40u, "WASI host config layout");

int main(void) {
    tinyvm_wasi_host_config_v1 config = {0};
    return tinyvm_wasi_host_v1_default_config(&config) == TINYVM_WASI_HOST_OK ? 0 : 1;
}
