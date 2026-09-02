//! API Gateway 路由（纯 API 网关业务配置，类似 Kong/APISIX）。
//!
//! 业务数据 → PostgreSQL（gateway_routes 表）。数据面代理 /apigw/{*rest}，
//! 鉴权用 API Key 的 `gateway` scope。`upstream_headers` 存加密 JSON，
//! 代理时注入上游请求头；列表/详情返回时脱敏。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRoute {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// 公开路径前缀，如 `/weather`。数据面入口：`/apigw{path_prefix}`。
    pub path_prefix: String,
    /// 上游 base，如 `https://api.openweathermap.org`。不允许含凭据。
    pub upstream_url: String,
    /// 允许的 HTTP 方法（逗号分隔），如 `GET,POST`。
    #[serde(default = "default_methods")]
    pub methods: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 代理时透传原始 query string。
    #[serde(default = "default_true")]
    pub preserve_query: bool,
    /// 代理时去掉 path_prefix（`/weather/current` → 上游 `/current`）。
    #[serde(default = "default_true")]
    pub strip_prefix: bool,
    /// 加密的 JSON 对象：代理时注入上游请求头（值不返回给前端）。
    #[serde(default)]
    pub upstream_headers: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_methods() -> String {
    "GET,POST,PUT,PATCH,DELETE".to_string()
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn default_true() -> bool {
    true
}
