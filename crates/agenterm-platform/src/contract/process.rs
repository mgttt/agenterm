//! OS-neutral process facts and typed failures consumed by facade services.

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessObservation {
    Live { start_identity: Option<String> },
    Dead { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessInfo {
    pub id: u32,
    pub parent_id: u32,
    pub executable_name: String,
}

#[allow(dead_code)] // Consumed only by the selected Unix process adapters.
pub(crate) fn transitive_descendant_ids(root_id: u32, processes: &[ProcessInfo]) -> Vec<u32> {
    use std::collections::HashSet;

    let mut seen = HashSet::from([root_id]);
    let mut parents = vec![root_id];
    let mut descendants = Vec::new();
    while let Some(parent_id) = parents.pop() {
        for process in processes {
            if process.parent_id == parent_id && seen.insert(process.id) {
                descendants.push(process.id);
                parents.push(process.id);
            }
        }
    }
    descendants.reverse();
    descendants
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipeProbeToken(pub(crate) usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PipeProbeError {
    Closed,
    Failed,
}

#[allow(dead_code)] // A target builds the full three-adapter error contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessErrorKind {
    IdOutOfRange,
    Inventory,
    InventoryTooLarge,
    KillOpen,
    Kill,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessError {
    kind: ProcessErrorKind,
    detail: String,
}

impl ProcessError {
    pub(crate) fn new(kind: ProcessErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProcessErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "process {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for ProcessError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_error_preserves_typed_kind_and_diagnostic() {
        let error = ProcessError::new(ProcessErrorKind::Unsupported, "adapter unavailable");
        assert_eq!(error.kind, ProcessErrorKind::Unsupported);
        assert_eq!(error.detail, "adapter unavailable");
    }

    #[test]
    fn descendant_walk_is_transitive_deepest_first_and_cycle_safe() {
        let processes = [
            ProcessInfo {
                id: 20,
                parent_id: 10,
                executable_name: "child".to_owned(),
            },
            ProcessInfo {
                id: 30,
                parent_id: 20,
                executable_name: "grandchild".to_owned(),
            },
            ProcessInfo {
                id: 40,
                parent_id: 99,
                executable_name: "unrelated".to_owned(),
            },
            ProcessInfo {
                id: 10,
                parent_id: 30,
                executable_name: "cycle".to_owned(),
            },
        ];
        assert_eq!(transitive_descendant_ids(10, &processes), vec![30, 20]);
    }
}
