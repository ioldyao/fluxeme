# AI Gateway

A reverse proxy gateway for large language model APIs. Provides a unified OpenAI-compatible endpoint that routes requests to upstream providers (OpenAI, Anthropic, etc.) with channel management, load balancing, usage tracking, and rate limiting.

## Features

- **Unified API** — Single endpoint compatible with OpenAI and Anthropic API formats
- **Channel Management** — Route requests to multiple upstream providers with weight-based load balancing
- **Model Marketplace** — Browse, subscribe, and manage models through the admin UI
- **API Key Management** — Multi-key support with user binding
- **Usage Tracking** — Token counting, cost calculation, and aggregate charts
- **Rate Limiting** — Per-key and per-user rate limits
- **Redis Caching** — Exact cache for non-streaming requests
- **Health Checks** — Monitor upstream model connectivity
- **SSO** — OIDC-based single sign-on
- **User Management** — Admin panel for managing users and permissions

## Quick Start

### Prerequisites

- Docker & Docker Compose (recommended), or
- Rust 1.88+ and Node.js 22+

### Docker Compose (recommended)

```bash
# Clone and configure
cp config/config.yaml config/config.local.yaml
# Edit config/config.local.yaml as needed

# Start with Redis
docker compose up -d
```

The gateway will be available at `http://localhost:8080`.

### Manual Build

```bash
# Backend
cargo build --release
./target/release/ai-gateway

# Frontend (separate terminal)
cd ui
pnpm install
pnpm run dev
```

## Configuration

Configuration is done via `config/config.yaml`. Key settings:

| Setting | Description | Default |
|---------|-------------|---------|
| `server.host` | Listen address | `0.0.0.0` |
| `server.port` | Listen port | `8080` |
| `admin.username` | Admin login username | `admin` |
| `admin.password` | Admin login password | `admin123` |
| `database.path` | SQLite database path | `data/gateway.db` |
| `jwt_secret` | JWT signing secret (use env var) | `${GATEWAY_JWT_SECRET}` |
| `cache.enabled` | Enable Redis cache | `false` |
| `cache.redis_url` | Redis connection URL | `redis://127.0.0.1:6379` |

Environment variables in config (`${VAR_NAME}`) are resolved at startup from `.env` or environment.

## Usage

### API Endpoints

The gateway proxies requests at `/v1/chat/completions` and `/v1/messages`, compatible with the OpenAI and Anthropic SDKs:

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

### Admin UI

The web admin interface is available at `http://localhost:8080/`. Log in with the admin credentials from your config.

## Architecture

```
                    ┌─────────────┐
                    │   Clients   │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  AI Gateway │
                    │  (Axum/Rust)│
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ OpenAI   │ │Anthropic │ │  vLLM    │
        │ Channel  │ │ Channel  │ │ Channel  │
        └──────────┘ └──────────┘ └──────────┘
```
