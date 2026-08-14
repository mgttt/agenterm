/*
 * agenterm_plain_window.c -- a minimal native window OWNED BY THIS CHILD
 * process, deliberately independent of libagenterm.
 *
 * Milestone 70: the `agt_native_window_*` exports must be validated against
 * a window that belongs to a process WE spawned and can kill, never against
 * the ABI's own window handle from agt_window_open. The header is explicit
 * (include/agenterm.h): native-window operations act on raw OS handles
 * obtained from agt_window_enumerate, NEVER on the ABI's window handle
 * (agt_window_close owns that one; the two are unrelated).
 *
 * This probe exists so crates/agenterm-abi/tests/native_window_ops.rs can:
 *
 *   - spawn it as a child process;
 *   - wait for the `ready <hwnd>` line (flushed immediately after the window
 *     is shown, so the parent never guesses with a sleep);
 *   - find the window by title (which embeds our pid) + process id;
 *   - run rect / move / set_topmost / show / close against the handle, then
 *     let WM_CLOSE (posted by agt_native_window_close) end us with exit
 *     code 0 -- the expected terminal state of this round.
 *
 * Windows: a plain top-level window created with RegisterClassW +
 * CreateWindowExW, shown with SW_SHOWNOACTIVATE, pumping GetMessageW so it
 * responds to WM_GETTEXT (otherwise enumeration cannot read the title), and
 * exiting on WM_CLOSE / WM_DESTROY with exit code 0.
 *
 * Non-Windows: main prints one line and exits 0 -- Linux/macOS CI runners
 * are headless and this round only targets Windows.
 *
 * Compiles warning-free under MSVC /W4 /WX and gcc/clang -Wall -Wextra
 * -Werror.
 */
#ifdef _WIN32

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <stdio.h>

#if defined(_MSC_VER)
#pragma comment(lib, "user32.lib")
#endif

#define CLASS_NAME L"AgentermPlainWindowProbeClass"
#define ID_FIXTURE_LABEL 1001
#define ID_FIXTURE_EDIT 1002
#define ID_FIXTURE_CLOSE 1003

/* WM_CLOSE (posted by agt_native_window_close) or WM_DESTROY ends the
 * message loop; exit code 0 is the expected terminal state for the parent
 * test. */
static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
        case WM_COMMAND:
            if (LOWORD(wParam) == ID_FIXTURE_CLOSE &&
                HIWORD(wParam) == BN_CLICKED) {
                DestroyWindow(hwnd);
                return 0;
            }
            return DefWindowProcW(hwnd, msg, wParam, lParam);
        case WM_CLOSE:
            DestroyWindow(hwnd);
            return 0;
        case WM_DESTROY:
            PostQuitMessage(0);
            return 0;
        default:
            return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}

int main(void) {
    HINSTANCE inst = GetModuleHandleW(NULL);

    WNDCLASSW wc;
    ZeroMemory(&wc, sizeof(wc));
    wc.lpfnWndProc = WndProc;
    wc.hInstance = inst;
    wc.lpszClassName = CLASS_NAME;
    wc.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    if (RegisterClassW(&wc) == 0) {
        fprintf(stderr, "agenterm_plain_window: RegisterClassW failed (%lu)\n",
                (unsigned long)GetLastError());
        return 1;
    }

    /* Title embeds our pid so the parent can match on BOTH the title and
     * process_id == the pid of the child it spawned. */
    wchar_t title[80];
    int n = swprintf(title, 80, L"agenterm-plain-window-probe-%lu",
                     (unsigned long)GetCurrentProcessId());
    if (n < 0 || (size_t)n >= 80) {
        fprintf(stderr, "agenterm_plain_window: title buffer too small\n");
        return 1;
    }

    /* Small fixed geometry (320x200 at 200,200), shown WITHOUT activation so
     * the probe never steals focus from the user. */
    HWND hwnd = CreateWindowExW(0, CLASS_NAME, title, WS_OVERLAPPEDWINDOW,
                                200, 200, 320, 200, NULL, NULL, inst, NULL);
    if (hwnd == NULL) {
        fprintf(stderr, "agenterm_plain_window: CreateWindowExW failed (%lu)\n",
                (unsigned long)GetLastError());
        return 1;
    }

    /* Keep the STATIC immediately before the EDIT in creation/tab order.
     * The standard Win32 accessibility proxy uses that label as the edit's
     * stable accessible Name while the edit contents remain its Value. */
    HWND label = CreateWindowExW(
        0, L"STATIC", L"Fixture Field", WS_CHILD | WS_VISIBLE | SS_LEFT,
        20, 20, 260, 20, hwnd, (HMENU)(INT_PTR)ID_FIXTURE_LABEL, inst, NULL);
    HWND edit = CreateWindowExW(
        WS_EX_CLIENTEDGE, L"EDIT", L"fixture-initial",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
        20, 44, 260, 24, hwnd, (HMENU)(INT_PTR)ID_FIXTURE_EDIT, inst, NULL);
    HWND close_button = CreateWindowExW(
        0, L"BUTTON", L"Fixture Close",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
        20, 84, 120, 28, hwnd, (HMENU)(INT_PTR)ID_FIXTURE_CLOSE, inst, NULL);
    if (label == NULL || edit == NULL || close_button == NULL) {
        fprintf(stderr,
                "agenterm_plain_window: child control creation failed (%lu)\n",
                (unsigned long)GetLastError());
        DestroyWindow(hwnd);
        return 1;
    }
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    /* The parent test waits for this line before touching the window. The
     * flush is what makes the handshake deterministic -- no sleep guessing. */
    printf("ready %lld\n", (long long)(intptr_t)hwnd);
    fflush(stdout);

    MSG msg = {0};
    int r;
    while ((r = GetMessageW(&msg, NULL, 0, 0)) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    if (r < 0) {
        fprintf(stderr, "agenterm_plain_window: GetMessageW failed (%lu)\n",
                (unsigned long)GetLastError());
        return 1;
    }
    /* WM_QUIT's wParam is the value PostQuitMessage got -- 0 here. */
    return (int)msg.wParam;
}

#else /* !_WIN32 */

#include <stdio.h>

int main(void) {
    /* Linux/macOS CI runners are headless; this round only targets Windows.
     * The probe still compiles and exits 0 so the file stays portable. */
    printf("agenterm_plain_window: non-Windows build; nothing to do\n");
    return 0;
}

#endif
