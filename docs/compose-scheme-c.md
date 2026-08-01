# Compose Scheme C (backend / portal / admin split)

This repository now supports a three-service Compose layout:

- `gateway` → Rust API backend
- `portal` → user-facing frontend service
- `admin` → admin frontend service

## Ports

Default local / intranet ports:

- API: `http://<host>:8080`
- Portal: `http://<host>:8081`
- Admin: `http://<host>:8082/admin`

## Full startup

```bash
make up
```

This starts:

- gateway
- portal
- admin
- redis
- jaeger
- postgres (if `DB_DEPLOYMENT=local`)
- clickhouse (if `CLICKHOUSE_DEPLOYMENT=local`)

## Full shutdown

```bash
make down
```

## Logs

```bash
make logs
```

## Rebuild / restart only one service

### Backend API only

```bash
make api
```

Equivalent to:

```bash
docker compose up -d --build gateway
```

### Portal only

```bash
make portal
```

Equivalent to:

```bash
docker compose up -d --build portal
```

### Admin only

```bash
make admin
```

Equivalent to:

```bash
docker compose up -d --build admin
```

This is the main benefit of scheme C: changing the admin frontend no longer requires rebuilding the Rust backend or the portal frontend.

## Build all images without starting

```bash
make build
```

Or build one target explicitly:

```bash
docker compose build gateway
docker compose build portal
docker compose build admin
```

## Runtime behavior

### gateway

- serves API only
- no longer needs to host portal/admin static bundles for Compose scheme C usage
- still contains the backend routes and SSO callback handling

### portal

- serves static files from `web/portal`
- proxies API requests to `http://127.0.0.1:8080`
- mounted at `/`

### admin

- serves static files from `web/admin`
- proxies API requests to `http://127.0.0.1:8080`
- mounted at `/admin`

## Typical workflow

### If you changed Rust backend only

```bash
make api
```

### If you changed portal only

```bash
make portal
```

### If you changed admin only

```bash
make admin
```

### If you changed shared frontend packages used by both apps

Rebuild both:

```bash
make portal
make admin
```

### If you changed auth / SSO behavior in backend and want everything fresh

```bash
make api
make portal
make admin
```

## Notes

- Compose still uses `network_mode: host`, so the services bind directly to host ports.
- If a port is already occupied, the corresponding service will fail to start.
- SSO callback still terminates in the backend, then redirects users to the configured portal/admin post-login URLs.
