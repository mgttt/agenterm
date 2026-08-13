//! Native X11 CLIPBOARD (ICCCM selection), not `xclip` / `xsel`.
//!
//! `SetSelectionOwner` seeds CLIPBOARD; a later `get_text` in the same
//! process returns that owned UTF-8 payload. Foreign owners are read with
//! `ConvertSelection`. No helper binary.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

use super::ClipboardError;

struct Atoms {
    clipboard: Atom,
    utf8_string: Atom,
    targets: Atom,
    incr: Atom,
    property: Atom,
    string: Atom,
    atom: Atom,
}

struct NativeClipboard {
    conn: RustConnection,
    window: Window,
    atoms: Atoms,
    owned: Option<Vec<u8>>,
}

static STATE: Mutex<Option<NativeClipboard>> = Mutex::new(None);

fn backend(message: impl ToString) -> ClipboardError {
    ClipboardError::Backend {
        message: message.to_string(),
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom, ClipboardError> {
    conn.intern_atom(false, name)
        .map_err(|error| backend(format!("X11 InternAtom send failed: {error}")))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| backend(format!("X11 InternAtom failed: {error}")))
}

impl NativeClipboard {
    fn open() -> Result<Self, ClipboardError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|error| ClipboardError::Unavailable {
                message: format!("X11 display could not be opened: {error}"),
            })?;
        let root = conn
            .setup()
            .roots
            .get(screen_num)
            .ok_or_else(|| backend("configured X11 screen does not exist"))?
            .root;
        let window = conn
            .generate_id()
            .map_err(|error| backend(format!("X11 generate_id failed: {error}")))?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            -10,
            -10,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|error| backend(format!("X11 CreateWindow failed: {error}")))?;
        let atoms = match intern_atoms(&conn) {
            Ok(atoms) => atoms,
            Err(error) => {
                let _ = conn.destroy_window(window);
                let _ = conn.flush();
                return Err(error);
            }
        };
        conn.flush()
            .map_err(|error| backend(format!("X11 clipboard flush failed: {error}")))?;
        Ok(Self {
            conn,
            window,
            atoms,
            owned: None,
        })
    }

    fn pump(&mut self) -> Result<(), ClipboardError> {
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(event)) => self.handle_event(event)?,
                Ok(None) => return Ok(()),
                Err(error) => {
                    return Err(backend(format!("X11 poll_for_event failed: {error}")));
                }
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> Result<(), ClipboardError> {
        match event {
            Event::SelectionRequest(request) => self.serve_request(&request),
            Event::SelectionClear(clear) if clear.selection == self.atoms.clipboard => {
                self.owned = None;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn serve_request(&self, request: &SelectionRequestEvent) -> Result<(), ClipboardError> {
        let property = if request.property == NONE {
            request.target
        } else {
            request.property
        };
        let ok =
            request.selection == self.atoms.clipboard && self.write_selection(request, property);
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if ok { property } else { NONE },
        };
        self.conn
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
            .map_err(|error| backend(format!("X11 SelectionNotify send failed: {error}")))?;
        self.conn
            .flush()
            .map_err(|error| backend(format!("X11 SelectionNotify flush failed: {error}")))
    }

    fn write_selection(&self, request: &SelectionRequestEvent, property: Atom) -> bool {
        let Some(bytes) = self.owned.as_deref() else {
            return false;
        };
        if request.target == self.atoms.targets {
            let targets = [
                self.atoms.targets,
                self.atoms.utf8_string,
                self.atoms.string,
            ];
            return self
                .conn
                .change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    self.atoms.atom,
                    &targets,
                )
                .and_then(|_| self.conn.flush())
                .is_ok();
        }
        if request.target == self.atoms.utf8_string || request.target == self.atoms.string {
            return self
                .conn
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    request.target,
                    bytes,
                )
                .and_then(|_| self.conn.flush())
                .is_ok();
        }
        false
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.pump()?;
        self.owned = Some(text.as_bytes().to_vec());
        self.conn
            .set_selection_owner(self.window, self.atoms.clipboard, CURRENT_TIME)
            .map_err(|error| backend(format!("X11 SetSelectionOwner send failed: {error}")))?;
        self.conn
            .flush()
            .map_err(|error| backend(format!("X11 SetSelectionOwner flush failed: {error}")))?;
        let owner = self
            .conn
            .get_selection_owner(self.atoms.clipboard)
            .map_err(|error| backend(format!("X11 GetSelectionOwner send failed: {error}")))?
            .reply()
            .map_err(|error| backend(format!("X11 GetSelectionOwner failed: {error}")))?
            .owner;
        if owner != self.window {
            self.owned = None;
            return Err(backend(
                "X11 SetSelectionOwner did not leave this connection as CLIPBOARD owner",
            ));
        }
        Ok(())
    }

    fn get_text(
        &mut self,
        max_read_bytes: usize,
        timeout: Duration,
    ) -> Result<String, ClipboardError> {
        self.pump()?;
        if let Some(bytes) = self.owned.as_deref() {
            if bytes.len() > max_read_bytes {
                return Err(ClipboardError::TooLarge {
                    limit: max_read_bytes,
                });
            }
            return String::from_utf8(bytes.to_vec()).map_err(|error| backend(error.to_string()));
        }
        self.convert_clipboard(self.atoms.utf8_string, max_read_bytes, timeout)
    }

    fn convert_clipboard(
        &mut self,
        target: Atom,
        max_read_bytes: usize,
        timeout: Duration,
    ) -> Result<String, ClipboardError> {
        let _ = self.conn.delete_property(self.window, self.atoms.property);
        self.conn
            .convert_selection(
                self.window,
                self.atoms.clipboard,
                target,
                self.atoms.property,
                CURRENT_TIME,
            )
            .map_err(|error| backend(format!("X11 ConvertSelection send failed: {error}")))?;
        self.conn
            .flush()
            .map_err(|error| backend(format!("X11 ConvertSelection flush failed: {error}")))?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(Event::SelectionNotify(notify)))
                    if notify.selection == self.atoms.clipboard
                        && notify.requestor == self.window
                        && notify.target == target =>
                {
                    if notify.property == NONE {
                        return Ok(String::new());
                    }
                    return self.read_property(notify.property, max_read_bytes);
                }
                Ok(Some(event)) => self.handle_event(event)?,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return Err(ClipboardError::Timeout {
                            message: format!(
                                "clipboard_timeout: X11 ConvertSelection exceeded {} ms",
                                timeout.as_millis()
                            ),
                        });
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    return Err(backend(format!("X11 poll_for_event failed: {error}")));
                }
            }
        }
    }

    fn read_property(
        &self,
        property: Atom,
        max_read_bytes: usize,
    ) -> Result<String, ClipboardError> {
        let long_length = u32::try_from(max_read_bytes / 4 + 2).unwrap_or(u32::MAX);
        let reply = self
            .conn
            .get_property(true, self.window, property, AtomEnum::ANY, 0, long_length)
            .map_err(|error| backend(format!("X11 GetProperty send failed: {error}")))?
            .reply()
            .map_err(|error| backend(format!("X11 GetProperty failed: {error}")))?;
        if reply.type_ == self.atoms.incr {
            return Err(ClipboardError::TooLarge {
                limit: max_read_bytes,
            });
        }
        if reply.bytes_after > 0 {
            return Err(ClipboardError::TooLarge {
                limit: max_read_bytes,
            });
        }
        if reply.value.len() > max_read_bytes {
            return Err(ClipboardError::TooLarge {
                limit: max_read_bytes,
            });
        }
        if reply.type_ == self.atoms.string {
            return Ok(reply.value.into_iter().map(char::from).collect());
        }
        String::from_utf8(reply.value).map_err(|error| backend(error.to_string()))
    }
}

fn intern_atoms(conn: &RustConnection) -> Result<Atoms, ClipboardError> {
    Ok(Atoms {
        clipboard: intern(conn, b"CLIPBOARD")?,
        utf8_string: intern(conn, b"UTF8_STRING")?,
        targets: intern(conn, b"TARGETS")?,
        incr: intern(conn, b"INCR")?,
        property: intern(conn, b"PLATFORM_CLIPBOARD")?,
        string: Atom::from(AtomEnum::STRING),
        atom: Atom::from(AtomEnum::ATOM),
    })
}

fn with_state<T>(
    f: impl FnOnce(&mut NativeClipboard) -> Result<T, ClipboardError>,
) -> Result<T, ClipboardError> {
    let mut guard = STATE.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        *guard = Some(NativeClipboard::open()?);
    }
    f(guard.as_mut().expect("native clipboard state just opened"))
}

pub(super) fn set_text(text: &str, _timeout: Duration) -> Result<(), ClipboardError> {
    with_state(|state| state.set_text(text))
}

pub(super) fn get_text(max_read_bytes: usize, timeout: Duration) -> Result<String, ClipboardError> {
    with_state(|state| state.get_text(max_read_bytes, timeout))
}

pub(super) fn has_unicode_text() -> bool {
    match with_state(|state| {
        state.pump()?;
        Ok(state.owned.as_ref().is_some_and(|bytes| !bytes.is_empty()))
    }) {
        Ok(true) => true,
        Ok(false) => match get_text(1, Duration::from_millis(200)) {
            Ok(text) => !text.is_empty(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn property_atom_is_product_neutral() {
        assert_eq!(
            intern_atoms_names(),
            [
                "CLIPBOARD",
                "UTF8_STRING",
                "TARGETS",
                "INCR",
                "PLATFORM_CLIPBOARD"
            ]
        );
    }

    fn intern_atoms_names() -> [&'static str; 5] {
        [
            "CLIPBOARD",
            "UTF8_STRING",
            "TARGETS",
            "INCR",
            "PLATFORM_CLIPBOARD",
        ]
    }
}
