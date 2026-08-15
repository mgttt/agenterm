//! Owner-modal Win32 multiline editor. Native handles and the nested message
//! loop stay inside this adapter.

use std::{io, mem, panic::AssertUnwindSafe, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{EnableWindow, SetFocus},
        WindowsAndMessaging::{
            BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT,
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL,
            ES_MULTILINE, ES_WANTRETURN, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
            GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, IsDialogMessageW, LoadCursorW, MSG,
            PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
            SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND, WM_DESTROY,
            WM_NCCREATE, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
            WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::text_review::TextReviewError;

const ID_CONFIRM: u16 = 1;
const ID_CANCEL: u16 = 2;

struct DialogState {
    done: bool,
    confirmed: bool,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(code: &'static str) -> TextReviewError {
    TextReviewError::failed(code, io::Error::last_os_error())
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
                    (*state).confirmed = id == ID_CONFIRM;
                    (*state).done = true;
                    return 0;
                }
            }
            WM_CLOSE | WM_DESTROY if !state.is_null() => {
                (*state).done = true;
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
        .map_err(|message| TextReviewError::failed("text_review_register_failed", message))?;
    Ok(class)
}

pub(crate) fn review_text(
    owner: Option<i64>,
    title: &str,
    prompt: &str,
    initial: &str,
) -> Result<Option<String>, TextReviewError> {
    let class = ensure_class()?;
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(last_error("text_review_module_handle_failed"));
    }
    let owner = owner.map_or(ptr::null_mut(), |value| value as isize as HWND);
    let mut state = DialogState {
        done: false,
        confirmed: false,
    };
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
            (&mut state as *mut DialogState).cast(),
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
        unsafe { DestroyWindow(hwnd) };
        return Err(last_error("text_review_control_create_failed"));
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
        SetForegroundWindow(hwnd);
        SetFocus(edit);
    }
    let mut message: MSG = unsafe { mem::zeroed() };
    let mut loop_error = None;
    loop {
        // DispatchMessageW enters dialog_proc, which updates this state through
        // GWLP_USERDATA; keep the callback-visible read inside the loop body.
        if state.done {
            break;
        }
        let status = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if status == -1 {
            loop_error = Some(last_error("text_review_message_failed"));
            break;
        }
        if status == 0 {
            unsafe { PostQuitMessage(message.wParam as i32) };
            break;
        }
        if unsafe { IsDialogMessageW(hwnd, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    let result = if state.confirmed {
        let len = unsafe { GetWindowTextLengthW(edit) };
        let mut value = vec![0u16; usize::try_from(len).unwrap_or(0) + 1];
        let copied = unsafe { GetWindowTextW(edit, value.as_mut_ptr(), value.len() as i32) };
        value.truncate(usize::try_from(copied).unwrap_or(0));
        Some(String::from_utf16_lossy(&value))
    } else {
        None
    };
    unsafe {
        DestroyWindow(hwnd);
        if !owner.is_null() {
            EnableWindow(owner, 1);
            SetForegroundWindow(owner);
        }
    }
    loop_error.map_or(Ok(result), Err)
}
