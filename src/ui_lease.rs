use std::sync::atomic::{AtomicU64, Ordering};

use crate::ui_bridge::UI_CLIENT_ID_MAX_BYTES;

pub(crate) const UI_LEASE_TTL_MS: u64 = 5_000;

static NEXT_LEASE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiLeaseRecord {
    pub(crate) lease_id: String,
    pub(crate) client_id: String,
    pub(crate) client_pid: u32,
    pub(crate) expires_unix_ms: u64,
    pub(crate) observed_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiLeaseError {
    InvalidClientId,
    InvalidClientPid,
    InvalidLeaseId,
    Conflict,
    NotAttached,
    OwnerMismatch,
    InvalidObservedSequence,
}

impl UiLeaseError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidClientId => "ui_lease_client_id_invalid",
            Self::InvalidClientPid => "ui_lease_client_pid_invalid",
            Self::InvalidLeaseId => "ui_lease_id_invalid",
            Self::Conflict => "ui_lease_conflict",
            Self::NotAttached => "ui_lease_not_attached",
            Self::OwnerMismatch => "ui_lease_owner_mismatch",
            Self::InvalidObservedSequence => "ui_lease_observed_sequence_invalid",
        }
    }

    pub(crate) const fn category(self) -> &'static str {
        match self {
            Self::InvalidClientId
            | Self::InvalidClientPid
            | Self::InvalidLeaseId
            | Self::InvalidObservedSequence => "validation",
            Self::Conflict => "conflict",
            Self::NotAttached => "availability",
            Self::OwnerMismatch => "conflict",
        }
    }

    pub(crate) const fn retryable(self) -> bool {
        matches!(self, Self::Conflict | Self::NotAttached)
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::InvalidClientId => "UI lease client ID is empty, oversized, or contains controls",
            Self::InvalidClientPid => "UI lease client PID must identify a live nonzero process",
            Self::InvalidLeaseId => "UI lease ID is empty, oversized, or contains controls",
            Self::Conflict => "another live UI client currently owns the interactive lease",
            Self::NotAttached => "no live UI client currently owns the interactive lease",
            Self::OwnerMismatch => "UI lease ID or client PID does not match the current owner",
            Self::InvalidObservedSequence => {
                "UI observed sequence must advance monotonically within the current journal"
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UiLeaseAuthority {
    active: Option<UiLeaseRecord>,
}

impl UiLeaseAuthority {
    pub(crate) fn active(&self) -> Option<&UiLeaseRecord> {
        self.active.as_ref()
    }

    pub(crate) fn attach(
        &mut self,
        client_id: &str,
        client_pid: u32,
        now_unix_ms: u64,
    ) -> Result<(UiLeaseRecord, bool), UiLeaseError> {
        validate_client_id(client_id)?;
        if client_pid == 0 {
            return Err(UiLeaseError::InvalidClientPid);
        }
        if let Some(active) = self.active.as_mut() {
            if active.client_id == client_id && active.client_pid == client_pid {
                active.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
                return Ok((active.clone(), false));
            }
            return Err(UiLeaseError::Conflict);
        }
        let sequence = NEXT_LEASE_ID.fetch_add(1, Ordering::Relaxed);
        let record = UiLeaseRecord {
            lease_id: format!(
                "ui-{:x}-{:x}-{:x}",
                std::process::id(),
                now_unix_ms,
                sequence
            ),
            client_id: client_id.to_owned(),
            client_pid,
            expires_unix_ms: now_unix_ms.saturating_add(UI_LEASE_TTL_MS),
            observed_sequence: 0,
        };
        self.active = Some(record.clone());
        Ok((record, true))
    }

    pub(crate) fn heartbeat(
        &mut self,
        lease_id: &str,
        client_pid: u32,
        now_unix_ms: u64,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        let Some(active) = self.active.as_mut() else {
            return Err(UiLeaseError::NotAttached);
        };
        if active.lease_id != lease_id || active.client_pid != client_pid {
            return Err(UiLeaseError::OwnerMismatch);
        }
        active.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
        Ok(active.clone())
    }

    pub(crate) fn detach(
        &mut self,
        lease_id: &str,
        client_pid: u32,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        let Some(active) = self.active.as_ref() else {
            return Err(UiLeaseError::NotAttached);
        };
        if active.lease_id != lease_id || active.client_pid != client_pid {
            return Err(UiLeaseError::OwnerMismatch);
        }
        Ok(self.active.take().expect("active UI lease was checked"))
    }

    pub(crate) fn acknowledge(
        &mut self,
        lease_id: &str,
        client_pid: u32,
        observed_sequence: u64,
        current_sequence: u64,
        now_unix_ms: u64,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        let Some(active) = self.active.as_mut() else {
            return Err(UiLeaseError::NotAttached);
        };
        if active.lease_id != lease_id || active.client_pid != client_pid {
            return Err(UiLeaseError::OwnerMismatch);
        }
        if observed_sequence < active.observed_sequence || observed_sequence > current_sequence {
            return Err(UiLeaseError::InvalidObservedSequence);
        }
        active.observed_sequence = observed_sequence;
        active.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
        Ok(active.clone())
    }

    pub(crate) fn reap_stale(
        &mut self,
        now_unix_ms: u64,
        mut process_is_alive: impl FnMut(u32) -> bool,
    ) -> Option<(UiLeaseRecord, &'static str)> {
        let active = self.active.as_ref()?;
        let reason = if active.expires_unix_ms <= now_unix_ms {
            "expired"
        } else if !process_is_alive(active.client_pid) {
            "client_exited"
        } else {
            return None;
        };
        self.active.take().map(|record| (record, reason))
    }
}

fn validate_client_id(client_id: &str) -> Result<(), UiLeaseError> {
    if client_id.is_empty()
        || client_id.len() > UI_CLIENT_ID_MAX_BYTES
        || client_id.chars().any(char::is_control)
    {
        return Err(UiLeaseError::InvalidClientId);
    }
    Ok(())
}

fn validate_lease_id(lease_id: &str) -> Result<(), UiLeaseError> {
    if lease_id.is_empty()
        || lease_id.len() > UI_CLIENT_ID_MAX_BYTES
        || lease_id.chars().any(char::is_control)
    {
        return Err(UiLeaseError::InvalidLeaseId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_live_client_owns_and_idempotently_renews_the_lease() {
        let mut authority = UiLeaseAuthority::default();
        let (first, created) = authority.attach("gui-a", 42, 1_000).unwrap();
        assert!(created);
        let (renewed, created) = authority.attach("gui-a", 42, 2_000).unwrap();
        assert!(!created);
        assert_eq!(renewed.lease_id, first.lease_id);
        assert!(renewed.expires_unix_ms > first.expires_unix_ms);
        assert_eq!(
            authority.attach("gui-b", 43, 2_000),
            Err(UiLeaseError::Conflict)
        );
    }

    #[test]
    fn heartbeat_and_detach_require_the_exact_owner() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, 1_000).unwrap();
        assert_eq!(
            authority.heartbeat(&lease.lease_id, 43, 2_000),
            Err(UiLeaseError::OwnerMismatch)
        );
        let renewed = authority.heartbeat(&lease.lease_id, 42, 2_000).unwrap();
        assert!(renewed.expires_unix_ms > lease.expires_unix_ms);
        assert_eq!(
            authority.detach(&lease.lease_id, 43),
            Err(UiLeaseError::OwnerMismatch)
        );
        assert_eq!(authority.detach(&lease.lease_id, 42).unwrap(), renewed);
        assert!(authority.active().is_none());
    }

    #[test]
    fn observed_sequence_is_monotonic_bounded_and_renews_the_owner() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, 1_000).unwrap();
        let acknowledged = authority
            .acknowledge(&lease.lease_id, 42, 7, 9, 2_000)
            .unwrap();
        assert_eq!(acknowledged.observed_sequence, 7);
        assert!(acknowledged.expires_unix_ms > lease.expires_unix_ms);
        assert_eq!(
            authority.acknowledge(&lease.lease_id, 42, 6, 9, 3_000),
            Err(UiLeaseError::InvalidObservedSequence)
        );
        assert_eq!(
            authority.acknowledge(&lease.lease_id, 42, 10, 9, 3_000),
            Err(UiLeaseError::InvalidObservedSequence)
        );
    }

    #[test]
    fn expired_or_dead_clients_are_recoverable_without_stealing_a_live_lease() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, 1_000).unwrap();
        assert!(authority.reap_stale(2_000, |_| true).is_none());
        assert_eq!(
            authority.reap_stale(2_000, |_| false),
            Some((lease.clone(), "client_exited"))
        );
        let (replacement, _) = authority.attach("gui-b", 43, 2_000).unwrap();
        assert_eq!(
            authority.reap_stale(replacement.expires_unix_ms, |_| true),
            Some((replacement, "expired"))
        );
    }
}
