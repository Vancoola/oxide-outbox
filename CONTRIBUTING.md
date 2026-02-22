# Contributing to Oxide Outbox 🦀

First off, thank you for considering contributing to Oxide Outbox! It’s people like you who make the Rust ecosystem so robust.

Below are the guidelines to help you get started and ensure a smooth contribution process.

---

## Getting Started

### Prerequisites
- **Rust Toolchain**: You’ll need the latest stable version of Rust (installed via [rustup](https://rustup.rs/)).
- **Docker**: Used for running integration tests (Postgres/Redis).
- **sqlx-cli**: Required if you are modifying database schemas in `outbox-postgres`.

### Local Setup
1. Clone the repository:
   ```bash
   git clone https://github.com/Vancoola/oxide-outbox
   cd oxide-outbox
   ```
2. Start the development dependencies:
    ```bash
   docker-compose up -d
    ```
3. Prepare the Database:
   Oxide Outbox requires a PostgreSQL database and specific tables.
   ```bash
   export DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/oxide
   cd outbox-postgres
   sqlx migrate run
   cd ..
   ```

4. Run tests to ensure everything is working:
    ```bash
   cargo test --all-features
   ```
   
---

## Project Architecture
Oxide Outbox is a `workspace-based` project:
- `outbox-core`: Contains the primary traits and the OutboxManager. Logic here must remain agnostic of specific databases.
- `outbox-postgres`: Implementation of the storage trait using sqlx.
- `outbox-redis`: Implementation of the idempotency provider.

---

## Branching Policy
1. `main`: The stable branch. All releases are tagged from here.
2. **Feature Branches**: Please use descriptive names: `feat/add-kafka-provider`, `fix/gc-interval-calculation`.

---

## Contribution Workflow
1. **Open an Issue**: For any major changes, please open an issue first to discuss the design. Small fixes can go straight to a PR.
2. Write Your Changes
3. **Add Tests**:
   - Unit tests go in the same file as the code.
   - Integration tests go in the tests/ directory within each crate.
4. **Format & Lint**:
    ```bash
   cargo clippy --all-targets --all-features -- -D warnings
    ```
5. **Submit a PR**: Provide a clear description of what you changed and why.

---

## Roadmap & High-Impact Areas
- Built-in DLQ / max-retries / poison message handling
- Additional publishers: Kafka, RabbitMQ, NATS, Redis Streams
- Metrics + tracing (opentelemetry)
- More examples (real-world patterns, error handling)

---

## Questions?
If you're unsure about where to start or how a specific part of the system works, feel free to open a Discussion or reach out in the issues!