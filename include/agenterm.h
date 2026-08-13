/*
 * agenterm.h -- C header for libagenterm (crates/agenterm-abi).
 *
 * This is the *mechanism* boundary between embedding consumers and the OS.
 * It deliberately contains no product concepts. Every symbol is prefixed
 * `agt_`. Milestone 1 shipped version / error / capability exports; milestone 2
 * adds the PTY mechanism; milestones 3a/3b add the window + frame mechanisms;
 * milestone 4 adds screenshot export (framebuffer -> PNG, native window -> PNG);
 * milestone 5 adds the process group (enumerate / kill / self pid).
 * milestone 8 adds the clipboard group (set / get / has-text).
 * milestone 9 adds the parent-console group (write stdout / write stderr).
 * milestone 10 adds the runtime-environment group (user config dir, default
 * terminal shell, environment probe, argument list).
 */
#ifndef AGENTERM_AGENT_ABI_H
#define AGENTERM_AGENT_ABI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- version & build ------------------------------------------------ */

/* ABI versioning contract (see crates/agenterm-abi/README.md):
 *   major: breaking change (signature change / symbol removal / semantic
 *          change). Consumers must reject a mismatched major.
 *   minor: additive export additions (a new mechanism); old consumers are
 *          unaffected.
 * agt_abi_version() returns (major << 16) | minor. Compare against the
 * AGT_ABI_* macros below instead of hard-coded literals. */
#define AGT_ABI_MAJOR 1
#define AGT_ABI_MINOR 2
#define AGT_ABI_VERSION ((AGT_ABI_MAJOR << 16) | AGT_ABI_MINOR)
uint32_t    agt_abi_version(void);

/* Human-readable build identity: "<crate version>+abi.<major>.<minor>"
 * (e.g. "0.1.16+abi.1.2"), derived at compile time from the crate version
 * and the ABI constants above. NUL-terminated, static, permanently valid. */
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
    AGT_CAP_PARENT_CONSOLE,
    AGT_CAP_ACCESSIBILITY_TREE
} agt_capability;

/* Returns AGT_OK or AGT_UNSUPPORTED only (never AGT_FAILED). As of
 * milestone 9, AGT_CAP_PTY, AGT_CAP_WINDOW_HOST, AGT_CAP_SCREENSHOT,
 * AGT_CAP_PROCESS_OBSERVE, AGT_CAP_CLIPBOARD and AGT_CAP_PARENT_CONSOLE all
 * report AGT_OK; AGT_CAP_ACCESSIBILITY_TREE reports AGT_OK when the host
 * accessibility stack is wired. Platform exception (milestone 22):
 * AGT_CAP_WINDOW_HOST reports AGT_UNSUPPORTED on macOS, mirroring
 * agt_window_open - AppKit requires the window event loop on the main
 * thread, and this ABI hosts it on a library-private thread, so the window
 * host mechanism does not exist on macOS. */
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

/* --- window & frame (milestones 3a / 3b) ------------------------------ */

typedef struct agt_window* agt_window_t;

/* Window events. Milestone 3a translated close / geometry / focus / render;
 * milestone 3b adds KEY / POINTER / WHEEL / IME. Platform events without a
 * translation are dropped by the library (never an error). */
typedef enum {
    AGT_EV_NONE = 0,
    AGT_EV_CLOSE_REQUEST = 1, /* the native window requested close */
    AGT_EV_GEOMETRY      = 2, /* width/height/scale valid */
    AGT_EV_FOCUS         = 3, /* focused valid */
    AGT_EV_RENDER_DUE    = 4, /* render() has stopped at the rendezvous */
    AGT_EV_KEY           = 5, /* keyboard event */
    AGT_EV_POINTER       = 6, /* pointer move / button / leave / capture */
    AGT_EV_WHEEL         = 7, /* mouse wheel */
    AGT_EV_IME           = 8  /* IME enabled / preedit / commit / disabled */
} agt_event_kind;

/* `modifiers` bitmask (valid for KEY / POINTER; 0 when not applicable). */
#define AGT_MOD_CONTROL 1u
#define AGT_MOD_SHIFT   2u
#define AGT_MOD_ALT     4u
#define AGT_MOD_META    8u

typedef struct {
    uint32_t kind;
    uint64_t generation;
    uint32_t width, height; /* only valid for AGT_EV_GEOMETRY */
    double   scale;         /* only valid for AGT_EV_GEOMETRY */
    int32_t  focused;       /* only valid for AGT_EV_FOCUS */

    uint32_t modifiers;     /* AGT_MOD_* bitmask; KEY / POINTER */

    /* KEY (AGT_EV_KEY) */
    uint8_t  key_state;          /* 0=released, 1=pressed */
    uint8_t  key_repeat;         /* 0/1 */
    uint8_t  key_named;          /* NamedKey code table; 0=unnamed, 255=unknown */
    uint8_t  key_physical;       /* 0=other,1=letter,2=digit,3=backspace,
                                    4=enter,5=space,6=tab */
    uint32_t key_physical_value; /* letter codepoint / digit value / 0 */
    uint8_t  text[16];           /* NormalizedKeyEvent::text, UTF-8 */
    uint8_t  text_len;           /* bytes used in text[16] */
    uint8_t  text_truncated;     /* 1 when text was truncated to fit */

    /* POINTER (AGT_EV_POINTER) and WHEEL position */
    double   pointer_x, pointer_y; /* logical position; valid when has_position */
    uint8_t  pointer_button;       /* 0=none/move,1=left,2=right,3=middle,4=other */
    uint8_t  pointer_state;        /* 0=released,1=pressed,2=moved,3=left,4=capture_lost */
    uint8_t  has_position;         /* 0/1 */

    /* WHEEL (AGT_EV_WHEEL) */
    double   wheel_x, wheel_y; /* scroll delta */
    uint8_t  wheel_unit;       /* 0=lines, 1=logical_pixels */

    /* IME (AGT_EV_IME) */
    uint8_t  ime_kind;        /* 0=enabled,1=preedit,2=commit,3=disabled */
    uint8_t  has_ime_cursor;  /* 0/1 */
    size_t   ime_cursor_begin; /* valid when has_ime_cursor */
    size_t   ime_cursor_end;
    size_t   ime_text_len;    /* text bytes; fetch via agt_window_event_text */
} agt_event;

typedef struct {
    const char* title;       /* required, NUL-terminated, UTF-8 */
    uint32_t width, height;  /* initial logical size, each >= 1 */
    int32_t no_activate;     /* non-zero: do not take foreground focus */
    int32_t ime_allowed;     /* non-zero: allow IME input */
} agt_window_spec;

/* Frame descriptor filled by agt_frame_begin. The `pixels` pointer is valid
 * ONLY between a successful agt_frame_begin and the matching
 * agt_frame_commit; it must never be stored or dereferenced past that
 * window. XRGB buffers are tightly packed (stride_px == width). */
typedef struct {
    uint32_t* pixels;
    uint32_t width, height;
    uint32_t stride_px;
} agt_frame_desc;

/* Open a native pixel window. The window event loop runs on a
 * library-private thread; events and frames rendezvous back through
 * agt_window_poll_event / agt_frame_begin. The returned handle belongs to
 * the calling thread (the loop thread never touches it). On a host without
 * the pixel-window mechanism this returns AGT_UNSUPPORTED; any other
 * failure is AGT_FAILED.
 *
 * Contract (milestone 22): on macOS this always returns AGT_UNSUPPORTED
 * with code "unsupported_platform" - AppKit requires the window/event loop
 * on the main thread, while this ABI hosts it on a library-private thread.
 * No thread is started and no retry can ever succeed, so treat the status
 * as permanent, not transient. AGT_CAP_WINDOW_HOST reports AGT_UNSUPPORTED
 * on macOS for the same reason. */
agt_status agt_window_open           (const agt_window_spec*, agt_window_t* out);

/* Pop the next event into *out, waiting up to timeout_ms. Timeout returns
 * AGT_FAILED with code "timeout"; a closed window with an empty queue
 * returns AGT_FAILED with code "closed". */
agt_status agt_window_poll_event     (agt_window_t, agt_event* out, uint32_t timeout_ms);

/* Fetch the text carried by the most recently polled event (IME
 * preedit/commit; never truncated into the POD record). Two-stage: call with
 * cap == 0 to learn the required byte count (*out_len), then allocate and
 * call again. With no pending text returns AGT_OK and *out_len == 0. On
 * insufficient capacity returns AGT_FAILED with code "buffer_too_small" and
 * writes the required byte count into *out_len. */
agt_status agt_window_event_text     (agt_window_t, uint8_t* buf, size_t cap, size_t* out_len);

/* Ask the loop thread to schedule a redraw. The next render() publishes a
 * fresh frame for agt_frame_begin. */
agt_status agt_window_request_redraw (agt_window_t);

/* Rendezvous half of the frame protocol: wait (up to timeout_ms) for the
 * loop thread's render() to publish a frame, then fill *out. Timeout
 * returns AGT_FAILED with code "timeout" (never AGT_UNSUPPORTED); calling
 * again while a previous frame is un-committed returns AGT_FAILED with code
 * "frame_pending". */
agt_status agt_frame_begin           (agt_window_t, agt_frame_desc* out, uint32_t timeout_ms);

/* Release the pending frame exactly once per frame: wake the loop thread so
 * it presents the pixels the caller wrote. Without a pending frame returns
 * AGT_FAILED with code "no_frame". */
agt_status agt_frame_commit          (agt_window_t);

/* Last known window geometry (physical pixels + scale factor). Before the
 * first geometry event / render this returns AGT_FAILED with code
 * "no_geometry". */
agt_status agt_window_metrics        (agt_window_t, uint32_t* w, uint32_t* h, double* scale);

/* Close the window and release the handle; must be called exactly once.
 * Wakes any caller blocked in agt_frame_begin / agt_window_poll_event and
 * lets the loop thread escape its rendezvous wait even if a taken frame was
 * never committed, so close never hangs. */
void       agt_window_close         (agt_window_t);

/* --- screenshot (milestone 4) --------------------------------------- */

/* Encode a caller-owned little-endian 0x00RRGGBB framebuffer as a PNG at
 * `path`. `pixel_count` must equal width*height, and both dimensions must be
 * >= 1, or AGT_FAILED with code "bad_dimensions" is returned. Other failures:
 * NULL/non-UTF-8 `path` -> "bad_path", NULL `pixels` -> "bad_pointer", side
 * > 16384 or pixel count > 64 Mi -> "frame_too_large", platform error ->
 * "screenshot_failed". Cropping is not supported in this version (the whole
 * buffer is always encoded). */
agt_status agt_screenshot_write_png(const char* path, const uint32_t* pixels,
                                    size_t pixel_count, uint32_t width,
                                    uint32_t height);

/* Capture a native window (or its strict client-area rectangle) to a PNG at
 * `path`. `native_window` is the platform window handle as intptr_t;
 * 0 -> AGT_FAILED with code "bad_handle". `area_kind` 0 = whole window,
 * 1 = client rectangle given by left/top/width/height; anything else ->
 * "bad_area". Platform failure -> "screenshot_failed". */
agt_status agt_screenshot_capture_window(intptr_t native_window, const char* path,
                                         int32_t area_kind, int32_t left,
                                         int32_t top, int32_t width,
                                         int32_t height);

/* --- process (milestone 5) ------------------------------------------ */

/* Single process record. `name` is UTF-8 and is not NUL-terminated by the
 * library; use `name_len` for its length. When the original executable name
 * exceeds 64 bytes it is truncated at a UTF-8 character boundary (a
 * multi-byte character is never split) and `name_truncated` is set to 1. */
typedef struct {
    uint32_t id;
    uint32_t parent_id;
    uint8_t  name[64];
    uint32_t name_len;       /* bytes actually written into name (<= 64) */
    uint32_t name_truncated; /* 1 when the original name exceeded 64 bytes */
} agt_process_info;

/* Enumerate live processes into a caller-allocated array (two-stage, spec 3.4):
 *   cap sufficient   -> AGT_OK, *out_count = records written
 *   cap insufficient -> AGT_FAILED{code="buffer_too_small"},
 *                      *out_count = required count
 * cap == 0 with buf == NULL is a legal "how big?" probe. NULL out_count
 * (or NULL buf with cap > 0) -> AGT_FAILED{code="bad_pointer"}; platform
 * failure -> AGT_FAILED{code="process_failed"}. */
agt_status agt_process_list(agt_process_info* buf, size_t cap, size_t* out_count);

/* Terminate the given process by pid. pid == 0 ->
 * AGT_FAILED{code="bad_pid"}; platform failure ->
 * AGT_FAILED{code="process_failed"}. */
agt_status agt_process_kill(uint32_t pid);

/* pid of the current process. Never fails. */
uint32_t   agt_process_self(void);

/* --- accessibility tree (milestone 6) ------------------------------- */

/* Fixed-size node record from the thread-local snapshot produced by
 * agt_a11y_tree_snapshot. Path ids and parent ids are truncated at a UTF-8
 * character boundary when longer than 64 bytes; truncated fields set the
 * matching *_truncated flag. Variable strings (role, name, text, action
 * names) are fetched with agt_a11y_node_string / agt_a11y_node_action_name. */
typedef struct {
    int32_t  bounds_x, bounds_y, bounds_width, bounds_height;
    uint8_t  id[64];
    uint32_t id_len;
    uint32_t id_truncated;
    uint8_t  parent_id[64];
    uint32_t parent_id_len;
    uint32_t parent_id_truncated;
    uint8_t  has_parent; /* 0/1 */
    uint32_t actions_count;
} agt_a11y_node;

typedef enum {
    AGT_A11Y_META_BACKEND = 0,
    AGT_A11Y_META_ROOT_ID = 1
} agt_a11y_meta_field;

typedef enum {
    AGT_A11Y_STR_ROLE = 0,
    AGT_A11Y_STR_NAME = 1,
    AGT_A11Y_STR_TEXT = 2,
    AGT_A11Y_STR_STATES = 3
} agt_a11y_string_kind;

typedef enum {
    AGT_A11Y_ACTION_CLICK = 0,
    AGT_A11Y_ACTION_FOCUS = 1
} agt_a11y_action_kind;

/* Capture a flattened accessibility tree for the host OS accessibility stack
 * (Windows UIA / macOS AX / Linux AT-SPI2 behind the platform adapter).
 * `window_handle` 0 observes all application roots; a non-zero native window
 * handle filters to that window's owning process. Replaces any prior snapshot
 * on this thread. *out_node_count receives the node count. Returns
 * AGT_UNSUPPORTED when the mechanism is absent on this build/host. */
agt_status agt_a11y_tree_snapshot(intptr_t window_handle, size_t* out_node_count);

/* Fetch snapshot metadata (backend label or root id). Two-stage buffer
 * protocol identical to agt_window_event_text. Valid only until the next
 * agt_a11y_tree_snapshot on this thread. */
agt_status agt_a11y_tree_meta_string(int32_t field, uint8_t* buf, size_t cap,
                                     size_t* out_len);

/* Copy the node at `index` (0 .. node_count-1) into *out. Out of range ->
 * AGT_FAILED{code="bad_index"}. No snapshot -> AGT_FAILED{code="no_snapshot"}. */
agt_status agt_a11y_tree_node(size_t index, agt_a11y_node* out);

/* Fetch a variable-length string for a node. Two-stage buffer protocol.
 * AGT_A11Y_STR_TEXT returns AGT_OK with *out_len == 0 when the node has no
 * text. Invalid field -> AGT_FAILED{code="bad_field"}. */
agt_status agt_a11y_node_string(size_t node_index, agt_a11y_string_kind kind,
                                uint8_t* buf, size_t cap, size_t* out_len);

/* Fetch an action name for a node. Two-stage buffer protocol. Out of range ->
 * AGT_FAILED{code="bad_index"}. */
agt_status agt_a11y_node_action_name(size_t node_index, size_t action_index,
                                     uint8_t* buf, size_t cap, size_t* out_len);

/* Perform click or focus on `node_id` (NUL-terminated UTF-8 child-index path,
 * e.g. "/0/2/5") without requiring a prior snapshot. `window_handle` uses the
 * same filter as agt_a11y_tree_snapshot. Returns AGT_UNSUPPORTED when the
 * mechanism is absent; resolution/actuation failures -> AGT_FAILED with typed
 * codes such as "a11y_node_not_found". */
agt_status agt_a11y_node_perform(intptr_t window_handle, const char* node_id,
                                   agt_a11y_action_kind action);

/* Write UTF-8 text through the host accessibility text interface
 * (Linux: AT-SPI EditableText SetTextContents / InsertText). `node_id`
 * is a NUL-terminated UTF-8 child-index path. `text == NULL` with
 * `len > 0` -> AGT_FAILED{code="bad_pointer"}; non-UTF-8 -> "bad_encoding".
 * A node that does not expose a writeable text interface ->
 * AGT_FAILED{code="a11y_text_unavailable"}. Never injects keystrokes. */
agt_status agt_a11y_node_set_text(intptr_t window_handle, const char* node_id,
                                  const uint8_t* text, size_t len);

/* --- clipboard (milestone 8) ---------------------------------------- */

/* Publish UTF-8 text. `text == NULL`, or a slice that is not valid UTF-8,
 * returns AGT_FAILED with code "bad_text". A platform failure (for example
 * no clipboard in this session) returns AGT_FAILED with code
 * "clipboard_failed". */
agt_status agt_clipboard_set_text(const uint8_t* text, size_t len);

/* Read UTF-8 clipboard text (two-stage, spec 3.4):
 *   cap sufficient   -> AGT_OK, *out_len = bytes written
 *   cap insufficient -> AGT_FAILED{code="buffer_too_small"},
 *                       *out_len = required bytes
 *   no Unicode text  -> AGT_OK with *out_len = 0
 * NULL out_len (or NULL buf with cap > 0) ->
 * AGT_FAILED{code="bad_pointer"}; platform failure ->
 * AGT_FAILED{code="clipboard_failed"}. Reads are internally capped (1 MiB
 * ceiling); a payload that exceeds the ceiling is reported as
 * "clipboard_failed" rather than delivered torn mid-character. */
agt_status agt_clipboard_get_text(uint8_t* buf, size_t cap, size_t* out_len);

/* 1 when the clipboard currently holds Unicode text, 0 otherwise. Never
 * fails. */
int32_t    agt_clipboard_has_text(void);

/* --- parent console (milestone 9) ------------------------------------ */

/* Write UTF-8 text to the parent console's stdout/stderr.
 *   text == NULL (with len > 0), or a slice that is not valid UTF-8
 *     -> AGT_FAILED with code "bad_text"
 *   no writable parent console -> AGT_UNSUPPORTED
 *     (the environment lacks the mechanism; intentionally NOT AGT_FAILED,
 *      see spec 3.1 - the two are never merged)
 *   write succeeded -> AGT_OK
 * len == 0 is legal input: an empty line is written and the platform result
 * is mapped as above. */
agt_status agt_parent_console_write_stdout(const uint8_t* text, size_t len);
agt_status agt_parent_console_write_stderr(const uint8_t* text, size_t len);

/* --- runtime environment (milestone 10) ------------------------------ */

/* User config directory (UTF-8), two-stage (spec 3.4):
 *   cap sufficient   -> AGT_OK, *out_len = bytes written
 *   cap insufficient -> AGT_FAILED{code="buffer_too_small"},
 *                       *out_len = required bytes
 * NULL out_len (or NULL buf with cap > 0) ->
 * AGT_FAILED{code="bad_pointer"}; platform failure ->
 * AGT_FAILED{code="runtime_failed"}; a path that is not valid UTF-8 ->
 * AGT_FAILED{code="bad_encoding"} (never lossy-replaced). */
agt_status agt_runtime_user_config_dir(uint8_t* buf, size_t cap, size_t* out_len);

/* Default terminal shell (UTF-8), two-stage (spec 3.4). Same status mapping
 * as agt_runtime_user_config_dir; never fails on a built library (the
 * platform always has a fallback shell). */
agt_status agt_runtime_default_shell(uint8_t* buf, size_t cap, size_t* out_len);

/* 1 when the process environment contains the ASCII variable `name`, 0
 * otherwise. `name == NULL` or a non-UTF-8 slice returns 0; this is a
 * query, not a fallible operation, so it never sets the error record. */
int32_t    agt_runtime_env_present(const uint8_t* name, size_t len);

/* Number of command-line arguments (excluding the image name). NULL
 * out_count -> AGT_FAILED{code="bad_pointer"}; platform failure ->
 * AGT_FAILED{code="runtime_failed"}. */
agt_status agt_runtime_arg_count(size_t* out_count);

/* Command-line argument `index` (UTF-8, excluding the image name),
 * two-stage (spec 3.4). index out of range ->
 * AGT_FAILED{code="bad_index"}; platform failure ->
 * AGT_FAILED{code="runtime_failed"}. */
agt_status agt_runtime_arg(size_t index, uint8_t* buf, size_t cap, size_t* out_len);

/* --- platform contract: macOS window host ----------------------------- */

/* The library-private window-loop thread model is validated on Windows only
 * (the message pump belongs to the creating thread). macOS is a hard
 * contract, not a limitation: AppKit requires the window/event loop on the
 * main thread, and this ABI hosts it on a library-private thread, so on
 * macOS agt_window_open always returns AGT_UNSUPPORTED (code
 * "unsupported_platform") and AGT_CAP_WINDOW_HOST reports AGT_UNSUPPORTED.
 * A main-thread host for macOS is left for a later milestone. */

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* AGENTERM_AGENT_ABI_H */
