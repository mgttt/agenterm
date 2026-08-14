/*
 * agenterm_input_target_window.c -- a child-owned native window that REPORTS
 * every injected input it receives, deliberately independent of libagenterm.
 *
 * Milestone 72: the four `agt_input_*` exports
 * (agt_input_pointer_move / agt_input_pointer_click / agt_input_type_text /
 * agt_input_send_keys) had zero success-path evidence: null_sweep.rs only
 * swept NULL pointers and dylib_load.rs only exercised an invalid button
 * (99) and NULL. This probe is the child-owned receiver so
 * crates/agenterm-abi/tests/input_inject_success.rs can prove the exports
 * actually DO something -- by design ONLY when the test's CI job sets
 * AGENTERM_ALLOW_INPUT_INJECTION=1, and ONLY against this child's window.
 *
 * Contract with the parent test:
 *
 *   - spawn it as a child process;
 *   - wait for the `ready <hwnd>` line (flushed immediately after the window
 *     is shown and brought to the foreground, so the parent never guesses
 *     with a sleep);
 *   - find the window by title (which embeds our pid) + process id;
 *   - inject pointer moves/clicks and keyboard events via the ABI;
 *   - read back one stdout line per received message, each flushed so the
 *     parent can poll without buffering surprises:
 *       WM_CHAR       -> "char <codepoint>"
 *       WM_LBUTTONDOWN-> "lbuttondown <x> <y>"   (client-area coords)
 *       WM_MOUSEMOVE  -> "mousemove <x> <y>"     (client-area coords)
 *       WM_KEYDOWN    -> "keydown <vk>"
 *   - end the probe with agt_native_window_close (WM_CLOSE) -> exit 0.
 *
 * Windows: a plain top-level window created with RegisterClassW +
 * CreateWindowExW, SHOWN AND ACTIVATED (this probe must hold the foreground
 * so injected input lands here and nowhere else), pumping GetMessageW so
 * TranslateMessage turns the injected key events into WM_CHAR.
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
#include <imm.h>

#include <stdio.h>

#if defined(_MSC_VER)
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "imm32.lib")
#elif defined(__MINGW32__)
/* mingw-w64 gcc links the core win32 libs by default but not imm32. */
#pragma comment(lib, "imm32")
#endif

#define CLASS_NAME L"AgentermInputTargetWindowClass"

/* One flushed stdout line per received message (see the contract above).
 * WM_CLOSE (posted by agt_native_window_close) or WM_DESTROY ends the
 * message loop; exit code 0 is the expected terminal state for the parent
 * test. */
static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
        case WM_CHAR:
            printf("char %u\n", (unsigned)wParam);
            fflush(stdout);
            return 0;
        case WM_LBUTTONDOWN: {
            int x = (int)(short)LOWORD(lParam);
            int y = (int)(short)HIWORD(lParam);
            printf("lbuttondown %d %d\n", x, y);
            fflush(stdout);
            return 0;
        }
        case WM_MOUSEMOVE: {
            int x = (int)(short)LOWORD(lParam);
            int y = (int)(short)HIWORD(lParam);
            printf("mousemove %d %d\n", x, y);
            fflush(stdout);
            return 0;
        }
        case WM_KEYDOWN:
            printf("keydown %u\n", (unsigned)wParam);
            fflush(stdout);
            return 0;
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
        fprintf(stderr, "agenterm_input_target_window: RegisterClassW failed (%lu)\n",
                (unsigned long)GetLastError());
        return 1;
    }

    /* Title embeds our pid so the parent can match on BOTH the title and
     * process_id == the pid of the child it spawned. */
    wchar_t title[80];
    int n = swprintf(title, 80, L"agenterm-input-target-%lu",
                     (unsigned long)GetCurrentProcessId());
    if (n < 0 || (size_t)n >= 80) {
        fprintf(stderr, "agenterm_input_target_window: title buffer too small\n");
        return 1;
    }

    /* Small fixed geometry (320x200 at 200,200). SW_SHOW activates the
     * window; SetForegroundWindow + SetFocus make THIS window the recipient
     * of the injected input (the parent re-verifies the foreground right
     * before every injection and refuses to proceed otherwise). */
    HWND hwnd = CreateWindowExW(0, CLASS_NAME, title, WS_OVERLAPPEDWINDOW,
                                200, 200, 320, 200, NULL, NULL, inst, NULL);
    if (hwnd == NULL) {
        fprintf(stderr, "agenterm_input_target_window: CreateWindowExW failed (%lu)\n",
                (unsigned long)GetLastError());
        return 1;
    }
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(hwnd);

    /* Disable IME for THIS window only: an active input-method editor (e.g.
     * a Chinese IME on a developer's desktop) consumes injected virtual keys
     * before they reach the window - SendInput's VK_A turns into an
     * IME-converted VK_PACKET (0xE5/0xE7) instead of WM_KEYDOWN 65 - so the
     * parent test must be able to observe the raw VK. ImmAssociateContext
     * with NULL removes the IME association of this one window; it changes
     * nothing outside it. */
    ImmAssociateContext(hwnd, NULL);

    /* The parent test waits for this line before injecting anything. The
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
        fprintf(stderr, "agenterm_input_target_window: GetMessageW failed (%lu)\n",
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
    printf("agenterm_input_target_window: non-Windows build; nothing to do\n");
    return 0;
}

#endif
