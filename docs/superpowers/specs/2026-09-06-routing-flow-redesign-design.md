# 路由流向图重构 + Explain Routing（候选评估）设计

日期：2026-09-06
分支：dev-credits

## 背景

后台「流控台 → 路由流向图」目前的问题：

1. **措辞误导**：拓扑节点颜色/进度条表达的是 `当前节点请求量 / 同级节点最大请求量`，却叫「负载」。管理员看到 `Endpoint A: High` 会误以为 CPU/并发/饱和度很高，实际只是请求量在兄弟节点中最高。
2. **节点信息缺失**：Endpoint 卡片没有显示真正影响调度的字段（breaker / weight / timeout / max_tokens），无法快速判断「为什么流量走了这里」。
3. **没有候选评估**：请求为什么选中某个 Endpoint、其他 Endpoint 为什么被排除，目前没有任何观测数据。

本设计分两阶段：

- **Phase 1**：只用现有真实数据重构 UI（改措辞、增强节点、线路语义、右侧 Inspector、最近路由事件可点击）。
- **Phase 2A–2D**：新增调度决策 trace 观测能力（事件 + Redis 短期完整 trace + ClickHouse 分层存储），最终在前端呈现 attempt 级「为什么这次请求走这里」。

## 已确认的关键事实（代码核实）

1. **同渠道优先重试存在**：`src/scheduler/dispatch.rs:94` `retry_next()` 先 `select_healthy_excluding(Some(&self.channel_id), ...)` 再扩大到全池。调用点：`call_with_retry` 的 ConnectFailed（256）与 retryable（266），以及 count_tokens / input_tokens executor（1271、1480）。
   - **结论**：列为**待重构行为**（见「重构待办」），不固化进观测协议。观测协议只记录触发原因（initial / retry / connection_failover），不表达渠道亲和语义。
2. **没有现成 binding_id**：熔断器身份是 `(model_id, endpoint_id)`（`routing.rs` `all_snapshots` / `restore_snapshots` 的键）。binding_id 与之一致。
3. **事件已分离**：`RouteDecided`（type/timestamp/request_id/model/channel_id/endpoint_id/user_id，无 result）与 `RequestCompleted`（latency/success/tokens）是独立事件。事件拆分方向与现有结构同向。

## 术语（全局替换）

| 旧 | 新 |
|---|---|
| 负载（load） | 请求热度（Traffic intensity） |
| 负载低 / 中 / 高 | 请求热度低 / 中 / 高（或 低/中/高流量） |
| 负载均衡 | 请求分布 |
| 路由渠道 | 路由渠道 · 请求分布 |
| 渠道端点 | 渠道端点 · 绑定级健康 |

任何表达「请求量相对高低」的视觉元素，辅助文案必须写明：

> 按时间范围内请求量计算，不代表 CPU、并发或容量饱和度。

## Phase 1：真实数据的 UI 重构

只消费现有接口，**不伪造任何候选评估数据**。

### 数据来源映射

| 节点字段 | 数据源 | 说明 |
|---|---|---|
| 请求数 / 占比 | WS `route_decided` 计数 + `routing/flow-snapshot` 24h 初始值 | 现有 |
| breaker 状态 / 可路由 | `GET /api/health/endpoints-live` | 现有，绑定级聚合 |
| weight / timeout / max_tokens | `GET /api/scheduler/models/:model_id/policy` | 现有，按选中模型拉取 |
| 最近探测 | `GET /api/probe-results/recent` | 现有 |
| P95 | `routing_health` 渠道级 24h P95 | **注意**：当前 CH 聚合是渠道级，无端点级分位数。Phase 1 在 Endpoint 卡片展示**渠道级 P95 并明确标注**，端点级分位数列入后续观测增强，不在本设计。 |
| 渠道启用 / 配置 | `useChannels` / `usePublicModels` | 现有 |

### 三列拓扑

保留 `模型 → 渠道 → Endpoint` 三列。

- **模型节点**：名称、时间范围总请求数、配置渠道数 / 绑定端点数、24h 成功率、整体健康态。
- **渠道节点**：请求数、流量占比、可路由端点计数、渠道级健康概览。
- **Endpoint 节点**：`breaker`（CLOSED / OPEN / HALF_OPEN）、`route_eligible`、`weight`、`timeout`、`max_tokens`、24h 请求数、渠道级 P95（标注）、最近探测结果、可路由状态。

### 线路语义（三种明确状态）

```
健康但 0 流量   ────────── 很淡很细实线（不代表异常）
不可路由        - - - - - 灰色虚线
有流量          ━━━━━━━━━━ 按绝对量缩放（sqrt / log 尺度）
```

- 线宽使用 `sqrt(request_count)` 或 `log1p(request_count)` 缩放，不用纯线性；范围 1px（淡）~ 6px。
- 0 请求 ≠ 不可路由：0 流量但 breaker CLOSED 的端点仍可能是正常候选。
- 线路颜色保留现有「请求热度低/中/高」的相对色阶，tooltip 显示绝对值：
  `697 requests · 51.5% of model traffic`。
- 重试 / failover 路径：Phase 1 不引入（后端无 attempt 数据），Phase 2D 再加。

### 右侧 Inspector

- 点击模型 / 渠道 / Endpoint 节点更新检查器（现有 FlowTowerContent 已有基础，迁移到 RoutingFlow 或复用）。
- 展示字段：路由状态、可用端点、24h 请求、24h 成功率、平均延迟、最高 P95；端点补充 weight/timeout/max_tokens/breaker/最近探测。
- **新增「调度决策」区块（占位）**：
  - 最终选择：`sub2api / 端点 1`（来自最近一次 route_decided）
  - 候选评估：明确显示「当前版本尚未记录候选评估详情」
  - **绝对不伪造**候选列表或排除原因。

### 底部「最近路由事件」

- 来源：现有 WS `route_decided` + `RequestCompleted`（结果列用 RequestCompleted 匹配，缺失则显示 pending）。
- 表格：时间 | 请求 | 选中端点 | 决策 | 结果 | 耗时。
- 点击事件行：高亮对应模型→渠道→Endpoint 路径，并切换右侧 Inspector 为该请求的调度决策占位视图。
- 决策列 Phase 1 显示 `weighted selection`（首个 route_decided 无 attempt 语义时统一显示该值），不区分 retry。

## Phase 2A：调度决策 Trace 内部模型

数据结构现在就定死，第二阶段实现不迁移 schema。

```rust
/// 一次请求的完整调度决策链路。
RouteDecisionTrace {
    request_id: String,
    model: String,          // 逻辑模型
    started_at: String,     // RFC3339
    final_binding_id: Option<String>,
    final_status: Option<u16>,
    attempts: Vec<RouteAttemptEvaluation>,
}

/// 单次端点选择（含 retry / failover 导致的再次选择）。
RouteAttemptEvaluation {
    attempt: u32,                       // 1-based
    timestamp: String,
    trigger: AttemptTrigger,            // initial | retry | connection_failover
    selected_binding_id: Option<String>,
    selection_reason: String,           // "weighted_selection" 等
    candidates: Vec<CandidateEvaluation>,
    outcome: Option<AttemptOutcome>,    // 该次上游调用的结果（若已发生）
}

enum AttemptTrigger {
    Initial,            // 首次选择
    Retry,              // retryable 错误消耗 budget 后重新选择
    ConnectionFailover, // connect 失败不消耗 budget 的重新选择
}

CandidateEvaluation {
    binding_id: String,             // "{model_id}:{endpoint_id}"
    endpoint_id: i64,
    channel_id: String,
    state: CandidateState,          // candidate | selected | excluded
    route_eligible: bool,           // 后端结论，前端不得自行推断
    checks: Vec<CheckResult>,       // 过滤条件结果，不是扁平 reasons[]
}

struct CheckResult {
    check: &'static str,            // enabled | breaker | channel_scope | upstream_model | already_attempted
    result: &'static str,           // pass | fail
    detail: Option<String>,         // 如 breaker open / long_unavailable / half_open
}

enum CandidateState { Candidate, Selected, Excluded }
```

明确规则：

1. **主键是 `binding_id`，不是 endpoint_id**：`DeepSeek × Endpoint 1` 与 `GPT × Endpoint 1` 可有不同 breaker/weight/timeout/max_tokens/upstream_model。`binding_id = "{model_id}:{endpoint_id}"`，与现有熔断器身份一致。
2. **trigger 只表达「为什么发生这次选择」**：initial / retry / connection_failover。**不含** same_channel_retry / cross_channel_failover（同渠道优先是待重构行为，不固化）。
3. **checks[] 结构化**：enabled / breaker / channel_scope / upstream_model / already_attempted，各自 pass/fail + detail，最终 state 单独表达 candidate / excluded / selected。前端可渲染 `✓ Enabled / ✕ Breaker: OPEN`。
4. **HALF_OPEN 语义**：记录 `route_eligible`（后端在 `is_healthy()`/`select_healthy_excluding` 处给出结论），前端看到 breaker_state=HALF_OPEN 也不得自行推断可路由。
5. **不写入 endpoint_url**：trace 只存 `binding_id / endpoint_id / channel_id`，前端按当前配置 resolve 名称与 URL。历史配置变化如需保留，可加可选 `endpoint_label: Option<String>`（短标签快照）。

## Phase 2B：事件系统

事件语义保持纯净、append-only，不在 route_decided 上回填 result。

```text
route_decided              // 完成一次路由选择（attempt 级）
  { type, timestamp, request_id, model, channel_id, endpoint_id,
    attempt, trigger, selection_reason, binding_id }

upstream_attempt_completed // 某次 Endpoint attempt 完成
  { type, timestamp, request_id, attempt, endpoint_id/channel_id,
    status, latency_ms, success, tokens? }

request_completed          // 整个请求生命周期完成（现有 RequestCompleted 扩展 attempt 汇总）
  { type, timestamp, request_id, model, final_endpoint_id,
    final_binding_id, retries, final_status, latency_ms, success }
```

- WebSocket 实时推送以上事件（现有 `/api/health/ws` 扩展）。
- Redis 短期完整 trace：`routing:decision-traces:{model}`，保留最近 50~100 条，TTL 5~30 分钟，保存完整 `RouteDecisionTrace`（含全部 candidates）。
- ClickHouse 只存轻量摘要 + 采样/条件完整 trace（见 2C）。

## Phase 2C：ClickHouse 分层存储（防观测数据炸弹）

数量估算：1 模型 20 端点 × 1000 req/s → 每秒 20,000 candidate 记录，不可接受。

| 类型 | 内容 | 落库条件 |
|---|---|---|
| 轻量决策摘要 | request_id, model, attempt, selected_binding_id, eligible_candidate_count, excluded_candidate_count, decision, result, latency, retry_count | **全部请求**（每 request 每 attempt 一行，不展开 candidates） |
| 完整 decision trace | 全部 candidates + checks + reasons | **采样**（默认 1%，设置项可调）+ 失败请求 + 发生 retry 的请求 + 诊断模式开启后的请求 |

- ClickHouse 只 append，不做 in-place 更新。
- 采样率存 `balancer_settings`（PG），键名建议 `routing_trace_sample_rate`（Phase 2C 实现时定）。

## Phase 2D：Explain Routing 前端

右侧最终呈现 attempt 级时间线：

```text
Request req_xxx
───────────────
Attempt #1
  A1  SELECTED
  A2  CANDIDATE
  B1  EXCLUDED · breaker OPEN
  A1 → 502

Attempt #2
  A1  EXCLUDED · already attempted
  A2  CANDIDATE
  B1  EXCLUDED · breaker OPEN
  B2  SELECTED
  B2 → 200

Final
  2 attempts · 1 failover · final endpoint: B2
```

Phase 1 的占位区块与之同构，Phase 2D 只接数据不改布局。

## 重构待办（独立于本设计，不在本次实现）

- **移除 `retry_next()` 的隐式同渠道优先**（`dispatch.rs:94`）：改为在全池 `select_healthy_excluding(None, ...)` 上直接重选，或把渠道亲和变成显式调度策略后再保留。观测协议已按无亲和设计，此项重构不会破坏 2A 模型。
- 端点级延迟分位数（P95/TTFT）聚合：Phase 1 只用渠道级并标注，端点级列为后续观测增强。

## 实现顺序

1. **Phase 1**（本分支立即实现，只用现有数据，候选区显示「尚未记录」，不伪造）
2. **Phase 2A** 内部模型落地（trace 结构 + `binding_id` 键 + dispatch 埋点）
3. **Phase 2B** 事件扩展（route_decided attempt 化 + upstream_attempt_completed + WS + Redis）
4. **Phase 2C** ClickHouse 分层落库
5. **Phase 2D** Explain Routing UI 接数据

Phase 2A–2D 为后续独立迭代，schema 以本文档为准。
