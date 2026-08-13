/*
 * agenterm_window.c -- real C consumer driving the window/frame rendezvous
 * (crates/agenterm-abi).
 *
 * Milestone 31: the window + frame "control-inversion rendezvous" is the most
 * delicate part of the ABI. The platform is a blocking callback loop that the
 * library hosts on a library-private thread; control comes back to the caller
 * through agt_frame_begin / agt_frame_commit. Until this probe it was only
 * ever driven from Rust tests -- this is the C consumer's first full drive:
 *
 *   1. agt_window_open (320x200, no_activate=1)
 *   2. three frames: agt_frame_begin -> fill pixels by stride ->
 *      agt_frame_commit, printing width/height/stride_px and the wall-clock
 *      cost of each frame
 *   3. one agt_window_poll_event (timeout 0), printing the event kind
 *   4. agt_window_metrics, printing width/height/scale
 *   5. agt_window_close
 *
 * Two rules every C consumer must follow, demonstrated here:
 *
 *   - stride: pixels are row-major by stride_px, which may be larger than
 *     width -- never index as if the buffer were tightly packed;
 *   - pointer lifetime: the frame's `pixels` pointer is valid ONLY between a
 *     successful agt_frame_begin and the matching agt_frame_commit; after
 *     commit it must never be stored or dereferenced.
 *
 * The platform renders on demand: after commit no further render() happens
 * until a redraw is requested, so each frame except the last is followed by
 * agt_window_request_redraw to schedule the next one.
 *
 * agt_window_open returning AGT_UNSUPPORTED (headless CI, macOS) is an
 * explicit, permanent skip: print the reason and exit 0. AGT_FAILED or any
 * other contract violation is a hard failure: print the reason to stderr and
 * exit 1. Compiles warning-free under MSVC /W4 /WX and gcc/clang -Wall
 * -Wextra -Werror.
 */
#include "agenterm.h"

#include <stdio.h>
#include <string.h>
#include <time.h>

/* Wall-clock millisecond timestamp (C11 timespec_get; VS2015+ and any
 * gcc/clang default mode). 0.0 on failure, which only makes the timing
 * printout degenerate, never the rendezvous itself. */
static double now_ms(void) {
    struct timespec ts;
    if (timespec_get(&ts, TIME_UTC) == 0) {
        return 0.0;
    }
    return (double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1000000.0;
}

/* Print the thread-local error record for `what` to stderr. */
static void print_last_error(const char* what) {
    agt_error err;
    err.operation = NULL;
    err.code = NULL;
    err.message = NULL;
    if (agt_last_error(&err) != AGT_OK) {
        fprintf(stderr, "FAIL: %s: (agt_last_error failed)\n", what);
        return;
    }
    fprintf(stderr, "FAIL: %s: %s: %s\n", what,
            err.code != NULL ? err.code : "?", err.message != NULL ? err.message : "?");
}

int main(void) {
    /* --- open ---------------------------------------------------------- */
    agt_window_spec spec;
    spec.title = "agenterm-c-window-probe";
    spec.width = 320;
    spec.height = 200;
    spec.no_activate = 1;
    spec.ime_allowed = 0;

    agt_window_t window = NULL;
    agt_status st = agt_window_open(&spec, &window);
    if (st == AGT_UNSUPPORTED) {
        /* Headless CI and macOS cannot host the pixel window (AppKit needs
         * the main thread). Permanent, documented, and NOT a failure: print
         * the reason and exit 0 so the test gate treats it as a skip. */
        agt_error err;
        err.operation = NULL;
        err.code = NULL;
        err.message = NULL;
        if (agt_last_error(&err) != AGT_OK) {
            printf("SKIP: agt_window_open unsupported (no error record)\n");
        } else {
            printf("SKIP: agt_window_open unsupported: %s: %s\n",
                   err.code != NULL ? err.code : "?",
                   err.message != NULL ? err.message : "?");
        }
        return 0;
    }
    if (st != AGT_OK) {
        print_last_error("agt_window_open");
        return 1;
    }
    if (window == NULL) {
        fprintf(stderr, "FAIL: agt_window_open returned a NULL handle\n");
        return 1;
    }
    printf("window opened (320x200 logical, no_activate)\n");

    /* --- three frames -------------------------------------------------- */
    /* Known pattern: one solid color per frame, visible in a screenshot. */
    static const uint32_t fill[3] = {0x00112233u, 0x00445566u, 0x00778899u};
    int frame;
    for (frame = 0; frame < 3; frame++) {
        agt_frame_desc desc;
        desc.pixels = NULL;
        desc.width = 0;
        desc.height = 0;
        desc.stride_px = 0;

        double t0 = now_ms();
        st = agt_frame_begin(window, &desc, 2000);
        if (st != AGT_OK) {
            print_last_error("agt_frame_begin");
            agt_window_close(window);
            return 1;
        }
        if (desc.pixels == NULL || desc.width == 0 || desc.height == 0) {
            fprintf(stderr, "FAIL: agt_frame_begin returned an invalid frame\n");
            agt_window_close(window);
            return 1;
        }

        /* Fill the visible area, row-major by stride_px. stride_px may be
         * larger than width, so the row base must be computed from the
         * stride, never from width -- assuming a packed buffer is the exact
         * bug this probe exists to prevent. */
        uint32_t row, col;
        for (row = 0; row < desc.height; row++) {
            uint32_t* line = desc.pixels + (size_t)row * (size_t)desc.stride_px;
            for (col = 0; col < desc.width; col++) {
                line[col] = fill[frame];
            }
        }

        st = agt_frame_commit(window);
        if (st != AGT_OK) {
            print_last_error("agt_frame_commit");
            agt_window_close(window);
            return 1;
        }
        double t1 = now_ms();
        printf("frame[%d] %ux%u stride_px=%u fill=0x%08x in %.1f ms\n",
               frame, (unsigned)desc.width, (unsigned)desc.height,
               (unsigned)desc.stride_px, (unsigned)fill[frame], t1 - t0);

        /* Schedule the next render: the platform only renders on demand. */
        if (frame < 2) {
            st = agt_window_request_redraw(window);
            if (st != AGT_OK) {
                print_last_error("agt_window_request_redraw");
                agt_window_close(window);
                return 1;
            }
        }
    }

    /* --- one event poll (timeout 0) ------------------------------------ */
    agt_event ev;
    memset(&ev, 0, sizeof(ev));
    st = agt_window_poll_event(window, &ev, 0);
    if (st == AGT_OK) {
        printf("poll_event: kind=%u generation=%llu\n", (unsigned)ev.kind,
               (unsigned long long)ev.generation);
    } else if (st == AGT_FAILED) {
        /* timeout 0 with an empty queue is the documented "no event" result
         * (code "timeout") -- also not a failure. */
        agt_error err;
        err.operation = NULL;
        err.code = NULL;
        err.message = NULL;
        if (agt_last_error(&err) != AGT_OK) {
            printf("poll_event: none (unreadable error record)\n");
        } else {
            printf("poll_event: none (%s)\n", err.code != NULL ? err.code : "?");
        }
    } else {
        fprintf(stderr, "FAIL: agt_window_poll_event returned unexpected status %d\n",
                (int)st);
        agt_window_close(window);
        return 1;
    }

    /* --- metrics ------------------------------------------------------- */
    uint32_t w = 0, h = 0;
    double scale = 0.0;
    st = agt_window_metrics(window, &w, &h, &scale);
    if (st != AGT_OK) {
        print_last_error("agt_window_metrics");
        agt_window_close(window);
        return 1;
    }
    printf("metrics: %ux%u scale=%.2f\n", (unsigned)w, (unsigned)h, scale);

    /* --- close --------------------------------------------------------- */
    agt_window_close(window);
    printf("window closed\n");
    return 0;
}
