/*
 * agenterm.h — C header for libagenterm (crates/agenterm-abi).
 *
 * This is the *mechanism* boundary between embedding consumers and the OS.
 * It deliberately contains no product concepts. Every symbol is prefixed
 * `agt_`. Milestone 1 implements only version / error / capability exports;
 * PTY / window / screenshot mechanisms arrive in later milestones.
 */
#ifndef AGENTERM_AGENT_ABI_H
#define AGENTERM_AGENT_ABI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- version & build ------------------------------------------------ */

/* (major << 16) | minor. Consumers must reject mismatched major. */
uint32_t    agt_abi_version(void);
/* Human-readable build identity; NUL-terminated, static, permanently valid. */
const char* agt_build_id(void);

/* --- status & error ------------------------------------------------- */

typedef enum {
    AGT_OK = 0,
    AGT_UNSUPPORTED = 1, /* capability/mechanism absent on this build */
    AGT_FAILED = 2       /* the call itself failed */
} agt_status;

/* AGT_UNSUPPORTED and AGT_FAILED are intentionally distinct and never merged:
 * callers must be able to tell "platform does not have it" from "it did not
 * work this time". */
typedef struct {
    const char* operation; /* static, permanently valid, NUL-terminated */
    const char* code;      /* static, permanently valid, NUL-terminated */
    const char* message;   /* thread-local, valid until next call on this thread */
} agt_error;

/* Fill *out with the last error recorded on this thread, or a "no error"
 * record. Returns AGT_OK on success. */
agt_status agt_last_error(agt_error* out);

/* --- capability negotiation ----------------------------------------- */

typedef enum {
    AGT_CAP_PTY = 1,
    AGT_CAP_PROCESS_SPAWN,
    AGT_CAP_PROCESS_OBSERVE,
    AGT_CAP_WINDOW_HOST,
    AGT_CAP_WINDOW_ENUMERATE,
    AGT_CAP_WINDOW_OP,
    AGT_CAP_SCREENSHOT,
    AGT_CAP_CLIPBOARD,
    AGT_CAP_IME,
    AGT_CAP_INPUT_INJECT,
    AGT_CAP_IPC,
    AGT_CAP_FONT_RASTER,
    AGT_CAP_FILESYSTEM_PUBLISH,
    AGT_CAP_SHARED_MEMORY,
    AGT_CAP_PARENT_CONSOLE
} agt_capability;

/* Returns AGT_OK or AGT_UNSUPPORTED only (never AGT_FAILED). */
agt_status agt_capability_query(agt_capability cap);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* AGENTERM_AGENT_ABI_H */
