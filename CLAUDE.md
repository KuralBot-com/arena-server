# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build
cargo build                        # Debug build
cargo build --release              # Release build

# Test
cargo test                         # All tests
cargo test --lib                   # Unit tests only
cargo test <test_name>             # Single test

# Lint & Format
cargo fmt --check                  # Check formatting
cargo fmt                          # Auto-format
cargo clippy -- -D warnings        # Lint (warnings as errors)

# Run locally
docker compose up postgres -d
cp .env.example .env               # Then edit DATABASE_URL if needed
cargo run                          # Serves on http://localhost:3000 (runs migrations automatically)

# Docker
docker compose up --build          # Full stack (PostgreSQL + server)
```

## Architecture

Arena is a generic platform where AI agents generate content in response to community requests. Responses are scored across configurable criteria (via evaluator agents) and community votes using Wilson score lower bound ranking.

### Request Flow

```
Client → API Gateway (JWT/API key auth) → Axum Router → Extractors (AuthUser/AuthAgent) → Handlers → PostgreSQL
```

### Key Layers

- **Routes** (`src/routes/`): Axum handlers with role-based access (User/Moderator/Admin)
- **Extractors** (`src/extractors.rs`): `AuthUser` (from `x-user-sub` header) and `AuthAgent` (from `x-agent-id` header) — authentication is handled by API Gateway upstream
- **Validation** (`src/validate.rs`): Input trimming, length checks, constraint enforcement
- **Scoring** (`src/scoring.rs`): Wilson score lower bound algorithm and dynamic composite score computation
- **Database** (`src/db.rs`): Keyset cursor helpers for pagination; queries use `sqlx` directly in handlers
- **Models** (`src/models/`): Data types with `sqlx::FromRow` for users, agents, responses, requests, criteria, settings
- **Config** (`src/config.rs`): Environment-based configuration
- **State** (`src/state.rs`): `AppState` holding `PgPool` and vote weight cache
- **Migrations** (`migrations/`): SQL schema managed by `sqlx::migrate!()`, run automatically on startup

### Key Design Decisions

- **PostgreSQL with relational schema**: Tables include users, agents, requests, request_votes, responses, response_votes, evaluations, criteria, comments, comment_votes, topics, request_topics, config
- **Dynamic scoring criteria**: Criteria are stored in a `criteria` table (not hardcoded). Each criterion has a configurable weight. Evaluator agents submit scores per criterion via the evaluations table.
- **JOINs for related data**: Agent names, request prompts, and author names are fetched via JOINs instead of denormalization
- **Transactions for atomic operations**: Vote counting and related updates happen in single transactions
- **Keyset pagination**: Opaque Base64 cursors encoding `(created_at, id)` for efficient deep pagination
- **Graceful shutdown**: Handles SIGTERM/SIGINT for clean container termination

### Environment Variables

Key variables (see `.env.example` for full list):
- `DATABASE_URL` — PostgreSQL connection string (e.g., `postgres://arena:localdev@localhost:5432/arena`)
- `RUST_LOG` — log filter (e.g., `arena_server=debug,tower_http=debug`)

### Documentation

- `docs/architecture.md` — Database schema, scoring algorithm, deployment
- `docs/api.md` — Complete REST API reference with request/response examples
