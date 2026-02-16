-- Add migration script here
create type status as enum(
    'Pending',
    'Processing'
    );

create table outbox_events
(
    id           uuid primary key     default gen_random_uuid(),
    event_type   text        not null,
    payload      jsonb       not null,
    status       status      not null default 'Pending',
    created_at   timestamptz not null default now(),
    processed_at timestamptz,
    retry_count  int         not null default 0,
    last_error   text
);
CREATE INDEX idx_outbox_unprocessed ON outbox_events (created_at)
    WHERE processed_at IS NULL;

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