//! SSRF / 上游安全策略（Runtime 执行侧）。
//!
//! 部署时对技能声明的每个 upstream 做校验，**不允许技能包自己决定任意
//! upstream**。SkillHub 管审批/策略，此处是 enforcement。

use url::Url;

#[derive(Debug)]
pub enum PolicyError {
    Blocked(String),
    Invalid(String),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::Blocked(m) => write!(f, "blocked: {m}"),
            PolicyError::Invalid(m) => write!(f, "invalid: {m}"),
        }
    }
}

impl std::error::Error for PolicyError {}

/// 上游策略配置。阶段 2 用默认值（禁私网），组合根可调整。
#[derive(Debug, Clone)]
pub struct UpstreamPolicy {
    /// 是否允许私网/保留地址段（默认 false = 禁）。
    pub allow_private: bool,
    /// 上游超时上界（毫秒）。
    pub max_timeout_ms: u64,
    /// 请求体大小上界（字节）。
    pub max_body_bytes: u64,
}

impl Default for UpstreamPolicy {
    fn default() -> Self {
        Self {
            allow_private: false,
            max_timeout_ms: 60_000,
            max_body_bytes: 10 * 1024 * 1024,
        }
    }
}

/// 禁止地址段（含 metadata IP 169.254.169.254 / 云厂商元数据）。
fn blocked_v4(ip: &std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_private()
        || ip.is_broadcast()
    {
        return true;
    }
    // 169.254.169.254 已属 link-local 段，这里再显式兜底。
    o == [169, 254, 169, 254]
}

fn blocked_v6(ip: &std::net::Ipv6Addr) -> bool {
    ip.is_unspecified() || ip.is_loopback() || ip.is_unique_local()
}

impl UpstreamPolicy {
    /// 校验 upstream URL；合法则返回规范化后的 URL。
    pub async fn validate(
        &self,
        upstream: &str,
        timeout_ms: Option<u64>,
    ) -> Result<Url, PolicyError> {
        let u = Url::parse(upstream).map_err(|e| PolicyError::Invalid(e.to_string()))?;
        if u.scheme() != "http" && u.scheme() != "https" {
            return Err(PolicyError::Blocked(format!(
                "scheme '{}' not allowed (http/https only)",
                u.scheme()
            )));
        }
        if let Some(t) = timeout_ms {
            if t == 0 || t > self.max_timeout_ms {
                return Err(PolicyError::Blocked(format!(
                    "timeout {t}ms out of bounds (1..={}ms)",
                    self.max_timeout_ms
                )));
            }
        }
        let host = u
            .host_str()
            .ok_or_else(|| PolicyError::Invalid("upstream has no host".into()))?;
        let port = u.port_or_known_default().unwrap_or(80);

        // 字面 IP 直接校验；主机名做 DNS 解析后逐个校验（防 DNS rebinding）。
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            self.check_ip(&ip)?;
        } else {
            let mut found = false;
            let addrs = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| PolicyError::Invalid(format!("dns resolve {host}: {e}")))?;
            for addr in addrs {
                found = true;
                self.check_ip(&addr.ip())?;
            }
            if !found {
                return Err(PolicyError::Invalid(format!("dns resolve {host}: no addresses")));
            }
        }
        Ok(u)
    }

    fn check_ip(&self, ip: &std::net::IpAddr) -> Result<(), PolicyError> {
        match ip {
            std::net::IpAddr::V4(v4) if blocked_v4(v4) => Err(PolicyError::Blocked(format!(
                "upstream IP {ip} is a reserved/private/loopback address"
            ))),
            std::net::IpAddr::V6(v6) if blocked_v6(v6) => Err(PolicyError::Blocked(format!(
                "upstream IP {ip} is a reserved/loopback address"
            ))),
            _ => Ok(()),
        }
    }

    /// 请求体大小上界校验。
    pub fn check_body(&self, len: usize) -> Result<(), PolicyError> {
        if len as u64 > self.max_body_bytes {
            return Err(PolicyError::Blocked(format!(
                "request body {len} bytes exceeds limit {}",
                self.max_body_bytes
            )));
        }
        Ok(())
    }
}
