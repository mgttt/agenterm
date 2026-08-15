//! Product-owned desktop-host action catalog.
//!
//! Platform hosts publish these ids through menus and global shortcuts. They
//! never implement placement policy: an id becomes the same `Command` used by
//! the public CLI.

use crate::place::PlaceAction;
use crate::{Command, CuReply, Executor, TargetRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostAction {
    pub id: u32,
    pub place: PlaceAction,
    pub label: &'static str,
    pub macos_shortcut: &'static str,
    pub windows_shortcut: &'static str,
}

pub const QUIT_ACTION_ID: u32 = 1000;

pub const PLACEMENT_ACTIONS: [HostAction; 18] = [
    action(1, PlaceAction::Center, "Center", "alt+cmd+c", "alt+win+c"),
    action(
        2,
        PlaceAction::Fullscreen,
        "Fullscreen",
        "alt+cmd+f",
        "alt+win+f",
    ),
    action(
        3,
        PlaceAction::LeftHalf,
        "Left Half",
        "alt+cmd+left",
        "alt+win+left",
    ),
    action(
        4,
        PlaceAction::RightHalf,
        "Right Half",
        "alt+cmd+right",
        "alt+win+right",
    ),
    action(
        5,
        PlaceAction::TopHalf,
        "Top Half",
        "alt+cmd+up",
        "alt+win+up",
    ),
    action(
        6,
        PlaceAction::BottomHalf,
        "Bottom Half",
        "alt+cmd+down",
        "alt+win+down",
    ),
    action(
        7,
        PlaceAction::UpperLeft,
        "Upper Left",
        "ctrl+cmd+left",
        "alt+win+u",
    ),
    action(
        8,
        PlaceAction::LowerLeft,
        "Lower Left",
        "ctrl+shift+cmd+left",
        "alt+win+j",
    ),
    action(
        9,
        PlaceAction::UpperRight,
        "Upper Right",
        "ctrl+cmd+right",
        "alt+win+i",
    ),
    action(
        10,
        PlaceAction::LowerRight,
        "Lower Right",
        "ctrl+shift+cmd+right",
        "alt+win+k",
    ),
    action(
        11,
        PlaceAction::NextDisplay,
        "Next Display",
        "ctrl+alt+cmd+right",
        "alt+win+n",
    ),
    action(
        12,
        PlaceAction::PreviousDisplay,
        "Previous Display",
        "ctrl+alt+cmd+left",
        "alt+win+p",
    ),
    action(
        13,
        PlaceAction::NextThird,
        "Next Third",
        "ctrl+alt+right",
        "alt+win+t",
    ),
    action(
        14,
        PlaceAction::PreviousThird,
        "Previous Third",
        "ctrl+alt+left",
        "alt+win+y",
    ),
    action(
        15,
        PlaceAction::Larger,
        "Larger",
        "ctrl+alt+shift+right",
        "alt+win+l",
    ),
    action(
        16,
        PlaceAction::Smaller,
        "Smaller",
        "ctrl+alt+shift+left",
        "alt+win+s",
    ),
    action(17, PlaceAction::Undo, "Undo", "alt+cmd+z", "alt+win+z"),
    action(
        18,
        PlaceAction::Redo,
        "Redo",
        "alt+shift+cmd+z",
        "alt+shift+win+z",
    ),
];

const fn action(
    id: u32,
    place: PlaceAction,
    label: &'static str,
    macos_shortcut: &'static str,
    windows_shortcut: &'static str,
) -> HostAction {
    HostAction {
        id,
        place,
        label,
        macos_shortcut,
        windows_shortcut,
    }
}

pub fn by_id(id: u32) -> Option<&'static HostAction> {
    PLACEMENT_ACTIONS.iter().find(|action| action.id == id)
}

pub fn command(id: u32) -> Option<Command> {
    by_id(id).map(|action| Command::WindowPlace {
        target: TargetRef::Current,
        action: action.place.kebab().to_owned(),
        window: None,
    })
}

/// Executes one desktop-host action through the same public command executor
/// used by the CLI. Native menu/hotkey hosts transport only the numeric id;
/// product meaning, authorization, audit and mechanism selection stay here.
pub fn execute(executor: &Executor, id: u32) -> Option<CuReply> {
    command(id).map(|command| executor.execute(&command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Authorization, Grant};
    use std::collections::BTreeSet;

    #[test]
    fn ids_and_shortcuts_are_nonempty_and_unique() {
        let mut ids = BTreeSet::new();
        let mut macos = BTreeSet::new();
        let mut windows = BTreeSet::new();
        for action in PLACEMENT_ACTIONS {
            assert!(ids.insert(action.id));
            assert!(macos.insert(action.macos_shortcut));
            assert!(windows.insert(action.windows_shortcut));
            assert!(!action.label.is_empty());
            assert!(command(action.id).is_some());
        }
        assert!(!ids.contains(&0));
        assert!(!ids.contains(&QUIT_ACTION_ID));
    }

    #[test]
    fn host_action_dispatches_through_command_authorization() {
        let executor = Executor::new(Authorization::new([Grant::Observe].into_iter().collect()));
        let reply = execute(&executor, PLACEMENT_ACTIONS[0].id).expect("known host action");
        assert!(!reply.ok);
        assert_eq!(reply.command, "window-place");
        assert_eq!(reply.target, "current");
        assert_eq!(
            reply.error.as_ref().map(|error| error.code.as_str()),
            Some("refused")
        );
        assert!(execute(&executor, u32::MAX).is_none());
    }
}
