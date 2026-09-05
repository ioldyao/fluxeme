# Gateway Request Lifecycle Observability

状态：已确认设计，正在实现。2026-09-05。

## 原则

> 所有经过 LLM 数据面且成功建立 RequestContext 的请求，必须产生且只产生一条
> Gateway Request Event；每次实际访问上游，必须产生一条 Gateway Attempt Event；
> 是否计费由 Billing/Settlement 独立决定。

- 观测数据存 ClickHouse；计费事实（billing_events / 钱包 / 资源包）存 PostgreSQL。
- `/admin/usage` 主数据是 **Request Log**，不是成功调用的 token 账单。
- 失败请求 = 0 token / 0 实扣，但必须有日志。
- 迁移期间旧 `usage_events` 与 billing 链路保持兼容，逐步收敛。

## 三层事件

| 事件 | 粒度 | 何时写 |
|---|---|---|
| `gateway_access_events` | 1 条 / 认证前或非推理请求 | 入口安全层 / 非推理 API |
| `gateway_request_events` | 恰好 1 条 / 认证后的 LLM 请求 | 统一 finalize（含 Drop 兜底） |
| `gateway_attempt_events` | 1 条 / 真实上游调用 | 每次 retry / fallback 之前完成前一条终态 |

### 边界

- 无效 API Key / 缺失 Authorization / 无法识别主体 → Access Event（不进 `/admin/usage`）。
- 认证成功后进入 LLM 数据面 → 必有一条 Request Event：
  模型不存在 / 无权限 / 无可用端点 / 余额不足 / 配额不足 / RPM-TPM 限流 /
  Guardrail / 参数或协议转换失败 / 上游错误 / timeout / retry-fallback /
  客户端中断 / 网关内部错误。
- Authentication vs Authorization：无效 Key → Access；有效 Key 无模型/团队权限 → Request, rejected。
- 鉴权前 IP/Ingress 防刷限流 → Access；鉴权后用户级 RPM/TPM → Request。
- `/health`、`/v1/models` 等非推理 API → 仅 Access Log。
- 现有 `request_id` 已在 HTTP 入口（authenticate 前）生成，直接贯穿 Access →
  Request → Attempt，不引入 ingress_id。
- `gateway_access_events` 不保存原始 Authorization / API Key / request body，只存
  脱敏 fingerprint、IP、路径、鉴权结果、错误类型、状态码、延迟；TTL 较短（7 天）。

### Request Event 核心字段（语义已定）

```
request_id / user_id / user_name / team_id / api_key_name / api_format
method / path / stream / client_ip / user_agent
requested_model   # 用户原始请求（恒有值）
resolved_model    # 重写/解析后（可空）
channel_id / endpoint_id / endpoint_url / upstream_model / provider
status            # succeeded | rejected | failed | cancelled
status_code
error_stage       # authentication/validation/authorization/rate_limit/routing/billing/guardrail/upstream/response_stream/gateway
error_kind / error_code / error_message
attempt_count / successful_attempt
prompt_tokens / completion_tokens / cache_read_tokens / cache_write_tokens / total_tokens
total_latency_ms / ttft_ms
client_disconnected / termination_reason
billing_payment_mode / wallet_amount
```

### 状态映射（修正后）

| 场景 | status | status_code | error_stage | error_kind |
|---|---|---|---|---|
| 正常成功 / 流式 EOF | succeeded | 200 | — | — |
| 模型不存在 / 无权限 / 未发布 | rejected | 404 / 403 | routing / authorization | model_not_found / model_not_allowed / model_not_published |
| 无可用端点 | **failed** | **503** | routing | no_available_endpoint |
| 余额不足 / 配额不足 | rejected | 402 / 429 | billing | insufficient_balance / quota_exceeded |
| 用户 RPM/TPM 限流 | rejected | 429 | rate_limit | rate_limit_exceeded |
| 内容审核拦截 | rejected | 400 | guardrail | content_blocked |
| 客户端参数/协议非法 | rejected | 400 | validation | invalid_request |
| 网关协议转换自身错误 | **failed** | **500** | gateway | protocol_conversion_failed |
| 上游 4xx/5xx / 重试耗尽 | failed | 502 | upstream | upstream_* |
| 网关整体 timeout | failed | 504 | gateway | overall_timeout |
| 客户端中断 | cancelled | 499 | response_stream | client_disconnect |

## 生命周期设计

### RequestLifecycle

- 在 handler 认证成功后立即创建（早于 trim_model / 限流 / 路由）。
- 持有 draft 事件 + `AtomicBool finalized` + 非阻塞 `GatewayEventRecorder`。
- `finalize_*()` 方法（success / rejected / failed / cancelled）只能成功一次。
- **Drop 兜底**：未 finalize 则生成 `failed / 500 / gateway / unfinalized_request`。
  Drop 只做 `try_send`（非阻塞）交给后台 EventWriter → Redis Stream → ClickHouse；
  不把 Drop 当作 crash-safe 持久化（SIGKILL/断电仍可能丢进程内未投递事件，第一版
  不引入 WAL）。

### AttemptLifecycle

- 每次真实上游调用一条；`call_with_retry` 与各 executor 的直接上游调用处接入。
- 下一次 retry/fallback 之前，前一条 attempt 必须已有终态。
- Request 最终 `attempt_count` / `successful_attempt` 汇总。

### 流式

- EOF → succeeded；客户端断开 → cancelled/499；idle timeout → failed/504；
  上游流错误 → failed/502；已知 partial usage 保留；已产生的 attempt 有终态。

## 实现顺序与状态

- [x] Phase 1 观测基础设施：事件类型 + CH 三表 + Redis Stream + GatewayEventRecorder
      （提交 `11d3678`，旧 usage_events/billing 未动）
- [x] Phase 2 Request 生命周期：request_id 贯穿 + RequestLifecycle + handler/scheduler 早期错误
      （提交 `ec34c67`，133 测试通过；含 streaming EOF/disconnect、Drop 兜底、recorder 入 AppState）
- [ ] Phase 3 Attempt 生命周期：AttemptLifecycle + retry/fallback/timeout
- [ ] Phase 4 Streaming 生命周期
- [ ] Phase 5 管理端 /admin/usage 切 Request Event + Attempts 时间线
- [ ] Phase 6 测试 + 迁移（exactly-once、retry/fallback、disconnect、旧链路兼容）

## 测试不变量

```
1 个 authenticated LLM request  == 1 条 gateway_request_event
N 次真实 upstream call          == N 条 gateway_attempt_event
attempt 失败 + retry 成功        → request succeeded, attempts=2
模型不存在                        → rejected, attempts=0
无可用端点                        → failed/503, attempts=0
客户端中断                        → cancelled/499, 已有 attempt 终态, partial usage 保留
显式 finalize + Drop             → 仍只有 1 条 request event
```
