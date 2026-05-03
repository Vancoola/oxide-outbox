-- Adds the dead-letter destination table for events that exceeded the
-- configured failure threshold.
--
-- Rows arrive here exclusively via the DLQ reaper (see
-- `outbox_core::dlq::processor::DlqProcessor`), which atomically moves them
-- out of `outbox_events` and into this table in a single transaction.
-- Workers never read from this table — it is meant to be inspected (and
-- cleaned up) by operators.

create table outbox_dead_letters
(
    id                uuid        primary key,
    idempotency_token text                 default null,
    event_type        text        not null,
    payload           jsonb       not null,
    original_status   status      not null,
    created_at        timestamptz not null,
    locked_until      timestamptz not null,
    failure_count     integer     not null,
    quarantined_at    timestamptz not null default now(),
    last_error        text                 default null
);

create index idx_outbox_dlq_quarantined_at
    on outbox_dead_letters (quarantined_at desc);

create index idx_outbox_dlq_event_type
    on outbox_dead_letters (event_type);
