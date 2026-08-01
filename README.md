# Fluxeme AI Gateway

Fluxeme is an LLM gateway for OpenAI-compatible and Anthropic-compatible APIs.
It provides:

- upstream channel and model management
- request routing and load balancing
- usage tracking and billing controls
- user-facing portal
- admin control plane

[中文文档](./README_zh.md)

---

## Current repository layout

This repository now uses a split frontend workspace:

- `src/` — Rust gateway backend
- `apps/portal` — user-facing frontend
- `apps/admin` — admin frontend
- `packages/ui` — shared UI primitives
- `packages/shared` — shared frontend utilities, types, i18n, query setup
- `packages/auth-core` — shared auth helper functions

Frontend production build outputs:

- `web/portal`
- `web/admin`

---

## Runtime model

### Routes

- Portal: `/`
- Admin: `/admin`
- Backend APIs: `/api/*`, `/v1/*`, `/health`, `/tokenize`, `/detokenize`

### Compose scheme C

The repository supports a three-service split for local / intranet deployment:

- `gateway` — Rust API backend
- `portal` — user frontend service
- `admin` — admin frontend service

This means you can rebuild and restart them independently.

---

## Prerequisites

| Dependency | Notes |
|---|---|
| Docker & Docker Compose | Main deployment path |
| Node.js + pnpm | Frontend workspace development |
| Rust toolchain | Backend local development |
| PostgreSQL 16 | Local or remote |
| Redis 7 | Required |
| ClickHouse | Optional but recommended for observability |
| Jaeger / OTLP collector | Optional tracing |

---

## Quick start with Compose

### 1. Prepare environment

```bash
cp .env.example .env
```

Minimum required variables:

| Variable | Requirement |
|---|---|
| `GATEWAY_JWT_SECRET` | Required |
| `GATEWAY_ENCRYPTION_KEY` | Required, at least 32 chars |
| `REDIS_PASSWORD` | Required |
| `DB_PASSWORD` | Required when using local Postgres |

Typical local values in `.env`:

```env
DB_DEPLOYMENT=local
CLICKHOUSE_DEPLOYMENT=local
```

### 2. Start everything

```bash
make up
```

### 3. Access services

Assuming the host machine is `localhost`:

- Portal: `http://localhost:8081/`
- Admin: `http://localhost:8082/admin`
- API: `http://localhost:8080/health`

---

## Compose commands

### Full stack

| Command | Description |
|---|---|
| `make up` | Start API + portal + admin + infra |
| `make down` | Stop the stack |
| `make logs` | Tail logs |
| `make restart` | Restart the full stack |
| `make build` | Rebuild compose images |

### Rebuild only one service

| Command | Description |
|---|---|
| `make api` | Rebuild/restart Rust backend only |
| `make portal` | Rebuild/restart portal only |
| `make admin` | Rebuild/restart admin only |

This is the main benefit of scheme C:

- changing the admin frontend does **not** require rebuilding the backend
- changing the portal frontend does **not** require rebuilding the admin frontend

For full details, see:

- `docs/compose-scheme-c.md`
- `docs/deployment-run.md`

---

## Local development

### Install workspace dependencies

From the repository root:

```bash
pnpm install
```

### Start frontends in dev mode

```bash
pnpm dev:portal
pnpm dev:admin
```

Default dev ports:

- portal: `5173`
- admin: `5174`

### Start backend in dev mode

```bash
cargo run
```

### Build frontends manually

```bash
pnpm build:portal
pnpm build:admin
# or
pnpm build:apps
```

### Verify backend

```bash
cargo check
cargo test
```

---

## Configuration

Primary config file:

- `config/config.yaml`

Example template:

- `config/config.yaml.example`

The config loader supports:

- `${VAR}`
- `${VAR:-default}`

### Important runtime settings

```yaml
server:
  host: 0.0.0.0
  port: 8080

database:
  pg_url: ""
  retention_days: 90

redis:
  enabled: true
  url: "redis://:${REDIS_PASSWORD}@127.0.0.1:16379"

jwt_secret: ${GATEWAY_JWT_SECRET}
encryption_key: ${GATEWAY_ENCRYPTION_KEY}
```

### SSO settings

```yaml
sso:
  enabled: false
  provider_name: SSO
  issuer_url: https://your-idp.example.com
  client_id: your-client-id
  client_secret: your-client-secret
  redirect_url: http://localhost:8080/api/sso/callback
  post_login_portal_url: http://localhost:8080/
  post_login_admin_url: http://localhost:8080/admin
```

Important:

- `redirect_url` is the OIDC callback endpoint handled by the backend
- `post_login_portal_url` is the final destination for regular users
- `post_login_admin_url` is the final destination for admins

---

## API example

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

Also supports:

- `POST /v1/messages`
- OpenAI-compatible SDKs
- Anthropic-compatible SDKs

---

## Default ports

| Port | Service |
|---|---|
| 8080 | Rust gateway API |
| 8081 | Portal frontend |
| 8082 | Admin frontend |
| 16379 | Redis |
| 5432 | PostgreSQL |
| 8123 | ClickHouse HTTP |
| 16686 | Jaeger UI |

---

## Verified state in this branch

Verified in the current split setup:

- `cargo check` passes
- `pnpm --dir apps/portal build` passes
- `pnpm --dir apps/admin build` passes
- compose config validates for:
  - `docker-compose.yml`
  - `docker-compose.yml + compose.psql.yml + compose.clickhouse.yml`
