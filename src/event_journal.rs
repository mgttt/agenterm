use std::{
    collections::VecDeque,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;

pub(crate) const EVENT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DEFAULT_EVENT_CAPACITY: usize = 4_096;

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
        kind: impl Into<String>,
        tab_id: Option<u64>,
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
            kind: kind.into(),
            tab_id,
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
            "tab.created",
            Some(7),
            serde_json::json!({"name": "worker"}),
        );
        journal.commit("tab.selected", Some(7), serde_json::json!({}));

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
        journal.commit("one", None, Value::Null);
        journal.commit("two", None, Value::Null);
        journal.commit("three", None, Value::Null);

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
        journal.commit("one", None, Value::Null);

        assert!(matches!(
            journal.read_after("epoch-a", 0, 10),
            Err(JournalReadError::Restart { .. })
        ));
        assert!(matches!(
            journal.read_after("epoch-b", 2, 10),
            Err(JournalReadError::FutureSequence { .. })
        ));
    }
}
