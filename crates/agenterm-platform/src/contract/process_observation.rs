//! Product-neutral single-process liveness and start-identity facts.

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessObservation {
    Live { start_identity: Option<String> },
    Dead { reason: String },
    Unknown { reason: String },
}
