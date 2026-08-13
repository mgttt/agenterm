/*
 * agenterm_probe.c -- real C consumer probe for libagenterm
 * (crates/agenterm-abi).
 *
 * Exercises the exported C ABI through the public header (include/agenterm.h):
 * version / build id, capability negotiation, process enumeration via the
 * two-stage "how big?" probe, and the thread-local error record. `%s` on the
 * build id also proves NUL termination.
 *
 * Every step is verified: anything that violates the documented ABI contract
 * prints a reason to stderr and exits 1. Compiles warning-free under MSVC
 * /W4 /WX and gcc/clang -Wall -Wextra -Werror. Build instructions live in
 * examples/c/README.md.
 *
 * NOTE on the agt_last_error() print below: the two-stage process-list probe
 * deliberately fails once (AGT_FAILED, code "buffer_too_small") to learn the
 * required count -- that is the documented negotiation signal, not a real
 * failure. It is why the final error record shows that entry instead of
 * "no error".
 */
#include "agenterm.h"

#include <stdio.h>
#include <stdlib.h>

static const char* cap_name(agt_capability cap) {
    switch (cap) {
    case AGT_CAP_PTY:
        return "PTY";
    case AGT_CAP_WINDOW_HOST:
        return "WINDOW_HOST";
    case AGT_CAP_SCREENSHOT:
        return "SCREENSHOT";
    case AGT_CAP_PROCESS_OBSERVE:
        return "PROCESS_OBSERVE";
    default:
        return "?";
    }
}

static int fail(const char* what, const char* why) {
    fprintf(stderr, "FAIL: %s: %s\n", what, why);
    return 1;
}

int main(void) {
    /* version & build id; %s requires a valid NUL-terminated C string */
    uint32_t version = agt_abi_version();
    printf("abi_version=0x%08x\n", (unsigned)version);
    if (version != AGT_ABI_VERSION) {
        return fail("agt_abi_version", "expected AGT_ABI_VERSION");
    }

    const char* build_id = agt_build_id();
    printf("build_id=%s\n", build_id);
    if (build_id == NULL) {
        return fail("agt_build_id", "NULL");
    }

    /* capability negotiation: the four milestone mechanisms must be AGT_OK */
    const agt_capability caps[4] = {AGT_CAP_PTY, AGT_CAP_WINDOW_HOST,
                                    AGT_CAP_SCREENSHOT, AGT_CAP_PROCESS_OBSERVE};
    size_t i;
    for (i = 0; i < 4; i++) {
        agt_status st = agt_capability_query(caps[i]);
        printf("capability(%s)=%d\n", cap_name(caps[i]), (int)st);
        if (st != AGT_OK) {
            return fail("agt_capability_query", cap_name(caps[i]));
        }
    }

    uint32_t self_pid = agt_process_self();
    printf("self_pid=%u\n", (unsigned)self_pid);
    if (self_pid == 0) {
        return fail("agt_process_self", "returned pid 0");
    }

    /* two-stage process enumeration: probe with NULL/0, then allocate and
     * fetch. The probe's AGT_FAILED is the documented negotiation signal. */
    size_t need = 0;
    agt_status st = agt_process_list(NULL, 0, &need);
    if (st != AGT_FAILED || need == 0) {
        return fail("agt_process_list probe", "expected AGT_FAILED + need > 0");
    }
    printf("process_list probe: need=%zu\n", need);

    agt_process_info* buf = (agt_process_info*)calloc(need, sizeof(agt_process_info));
    if (buf == NULL) {
        return fail("calloc", "out of memory");
    }
    size_t got = 0;
    st = agt_process_list(buf, need, &got);
    if (st != AGT_OK || got == 0) {
        free(buf);
        return fail("agt_process_list fetch", "expected AGT_OK + got > 0");
    }

    size_t shown = got < 3 ? got : 3;
    size_t k;
    for (k = 0; k < shown; k++) {
        /* name is NOT NUL-terminated: use name_len with %.*s */
        printf("proc[%zu] id=%u parent_id=%u name=%.*s\n", k,
               (unsigned)buf[k].id, (unsigned)buf[k].parent_id,
               (int)buf[k].name_len, buf[k].name);
    }
    free(buf);

    /* read the thread-local error record once; it still holds the probe's
     * buffer_too_small entry (see file header) */
    agt_error err = {NULL, NULL, NULL};
    if (agt_last_error(&err) != AGT_OK) {
        return fail("agt_last_error", "call failed");
    }
    if (err.operation == NULL || err.code == NULL || err.message == NULL) {
        return fail("agt_last_error", "NULL field");
    }
    printf("last_error: operation=%s code=%s message=%s\n", err.operation,
           err.code, err.message);

    return 0;
}
