-- Outbox table — schema is identical to outbox-postgres/migrations/20260213152819_init.sql.
-- We copy it inline here so the example is self-contained; in your own project
-- you can either copy these statements or run sqlx::migrate! against the file
-- shipped with outbox-postgres.

create type status as enum (
    'Pending',
    'Processing',
    'Sent'
);

create table outbox_events
(
    id                uuid primary key     default gen_random_uuid(),
    idempotency_token text                 default null,
    event_type        text        not null,
    payload           jsonb       not null,
    status            status      not null default 'Pending',
    created_at        timestamptz not null default now(),
    locked_until      timestamptz not null default '-infinity'
);

create index idx_outbox_processing_queue
    on outbox_events (locked_until asc, status)
    where status in ('Pending', 'Processing');

create unique index idx_outbox_idempotency
    on outbox_events (idempotency_token);

create or replace function notify_outbox_event() returns trigger as
$$
begin
    perform pg_notify('outbox_event', 'ping');
    return new;
end;
$$ language plpgsql;

create trigger outbox_events_notify_trigger
    after insert or update
    on outbox_events
    for each row
execute function notify_outbox_event();
