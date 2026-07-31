# AI Gateway

## Data Storage — HARD RULE（不可违背）

**可观测性数据存 ClickHouse，其余业务数据存 PostgreSQL。两者完全解耦，
禁止任何跨存储兜底 / 回退 / 容错机制（不存在"CH 挂了就读 PG"这种逻辑）。**
任何涉及"数据存哪"的判断，必须从代码确认写入/读取路径，禁止凭
"我觉得 / 应该 / 可能 / 印象" 下结论。

### 归属表（唯一事实来源）

| 数据 | 存储 |
|---|---|
| 请求观测 `usage_events`（token/延迟/body/endpoint_url/endpoint_id） | ClickHouse |
| 探测结果 `probe_results`（通道/端点通断状态） | ClickHouse |
| 计费元数据 `billing_events`（价格快照/cost/钱包扣费） | PostgreSQL |
| 用户/渠道/端点/模型/API Key/路由规则/公告/配置/设置 | PostgreSQL |

### 读取

- 观测类接口（用量日志、`recent-paths`、`routing_health`、`/api/probe-results/recent`、
  探测最新结果）**只读 ClickHouse**。
- 业务类接口（钱包/账单/用户/渠道/配置）**只读 PostgreSQL**。
- 每类数据只属于一个存储，读取也只从该存储读。不做"存储是否可用"判断，不回退。

### 新增功能

写任何持久化代码前，先判定数据类别再选存储；查 `src/ch_backend.rs`（CH 侧）
与 `src/db/pg_backend.rs`（PG 侧）的现有表确认归属。禁止凭印象 / 我觉得。

## gstack

Use the `/browse` skill from gstack for all web browsing. Never use `mcp__claude-in-chrome__*` tools.

Available gstack skills:
- `/office-hours`
- `/plan-ceo-review`
- `/plan-eng-review`
- `/plan-design-review`
- `/design-consultation`
- `/design-shotgun`
- `/design-html`
- `/review`
- `/ship`
- `/land-and-deploy`
- `/canary`
- `/benchmark`
- `/browse`
- `/connect-chrome`
- `/qa`
- `/qa-only`
- `/design-review`
- `/setup-browser-cookies`
- `/setup-deploy`
- `/setup-gbrain`
- `/retro`
- `/investigate`
- `/document-release`
- `/document-generate`
- `/codex`
- `/cso`
- `/autoplan`
- `/plan-devex-review`
- `/devex-review`
- `/careful`
- `/freeze`
- `/guard`
- `/unfreeze`
- `/gstack-upgrade`
- `/learn`

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: This project has a knowledge graph. ALWAYS use the
code-review-graph MCP tools BEFORE using Grep/Glob/Read to explore
the codebase.** The graph is faster, cheaper (fewer tokens), and gives
you structural context (callers, dependents, test coverage) that file
scanning cannot.

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes_tool` or `query_graph_tool` instead of Grep
- **Understanding impact**: `get_impact_radius_tool` instead of manually tracing imports
- **Code review**: `detect_changes_tool` + `get_review_context_tool` instead of reading entire files
- **Finding relationships**: `query_graph_tool` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview_tool` + `list_communities_tool`

Fall back to Grep/Glob/Read **only** when the graph doesn't cover what you need.

### Key Tools

| Tool | Use when |
| ------ | ---------- |
| `detect_changes_tool` | Reviewing code changes — gives risk-scored analysis |
| `get_review_context_tool` | Need source snippets for review — token-efficient |
| `get_impact_radius_tool` | Understanding blast radius of a change |
| `get_affected_flows_tool` | Finding which execution paths are impacted |
| `query_graph_tool` | Tracing callers, callees, imports, tests, dependencies |
| `semantic_search_nodes_tool` | Finding functions/classes by name or keyword |
| `get_architecture_overview_tool` | Understanding high-level codebase structure |
| `refactor_tool` | Planning renames, finding dead code |

### Workflow

1. The graph auto-updates on file changes (via hooks).
2. Use `detect_changes_tool` for code review.
3. Use `get_affected_flows_tool` to understand impact.
4. Use `query_graph_tool` pattern="tests_for" to check coverage.
