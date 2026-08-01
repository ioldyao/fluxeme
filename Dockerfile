# ── Frontend build ──
FROM swr.cn-north-4.myhuaweicloud.com/ddn-k8s/docker.io/node:22-alpine AS frontend-builder
ENV CI=true
RUN corepack enable && corepack prepare pnpm@latest --activate
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps ./apps
COPY packages ./packages
RUN pnpm install --frozen-lockfile
RUN pnpm build:apps

# ── Backend build ──
FROM rust:1.88-alpine AS backend-builder
COPY docker/cargo-config.toml /usr/local/cargo/config.toml
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache musl-dev openssl-dev pkgconfig openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release -j $(nproc) && \
    rm -rf src
COPY src/ src/
RUN touch src/main.rs && \
    cargo build --release -j $(nproc) && \
    strip target/release/fluxeme

# ── API runtime ──
FROM alpine:3.20 AS api
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache ca-certificates tzdata
WORKDIR /app
COPY --from=backend-builder /app/target/release/fluxeme .
EXPOSE 8080
CMD ["./fluxeme"]

# ── Shared frontend runtime ──
FROM swr.cn-north-4.myhuaweicloud.com/ddn-k8s/docker.io/node:22-alpine AS frontend-runtime
WORKDIR /app
COPY docker/frontend-server.mjs /app/frontend-server.mjs
COPY --from=frontend-builder /app/web ./web

# ── Portal runtime ──
FROM frontend-runtime AS portal
ENV PORT=8081 \
    STATIC_ROOT=/app/web/portal \
    ROUTE_BASE=/ \
    API_ORIGIN=http://127.0.0.1:8080
EXPOSE 8081
CMD ["node", "/app/frontend-server.mjs"]

# ── Admin runtime ──
FROM frontend-runtime AS admin
ENV PORT=8082 \
    STATIC_ROOT=/app/web/admin \
    ROUTE_BASE=/admin \
    API_ORIGIN=http://127.0.0.1:8080
EXPOSE 8082
CMD ["node", "/app/frontend-server.mjs"]
