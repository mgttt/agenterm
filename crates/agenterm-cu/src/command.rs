//! Abstract, target-agnostic command set (PRD_02_29).

use serde::{Deserialize, Serialize};

use crate::target::TargetRef;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointerButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "verb", rename_all = "kebab-case")]
pub enum Command {
    Capabilities {
        target: TargetRef,
    },
    Windows {
        target: TargetRef,
    },
    Tree {
        target: TargetRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
    Screenshot {
        target: TargetRef,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
    /// Move the pointer to absolute target-session screen coordinates without
    /// pressing, releasing, clicking, dragging, or scrolling any button.
    PointerMove {
        target: TargetRef,
        x: i32,
        y: i32,
    },
    Click {
        target: TargetRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        node: Option<String>,
        /// Accessible-name substring; resolved with the same showing/visible
        /// matcher as `WaitCondition::NodeNameContains` (exactly one match),
        /// then acted via `--node`. Two or more showing hits are
        /// `a11y_node_ambiguous`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        coords: Option<[i32; 2]>,
        #[serde(default)]
        degraded: bool,
        #[serde(default = "default_clicks")]
        clicks: u32,
        #[serde(default)]
        button: PointerButton,
    },
    Focus {
        target: TargetRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    SendText {
        target: TargetRef,
        text: String,
        /// Optional window scope. With `--name`, name-address the unique
        /// showing node. Without `--name`, write the showing focused node
        /// (same innermost Text candidate as `GetText` without `--name`).
        /// Neither flag keeps the plain focused inject.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Read the target session's native Unicode-text clipboard directly.
    /// This is independent of accessibility-node `copy` / `paste`; absence
    /// of Unicode text is a successful empty string and the native ABI owns
    /// the bounded whole-payload read.
    ClipboardRead {
        target: TargetRef,
    },
    /// Copy AT-SPI `Text.GetText` onto the native clipboard
    /// (`agt_clipboard_set_text`). With `--name`, the unique showing named
    /// node. With `--window` and no `--name`, the showing focused node
    /// (same innermost Text candidate as `GetText` without `--name`).
    /// Never XTest / `--coords` / screenshot when `--window` is set. A
    /// node with no Text interface typed-fails (`a11y_text_unavailable`).
    Copy {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Write clipboard text via native AT-SPI `EditableText` / `Text`.
    /// With `--name`, the unique showing named field. With `--window` and
    /// no `--name`, the showing focused node (same innermost Text
    /// candidate as `GetText` without `--name`). `--text` only seeds the
    /// clipboard; the field write always reads the clipboard. Never XTest
    /// / `--coords` when `--window` is set.
    Paste {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    SendKeys {
        target: TargetRef,
        keys: String,
        /// Optional window scope. With `--name`, name-address the unique
        /// showing node and deliver Device/key events. Without `--name`,
        /// target the showing focused node (same innermost Text candidate
        /// as `GetText` without `--name`). Neither flag keeps the plain
        /// focused inject.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// One-shot AT-SPI `Component.ScrollTo(TopEdge)` on the unique showing
    /// named node. Success is `via=scroll-to`. Missing / false /
    /// `UnknownMethod` typed-fails (`a11y_scroll_unavailable`). Never
    /// Action `scroll*`, XTest wheel, `--coords`, or screenshot.
    Scroll {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Independent AT-SPI `Component.GetExtents(Screen)` for the unique
    /// showing named node. Snapshot `node.bounds` do not count. Empty
    /// extents typed-fail (`a11y_extents_unavailable`).
    GetExtents {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// One-shot AT-SPI `Text.SetSelection(0, start, end)` on the unique
    /// showing named node. Success is `via=set-selection`. Missing Text /
    /// `UnknownMethod` typed-fails (`a11y_selection_unavailable`).
    /// SetSelection false typed-fails (`a11y_selection_no_effect`). Never
    /// XTest, mouse-drag, `--coords`, or screenshot. The reply is not
    /// proof; callers observe via `get-selection`.
    Select {
        target: TargetRef,
        start: i32,
        end: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Independent AT-SPI `Text.GetNSelections` + `GetSelection(0)` for
    /// the unique showing named node. Not the `select` reply payload.
    /// Missing Text typed-fails (`a11y_selection_unavailable`). `n == 0`
    /// is empty success.
    GetSelection {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// One-shot AT-SPI `Text.SetCaretOffset` on the unique showing named
    /// node. Success is `via=set-caret-offset`. Missing Text /
    /// `UnknownMethod` typed-fails (`a11y_caret_unavailable`).
    /// SetCaretOffset false typed-fails (`a11y_caret_no_effect`). Never
    /// XTest, `--coords`, or screenshot. The reply is not proof; callers
    /// observe via `get-caret`.
    SetCaret {
        target: TargetRef,
        offset: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Independent AT-SPI `Text.CaretOffset` / `GetCaretOffset` for the
    /// unique showing named node. Not the `set-caret` reply payload.
    /// Missing Text typed-fails (`a11y_caret_unavailable`).
    GetCaret {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// One-shot independent AT-SPI `Text.GetText` for the unique showing
    /// named node, or — with no `name` — for the node carrying the AT-SPI
    /// `focused` state. Not a `wait --text-equals` poll and not `send-text` /
    /// `paste` / `copy` `matched.text`, `last_text_write_via`, the WebKit
    /// eval helper's queued-job `OK`, or a tree snapshot `text`. Missing
    /// Text typed-fails (`a11y_text_unavailable`). Never XTest / `--coords`.
    GetText {
        target: TargetRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    Wait {
        target: TargetRef,
        timeout_ms: u64,
        #[serde(flatten)]
        condition: WaitCondition,
    },
    WindowPlace {
        target: TargetRef,
        action: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "wait", rename_all = "kebab-case")]
pub enum WaitCondition {
    WindowCountGte {
        count: usize,
    },
    WindowTitleContains {
        pattern: String,
    },
    FocusedHandle {
        handle: isize,
    },
    /// Polls the accessibility tree until exactly one showing node matches.
    /// Two or more showing hits fail typed (`a11y_node_ambiguous`) instead of
    /// picking the first. Never falls back to pixels: addressing stays
    /// `accessibility-tree`.
    NodeNameContains {
        pattern: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
    /// Polls AT-SPI `Text.GetText` on the unique showing node addressed by
    /// `--name` until that independent text equals `expected`. Snapshot
    /// `node.text`, `send-text` / `paste` / `copy` `matched.text`,
    /// `last_text_write_via`, and the WebKit eval helper's queued-job `OK`
    /// are not this condition.
    /// Timeout is typed. Never screenshot / XTest / `--coords`.
    NodeTextEquals {
        expected: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
    /// Same independent `Text.GetText` poll as `NodeTextEquals`, but the
    /// hit is `gettext.contains(substring)`. Snapshot `node.text`,
    /// `send-text` / `paste` / `copy` `matched.text`, `last_text_write_via`,
    /// and the WebKit eval helper's queued-job `OK` are not this condition.
    /// Timeout is typed. Never screenshot / XTest / `--coords`.
    NodeTextContains {
        substring: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<isize>,
    },
}

fn default_clicks() -> u32 {
    1
}

impl Command {
    pub fn verb(&self) -> String {
        match self {
            Self::Capabilities { .. } => "capabilities".into(),
            Self::Windows { .. } => "windows".into(),
            Self::Tree { .. } => "tree".into(),
            Self::Screenshot { .. } => "screenshot".into(),
            Self::PointerMove { .. } => "pointer-move".into(),
            Self::Click { .. } => "click".into(),
            Self::Focus { .. } => "focus".into(),
            Self::SendText { .. } => "send-text".into(),
            Self::ClipboardRead { .. } => "clipboard-read".into(),
            Self::Copy { .. } => "copy".into(),
            Self::Paste { .. } => "paste".into(),
            Self::SendKeys { .. } => "send-keys".into(),
            Self::Scroll { .. } => "scroll".into(),
            Self::GetExtents { .. } => "get-extents".into(),
            Self::Select { .. } => "select".into(),
            Self::GetSelection { .. } => "get-selection".into(),
            Self::SetCaret { .. } => "set-caret".into(),
            Self::GetCaret { .. } => "get-caret".into(),
            Self::GetText { .. } => "get-text".into(),
            Self::Wait { .. } => "wait".into(),
            Self::WindowPlace { .. } => "window-place".into(),
        }
    }

    pub fn target(&self) -> TargetRef {
        match self {
            Self::Capabilities { target, .. }
            | Self::Windows { target, .. }
            | Self::Tree { target, .. }
            | Self::Screenshot { target, .. }
            | Self::PointerMove { target, .. }
            | Self::Click { target, .. }
            | Self::Focus { target, .. }
            | Self::SendText { target, .. }
            | Self::ClipboardRead { target, .. }
            | Self::Copy { target, .. }
            | Self::Paste { target, .. }
            | Self::SendKeys { target, .. }
            | Self::Scroll { target, .. }
            | Self::GetExtents { target, .. }
            | Self::Select { target, .. }
            | Self::GetSelection { target, .. }
            | Self::SetCaret { target, .. }
            | Self::GetCaret { target, .. }
            | Self::GetText { target, .. }
            | Self::Wait { target, .. }
            | Self::WindowPlace { target, .. } => *target,
        }
    }

    pub fn required_grant(&self) -> crate::auth::Grant {
        match self {
            Self::PointerMove { .. }
            | Self::Click { .. }
            | Self::Focus { .. }
            | Self::SendText { .. }
            | Self::Copy { .. }
            | Self::Paste { .. }
            | Self::SendKeys { .. }
            | Self::Scroll { .. }
            | Self::Select { .. }
            | Self::SetCaret { .. }
            | Self::WindowPlace { .. } => crate::auth::Grant::Actuate,
            _ => crate::auth::Grant::Observe,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Grant;

    #[test]
    fn clipboard_read_is_target_neutral_observation() {
        let command = Command::ClipboardRead {
            target: TargetRef::Vnc,
        };
        assert_eq!(command.verb(), "clipboard-read");
        assert_eq!(command.target(), TargetRef::Vnc);
        assert_eq!(command.required_grant(), Grant::Observe);
        assert_eq!(
            serde_json::to_value(&command).expect("serialize"),
            serde_json::json!({ "verb": "clipboard-read", "target": "vnc" })
        );
    }

    #[test]
    fn pointer_move_is_target_neutral_actuation_with_explicit_coordinates() {
        let command = Command::PointerMove {
            target: TargetRef::Ssh,
            x: -320,
            y: 1440,
        };
        assert_eq!(command.verb(), "pointer-move");
        assert_eq!(command.target(), TargetRef::Ssh);
        assert_eq!(command.required_grant(), Grant::Actuate);
        let json = serde_json::to_value(&command).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "verb": "pointer-move",
                "target": "ssh",
                "x": -320,
                "y": 1440
            })
        );
        let decoded: Command = serde_json::from_value(json).expect("deserialize");
        assert!(matches!(
            decoded,
            Command::PointerMove {
                target: TargetRef::Ssh,
                x: -320,
                y: 1440
            }
        ));
    }
}
