# ── Frontend build ──
FROM swr.cn-north-4.myhuaweicloud.com/ddn-k8s/docker.io/node:22-alpine AS frontend
ENV CI=true
RUN corepack enable && corepack prepare pnpm@10.29.1 --activate
WORKDIR /app

# 1. Workspace definition files — pnpm needs these to resolve workspace links
COPY ui/pnpm-lock.yaml ui/package.json ui/pnpm-workspace.yaml ./

# 2. Workspace sub-package manifests
COPY ui/packages/shared/package.json   ./packages/shared/package.json
COPY ui/packages/user-app/package.json  ./packages/user-app/package.json
COPY ui/packages/admin-app/package.json ./packages/admin-app/package.json

# 3. Install — now pnpm can link @fluxeme/shared → @fluxeme/user-app/admin-app
RUN pnpm install --frozen-lockfile

# 4. Source code
COPY ui/ ./

# 5. Build
RUN pnpm run build
