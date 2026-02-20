-- Add migration script here
create type status as enum (
    'Pending',
    'Processing',
    'Sent'
    );

create table outbox_events
(
    id           uuid primary key     default gen_random_uuid(),
    event_type   text        not null,
    payload      jsonb       not null,
    status       status      not null default 'Pending',
    created_at   timestamptz not null default now(),
    locked_until timestamptz not null default '-infinity'
);
CREATE INDEX idx_outbox_processing_queue
    ON outbox_events (locked_until ASC, status)
    WHERE status IN ('Pending', 'Processing');

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