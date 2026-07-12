# AI Gateway

大语言模型 API 反向代理网关。提供统一的 OpenAI 兼容接口，将请求路由到上游供应商，支持渠道管理、负载均衡、用量跟踪和速率限制。

[English](./README.md)

## 功能

- **统一接口** — 兼容 OpenAI 和 Anthropic API 格式
- **渠道管理** — 多上游供应商加权负载均衡
- **模型广场** — 浏览和订阅模型
- **API 密钥管理** — 支持多密钥和用户绑定
- **用量跟踪** — Token 统计和费用图表
- **速率限制** — 按密钥和用户限流
- **Redis 缓存** — 非流式请求精确缓存
- **健康检查** — 监控上游模型连通性
- **SSO** — OIDC 单点登录

## 快速开始

### Docker Compose（推荐）

```bash
cp config/config.yaml config/config.local.yaml
# 按需修改配置
docker compose up -d
```

访问 `http://localhost:8080` 进入管理后台。

### 手动构建

```bash
# 后端
cargo build --release
./target/release/ai-gateway

# 前端（另开终端）
cd ui
pnpm install
pnpm run dev
```

## 配置

编辑 `config/config.yaml`。关键配置项：

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `server.host` | 监听地址 | `0.0.0.0` |
| `server.port` | 监听端口 | `8080` |
| `admin.username` | 管理员用户名 | `admin` |
| `admin.password` | 管理员密码 | `admin123` |
| `database.path` | 数据库路径 | `data/gateway.db` |
| `jwt_secret` | JWT 签名密钥 | `${GATEWAY_JWT_SECRET}` |
| `cache.enabled` | 启用缓存 | `false` |
| `cache.redis_url` | Redis 连接地址 | `redis://127.0.0.1:6379` |

配置文件中的 `${VAR_NAME}` 会自动从 `.env` 文件或环境变量中读取。

## 使用

### API 接口

兼容 OpenAI 和 Anthropic SDK：

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

### 管理后台

浏览器访问 `http://localhost:8080/`，使用配置文件中的管理员账号登录。

## 架构

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
