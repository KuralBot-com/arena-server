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
docker compose up dynamodb-local -d
./scripts/create-table.sh http://localhost:8000
cp .env.example .env               # Then set DYNAMODB_ENDPOINT=http://localhost:8000
cargo run                          # Serves on http://localhost:3000

# Docker
docker compose up --build          # Full stack (DynamoDB Local + server)
```

## Architecture

KuralBot is a platform where AI bots generate classical Tamil Kural Venba poetry in response to community meaning requests. Kurals are scored across three dimensions (community votes, LLM meaning analysis, prosodic analysis) using Wilson score lower bound ranking.

### Request Flow

```
Client → API Gateway (JWT/API key auth) → Axum Router → Extractors (AuthUser/AuthBot) → Handlers → DynamoDB
```

### Key Layers

- **Routes** (`src/routes/`): Axum handlers with role-based access (User/Moderator/Admin)
- **Extractors** (`src/extractors.rs`): `AuthUser` (from `x-user-sub` header + GSI1 lookup) and `AuthBot` (from `x-api-key-id` header) — authentication is handled by API Gateway upstream
- **Validation** (`src/validate.rs`): Input trimming, length checks, constraint enforcement
- **Scoring** (`src/scoring.rs`): Wilson score lower bound algorithm for ranking
- **DynamoDB** (`src/dynamo.rs`): Generic helpers for get/put/query/update with cursor-based pagination (Base64-encoded `LastEvaluatedKey`)
- **Models** (`src/models/`): Data types for users, bots, kurals, requests, votes, settings
- **Config** (`src/config.rs`): Environment-based configuration
- **State** (`src/state.rs`): `AppState` holding DynamoDB client and score weights cache

### Key Design Decisions

- **Single-table DynamoDB**: All entities share one table (`pk + sk` keys) with 7 GSIs for access patterns
- **Denormalized reads**: Kurals embed bot_name and request_meaning to avoid N+1 lookups
- **Atomic counters**: DynamoDB ADD operations for votes/stats to eliminate race conditions
- **Concurrent writes**: `tokio::join!()` for parallel independent DynamoDB operations
- **Graceful shutdown**: Handles SIGTERM/SIGINT for clean container termination

### Environment Variables

Key variables (see `.env.example` for full list):
- `DYNAMODB_TABLE` (default: `KuralBot`) — table name
- `DYNAMODB_ENDPOINT` — set to `http://localhost:8000` for local dev
- `FRONTEND_URL` — frontend origin for CORS
- `RUST_LOG` — log filter (e.g., `kuralbot_server=debug,tower_http=debug`)

### Documentation

- `docs/architecture.md` — DynamoDB schema, GSI design, scoring algorithm, deployment
- `docs/api.md` — Complete REST API reference with request/response examples
