# ── Frontend build ──
FROM swr.cn-north-4.myhuaweicloud.com/ddn-k8s/docker.io/node:22-alpine AS frontend
ENV CI=true
RUN corepack enable && corepack prepare pnpm@10.29.1 --activate
WORKDIR /app

# 1. Workspace definition — pnpm needs this to resolve workspace links
COPY ui/pnpm-lock.yaml ui/package.json ui/pnpm-workspace.yaml ./

# 2. Sub-package manifests
COPY ui/packages/shared/package.json   ./packages/shared/package.json
COPY ui/packages/user-app/package.json  ./packages/user-app/package.json
COPY ui/packages/admin-app/package.json ./packages/admin-app/package.json

# 3. Install — now pnpm can link @fluxeme/shared → @fluxeme/user-app/admin-app
RUN pnpm config set registry https://registry.npmmirror.com && \
    pnpm install --frozen-lockfile

# 4. Source code
COPY ui/ ./

# 5. Build
RUN pnpm run build

# ── Backend build ──
FROM rust:1.88-alpine AS backend
COPY docker/cargo-config.toml /usr/local/cargo/config.toml
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache musl-dev openssl-dev pkgconfig openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Workspace 成员 manifest：依赖缓存阶段必须能解析 crates/*（否则 cargo 报
# "failed to load manifest for workspace member"）。
COPY crates/contract/Cargo.toml crates/contract/Cargo.toml
COPY crates/skillhub/Cargo.toml crates/skillhub/Cargo.toml
COPY crates/skill-backing/Cargo.toml crates/skill-backing/Cargo.toml
RUN mkdir -p src crates/contract/src crates/skillhub/src crates/skill-backing/src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > crates/contract/src/lib.rs && \
    echo "" > crates/skillhub/src/lib.rs && \
    echo "" > crates/skill-backing/src/lib.rs && \
    cargo build --release -j $(nproc) && \
    rm -rf src crates/contract/src crates/skillhub/src crates/skill-backing/src
COPY src/ src/
COPY crates/ crates/
# Docker COPY 保留宿主机 mtime：依赖缓存阶段用空 lib.rs 编译出的假产物可能
# 比真实源码"更新"，cargo 指纹会误判为已编译（导致引用到空 crate）。
# 强制 touch 全部 .rs 源，保证必然重编。
RUN find src crates -name '*.rs' -exec touch {} + && \
    cargo build --release -j $(nproc) && \
    strip target/release/fluxeme

# ── Runtime ──
FROM alpine:3.20
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.ustc.edu.cn/g' /etc/apk/repositories && \
    apk add --no-cache ca-certificates tzdata
WORKDIR /app
COPY --from=backend /app/target/release/fluxeme .
COPY --from=frontend /app/packages/user-app/dist ./web
# Copy shared public assets (fonts, icons) from the monorepo root
COPY --from=frontend /app/public/fonts ./web/fonts
COPY --from=frontend /app/public/icons ./web/icons
COPY --from=frontend /app/public/favicon.svg ./web/favicon.svg
COPY --from=frontend /app/public/icons.svg ./web/icons.svg
COPY --from=frontend /app/packages/admin-app/dist ./web/admin
EXPOSE 8080
CMD ["./fluxeme"]
