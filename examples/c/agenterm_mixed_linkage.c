/*
 * agenterm_mixed_linkage.c -- milestone 59 measurement instrument: link
 * libagenterm STATICALLY into this executable while the SAME process ALSO
 * loads the dynamic library (LoadLibrary / dlopen), then measure whether the
 * two copies' per-process state (the thread-local LAST_ERROR / MSG_BUF /
 * A11Y_SNAPSHOT in crates/agenterm-abi/src/lib.rs) is independent.
 *
 * This probe is a MEASURING INSTRUMENT, not an assertion engine. The real
 * per-platform behavior cannot be predicted, only measured -- Windows
 * modules are independent, ELF interposition may collapse the two copies on
 * Linux, and macOS two-level namespace behaves differently again. The
 * program therefore ALWAYS exits 0 and prints the observed facts to stdout;
 * the assertions that become possible once all three platforms' data is in
 * are deliberately NOT written here.
 *
 * Everything touched is safe and read-only. The deliberate failures are
 * NULL-pointer contract violations that are validated BEFORE any platform
 * access: agt_process_list(NULL, 0, NULL) -> AGT_FAILED{code="bad_pointer",
 * "out_count is null"} and agt_process_list(NULL, 1, &n) -> AGT_FAILED
 * {code="bad_pointer", "buf is null"}. This probe NEVER calls agt_input_*,
 * agt_native_window_*, agt_process_kill, agt_clipboard_set_text or
 * agt_screenshot_*.
 *
 * The dynamic-copy path is resolved through LoadLibrary/GetProcAddress
 * (Windows) or dlopen/dlsym (Unix); the cdylib path is passed as argv[1]
 * (a fallback default name is used when argv[1] is absent). Compiles
 * warning-free under MSVC /W4 /WX and gcc/clang -Wall -Wextra -Werror.
 * The exact build/link command lives in
 * crates/agenterm-abi/tests/mixed_linkage.rs.
 */
#include "agenterm.h"

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
typedef HMODULE dyn_handle_t;
#pragma warning(push)
/* C4152 (function/data pointer conversion) fires on the dlsym-style
 * LoadLibrary/GetProcAddress plumbing below -- going through void* is the
 * standard POSIX/Windows dynamic-lookup idiom and is deliberate here (it is
 * also what keeps the same source warning-free under gcc/clang, where a
 * direct FARPROC -> fn-pointer cast would trip -Wcast-function-type). */
#pragma warning(disable : 4152)
#else
#include <dlfcn.h>
typedef void* dyn_handle_t;
#endif

/* --- dynamic-copy function-pointer surface (subset this probe uses) ----- */
typedef uint32_t (*abi_version_fn)(void);
typedef uint32_t (*process_self_fn)(void);
typedef agt_status (*process_list_fn)(agt_process_info*, size_t, size_t*);
typedef agt_status (*last_error_fn)(agt_error*);
typedef agt_status (*a11y_snapshot_fn)(intptr_t, size_t*);
typedef agt_status (*a11y_tree_node_fn)(size_t, agt_a11y_node*);

typedef struct {
    dyn_handle_t handle;
    abi_version_fn abi_version;
    process_self_fn process_self;
    process_list_fn process_list;
    last_error_fn last_error;
    a11y_snapshot_fn a11y_snapshot;
    a11y_tree_node_fn a11y_tree_node;
} dynamic_api;

#ifdef _WIN32
static void* dyn_resolve(dyn_handle_t handle, const char* name) {
    return (void*)GetProcAddress(handle, name);
}
#else
static void* dyn_resolve(dyn_handle_t handle, const char* name) {
    return dlsym(handle, name);
}
#endif

static const char* status_name(agt_status st) {
    switch (st) {
    case AGT_OK:
        return "AGT_OK";
    case AGT_UNSUPPORTED:
        return "AGT_UNSUPPORTED";
    case AGT_FAILED:
        return "AGT_FAILED";
    default:
        return "AGT_?";
    }
}

/* Open the dynamic library and resolve every symbol this probe needs.
 * Returns 0 on success. On any failure prints a line starting with the
 * [fatal] marker -- the Rust test turns that into a hard failure -- but the
 * probe itself still exits 0 (it is a measuring instrument). */
static int load_dynamic(dynamic_api* api, const char* path) {
    memset(api, 0, sizeof(*api));
#ifdef _WIN32
    api->handle = LoadLibraryA(path);
    if (api->handle == NULL) {
        printf("[fatal] dynamic load failed: LoadLibraryA(\"%s\") -> NULL "
               "(error %lu)\n",
               path, (unsigned long)GetLastError());
        return -1;
    }
#else
    api->handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (api->handle == NULL) {
        const char* why = dlerror();
        printf("[fatal] dynamic load failed: dlopen(\"%s\") -> NULL (%s)\n",
               path, why != NULL ? why : "no dlerror detail");
        return -1;
    }
#endif
#define RESOLVE(field, symbol)                                                     \
    do {                                                                           \
        api->field = (void*)dyn_resolve(api->handle, symbol);                      \
        if (api->field == NULL) {                                                  \
            printf("[fatal] dynamic symbol missing: %s\n", symbol);                \
            return -1;                                                             \
        }                                                                          \
    } while (0)
    RESOLVE(abi_version, "agt_abi_version");
    RESOLVE(process_self, "agt_process_self");
    RESOLVE(process_list, "agt_process_list");
    RESOLVE(last_error, "agt_last_error");
    RESOLVE(a11y_snapshot, "agt_a11y_tree_snapshot");
    RESOLVE(a11y_tree_node, "agt_a11y_tree_node");
#undef RESOLVE
    return 0;
}

/* Read a copy's thread-local error record into `out` as "op: code: message". */
static void read_last_error(last_error_fn read, char* out, size_t cap) {
    agt_error err;
    err.operation = NULL;
    err.code = NULL;
    err.message = NULL;
    agt_status st = read(&err);
    if (st != AGT_OK) {
        snprintf(out, cap, "<agt_last_error failed with %s>", status_name(st));
        return;
    }
    if (err.operation == NULL || err.code == NULL || err.message == NULL) {
        snprintf(out, cap, "<agt_last_error returned a NULL field>");
        return;
    }
    snprintf(out, cap, "%s: %s: %s", err.operation, err.code, err.message);
}

int main(int argc, char** argv) {
    const char* dyn_path = argc >= 2 ? argv[1] : NULL;

    printf("=== libagenterm mixed static+dynamic linkage probe (milestone 59) ===\n");
    printf("static  copy: linked into this executable from the static archive\n");
#ifdef _WIN32
    printf("dynamic copy: LoadLibrary'd from \"%s\"\n",
           dyn_path != NULL ? dyn_path : "agenterm.dll (default)");
#else
    printf("dynamic copy: dlopen'd from \"%s\"\n",
           dyn_path != NULL ? dyn_path : "libagenterm.so (default)");
#endif

    dynamic_api dyn;
    if (load_dynamic(&dyn, dyn_path != NULL ? dyn_path
#ifdef _WIN32
                                            : "agenterm.dll"
#else
                                            : "libagenterm.so"
#endif
            ) != 0) {
        printf("(dynamic copy unavailable -- cross-copy experiments skipped)\n");
        return 0;
    }

    /* --- identity: both copies must agree (same ABI, same process) ------ */
    uint32_t static_version = agt_abi_version();
    uint32_t dynamic_version = dyn.abi_version();
    uint32_t static_pid = agt_process_self();
    uint32_t dynamic_pid = dyn.process_self();
    printf("static_abi_version=0x%08x\n", (unsigned)static_version);
    printf("dynamic_abi_version=0x%08x\n", (unsigned)dynamic_version);
    printf("static_process_self=%u\n", (unsigned)static_pid);
    printf("dynamic_process_self=%u\n", (unsigned)dynamic_pid);

    /* --- baseline: each copy's error record before any deliberate failure */
    char buf[512];
    read_last_error(agt_last_error, buf, sizeof(buf));
    printf("static_initial_last_error=%s\n", buf);
    read_last_error(dyn.last_error, buf, sizeof(buf));
    printf("dynamic_initial_last_error=%s\n", buf);

    /* --- experiment A (control): static triggers, static reads ----------- */
    printf("\n--- experiment A (control): static copy triggers a failure, static "
           "copy reads ---\n");
    agt_status st_a = agt_process_list(NULL, 0, NULL);
    printf("static_trigger=agt_process_list(NULL,0,NULL) -> %s\n",
           status_name(st_a));
    read_last_error(agt_last_error, buf, sizeof(buf));
    printf("static_read_after_static_trigger=%s\n", buf);

    /* --- experiment B (the measurement): static triggered, DYNAMIC reads - */
    printf("\n--- experiment B (measurement): static copy triggered, DYNAMIC copy "
           "reads ---\n");
    read_last_error(dyn.last_error, buf, sizeof(buf));
    printf("dynamic_read_after_static_trigger=%s\n", buf);

    /* --- experiment C (control): dynamic triggers, dynamic reads --------- */
    printf("\n--- experiment C (control): dynamic copy triggers a DIFFERENT "
           "failure, dynamic copy reads ---\n");
    size_t dyn_need = 0;
    agt_status st_c = dyn.process_list(NULL, 1, &dyn_need);
    printf("dynamic_trigger=agt_process_list(NULL,1,&need) -> %s\n",
           status_name(st_c));
    read_last_error(dyn.last_error, buf, sizeof(buf));
    printf("dynamic_read_after_dynamic_trigger=%s\n", buf);

    /* --- experiment D (the reverse measurement): dynamic triggered, static
     * reads --------------------------------------------------------------- */
    printf("\n--- experiment D (measurement): dynamic copy triggered, STATIC copy "
           "reads ---\n");
    read_last_error(agt_last_error, buf, sizeof(buf));
    printf("static_read_after_dynamic_trigger=%s\n", buf);

    /* --- A11Y_SNAPSHOT bridge (best effort; see the printed verdict) ----- */
    printf("\n--- a11y snapshot bridge (best effort) ---\n");
    size_t a11y_count = 0;
    agt_status a11y_st = agt_a11y_tree_snapshot(0, &a11y_count);
    printf("static_a11y_snapshot=agt_a11y_tree_snapshot(0,&count) -> %s",
           status_name(a11y_st));
    if (a11y_st == AGT_UNSUPPORTED) {
        printf(" (a11y mechanism not wired on this host: no snapshot can be "
               "produced, so the A11Y_SNAPSHOT bridge cannot be probed here)\n");
    } else if (a11y_st == AGT_OK) {
        printf(" (static copy produced a %zu-node snapshot)\n", a11y_count);
        /* static wrote the snapshot: does the DYNAMIC copy's accessor see it? */
        agt_a11y_node node;
        memset(&node, 0, sizeof(node));
        agt_status nst = dyn.a11y_tree_node(0, &node);
        printf("dynamic_read_after_static_snapshot=agt_a11y_tree_node(0,&node) "
               "-> %s\n",
               status_name(nst));
        if (nst == AGT_FAILED) {
            read_last_error(dyn.last_error, buf, sizeof(buf));
            printf("dynamic_last_error_after_static_snapshot=%s\n", buf);
        }
        /* reverse: dynamic writes its own snapshot, static accessor reads. */
        size_t dyn_a11y_count = 0;
        agt_status dst = dyn.a11y_snapshot(0, &dyn_a11y_count);
        printf("dynamic_a11y_snapshot=agt_a11y_tree_snapshot(0,&count) -> %s",
               status_name(dst));
        if (dst == AGT_OK) {
            printf(" (dynamic copy produced a %zu-node snapshot)\n",
                   dyn_a11y_count);
            memset(&node, 0, sizeof(node));
            agt_status sst = agt_a11y_tree_node(0, &node);
            printf("static_read_after_dynamic_snapshot=agt_a11y_tree_node(0,&node) "
                   "-> %s\n",
                   status_name(sst));
            if (sst == AGT_FAILED) {
                read_last_error(agt_last_error, buf, sizeof(buf));
                printf("static_last_error_after_dynamic_snapshot=%s\n", buf);
            }
        } else {
            printf("\n");
        }
    } else {
        printf("\n");
    }

    printf("\n(probe complete: measured facts above, no assertions made)\n");
    return 0;
}

#ifdef _WIN32
#pragma warning(pop) /* C4152 */
#endif
