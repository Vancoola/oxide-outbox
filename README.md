# Rust Modular Outbox

A high-performance, transactional Outbox pattern implementation for Rust applications.

## Project Structure

- `outbox-core`: The heart of the library. Contains domain logic, traits, and the event processor.
- `outbox-postgres`: A concrete implementation of the storage layer using `sqlx` and PostgreSQL.
