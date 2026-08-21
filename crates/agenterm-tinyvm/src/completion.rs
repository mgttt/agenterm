//! Bounded, event-loop-neutral completion ownership for native modules.
//!
//! A platform starts work by reserving one request and its maximum response
//! bytes. The worker or native API remains outside tinyvm; it returns its
//! result to the owning event loop, which completes the request here. No
//! thread, executor, wake primitive, or platform API is embedded in the VM.

use alloc::vec::Vec;

use crate::{GuestResourceHandle, HostResourceTable, ResourceTableError};

/// Stable results returned by the reusable versioned guest import protocol.
pub const COMPLETION_PENDING: i32 = 0;
pub const COMPLETION_READY: i32 = 1;
pub const COMPLETION_STALE: i32 = 2;
pub const COMPLETION_BUFFER_TOO_SMALL: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionError {
    Full,
    AllocationFailed,
    StaleHandle,
    AlreadyCompleted,
    NotReady,
    PayloadTooLarge,
    ByteBudgetExceeded,
}

/// A rejected completion returns ownership of its payload to the host.
#[derive(Debug)]
pub struct CompletionRejection {
    pub error: CompletionError,
    pub payload: Vec<u8>,
}

/// One completed native operation, removed from the queue.
#[derive(Debug)]
pub struct HostCompletion {
    pub status: i32,
    pub payload: Vec<u8>,
}

pub enum CompletionPoll<'a> {
    Pending,
    Ready { status: i32, payload: &'a [u8] },
}

pub(crate) enum CompletionState {
    Pending {
        reserved_bytes: usize,
    },
    Ready {
        reserved_bytes: usize,
        status: i32,
        payload: Vec<u8>,
    },
}

/// Single-owner completion queue shared by every platform host.
///
/// The queue is deliberately not an executor and is not synchronized. A
/// platform may do work on any mechanism it owns, then marshal completion onto
/// the runtime's owner/event-loop thread before calling [`Self::try_complete`].
/// Request handles inherit the resource table's domain/generation identity and
/// therefore cannot alias a sibling or replacement runtime.
pub struct HostCompletionQueue {
    table: HostResourceTable<CompletionState>,
    max_reserved_bytes: usize,
    reserved_bytes: usize,
}

impl HostCompletionQueue {
    /// Create a queue with a fresh process-lifetime handle domain.
    pub fn with_domain_allocator(
        max_pending: u16,
        max_reserved_bytes: usize,
        allocator: &mut crate::ResourceDomainAllocator,
    ) -> Result<Self, CompletionError> {
        let domain = allocator.claim().map_err(map_table_error)?;
        let (table, _) =
            crate::HostResourceTable::new_tracked(domain, max_pending).map_err(map_table_error)?;
        Ok(Self::new(table, max_reserved_bytes))
    }

    pub(crate) fn new(
        table: HostResourceTable<CompletionState>,
        max_reserved_bytes: usize,
    ) -> Self {
        Self {
            table,
            max_reserved_bytes,
            reserved_bytes: 0,
        }
    }

    pub const fn len(&self) -> u16 {
        self.table.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub const fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }

    pub const fn max_reserved_bytes(&self) -> usize {
        self.max_reserved_bytes
    }

    pub const fn domain(&self) -> crate::ResourceHandleDomain {
        self.table.domain()
    }

    pub(crate) fn activity(&self) -> crate::resource_table::ResourceActivity {
        self.table
            .activity()
            .expect("completion queues always use tracked resource tables")
    }

    /// Reserve one stable request identity and its complete response allowance
    /// before external work starts.
    pub fn begin(
        &mut self,
        max_payload_bytes: usize,
    ) -> Result<GuestResourceHandle, CompletionError> {
        let next = self
            .reserved_bytes
            .checked_add(max_payload_bytes)
            .ok_or(CompletionError::ByteBudgetExceeded)?;
        if next > self.max_reserved_bytes {
            return Err(CompletionError::ByteBudgetExceeded);
        }
        let handle = self
            .table
            .insert(CompletionState::Pending {
                reserved_bytes: max_payload_bytes,
            })
            .map_err(map_table_error)?;
        self.reserved_bytes = next;
        Ok(handle)
    }

    /// Publish a result without copying its payload. Rejection returns payload
    /// ownership so the platform can diagnose, retry elsewhere, or drop it.
    pub fn try_complete(
        &mut self,
        handle: GuestResourceHandle,
        status: i32,
        payload: Vec<u8>,
    ) -> Result<(), CompletionRejection> {
        let state = match self.table.get_mut(handle) {
            Ok(state) => state,
            Err(error) => {
                return Err(CompletionRejection {
                    error: map_table_error(error),
                    payload,
                });
            }
        };
        match state {
            CompletionState::Pending { reserved_bytes } if payload.len() <= *reserved_bytes => {
                *state = CompletionState::Ready {
                    reserved_bytes: *reserved_bytes,
                    status,
                    payload,
                };
                Ok(())
            }
            CompletionState::Pending { .. } => Err(CompletionRejection {
                error: CompletionError::PayloadTooLarge,
                payload,
            }),
            CompletionState::Ready { .. } => Err(CompletionRejection {
                error: CompletionError::AlreadyCompleted,
                payload,
            }),
        }
    }

    pub fn poll(&self, handle: GuestResourceHandle) -> Result<CompletionPoll<'_>, CompletionError> {
        match self.table.get(handle).map_err(map_table_error)? {
            CompletionState::Pending { .. } => Ok(CompletionPoll::Pending),
            CompletionState::Ready {
                status, payload, ..
            } => Ok(CompletionPoll::Ready {
                status: *status,
                payload,
            }),
        }
    }

    /// Remove one ready result and release its full reserved byte allowance.
    pub fn take(&mut self, handle: GuestResourceHandle) -> Result<HostCompletion, CompletionError> {
        if matches!(
            self.table.get(handle).map_err(map_table_error)?,
            CompletionState::Pending { .. }
        ) {
            return Err(CompletionError::NotReady);
        }
        let CompletionState::Ready {
            reserved_bytes,
            status,
            payload,
        } = self.table.remove(handle).map_err(map_table_error)?
        else {
            return Err(CompletionError::NotReady);
        };
        self.reserved_bytes -= reserved_bytes;
        Ok(HostCompletion { status, payload })
    }

    /// Cancel either a pending or completed request and invalidate its handle.
    pub fn cancel(&mut self, handle: GuestResourceHandle) -> Result<(), CompletionError> {
        let state = self.table.remove(handle).map_err(map_table_error)?;
        self.reserved_bytes -= match state {
            CompletionState::Pending { reserved_bytes }
            | CompletionState::Ready { reserved_bytes, .. } => reserved_bytes,
        };
        Ok(())
    }

    pub fn clear(&mut self) {
        self.table.clear();
        self.reserved_bytes = 0;
    }
}

fn map_table_error(error: ResourceTableError) -> CompletionError {
    match error {
        ResourceTableError::Full => CompletionError::Full,
        ResourceTableError::AllocationFailed => CompletionError::AllocationFailed,
        ResourceTableError::StaleHandle => CompletionError::StaleHandle,
        ResourceTableError::DomainExhausted | ResourceTableError::InvalidLimit => {
            CompletionError::AllocationFailed
        }
    }
}
