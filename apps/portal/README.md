# Fluxeme Portal

User-facing frontend application for Fluxeme.

## Development

From the repository root:

```bash
pnpm dev:portal
```

Or directly from this app directory:

```bash
pnpm dev
```

Default dev port: `5173`

## Build

From the repository root:

```bash
pnpm build:portal
```

Or locally in this app directory:

```bash
pnpm build
```

Build output is written to:

- `../../web/portal`

## Routing

- App basename: none
- Portal is mounted at `/`

## Related apps and packages

- Admin frontend: `../admin`
- Shared UI primitives: `../../packages/ui`
- Shared frontend utilities/types/i18n: `../../packages/shared`
- Shared auth helpers: `../../packages/auth-core`
