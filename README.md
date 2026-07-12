# AI Gateway

A reverse proxy gateway for large language model APIs. Provides a unified OpenAI-compatible endpoint that routes requests to upstream providers with channel management, load balancing, usage tracking, and rate limiting.

大语言模型 API 反向代理网关。提供统一的 OpenAI 兼容接口，将请求路由到上游供应商，支持渠道管理、负载均衡、用量跟踪和速率限制。

## Features / 功能

- **Unified API** — Single endpoint compatible with OpenAI and Anthropic API formats / 统一接口，兼容 OpenAI 和 Anthropic API 格式
- **Channel Management** — Route requests to multiple upstream providers with weight-based load balancing / 渠道管理，多上游供应商加权负载均衡
- **Model Marketplace** — Browse, subscribe, and manage models through the admin UI / 模型广场，浏览和订阅模型
- **API Key Management** — Multi-key support with user binding / API 密钥管理，支持多密钥和用户绑定
- **Usage Tracking** — Token counting, cost calculation, and aggregate charts / 用量跟踪，Token 统计和费用图表
- **Rate Limiting** — Per-key and per-user rate limits / 速率限制，按密钥和用户限流
- **Redis Caching** — Exact cache for non-streaming requests / Redis 缓存，非流式请求精确缓存
- **Health Checks** — Monitor upstream model connectivity / 健康检查，监控上游模型连通性
- **SSO** — OIDC-based single sign-on / OIDC 单点登录

## Quick Start / 快速开始

### Docker Compose (recommended / 推荐)

```bash
# Clone and configure
cp config/config.yaml config/config.local.yaml
# Edit config/config.local.yaml as needed / 按需修改配置

# Start with Redis / 启动（含 Redis）
docker compose up -d
```

The gateway will be available at `http://localhost:8080`.
访问 `http://localhost:8080` 进入管理后台。

### Manual Build / 手动构建

```bash
# Backend / 后端
cargo build --release
./target/release/ai-gateway

# Frontend / 前端（另开终端）
cd ui
pnpm install
pnpm run dev
```

## Configuration / 配置

Edit `config/config.yaml`. Key settings / 关键配置项：

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `server.host` | Listen address / 监听地址 | `0.0.0.0` |
| `server.port` | Listen port / 监听端口 | `8080` |
| `admin.username` | Admin login / 管理员用户名 | `admin` |
| `admin.password` | Admin login / 管理员密码 | `admin123` |
| `database.path` | SQLite path / 数据库路径 | `data/gateway.db` |
| `jwt_secret` | JWT signing secret / JWT 签名密钥 | `${GATEWAY_JWT_SECRET}` |
| `cache.enabled` | Enable Redis cache / 启用缓存 | `false` |
| `cache.redis_url` | Redis URL / Redis 连接地址 | `redis://127.0.0.1:6379` |

Environment variables in config (`${VAR_NAME}`) are resolved from `.env` or environment.
配置文件中的 `${VAR_NAME}` 会自动从 `.env` 文件或环境变量中读取。

## Usage / 使用

### API Endpoints / API 接口

Compatible with OpenAI and Anthropic SDKs / 兼容 OpenAI 和 Anthropic SDK：

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

### Admin UI / 管理后台

Open `http://localhost:8080/` in browser. Log in with admin credentials from config.
浏览器访问 `http://localhost:8080/`，使用配置文件中的管理员账号登录。

## Architecture / 架构

```
                    ┌─────────────┐
                    │   Clients   │
                    │    客户端    │
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
