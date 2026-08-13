/*
 * agenterm.h — C header for libagenterm (crates/agenterm-abi).
 *
 * This is the *mechanism* boundary between embedding consumers and the OS.
 * It deliberately contains no product concepts. Every symbol is prefixed
 * `agt_`. Milestone 1 shipped version / error / capability exports; milestone 2
 * adds the PTY mechanism. Window / screenshot mechanisms arrive later.
 */
#ifndef AGENTERM_AGENT_ABI_H
#define AGENTERM_AGENT_ABI_H

#include <stddef.h>
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

/* --- pty ------------------------------------------------------------ */

/* Opaque, library-owned PTY handle. Cross-thread safe: any thread may call
 * the agt_pty_* functions on the same handle, and close may run while another
 * thread is blocked in read (the blocked read is unblocked). The handle is
 * released by agt_pty_close, which must be called exactly once. */
typedef struct agt_pty* agt_pty_t;

typedef struct {
    const char* program;        /* required, NUL-terminated, UTF-8 */
    /* argv[0] is the program name by POSIX convention and is not re-passed
     * as an argument; arguments are argv[1..argc]. NULL/0 = no arguments. */
    const char* const* argv;
    size_t argc;
    const char* cwd;            /* NULL = inherit the caller's directory */
    /* "K=V" entries; NULL or envc == 0 = inherit the parent environment. */
    const char* const* envp;
    size_t envc;
    uint16_t cols, rows;        /* terminal size, each >= 1 */
} agt_pty_spawn;

/* Spawn program in a new PTY; *out receives an opaque library-owned handle.
 * On failure returns AGT_FAILED (never AGT_UNSUPPORTED); the reason is
 * available via agt_last_error. */
agt_status agt_pty_open  (const agt_pty_spawn*, agt_pty_t* out);

/* Block until data is available or the PTY is closed. Caller-allocated buffer:
 * the library never takes memory ownership. EOF is AGT_OK with *out_len == 0.
 * cap == 0 fails with code "buffer_too_small" and *out_len = required length. */
agt_status agt_pty_read  (agt_pty_t, uint8_t* buf, size_t cap, size_t* out_len);

/* Write len bytes to the PTY master; on success *written == len. */
agt_status agt_pty_write (agt_pty_t, const uint8_t*, size_t, size_t* written);

/* Resize the PTY to cols x rows (each >= 1). */
agt_status agt_pty_resize(agt_pty_t, uint16_t cols, uint16_t rows);

/* Wait up to timeout_ms for the process to exit; on exit *exit_code is filled
 * and AGT_OK is returned. On timeout returns AGT_FAILED with code "timeout"
 * (never AGT_UNSUPPORTED). The underlying blocking wait runs on a
 * library-private thread. */
agt_status agt_pty_wait  (agt_pty_t, uint32_t timeout_ms, int32_t* exit_code);

/* Release the handle; must be called exactly once. Unblocks any thread
 * currently blocked in agt_pty_read on the same handle. */
void       agt_pty_close (agt_pty_t);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* AGENTERM_AGENT_ABI_H */
