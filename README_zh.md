# AI Gateway

大语言模型 API 反向代理网关。兼容 OpenAI/Anthropic 协议，支持多供应商路由、渠道管理、负载均衡、用量跟踪、限流和完整管理后台。

[English](./README.md)

---

## 部署要求

| 依赖 | 说明 |
|------|------|
| Docker & Docker Compose | 部署方式 |
| PostgreSQL 16 | 主数据库（docker compose 自动启动）|
| Redis 7 | 缓存和消息队列（docker compose 自动启动）|
| ClickHouse | 观测数据存储，与 PostgreSQL 解耦（docker compose 自动启动）|
| Jaeger | 分布式追踪（docker compose 自动启动）|

---

## 快速部署

### 1. 配置环境变量

```bash
cp .env.example .env
```

必须修改以下值（没有默认值，缺失会启动失败）：

| 变量 | 要求 | 生成示例 |
|------|------|----------|
| `GATEWAY_JWT_SECRET` | 任意字符串 | `openssl rand -base64 32` |
| `GATEWAY_ENCRYPTION_KEY` | ≥32字符，不能和 JWT 相同 | `openssl rand -base64 32` |
| `REDIS_PASSWORD` | Redis 密码 | `openssl rand -base64 16` |
| `DB_PASSWORD` | PostgreSQL 密码 | 随意设置 |

其他变量按需调整（`DB_DEPLOYMENT`、`CLICKHOUSE_DEPLOYMENT` 等）。

### 2. 启动

```bash
make up
```

访问 `http://localhost:8080`，首次打开会引导注册管理员账号。

| 命令 | 说明 |
|------|------|
| `make up` | 启动全部服务 |
| `make down` | 停止全部服务 |
| `make logs` | 查看日志 |
| `make restart` | 重启 |
| `make build` | 重新构建镜像 |

### 3. 配置模型和渠道

启动后通过管理后台操作：
1. 登录后进入 **Channels** 页面 → 添加上游供应商（OpenAI / Anthropic / vLLM 等）
2. 进入 **Models** 页面 → 添加模型并绑定渠道
3. 进入 **Model Marketplace** → 发布模型给用户使用

---

## 手动构建（不通过 Docker）

### 后端

```bash
# 需要 Rust 1.88+，PostgreSQL 和 Redis 需自行启动
cp .env.example .env
# 编辑 .env，配置 DB_HOST、REDIS_PASSWORD 等指向本地实例

cargo build --release
./target/release/ai-gateway
```

### 前端

```bash
cd ui
pnpm install
pnpm run dev       # 开发模式，API 代理到 localhost:8080
pnpm run build     # 生产构建，输出到 ../web/
```

---

## 配置说明

主配置文件：`config/config.yaml`。支持 `${VAR}` 和 `$(VAR:-default}` 语法引用环境变量。

```yaml
server:
  host: 0.0.0.0
  port: 8080

database:
  pg_url: ""                    # 为空时从 DB_USER/DB_PASSWORD/DB_HOST/DB_PORT/DB_NAME 拼装
  retention_days: 90            # 用量日志保留天数

redis:
  enabled: true
  url: "redis://:${REDIS_PASSWORD}@127.0.0.1:16379"   # 注意端口 16379

jwt_secret: ${GATEWAY_JWT_SECRET}
encryption_key: ${GATEWAY_ENCRYPTION_KEY}
```

首次启动时，`config.yaml` 里的 `channels`、`models`、`routing_rules` 配置会自动写入数据库（种子数据）。

> 管理后台运行时配置（超时、重试、缓存 TTL、计费开关）可在 **Settings** 页面实时修改。

---

## API 使用

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]}'
```

也支持 Anthropic 格式（`POST /v1/messages`）和 OpenAI 兼容 SDK。

---

## 端口占用

| 端口 | 服务 |
|------|------|
| 8080 | AI Gateway（代理 + 管理后台）|
| 16379 | Redis |
| 5432 | PostgreSQL |
| 8123 | ClickHouse HTTP |
| 16686 | Jaeger UI |
