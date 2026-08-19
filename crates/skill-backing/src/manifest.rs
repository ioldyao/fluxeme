//! fluxeme.yaml（backing-api 运行时声明）解析 → 契约 `SkillManifest`。
//!
//! 解释权在 Skill Runtime：SkillHub 只原样保存 manifest_yaml，本模块负责
//! 把它解析成语义化结构并交给部署/请求链使用。

use fluxeme_contract::{EndpointDecl, SkillManifest, SkillSlug};
use serde::Deserialize;

#[derive(Debug)]
pub enum ManifestError {
    Parse(String),
    Invalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Parse(m) => write!(f, "manifest parse: {m}"),
            ManifestError::Invalid(m) => write!(f, "manifest invalid: {m}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// fluxeme.yaml 文件结构（未知字段忽略，向前兼容）。
#[derive(Debug, Deserialize)]
struct ManifestFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    runtime: Option<RuntimeBlock>,
    #[serde(default)]
    endpoints: Vec<ManifestEndpoint>,
}

#[derive(Debug, Deserialize)]
struct RuntimeBlock {
    #[serde(default)]
    #[allow(dead_code)]
    #[serde(rename = "type")]
    r#type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestEndpoint {
    name: String,
    method: String,
    path: String,
    upstream: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// 解析 fluxeme.yaml → 契约 SkillManifest。
pub fn parse_manifest(yaml: &str) -> Result<SkillManifest, ManifestError> {
    let mf: ManifestFile =
        serde_yaml::from_str(yaml).map_err(|e| ManifestError::Parse(e.to_string()))?;
    let endpoints = mf
        .endpoints
        .into_iter()
        .map(|e| {
            let method = e.method.trim().to_uppercase();
            if method.is_empty() {
                return Err(ManifestError::Invalid("endpoint method is empty".into()));
            }
            if !e.path.starts_with('/') {
                return Err(ManifestError::Invalid(format!(
                    "endpoint '{}' path must start with '/'",
                    e.name
                )));
            }
            Ok(EndpointDecl {
                name: e.name,
                method,
                path: e.path,
                upstream: e.upstream,
                timeout_ms: e.timeout_ms,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SkillManifest {
        name: SkillSlug(mf.name.unwrap_or_default()),
        version: mf.version.unwrap_or_default(),
        endpoints,
    })
}
