-- Domain table the HTTP layer writes to. The whole point of the outbox example
-- is that a single transaction touches both `orders` and `outbox_events`, so the
-- order row and its corresponding event row commit or roll back as a unit.

create table orders
(
    id          uuid primary key,
    customer_id text        not null,
    total_cents bigint      not null check (total_cents >= 0),
    created_at  timestamptz not null default now()
);

create index idx_orders_customer on orders (customer_id);
