use std::{
    collections::VecDeque,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;

pub(crate) const EVENT_SCHEMA_VERSION: u32 = 1;
pub(crate) const EVENT_CATALOG_SCHEMA_VERSION: u32 = 1;
pub(crate) const DEFAULT_EVENT_CAPACITY: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventKind {
    ComposerDraft,
    ComposerSubmitted,
    ComposerSubmissionFinished,
    FocusChanged,
    LayoutTabsVisibility,
    LayoutTabsWidth,
    TabClosed,
    TabCreated,
    TabNote,
    TabParent,
    TabRenamed,
    TabSelected,
    TabState,
    TerminalOutput,
    TerminalPasted,
    TerminalViewport,
    WindowVisibility,
    WorkingContextCwd,
    WorkingContextCwdEditor,
    WorkingContextCwdRequested,
    WorkingContextProxyEditor,
    WorkingContextProxyRequested,
    WorkingContextProxySubmitted,
    WorkspaceSaved,
    WorkspaceShutdown,
}

impl EventKind {
    pub(crate) const ALL: [Self; 25] = [
        Self::ComposerDraft,
        Self::ComposerSubmitted,
        Self::ComposerSubmissionFinished,
        Self::FocusChanged,
        Self::LayoutTabsVisibility,
        Self::LayoutTabsWidth,
        Self::TabClosed,
        Self::TabCreated,
        Self::TabNote,
        Self::TabParent,
        Self::TabRenamed,
        Self::TabSelected,
        Self::TabState,
        Self::TerminalOutput,
        Self::TerminalPasted,
        Self::TerminalViewport,
        Self::WindowVisibility,
        Self::WorkingContextCwd,
        Self::WorkingContextCwdEditor,
        Self::WorkingContextCwdRequested,
        Self::WorkingContextProxyEditor,
        Self::WorkingContextProxyRequested,
        Self::WorkingContextProxySubmitted,
        Self::WorkspaceSaved,
        Self::WorkspaceShutdown,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ComposerDraft => "composer.draft",
            Self::ComposerSubmitted => "composer.submitted",
            Self::ComposerSubmissionFinished => "composer.submission-finished",
            Self::FocusChanged => "focus.changed",
            Self::LayoutTabsVisibility => "layout.tabs.visibility",
            Self::LayoutTabsWidth => "layout.tabs.width",
            Self::TabClosed => "tab.closed",
            Self::TabCreated => "tab.created",
            Self::TabNote => "tab.note",
            Self::TabParent => "tab.parent",
            Self::TabRenamed => "tab.renamed",
            Self::TabSelected => "tab.selected",
            Self::TabState => "tab.state",
            Self::TerminalOutput => "terminal.output",
            Self::TerminalPasted => "terminal.pasted",
            Self::TerminalViewport => "terminal.viewport",
            Self::WindowVisibility => "window.visibility",
            Self::WorkingContextCwd => "working-context.cwd",
            Self::WorkingContextCwdEditor => "working-context.cwd.editor",
            Self::WorkingContextCwdRequested => "working-context.cwd.requested",
            Self::WorkingContextProxyEditor => "working-context.proxy.editor",
            Self::WorkingContextProxyRequested => "working-context.proxy.requested",
            Self::WorkingContextProxySubmitted => "working-context.proxy.submitted",
            Self::WorkspaceSaved => "workspace.saved",
            Self::WorkspaceShutdown => "workspace.shutdown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EventSpec {
    pub(crate) kind: &'static str,
    pub(crate) state_path: &'static str,
    pub(crate) payload: &'static str,
    pub(crate) scope: &'static str,
    pub(crate) since: &'static str,
}

const fn event_spec(
    kind: EventKind,
    state_path: &'static str,
    payload: &'static str,
    scope: &'static str,
    since: &'static str,
) -> EventSpec {
    EventSpec {
        kind: kind.as_str(),
        state_path,
        payload,
        scope,
        since,
    }
}

pub(crate) const EVENT_CATALOG: [EventSpec; 25] = [
    event_spec(
        EventKind::ComposerDraft,
        "tabs[].draft",
        "{length:u64}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::ComposerSubmitted,
        "tabs[].draft",
        "{length:u64}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::ComposerSubmissionFinished,
        "tabs[].submit_pending",
        "{enter_written:bool,terminal_finalized:bool}",
        "tab",
        "0.1.7",
    ),
    event_spec(
        EventKind::FocusChanged,
        "focus.surface",
        "{from:string,to:string}",
        "server",
        "0.1.6",
    ),
    event_spec(
        EventKind::LayoutTabsVisibility,
        "layout.sidebar.visible",
        "{visible:bool,cause:string,operation_id:string}",
        "server",
        "0.1.6",
    ),
    event_spec(
        EventKind::LayoutTabsWidth,
        "layout.sidebar.configured_width",
        "{configured_width:u16,effective_width:i32,cause:string,operation_id:string}",
        "server",
        "0.1.6",
    ),
    event_spec(
        EventKind::TabClosed,
        "tabs[]",
        "{index:u32,parent_id:u64?,exit_code:i32?,promoted_children:[u64],active_id:u64?}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabCreated,
        "tabs[]",
        "{index:u32,parent_id:u64?,selected:bool}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabNote,
        "tabs[].note",
        "{previous_note:string,note:string}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabParent,
        "tabs[].parent_id",
        "{previous_parent_id:u64?,parent_id:u64?}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabRenamed,
        "tabs[].name",
        "{previous_name:string,name:string}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabSelected,
        "tabs[].active",
        "{}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TabState,
        "tabs[].state",
        "{state:string,exit_code:i32?,error:string?}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TerminalOutput,
        "inspect.windows[].output_bytes",
        "{output_bytes:u64,advanced_by:u64}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TerminalPasted,
        "inspect.windows[].output_bytes",
        "{characters:u64,bytes:u64,bracketed:bool,source:string}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::TerminalViewport,
        "tabs[].scrollback_offset",
        "{scrollback_offset:u64,source:string}",
        "tab",
        "0.1.5",
    ),
    event_spec(
        EventKind::WindowVisibility,
        "window.visible",
        "{visible:bool,reason:string}",
        "server",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextCwd,
        "tabs[].working_context.cwd",
        "{path:string?,source:string,pending:bool}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextCwdEditor,
        "modal.kind",
        "{open:bool}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextCwdRequested,
        "tabs[].working_context.cwd",
        "{path:string,source:string,pending:bool,disposition:string,composer_mode:string?|shell:string?}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextProxyEditor,
        "modal.kind",
        "{open:bool}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextProxyRequested,
        "tabs[].working_context.proxy",
        "{configured:bool,source:string,request_pending:bool,disposition:string}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkingContextProxySubmitted,
        "tabs[].working_context.proxy",
        "{sensitive:bool}",
        "tab",
        "0.1.6",
    ),
    event_spec(
        EventKind::WorkspaceSaved,
        "startup.workspace_file_exists",
        "{}",
        "server",
        "0.1.5",
    ),
    event_spec(
        EventKind::WorkspaceShutdown,
        "server-list[]",
        "{saved:bool,destroyed:bool?}",
        "server",
        "0.1.5",
    ),
];
const _: [(); EventKind::ALL.len()] = [(); EVENT_CATALOG.len()];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EventPosition {
    pub(crate) epoch: String,
    pub(crate) sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct EventEnvelope {
    pub(crate) schema_version: u32,
    pub(crate) epoch: String,
    pub(crate) sequence: u64,
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tab_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) operation_id: Option<String>,
    pub(crate) payload: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JournalReadError {
    Restart {
        requested_epoch: String,
        current: EventPosition,
    },
    Gap {
        requested_after: u64,
        earliest_available: u64,
        current: EventPosition,
    },
    FutureSequence {
        requested_after: u64,
        current: EventPosition,
    },
}

impl fmt::Display for JournalReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restart {
                requested_epoch,
                current,
            } => write!(
                formatter,
                "server_restart: requested epoch {requested_epoch}, current epoch {} at sequence {}",
                current.epoch, current.sequence
            ),
            Self::Gap {
                requested_after,
                earliest_available,
                current,
            } => write!(
                formatter,
                "journal_gap: sequence {requested_after} is older than earliest available {}; \
                 current sequence is {}",
                earliest_available, current.sequence
            ),
            Self::FutureSequence {
                requested_after,
                current,
            } => write!(
                formatter,
                "future_sequence: requested sequence {requested_after}, current sequence is {}",
                current.sequence
            ),
        }
    }
}

impl JournalReadError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Restart { .. } => "server_restart",
            Self::Gap { .. } => "journal_gap",
            Self::FutureSequence { .. } => "future_sequence",
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            Self::Restart {
                requested_epoch,
                current,
            } => serde_json::json!({
                "code": "server_restart",
                "requested_epoch": requested_epoch,
                "current": current,
            }),
            Self::Gap {
                requested_after,
                earliest_available,
                current,
            } => serde_json::json!({
                "code": "journal_gap",
                "requested_after": requested_after,
                "earliest_available": earliest_available,
                "current": current,
            }),
            Self::FutureSequence {
                requested_after,
                current,
            } => serde_json::json!({
                "code": "future_sequence",
                "requested_after": requested_after,
                "current": current,
            }),
        }
    }
}

pub(crate) struct EventJournal {
    epoch: String,
    sequence: u64,
    capacity: usize,
    events: VecDeque<EventEnvelope>,
}

impl EventJournal {
    pub(crate) fn new() -> Self {
        Self::with_epoch(new_epoch(), DEFAULT_EVENT_CAPACITY)
    }

    fn with_epoch(epoch: impl Into<String>, capacity: usize) -> Self {
        Self {
            epoch: epoch.into(),
            sequence: 0,
            capacity: capacity.max(1),
            events: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    pub(crate) fn position(&self) -> EventPosition {
        EventPosition {
            epoch: self.epoch.clone(),
            sequence: self.sequence,
        }
    }

    pub(crate) fn commit(
        &mut self,
        kind: EventKind,
        tab_id: Option<u64>,
        payload: Value,
    ) -> EventEnvelope {
        self.commit_correlated(kind, tab_id, None, None, payload)
    }

    pub(crate) fn commit_correlated(
        &mut self,
        kind: EventKind,
        tab_id: Option<u64>,
        request_id: Option<String>,
        operation_id: Option<String>,
        payload: Value,
    ) -> EventEnvelope {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("event sequence exhausted");
        let event = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            epoch: self.epoch.clone(),
            sequence: self.sequence,
            kind: kind.as_str().to_owned(),
            tab_id,
            request_id,
            operation_id,
            payload,
        };
        if self.events.len() == self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event.clone());
        event
    }

    pub(crate) fn read_after(
        &self,
        epoch: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalReadError> {
        let current = self.position();
        if epoch != self.epoch {
            return Err(JournalReadError::Restart {
                requested_epoch: epoch.to_owned(),
                current,
            });
        }
        if after > self.sequence {
            return Err(JournalReadError::FutureSequence {
                requested_after: after,
                current,
            });
        }
        if let Some(earliest) = self.events.front().map(|event| event.sequence)
            && after.saturating_add(1) < earliest
        {
            return Err(JournalReadError::Gap {
                requested_after: after,
                earliest_available: earliest,
                current,
            });
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .take(limit.max(1))
            .cloned()
            .collect())
    }
}

fn new_epoch() -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}-{}-{}",
        std::process::id(),
        started.as_secs(),
        started.subsec_nanos()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_orders_events_and_reads_after_a_snapshot_position() {
        let mut journal = EventJournal::with_epoch("epoch-a", 4);
        let baseline = journal.position();
        journal.commit(
            EventKind::TabCreated,
            Some(7),
            serde_json::json!({"name": "worker"}),
        );
        journal.commit(EventKind::TabSelected, Some(7), serde_json::json!({}));

        let events = journal
            .read_after(&baseline.epoch, baseline.sequence, 10)
            .unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| (event.sequence, event.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "tab.created"), (2, "tab.selected")]
        );
        assert!(journal.read_after("epoch-a", 2, 10).unwrap().is_empty());
    }

    #[test]
    fn bounded_history_reports_a_gap_instead_of_silent_loss() {
        let mut journal = EventJournal::with_epoch("epoch-a", 2);
        journal.commit(EventKind::TabCreated, None, Value::Null);
        journal.commit(EventKind::TabSelected, None, Value::Null);
        journal.commit(EventKind::TabClosed, None, Value::Null);

        assert!(matches!(
            journal.read_after("epoch-a", 0, 10),
            Err(JournalReadError::Gap {
                requested_after: 0,
                earliest_available: 2,
                ..
            })
        ));
        assert_eq!(
            journal
                .read_after("epoch-a", 1, 10)
                .unwrap()
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn restart_and_future_positions_are_distinct_errors() {
        let mut journal = EventJournal::with_epoch("epoch-b", 2);
        journal.commit(EventKind::TabCreated, None, Value::Null);

        assert!(matches!(
            journal.read_after("epoch-a", 0, 10),
            Err(JournalReadError::Restart { .. })
        ));
        assert!(matches!(
            journal.read_after("epoch-b", 2, 10),
            Err(JournalReadError::FutureSequence { .. })
        ));
    }

    #[test]
    fn event_catalog_is_a_complete_unique_view_of_the_closed_kind_set() {
        assert_eq!(EVENT_CATALOG.len(), EventKind::ALL.len());
        for kind in EventKind::ALL {
            let matches = EVENT_CATALOG
                .iter()
                .filter(|spec| spec.kind == kind.as_str())
                .count();
            assert_eq!(matches, 1, "catalog entry for {}", kind.as_str());
        }
        for spec in EVENT_CATALOG {
            assert!(!spec.state_path.is_empty());
            assert!(!spec.payload.is_empty());
            assert!(matches!(spec.scope, "server" | "tab"));
            assert!(matches!(spec.since, "0.1.5" | "0.1.6" | "0.1.7"));
        }
    }
}
