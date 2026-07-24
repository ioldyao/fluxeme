# ── Frontend build ──
FROM node:22-alpine AS frontend
ENV CI=true
RUN corepack enable && corepack prepare pnpm@latest --activate
WORKDIR /app
COPY ui/pnpm-lock.yaml ui/package.json ./
RUN pnpm install --frozen-lockfile
COPY ui/ .
RUN pnpm run build

# ── Backend build ──
FROM rust:1.88-alpine AS backend
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release -j $(nproc); \
    rm -rf src
COPY src/ src/
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache openssl-dev pkgconfig openssl-libs-static
RUN cargo build --release -j $(nproc) && \
    strip target/release/ai-gateway

# ── Runtime ──
FROM alpine:3.20
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache ca-certificates tzdata
WORKDIR /app
COPY --from=backend /app/target/release/ai-gateway .
COPY --from=frontend /web ./web
EXPOSE 8080
CMD ["./ai-gateway"]
