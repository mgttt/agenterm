//! Interactive replaceable-UI leases for GUI clients.
//!
//! Multiple live GUI clients may hold concurrent leases on the same server.
//! Each lease is identified by `lease_id` and verified by matching `client_pid`.
//! CLI / mux / Control Center do not use this lease surface.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{UpgradeIdentity, ui_bridge::UI_CLIENT_ID_MAX_BYTES};

pub(crate) const UI_LEASE_TTL_MS: u64 = 5_000;

/// Soft upper bound on concurrent interactive GUI leases per server.
pub(crate) const UI_LEASE_MAX_CLIENTS: usize = 16;

static NEXT_LEASE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiLeaseRecord {
    pub(crate) lease_id: String,
    pub(crate) client_id: String,
    pub(crate) client_pid: u32,
    pub(crate) client_build: Option<UpgradeIdentity>,
    pub(crate) expires_unix_ms: u64,
    pub(crate) observed_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiLeaseError {
    InvalidClientId,
    InvalidClientPid,
    InvalidLeaseId,
    /// Capacity only — not "another owner blocks you".
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
            Self::Conflict => "too many concurrent interactive UI leases on this server",
            Self::NotAttached => "no live UI lease matches the supplied identity",
            Self::OwnerMismatch => "UI lease ID or client PID does not match a live lease",
            Self::InvalidObservedSequence => {
                "UI observed sequence must advance monotonically within the current journal"
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UiLeaseAuthority {
    leases: Vec<UiLeaseRecord>,
}

impl UiLeaseAuthority {
    /// First live lease if any (status / visibility convenience).
    pub(crate) fn active(&self) -> Option<&UiLeaseRecord> {
        self.leases.first()
    }

    pub(crate) fn leases(&self) -> &[UiLeaseRecord] {
        &self.leases
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.leases.is_empty()
    }

    pub(crate) fn verify_owner(
        &self,
        lease_id: &str,
        client_pid: u32,
    ) -> Result<&UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        self.leases
            .iter()
            .find(|lease| lease.lease_id == lease_id && lease.client_pid == client_pid)
            .ok_or(if self.leases.is_empty() {
                UiLeaseError::NotAttached
            } else {
                UiLeaseError::OwnerMismatch
            })
    }

    fn find_mut(
        &mut self,
        lease_id: &str,
        client_pid: u32,
    ) -> Result<&mut UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        let empty = self.leases.is_empty();
        self.leases
            .iter_mut()
            .find(|lease| lease.lease_id == lease_id && lease.client_pid == client_pid)
            .ok_or(if empty {
                UiLeaseError::NotAttached
            } else {
                UiLeaseError::OwnerMismatch
            })
    }

    pub(crate) fn attach(
        &mut self,
        client_id: &str,
        client_pid: u32,
        client_build: Option<UpgradeIdentity>,
        now_unix_ms: u64,
    ) -> Result<(UiLeaseRecord, bool), UiLeaseError> {
        validate_client_id(client_id)?;
        if client_pid == 0 {
            return Err(UiLeaseError::InvalidClientPid);
        }
        // Renew exact client identity (same GUI process).
        if let Some(existing) = self
            .leases
            .iter_mut()
            .find(|lease| lease.client_id == client_id && lease.client_pid == client_pid)
        {
            existing.client_build = client_build;
            existing.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
            return Ok((existing.clone(), false));
        }
        if self.leases.len() >= UI_LEASE_MAX_CLIENTS {
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
            client_build,
            expires_unix_ms: now_unix_ms.saturating_add(UI_LEASE_TTL_MS),
            observed_sequence: 0,
        };
        self.leases.push(record.clone());
        Ok((record, true))
    }

    pub(crate) fn heartbeat(
        &mut self,
        lease_id: &str,
        client_pid: u32,
        now_unix_ms: u64,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        let lease = self.find_mut(lease_id, client_pid)?;
        lease.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
        Ok(lease.clone())
    }

    pub(crate) fn detach(
        &mut self,
        lease_id: &str,
        client_pid: u32,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        validate_lease_id(lease_id)?;
        let position = self
            .leases
            .iter()
            .position(|lease| lease.lease_id == lease_id && lease.client_pid == client_pid)
            .ok_or(if self.leases.is_empty() {
                UiLeaseError::NotAttached
            } else {
                UiLeaseError::OwnerMismatch
            })?;
        Ok(self.leases.remove(position))
    }

    pub(crate) fn acknowledge(
        &mut self,
        lease_id: &str,
        client_pid: u32,
        observed_sequence: u64,
        current_sequence: u64,
        now_unix_ms: u64,
    ) -> Result<UiLeaseRecord, UiLeaseError> {
        let lease = self.find_mut(lease_id, client_pid)?;
        if observed_sequence < lease.observed_sequence || observed_sequence > current_sequence {
            return Err(UiLeaseError::InvalidObservedSequence);
        }
        lease.observed_sequence = observed_sequence;
        lease.expires_unix_ms = now_unix_ms.saturating_add(UI_LEASE_TTL_MS);
        Ok(lease.clone())
    }

    /// Reap every stale lease; returns detached records with reasons.
    pub(crate) fn reap_stale(
        &mut self,
        now_unix_ms: u64,
        mut process_is_alive: impl FnMut(u32) -> bool,
    ) -> Vec<(UiLeaseRecord, &'static str)> {
        let mut removed = Vec::new();
        self.leases.retain(|lease| {
            let reason = if lease.expires_unix_ms <= now_unix_ms {
                Some("expired")
            } else if !process_is_alive(lease.client_pid) {
                Some("client_exited")
            } else {
                None
            };
            if let Some(reason) = reason {
                removed.push((lease.clone(), reason));
                false
            } else {
                true
            }
        });
        removed
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

    fn test_build(profile: &str) -> UpgradeIdentity {
        UpgradeIdentity {
            protocol_version: Some(1),
            version: Some("0.1.9".to_owned()),
            git_commit: Some("a".repeat(40)),
            profile: Some(profile.to_owned()),
            cargo_lock_sha256: Some("b".repeat(64)),
            artifact_manifest_sha256: Some("c".repeat(64)),
        }
    }

    #[test]
    fn multiple_live_clients_may_attach_concurrently() {
        let mut authority = UiLeaseAuthority::default();
        let (first, created) = authority.attach("gui-a", 42, None, 1_000).unwrap();
        assert!(created);
        let (renewed, created) = authority.attach("gui-a", 42, None, 2_000).unwrap();
        assert!(!created);
        assert_eq!(renewed.lease_id, first.lease_id);
        assert!(renewed.expires_unix_ms > first.expires_unix_ms);

        let (second, created) = authority.attach("gui-b", 43, None, 2_000).unwrap();
        assert!(created);
        assert_ne!(second.lease_id, first.lease_id);
        assert_eq!(authority.leases().len(), 2);
        assert!(authority.verify_owner(&first.lease_id, 42).is_ok());
        assert!(authority.verify_owner(&second.lease_id, 43).is_ok());
    }

    #[test]
    fn lease_retains_the_actual_build_of_its_current_owner() {
        let mut authority = UiLeaseAuthority::default();
        let first_build = test_build("dev");
        let (first, _) = authority
            .attach("gui-a", 42, Some(first_build.clone()), 1_000)
            .unwrap();
        assert_eq!(first.client_build, Some(first_build));

        let replacement_build = test_build("release-fast");
        let (renewed, created) = authority
            .attach("gui-a", 42, Some(replacement_build.clone()), 2_000)
            .unwrap();
        assert!(!created);
        assert_eq!(renewed.lease_id, first.lease_id);
        assert_eq!(renewed.client_build, Some(replacement_build));
    }

    #[test]
    fn heartbeat_and_detach_require_the_exact_owner() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, None, 1_000).unwrap();
        let (other, _) = authority.attach("gui-b", 43, None, 1_000).unwrap();
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
        assert_eq!(authority.leases().len(), 1);
        assert_eq!(authority.active().map(|l| l.lease_id.as_str()), Some(other.lease_id.as_str()));
    }

    #[test]
    fn owner_verification_is_read_only_and_exact() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, None, 1_000).unwrap();
        assert_eq!(
            authority
                .verify_owner(&lease.lease_id, 42)
                .unwrap()
                .expires_unix_ms,
            lease.expires_unix_ms
        );
        assert_eq!(
            authority.verify_owner(&lease.lease_id, 43),
            Err(UiLeaseError::OwnerMismatch)
        );
        assert_eq!(
            authority.verify_owner("bad\nlease", 42),
            Err(UiLeaseError::InvalidLeaseId)
        );
    }

    #[test]
    fn observed_sequence_is_monotonic_bounded_and_renews_the_owner() {
        let mut authority = UiLeaseAuthority::default();
        let (lease, _) = authority.attach("gui", 42, None, 1_000).unwrap();
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
    fn expired_or_dead_clients_are_reaped_independently() {
        let mut authority = UiLeaseAuthority::default();
        let (live, _) = authority.attach("gui-live", 42, None, 1_000).unwrap();
        let (dead, _) = authority.attach("gui-dead", 43, None, 1_000).unwrap();
        // Only pid 43 is dead; do not reap by expiry yet.
        let removed = authority.reap_stale(1_500, |pid| pid != 43);
        assert_eq!(removed, vec![(dead, "client_exited")]);
        assert_eq!(authority.leases().len(), 1);
        assert_eq!(
            authority.active().map(|l| l.lease_id.as_str()),
            Some(live.lease_id.as_str())
        );
        let removed = authority.reap_stale(live.expires_unix_ms, |_| true);
        assert_eq!(removed, vec![(live, "expired")]);
        assert!(authority.is_empty());
    }

    #[test]
    fn capacity_bounds_concurrent_leases() {
        let mut authority = UiLeaseAuthority::default();
        for index in 0..UI_LEASE_MAX_CLIENTS {
            authority
                .attach(&format!("gui-{index}"), 100 + index as u32, None, 1_000)
                .unwrap();
        }
        assert_eq!(
            authority.attach("gui-overflow", 999, None, 1_000),
            Err(UiLeaseError::Conflict)
        );
    }
}
