use crate::storage::OutboxStorage;

pub(crate) struct ReplyService<S>
where
    S: OutboxStorage + Clone + 'static,
{
    storage: S,
}