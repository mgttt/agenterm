//! Owner-attached **modeless** Win32 multiline editor.
//!
//! This adapter deliberately runs no message loop. An earlier version pumped
//! `GetMessageW` here, from inside the host's own application callback: the
//! host could not repaint, could not answer its control endpoint, and every
//! reentrant native event piled into the bounded deferred queue until it
//! overflowed and forced an exit. Creation, observation and dismissal are all
//! non-blocking now, and the host's existing loop delivers the messages.
//!
//! The dialog state lives on the heap rather than the creating stack frame for
//! the same reason: nothing keeps that frame alive once creation returns.

use std::{io, mem, panic::AssertUnwindSafe, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{EnableWindow, SetFocus},
        WindowsAndMessaging::{
            BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT,
            CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOVSCROLL, ES_MULTILINE,
            ES_WANTRETURN, GWLP_USERDATA, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
            HWND_TOP, IDC_ARROW, IsDialogMessageW, LoadCursorW, MSG, RegisterClassW, SW_SHOW,
            SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
            SetWindowPos, ShowWindow, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_NCCREATE, WM_SETFONT,
            WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_OVERLAPPED,
            WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::text_review::{TextReviewError, TextReviewPoll};

const ID_CONFIRM: u16 = 1;
const ID_CANCEL: u16 = 2;

thread_local! {
    /// The review currently accepting dialog navigation on this thread. The
    /// host loop consults it before translating a message so Tab, Escape and
    /// the default button behave, which `IsDialogMessageW` would otherwise only
    /// provide from inside a pump this adapter no longer owns.
    static ACTIVE_DIALOG: std::cell::Cell<HWND> = const { std::cell::Cell::new(ptr::null_mut()) };
}

/// Gives an open review first refusal on a message the host is about to
/// dispatch. Returns true when the review consumed it.
///
/// A host that never calls this still completes reviews through mouse input;
/// it only loses keyboard navigation inside them.
pub(crate) fn filter_dialog_message(message: &MSG) -> bool {
    let hwnd = ACTIVE_DIALOG.with(std::cell::Cell::get);
    if hwnd.is_null() {
        return false;
    }
    unsafe { IsDialogMessageW(hwnd, message) != 0 }
}

struct DialogState {
    done: bool,
    confirmed: bool,
    wake: Option<Box<dyn FnOnce()>>,
}

impl DialogState {
    /// Idempotent: a review reaches a terminal state once, however many
    /// messages report it, and wakes its host exactly once.
    fn finish(&mut self, confirmed: bool) {
        if self.done {
            return;
        }
        self.done = true;
        self.confirmed = confirmed;
        if let Some(wake) = self.wake.take() {
            // The host's wake path must not be able to unwind into a Win32
            // window procedure.
            let _ = std::panic::catch_unwind(AssertUnwindSafe(wake));
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn failed(code: &'static str, message: impl ToString) -> TextReviewError {
    TextReviewError::Failed {
        code: code.into(),
        message: message.to_string(),
    }
}

fn last_error(code: &'static str) -> TextReviewError {
    failed(code, io::Error::last_os_error())
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
        if message == WM_NCCREATE {
            let create = lparam as *const CREATESTRUCTW;
            if !create.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
            }
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
        match message {
            WM_COMMAND if !state.is_null() => {
                let id = (wparam & 0xffff) as u16;
                if id == ID_CONFIRM || id == ID_CANCEL {
                    (*state).finish(id == ID_CONFIRM);
                    return 0;
                }
            }
            // The title-bar close and any destroy path are dismissals, never
            // confirmations: text the human did not accept must not be sent.
            WM_CLOSE | WM_DESTROY if !state.is_null() => {
                (*state).finish(false);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }))
    .unwrap_or(0)
}

fn ensure_class() -> Result<Vec<u16>, TextReviewError> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    let class = wide("AgenTermPlatformTextReview");
    let registration = REGISTERED.get_or_init(|| {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let descriptor = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(dialog_proc),
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: (COLOR_WINDOW + 1) as _,
            lpszClassName: class.as_ptr(),
            ..unsafe { mem::zeroed() }
        };
        if unsafe { RegisterClassW(&descriptor) } == 0 {
            Err(io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    });
    registration
        .as_ref()
        .map_err(|message| failed("text_review_register_failed", message))?;
    Ok(class)
}

pub(crate) struct NativeTextReview {
    hwnd: HWND,
    edit: HWND,
    owner: HWND,
    /// Owns the state the window procedure writes through `GWLP_USERDATA`.
    /// Kept boxed and never moved for as long as `hwnd` exists.
    state: Box<DialogState>,
    finished: bool,
}

impl NativeTextReview {
    pub(crate) fn try_poll(&mut self) -> TextReviewPoll {
        if self.finished {
            return TextReviewPoll::Ready(None);
        }
        if !self.state.done {
            return TextReviewPoll::Pending;
        }
        let confirmed = self.state.confirmed;
        let text = confirmed.then(|| read_window_text(self.edit));
        self.dismiss();
        TextReviewPoll::Ready(text)
    }

    /// Restores the owner before destroying the review, so focus returns to a
    /// window that can accept it.
    fn dismiss(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        ACTIVE_DIALOG.with(|active| {
            if active.get() == self.hwnd {
                active.set(ptr::null_mut());
            }
        });
        unsafe {
            if !self.owner.is_null() {
                EnableWindow(self.owner, 1);
            }
            // Detach the state before the destroy path can re-enter the window
            // procedure through it.
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.hwnd);
            if !self.owner.is_null() {
                SetForegroundWindow(self.owner);
            }
        }
    }
}

impl Drop for NativeTextReview {
    fn drop(&mut self) {
        self.dismiss();
    }
}

fn read_window_text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let mut value = vec![0u16; usize::try_from(len).unwrap_or(0) + 1];
    let copied = unsafe { GetWindowTextW(hwnd, value.as_mut_ptr(), value.len() as i32) };
    value.truncate(usize::try_from(copied).unwrap_or(0));
    String::from_utf16_lossy(&value)
}

pub(crate) fn open_review(
    owner: Option<i64>,
    title: &str,
    prompt: &str,
    initial: &str,
    wake: Box<dyn FnOnce() + Send + 'static>,
) -> Result<NativeTextReview, TextReviewError> {
    // One review at a time per thread. Without this, a second review would
    // steal `ACTIVE_DIALOG` and the first would silently lose its keyboard
    // navigation while still disabling the owner.
    if !ACTIVE_DIALOG.with(std::cell::Cell::get).is_null() {
        return Err(failed(
            "text_review_already_open",
            "a text review is already open on this thread",
        ));
    }
    let class = ensure_class()?;
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(last_error("text_review_module_handle_failed"));
    }
    let owner = owner.map_or(ptr::null_mut(), |value| value as isize as HWND);
    let mut state = Box::new(DialogState {
        done: false,
        confirmed: false,
        wake: Some(wake),
    });
    let title = wide(title);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            660,
            440,
            owner,
            ptr::null_mut(),
            instance,
            (&raw mut *state).cast(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("text_review_create_failed"));
    }
    let create = |class_name: &str,
                  text: &str,
                  ex_style: u32,
                  style: u32,
                  x: i32,
                  y: i32,
                  width: i32,
                  height: i32,
                  id: u16| {
        let class_name = wide(class_name);
        let text = wide(text);
        unsafe {
            CreateWindowExW(
                ex_style,
                class_name.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                width,
                height,
                hwnd,
                usize::from(id) as _,
                instance,
                ptr::null_mut(),
            )
        }
    };
    let label = create("STATIC", prompt, 0, 0, 18, 16, 610, 24, 10);
    let edit = create(
        "EDIT",
        initial,
        WS_EX_CLIENTEDGE,
        WS_TABSTOP
            | WS_VSCROLL
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | ES_WANTRETURN as u32,
        18,
        46,
        610,
        310,
        11,
    );
    let confirm = create(
        "BUTTON",
        "Paste",
        0,
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        442,
        372,
        88,
        30,
        ID_CONFIRM,
    );
    let cancel = create(
        "BUTTON",
        "Cancel",
        0,
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        540,
        372,
        88,
        30,
        ID_CANCEL,
    );
    if [label, edit, confirm, cancel]
        .iter()
        .any(|child| child.is_null())
    {
        let error = last_error("text_review_control_create_failed");
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DestroyWindow(hwnd);
        }
        return Err(error);
    }
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    for child in [label, edit, confirm, cancel] {
        unsafe { SendMessageW(child, WM_SETFONT, font as usize, 1) };
    }
    if !owner.is_null() {
        unsafe { EnableWindow(owner, 0) };
    }
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        // `SetForegroundWindow` is allowed to fail under the foreground lock.
        // The owner is already disabled at this point, so a review left behind
        // the owner would read to a human as a frozen application with nothing
        // to click. Raising it within its own thread's Z-order does not need
        // foreground rights and is the recovery.
        if SetForegroundWindow(hwnd) == 0 {
            SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        }
        SetFocus(edit);
    }
    ACTIVE_DIALOG.with(|active| active.set(hwnd));
    Ok(NativeTextReview {
        hwnd,
        edit,
        owner,
        state,
        finished: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    /// The invariant that broke: this adapter must not run a message loop.
    ///
    /// The previous implementation pumped `GetMessageW` here and only returned
    /// once a human had answered, which is why no test could reach it — the
    /// call never came back without an interactive desktop. A bounded
    /// `open_review` plus a non-blocking `try_poll` is the whole difference,
    /// and asserting it costs one window.
    #[test]
    fn open_review_returns_without_pumping_and_polls_pending() {
        let woken = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&woken);
        let started = Instant::now();
        let mut review = open_review(
            None,
            "test review",
            "test prompt",
            "initial text",
            Box::new(move || signal.store(true, Ordering::Release)),
        )
        .expect("a review opens on a Windows test desktop");
        let elapsed = started.elapsed();

        // A pump would not return at all; the bound only has to be far below
        // human answer time to tell the two designs apart.
        assert!(
            elapsed < Duration::from_secs(2),
            "open_review blocked for {elapsed:?}, which means it pumped"
        );
        assert!(
            matches!(review.try_poll(), TextReviewPoll::Pending),
            "an unanswered review must report Pending, not block"
        );
        assert!(
            !woken.load(Ordering::Acquire),
            "the host is woken on completion, not on open"
        );

        // A second review would steal the thread's dialog navigation from the
        // first while still disabling its owner.
        let refused = open_review(None, "second", "second", "", Box::new(|| {}));
        assert!(matches!(
            refused,
            Err(TextReviewError::Failed { ref code, .. }) if code == "text_review_already_open"
        ));

        drop(review);
        assert!(
            ACTIVE_DIALOG.with(std::cell::Cell::get).is_null(),
            "dropping a review releases the thread's dialog slot"
        );
        // The slot really is reusable, not merely reported as clear.
        let reopened = open_review(None, "third", "third", "", Box::new(|| {}))
            .expect("the slot is reusable once the previous review is dropped");
        drop(reopened);
    }

    /// A dismissed review reports its outcome instead of leaving the caller to
    /// guess, and reports it once however many messages describe the dismissal.
    #[test]
    fn dismissal_is_terminal_idempotent_and_wakes_once() {
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let mut state = DialogState {
            done: false,
            confirmed: false,
            wake: Some(Box::new(move || {
                counter.fetch_add(1, Ordering::AcqRel);
            })),
        };

        state.finish(true);
        assert!(state.done);
        assert!(state.confirmed);

        // WM_CLOSE followed by WM_DESTROY is an ordinary teardown sequence: the
        // second must not turn a confirmation into a cancellation.
        state.finish(false);
        assert!(
            state.confirmed,
            "a later message must not rewrite the answer"
        );
        assert_eq!(
            wakes.load(Ordering::Acquire),
            1,
            "the host is woken exactly once per review"
        );
    }

    /// No review open means the host owns every message, unchanged.
    #[test]
    fn message_filter_is_inert_without_an_open_review() {
        let message: MSG = unsafe { mem::zeroed() };
        assert!(ACTIVE_DIALOG.with(std::cell::Cell::get).is_null());
        assert!(!filter_dialog_message(&message));
    }
}
