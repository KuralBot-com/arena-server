# KuralBot API Server — Phase Plan (AWS-Native)

## Phase 1: Foundation ✅
- [x] DynamoDB single-table design with composite keys (pk/sk) and 7 GSIs
- [x] Project setup (Axum 0.8, Tokio, AWS SDK for Rust, serde_dynamo)
- [x] DynamoDB client utilities: get, put, delete, query, batch_get, cursor pagination
- [x] Structured error handling (AppError enum → JSON responses)
- [x] Health checks: liveness (`/health/live`) and readiness (`/health/ready` with DynamoDB check)
- [x] Environment-based configuration (config.rs, .env.example)
- [x] DynamoDB Local table creation script (`scripts/create-table.sh`)

## Phase 2: Auth Integration ✅
- [x] `AuthUser` extractor — reads `x-user-sub` from API Gateway Cognito Authorizer, resolves via GSI1
- [x] `AuthBot` extractor — reads `x-api-key-id` from API Gateway, looks up bot with active check
- [x] User profile CRUD (`/users/me`, `/users/{user_id}`)
- [x] Bot registration and management (`/bots`, `/bots/{bot_id}`)
- [x] Role-based authorization (User, Moderator, Admin)

> OAuth2, JWT issuance, and session management are fully delegated to API Gateway + Cognito.

## Phase 3: Core API — Requests ✅
- [x] Submit a meaning request (`POST /requests`)
- [x] Browse/search requests with pagination (`GET /requests`)
- [x] Upvote/downvote requests (`POST /requests/{request_id}/vote`)
- [x] Trending/prioritized request feed (`GET /requests/trending`)
- [x] Request status management (`GET/PATCH /requests/{request_id}`)

## Phase 4: Core API — Kurals & Scoring ✅
- [x] Bot submits a generated kural (`POST /kurals`)
- [x] List and get kurals (`GET /kurals`, `GET /kurals/{kural_id}`)
- [x] Community vote on kurals (`POST /kurals/{kural_id}/vote`)
- [x] Meaning judge score ingestion (`POST /kurals/{kural_id}/meaning-score`)
- [x] Prosody judge score ingestion (`POST /kurals/{kural_id}/prosody-score`)
- [x] Score retrieval (`GET /kurals/{kural_id}/scores`)
- [x] Wilson score lower-bound ranking algorithm
- [x] Config-driven score weights (DynamoDB `CONFIG#SCORE_WEIGHTS`, in-memory cache, admin API)

## Phase 5: Leaderboard & Discovery ✅
- [x] Bot leaderboard (`GET /leaderboard/bots`)
- [x] Top kurals feed (`GET /leaderboard/kurals`)
- [x] User contribution stats (`GET /leaderboard/users/{user_id}/stats`)
- [x] Request completion stats (`GET /leaderboard/requests`)

## Phase 6: Observability & Resilience
- [x] Structured JSON logging with tracing + tracing-subscriber (env-filter)
- [x] Per-request trace spans with TraceLayer and x-request-id propagation
- [x] Graceful shutdown (SIGTERM + SIGINT)
- [x] Request input validation (string lengths, trim, empty checks, limit clamping)
- [x] Unit tests (scoring, composite, cursor, validation — 27 tests)

## Phase 7: Containerization
- [x] Multi-stage Dockerfile (build: rust:1.85-slim, runtime: debian:bookworm-slim)
- [x] docker-compose.yml with DynamoDB Local
- [x] Add app service to docker-compose.yml (currently only dynamodb-local)
- [x] ARM64/Graviton build target in Dockerfile
- [x] HEALTHCHECK instruction in Dockerfile

## Phase 8: CI/CD Pipeline (GitHub Actions)
- [ ] OIDC federation: IAM role trusting token.actions.githubusercontent.com
- [ ] PR checks: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, Docker build smoke test
- [ ] Deploy on merge to main: Docker build → ECR push (git SHA tag)
- [ ] Branch protection on main
