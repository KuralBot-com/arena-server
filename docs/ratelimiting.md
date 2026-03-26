# Rate Limiting Strategy

## Problem

The current rate limiter applies a single token bucket per IP (`SmartIpKeyExtractor`, 10 burst / 5 per second) uniformly to all API routes. This causes:

- **Legitimate users hit limits within seconds** — a single page load fires 5-10 parallel API calls (requests list, topics, criteria, leaderboard, user profile), exhausting the burst immediately.
- **Users behind shared IPs are penalized** — corporate NATs, VPNs, university networks, and mobile carriers share one bucket across dozens or hundreds of users.
- **No differentiation by intent** — a user browsing the homepage and a scraper paginating through all prompts are treated identically.

## Objective

Protect against automated scraping of prompts, kurals, and vote data while allowing normal website browsing to feel unrestricted. A legitimate user should never encounter a rate limit during normal use.

## Threat Model

The primary threat is **anonymous automated scraping** — scripts that:
- Paginate through `GET /requests` and `GET /responses` to harvest all content
- Enumerate detail endpoints (`GET /requests/{id}`) sequentially to scrape individual items with votes
- Run without authentication to avoid identity linkage

Lower-priority threats:
- Authenticated users running scraping scripts (traceable, can be banned)
- Agents making excessive API calls (revocable API keys)

## Design Principles

1. **Authenticated users are trusted** — their identity is verified and abuse is traceable/bannable. Give them generous limits keyed by identity, not IP.
2. **Anonymous users are untrusted** — rate limit by IP, but differentiate by what they're accessing and how cacheable it is.
3. **Read vs write matters** — reads are the scraping vector; writes need protection against spam but are already gated by auth.
4. **Data staleness matters** — cached/stable endpoints (topics, criteria, settings) are cheap to serve and safe to allow generously. Real-time list/detail endpoints are the scraping target.

## Prerequisites

### Server-Side Auth Validation

> **Status: Required before implementing tiered rate limiting.**

The server currently trusts the `x-user-sub` header without validation — it was previously set by API Gateway after JWT verification. With the move to ALB + frontend OAuth2, this header is **forgeable**. A scraper can send `x-user-sub: <any-value>` to bypass anonymous rate limits.

Before implementing identity-based rate limiting, the server must validate authentication itself. Options:

| Option | Mechanism | Pros | Cons |
|--------|-----------|------|------|
| **ALB OIDC headers** | ALB authenticates via OIDC, forwards signed `x-amzn-oidc-data` JWT | No code needed for auth flow; ALB-signed JWT is tamper-proof | Tied to AWS ALB; need to verify ALB signature |
| **Server-side JWT validation** | Frontend sends OAuth2 ID token; server verifies signature against provider JWKS | Provider-agnostic; works with any proxy | Need JWKS caching, multi-provider support |
| **Session tokens** | Frontend exchanges OAuth2 token for a server-issued session | Full control; decoupled from OAuth2 provider | Need session storage (DB or Redis) |

Whichever option is chosen, the rate limit key extractor needs a **lightweight auth check** (JWT signature verification with cached keys, or session lookup) that runs before the handler — without a full DB round-trip on every request.

## Caller Classification

Every incoming request is classified into one of three tiers based on auth:

| Tier | Identification | Rate Limit Key | Trust Level |
|------|---------------|----------------|-------------|
| **Authenticated User** | Verified JWT / session with valid `sub` claim | `user:{user_id}` | High — identity verified, traceable, bannable |
| **Authenticated Agent** | `Authorization: Bearer <api_key>` with valid key hash in DB | `agent:{key_hash}` | High — API key is revocable |
| **Anonymous** | No valid auth token | `anon:{ip}` | Low — unknown identity |

Key design: **authenticated users are keyed by user ID, not IP**. This means 50 employees behind one corporate NAT each get their own bucket. Anonymous users are keyed by IP since there's no better identifier.

## Endpoint Classification

Every API endpoint (excluding health checks) falls into one of five categories:

### Cached Stable (low scrape risk, high staleness tolerance)

Data that changes infrequently and is already served with cache headers. Safe to allow generously even for anonymous users — these are the first things a landing page loads.

| Endpoint | Cache TTL | Notes |
|----------|-----------|-------|
| `GET /stats` | 300s | Aggregate counts |
| `GET /topics` | 60s | Topic list with counts |
| `GET /criteria` | 60s | Scoring criteria definitions |
| `GET /leaderboard/agents` | 60s | Agent rankings |
| `GET /settings/vote-weight` | 300s | Global config |
| `GET /agents/{id}` | 60s | Single agent metadata |
| `GET /users/{id}` | 60s | Public profile |
| `GET /requests/{id}/topics` | 10s | Topic assignments |
| `GET /responses/{id}/scores` | — | Scoring breakdown |

### Public Lists (high scrape risk)

Paginated list endpoints returning multiple items per page. These are the primary scraping target — a script paginates through cursors to harvest all content.

| Endpoint | Notes |
|----------|-------|
| `GET /requests` | Paginated prompt list with vote counts |
| `GET /responses` | Paginated response list with scores |
| `GET /requests/{id}/comments` | Comment threads |
| `GET /responses/{id}/comments` | Comment threads |

### Public Detail (high scrape risk)

Individual resource endpoints. Scraping pattern: enumerate IDs sequentially.

| Endpoint | Notes |
|----------|-------|
| `GET /requests/{id}` | Single prompt with full vote data |
| `GET /responses/{id}` | Single response with scores |
| `GET /criteria/{id}` | Single criterion |

### Authenticated Read (no scrape risk)

Endpoints that require authentication. Rate limiting protects against misbehaving clients, not scraping.

| Endpoint | Notes |
|----------|-------|
| `GET /users/me` | Own profile |
| `GET /agents` | Own agents list |
| `GET /agents/{id}/credentials` | Own credentials |

### Write (mutation)

All `POST`, `PUT`, `PATCH`, `DELETE` operations. Already gated by auth (user or agent). Rate limiting prevents spam and accidental double-submits.

| Endpoint Category | Examples |
|-------------------|----------|
| User write | `POST /requests`, `POST /comments`, `POST /{id}/vote`, `PATCH /users/me` |
| Agent write | `POST /responses`, `POST /evaluations` |
| Admin write | `POST /criteria`, `PUT /settings/vote-weight`, `PATCH /requests/{id}` (status) |

## Rate Limit Matrix

Values are `burst_size / refill_per_second`. Burst is the maximum number of requests that can be made instantly; refill is how many tokens are added per second.

```
                      Cached     Public     Public     Auth
Caller                Stable     Lists      Detail     Read       Write
─────────────────────────────────────────────────────────────────────────
Authenticated User    100/30     100/30     100/30     100/30     20/5
  (by user_id)

Authenticated Agent   —          —          —          —          60/20
  (by agent_id)

Anonymous             40/15      10/2       15/3       —          —
  (by IP)
─────────────────────────────────────────────────────────────────────────
```

### How to read this

- **Authenticated User, any read endpoint**: 100 burst, 30/sec refill. A page load firing 10 parallel requests uses 10% of burst. Normal browsing will never hit this limit. Keyed by `user_id`, so shared IPs don't matter.
- **Anonymous, Cached Stable**: 40 burst, 15/sec. A first-time visitor's landing page loads topics, criteria, leaderboard, stats — roughly 6 requests. Uses 15% of burst. Comfortable.
- **Anonymous, Public Lists**: 10 burst, 2/sec. This is the scraper-targeting bucket. A user browsing and clicking "next page" a few times is fine. A script paginating through 1000 pages is throttled to ~2 pages/sec — takes 8+ minutes, making bulk scraping impractical.
- **Anonymous, Public Detail**: 15 burst, 3/sec. A user clicking into a few individual requests is fine. Sequential ID enumeration is slowed to 3 items/sec.
- **Agent write**: 60 burst, 20/sec. Agents submit responses and evaluations in batches. Separate bucket per `agent_id`.
- **User write**: 20 burst, 5/sec. Prevents vote-spamming and comment flooding.

### Why authenticated users get uniform read limits

Once a user is authenticated, their identity is known. Differentiating between "cached stable" and "public list" reads adds complexity without meaningful protection — if a logged-in user abuses the system, the response is banning the account, not fine-grained rate limiting. A single generous read bucket per user keeps things simple.

## Implementation Architecture

### Route Groups

Replace the current single `GovernorLayer` on all API routes with five route groups, each with its own governor config:

```
health_routes           → no rate limit (unchanged)
cached_stable_routes    → GovernorLayer(cached_config)
public_list_routes      → GovernorLayer(list_config)
public_detail_routes    → GovernorLayer(detail_config)
auth_read_routes        → GovernorLayer(auth_read_config)
write_routes            → GovernorLayer(write_config)
```

### Custom Key Extractor

A single `ArenaKeyExtractor` replaces `SmartIpKeyExtractor`. For each request it:

1. Checks for a valid user auth token (JWT signature verification or session lookup — **no DB round-trip**)
   - If valid → returns `user:{user_id}`
2. Checks for `Authorization: Bearer` header and validates agent key hash
   - If valid → returns `agent:{key_hash}`
3. Falls back to IP extraction
   - Returns `anon:{ip}` (using the same logic as `SmartIpKeyExtractor` for `X-Forwarded-For` / `X-Real-IP` handling behind ALB)

Since `tower-governor` creates separate buckets per key, authenticated users and anonymous users naturally get independent rate limits even when sharing the same governor config.

### Per-Tier Limit Override

The key extractor can return different governor configs based on the caller tier. When the key starts with `user:`, the governor applies the authenticated-user limits. When it starts with `anon:`, it applies the anonymous limits. This is achieved by embedding the tier in the key and using separate governor instances per route group.

### Configuration

```bash
# Authenticated user limits (by user_id)
RATE_LIMIT_USER_READ_BURST=100
RATE_LIMIT_USER_READ_PER_SEC=30
RATE_LIMIT_USER_WRITE_BURST=20
RATE_LIMIT_USER_WRITE_PER_SEC=5

# Authenticated agent limits (by agent_id)
RATE_LIMIT_AGENT_BURST=60
RATE_LIMIT_AGENT_PER_SEC=20

# Anonymous limits (by IP) — per endpoint category
RATE_LIMIT_ANON_CACHED_BURST=40
RATE_LIMIT_ANON_CACHED_PER_SEC=15
RATE_LIMIT_ANON_LIST_BURST=10
RATE_LIMIT_ANON_LIST_PER_SEC=2
RATE_LIMIT_ANON_DETAIL_BURST=15
RATE_LIMIT_ANON_DETAIL_PER_SEC=3

# Cleanup interval for evicting stale rate limiter entries
RATE_LIMIT_CLEANUP_SECS=60
```

## Response Headers

All rate-limited responses include standard headers (already exposed via CORS):

```
X-RateLimit-Limit: 40          # Bucket capacity
X-RateLimit-Remaining: 35      # Tokens remaining
X-RateLimit-After: 0           # Seconds until a token is available
Retry-After: 2                 # (Only on 429) Seconds to wait
```

## Edge Cases

### CORS Preflight (OPTIONS)

`OPTIONS` requests must be exempt from rate limiting. The browser sends these automatically before cross-origin requests — counting them against the budget would halve the effective limit.

### Health Checks

`/health`, `/health/live`, `/health/ready` remain exempt (already the case).

### Mixed Auth Endpoints

Endpoints like `GET /requests` use `MaybeAuthUser` — they work for both authenticated and anonymous users. The key extractor determines the caller tier and applies the appropriate bucket:
- Authenticated user → `user:{id}` with generous limits
- Anonymous → `anon:{ip}` with the Public Lists limit

### Agent Endpoints Serving User Requests

Agents authenticate via API key but only access write endpoints (`POST /responses`, `POST /evaluations`). They don't hit read endpoints, so only the write bucket applies.

## Scraping Deterrence Summary

| Scraping Pattern | Mitigation |
|------------------|------------|
| Paginate all requests/responses | Anonymous Public Lists: 10 burst, 2/sec — 1000 pages takes 8+ minutes |
| Enumerate detail IDs | Anonymous Public Detail: 15 burst, 3/sec — 10,000 items takes 55+ minutes |
| Forge auth headers to bypass limits | Server-side JWT validation — forged headers rejected before rate limit tier upgrade |
| Distribute across IPs | Out of scope for application-level rate limiting — use WAF/CloudFront |
| Authenticated scraping | User identity is known — detect via access patterns, ban account |
| Agent key abuse | API key is revocable — revoke credential |

## Future Considerations

- **Adaptive rate limiting** — if an anonymous IP consistently exhausts the Public Lists bucket, progressively lower their limits or require CAPTCHA.
- **WAF integration** — AWS WAF or CloudFront rate limiting for distributed scraping from many IPs (bot farms).
- **Read-your-writes exemption** — after a user creates a request, exempt the immediate `GET /requests/{id}` redirect from the detail bucket (relevant only for anonymous, which can't create requests, so likely not needed).
