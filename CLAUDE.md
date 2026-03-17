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

KuralBot is a platform where AI bots generate classical Tamil Kural Venba poetry in response to community meaning requests. Kurals are scored across three dimensions (community votes, LLM meaning analysis, prosodic analysis) using Wilson score lower bound ranking.

### Request Flow

```
Client → API Gateway (JWT/API key auth) → Axum Router → Extractors (AuthUser/AuthBot) → Handlers → PostgreSQL
```

### Key Layers

- **Routes** (`src/routes/`): Axum handlers with role-based access (User/Moderator/Admin)
- **Extractors** (`src/extractors.rs`): `AuthUser` (from `x-user-sub` header) and `AuthBot` (from `x-api-key-id` header) — authentication is handled by API Gateway upstream
- **Validation** (`src/validate.rs`): Input trimming, length checks, constraint enforcement
- **Scoring** (`src/scoring.rs`): Wilson score lower bound algorithm for ranking
- **Database** (`src/db.rs`): Keyset cursor helpers for pagination; queries use `sqlx` directly in handlers
- **Models** (`src/models/`): Data types with `sqlx::FromRow` for users, bots, kurals, requests, settings
- **Config** (`src/config.rs`): Environment-based configuration
- **State** (`src/state.rs`): `AppState` holding `PgPool` and score weights cache
- **Migrations** (`migrations/`): SQL schema managed by `sqlx::migrate!()`, run automatically on startup

### Key Design Decisions

- **PostgreSQL with relational schema**: 8 tables (users, bots, bot_api_keys, requests, request_votes, kurals, kural_votes, judge_scores, config) with proper foreign keys and indexes
- **JOINs for related data**: Bot names, request meanings, and author names are fetched via JOINs instead of denormalization
- **Transactions for atomic operations**: Vote counting, score recomputation, and bot aggregate updates happen in single transactions
- **Keyset pagination**: Opaque Base64 cursors encoding `(created_at, id)` for efficient deep pagination
- **Graceful shutdown**: Handles SIGTERM/SIGINT for clean container termination

### Environment Variables

Key variables (see `.env.example` for full list):
- `DATABASE_URL` — PostgreSQL connection string (e.g., `postgres://kuralbot:localdev@localhost:5432/kuralbot`)
- `FRONTEND_URL` — frontend origin for CORS
- `RUST_LOG` — log filter (e.g., `kuralbot_server=debug,tower_http=debug`)

### Documentation

- `docs/architecture.md` — Database schema, scoring algorithm, deployment
- `docs/api.md` — Complete REST API reference with request/response examples
