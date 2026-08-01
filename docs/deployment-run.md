# Fluxeme Deployment / Run Guide

## Overview

The repository now contains three layers:

- **Rust gateway backend**: `src/`
- **Portal frontend**: `apps/portal`
- **Admin frontend**: `apps/admin`

Shared frontend code lives in:

- `packages/ui`
- `packages/shared`
- `packages/auth-core`

### Runtime URLs

Default routing after the split:

- **Portal**: `/`
- **Admin**: `/admin`
- **Backend APIs**: `/api/*`, `/v1/*`, `/health`, `/tokenize`, `/detokenize`

### Build outputs

Frontend production bundles are emitted to:

- `web/portal`
- `web/admin`

The Rust server serves those two outputs separately:

- `/admin/*` → `web/admin/index.html`
- everything else in the portal surface → `web/portal/index.html`

---

## 1. Prerequisites

### Required

- Node.js compatible with the workspace toolchain
- `pnpm` (workspace package manager)
- Rust toolchain
- PostgreSQL
- Redis

### Optional

- ClickHouse for observability
- Jaeger / OTLP collector for tracing
- OIDC identity provider for SSO

---

## 2. Install dependencies

From the repository root:

```bash
pnpm install
```

This installs all workspace packages:

- `apps/portal`
- `apps/admin`
- `packages/*`

---

## 3. Local development

### 3.1 Frontend apps

From the repo root:

```bash
pnpm dev:portal
pnpm dev:admin
```

These map to:

- portal dev server: `apps/portal` on port `5173`
- admin dev server: `apps/admin` on port `5174`

The admin app is configured with a browser router basename of `/admin`.

### 3.2 Backend

Run the Rust gateway separately:

```bash
cargo run
```

Or verify compilation only:

```bash
cargo check
```

### 3.3 Full local stack with Docker Compose

The repository already includes a compose-based flow via `Makefile`.

Start the stack:

```bash
make up
```

Stop it:

```bash
make down
```

Tail logs:

```bash
make logs
```

Rebuild compose images:

```bash
make build
```

### Compose notes

`docker-compose.yml` starts:

- `gateway`
- `redis`
- `jaeger`

Database fragments are controlled by:

- `compose.psql.yml`
- `compose.clickhouse.yml`

The `Makefile` decides which fragments are included based on env vars like:

- `DB_DEPLOYMENT`
- `CLICKHOUSE_DEPLOYMENT`

---

## 4. Configuration

Primary example config:

- `config/config.yaml.example`

Runtime config path is controlled by:

```bash
GATEWAY_CONFIG=/app/config/config.yaml
```

### Required secrets

Set these before starting the gateway:

```bash
GATEWAY_JWT_SECRET=...
GATEWAY_ENCRYPTION_KEY=...
```

Optional rotation support:

```bash
GATEWAY_PREVIOUS_ENCRYPTION_KEY=...
```

### Database / Redis environment

Typical env vars used by the gateway/container:

```bash
DB_HOST=...
DB_PORT=5432
DB_NAME=aigateway
DB_USER=...
DB_PASSWORD=...
REDIS_PASSWORD=...
```

### ClickHouse environment

```bash
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=default
CLICKHOUSE_PASSWORD=
CLICKHOUSE_DB=aigateway
```

### Tracing

```bash
OTLP_ENDPOINT=http://localhost:4317
```

If unset, tracing export is disabled.

---

## 5. SSO configuration

SSO config lives under `sso:` in `config.yaml`.

Example fields:

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

### Important distinction

- `redirect_url` is the **OIDC callback endpoint** handled by the Rust backend
- `post_login_portal_url` is the **final destination for regular users**
- `post_login_admin_url` is the **final destination for admins**

### Current behavior

After SSO callback:

- backend sets the session cookie
- backend inspects the authenticated user role
- backend redirects directly to either portal or admin

This means frontend no longer needs to decide which app should own the session after callback.

---

## 6. Frontend build commands

From the repo root:

```bash
pnpm build:portal
pnpm build:admin
pnpm build:apps
```

Direct app commands also work:

```bash
pnpm --dir apps/portal build
pnpm --dir apps/admin build
```

Linting:

```bash
pnpm lint:portal
pnpm lint:admin
```

---

## 7. Backend build / verification

Compile-check backend:

```bash
cargo check
```

Run tests:

```bash
cargo test
```

---

## 8. Production deployment shape

### Recommended topology

- Rust gateway serves APIs and auth/session logic
- Portal bundle built into `web/portal`
- Admin bundle built into `web/admin`

Current server-side routing already supports this split.

### Recommended URL model

Current implementation supports:

- portal at `/`
- admin at `/admin`

If you later move to separate domains/subdomains, keep the same split conceptually:

- `app.example.com`
- `admin.example.com`

Then update:

- reverse proxy rules
- SSO post-login URLs
- allowed CORS origins

### Static file serving responsibility

Current Rust backend can serve both built apps directly.

Longer-term, you can move static assets to a dedicated web server or CDN while keeping Rust responsible for:

- API routing
- session cookies
- SSO callback handling
- role-based redirect after login

---

## 9. Release checklist

Before deploying a release:

```bash
pnpm install
pnpm --dir apps/portal build
pnpm --dir apps/admin build
cargo check
cargo test
```

Then verify:

- portal loads at `/`
- admin loads at `/admin`
- hard refresh works on both route trees
- login/logout works
- SSO redirects regular users to portal
- SSO redirects admins to admin
- API calls succeed from both apps

---

## 10. Quick commands reference

### Frontend

```bash
pnpm dev:portal
pnpm dev:admin
pnpm build:portal
pnpm build:admin
pnpm build:apps
pnpm lint:portal
pnpm lint:admin
```

### Backend

```bash
cargo run
cargo check
cargo test
```

### Compose / stack

```bash
make up
make down
make logs
make restart
make build
```

---

## 11. Current verified state

The following have already been verified in this branch:

- `cargo check` passes
- `pnpm --dir apps/portal build` passes
- `pnpm --dir apps/admin build` passes

So this document reflects the current working repository layout, not a planned future state.
