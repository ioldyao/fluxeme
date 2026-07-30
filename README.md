# Fluxeme AI Gateway

A reverse proxy gateway for LLM APIs. Compatible with OpenAI and Anthropic protocols — routes requests to upstream providers with channel management, load balancing, usage tracking, rate limiting, and a full admin UI.

[中文文档](./README_zh.md)

---

## Requirements

| Dependency | Notes |
|------------|-------|
| Docker & Docker Compose | Deployment |
| PostgreSQL 16 | Started by docker compose |
| Redis 7 | Started by docker compose |
| ClickHouse | Observability data store, decoupled from PostgreSQL (docker compose) |
| Jaeger | Distributed tracing (docker compose) |

---

## Quick Deploy

### 1. Configure environment

```bash
cp .env.example .env
```

Required — must be set (no defaults, gateway will panic if missing):

| Variable | Requirement | Example |
|----------|-------------|---------|
| `GATEWAY_JWT_SECRET` | Any string | `openssl rand -base64 32` |
| `GATEWAY_ENCRYPTION_KEY` | ≥32 chars, must differ from JWT secret | `openssl rand -base64 32` |
| `REDIS_PASSWORD` | Redis password | `openssl rand -base64 16` |
| `DB_PASSWORD` | PostgreSQL password | anything |

Optionally adjust `DB_DEPLOYMENT`, `CLICKHOUSE_DEPLOYMENT`, etc.

### 2. Start

```bash
make up
```

Open `http://localhost:8080` — the first visit will guide you through admin registration.

| Command | Description |
|---------|-------------|
| `make up` | Start all services |
| `make down` | Stop all services |
| `make logs` | Tail logs |
| `make restart` | Restart |
| `make build` | Rebuild images |

### 3. Configure models & channels

Via the admin UI:
1. **Channels** → add upstream providers (OpenAI, Anthropic, vLLM, etc.)
2. **Models** → add models and bind them to channels
3. **Model Marketplace** → publish models for users

---

## Manual Build (without Docker)

### Backend

```bash
# Requires Rust 1.88+, a running PostgreSQL and Redis
cp .env.example .env
# Edit .env — set DB_HOST, REDIS_PASSWORD etc. to your local instances

cargo build --release
./target/release/ai-gateway
```

### Frontend

```bash
cd ui
pnpm install
pnpm run dev       # Dev mode, proxies API to localhost:8080
pnpm run build     # Production build → ../web/
```

---

## Configuration

Main config file: `config/config.yaml`. Supports `${VAR}` and `${VAR:-default}` env var expansion.

```yaml
server:
  host: 0.0.0.0
  port: 8080

database:
  pg_url: ""                    # When empty, built from DB_USER/DB_PASSWORD/DB_HOST/DB_PORT/DB_NAME
  retention_days: 90            # Usage log retention

redis:
  enabled: true
  url: "redis://:${REDIS_PASSWORD}@127.0.0.1:16379"   # Note port 16379

jwt_secret: ${GATEWAY_JWT_SECRET}
encryption_key: ${GATEWAY_ENCRYPTION_KEY}
```

On first start, `channels`, `models`, and `routing_rules` defined in `config.yaml` are auto-seeded into the database.

> Runtime config (timeouts, retries, cache TTL, billing toggle) can be changed live via the **Settings** page.

---

## API

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

Also supports Anthropic format (`POST /v1/messages`) and any OpenAI-compatible SDK.

---

## Ports

| Port | Service |
|------|---------|
| 8080 | Fluxeme AI Gateway (proxy + admin UI) |
| 16379 | Redis |
| 5432 | PostgreSQL |
| 8123 | ClickHouse HTTP |
| 16686 | Jaeger UI |
