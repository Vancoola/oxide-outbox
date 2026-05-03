use crate::object::EventId;

/// One entry tracked by [`DlqHeap`]: an event id together with its current
/// aggregated failure count.
///
/// The struct is `#[non_exhaustive]`: future revisions may add fields like
/// `last_failed_at` or `last_error` without breaking downstream callers.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct DlqEntry {
    pub id: EventId,
    pub failure_count: u32,
    pub last_error: Option<String>,
}

impl DlqEntry {
    #[must_use]
    pub fn new(id: EventId, failure_count: u32, last_error: Option<String>) -> Self {
        Self {
            id,
            failure_count,
            last_error,
        }
    }
}
