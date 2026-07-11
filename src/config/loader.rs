use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::config::types::AppConfig;
use crate::db::Database;
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, ModelChannel, Pricing};
use crate::domain::routing::RoutingRule;
use crate::domain::user::{ApiKey, RateLimit, User};

/// Replace `${VAR}` patterns in a string with their environment variable values.
/// If the env var is not set, the placeholder is left as-is.
fn expand_env_vars(content: &str) -> String {
    let mut result = String::new();
    let mut rest = content;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            let var_name = &after[..end];
            match std::env::var(var_name) {
                Ok(val) => result.push_str(&val),
                Err(_) => {
                    // Leave placeholder as-is so callers can detect unset vars
                    result.push_str(&format!("${{{}}}", var_name));
                }
            }
            rest = &after[end + 1..];
        } else {
            result.push_str("${");
            rest = after;
        }
    }
    result.push_str(rest);
    result
}

pub fn load_config(path: &str) -> Result<AppConfig, String> {
    let content = fs::read_to_string(Path::new(path))
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    // Expand ${VAR} environment variable references before YAML parsing
    let content = expand_env_vars(&content);

    let config: AppConfig =
        serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

    // Validate
    if config.server.host.is_empty() {
        return Err("Server host is empty".into());
    }
    if config.server.port == 0 {
        return Err("Server port is 0".into());
    }
    if config.admin.username.is_empty() {
        return Err("Admin username is empty".into());
    }
    if config.admin.password.is_empty() {
        return Err("Admin password is empty".into());
    }

    tracing::info!(
        "Config loaded: server {}:{}",
        config.server.host,
        config.server.port
    );

    Ok(config)
}

/// Resolve the JWT secret from config or environment variable.
/// Panics if neither source provides a value.
pub fn resolve_jwt_secret(cfg: &AppConfig) -> String {
    let secret = std::env::var("GATEWAY_JWT_SECRET")
        .ok()
        .or_else(|| cfg.jwt_secret.clone())
        .unwrap_or_else(|| {
            panic!("GATEWAY_JWT_SECRET must be set via config or environment variable");
        });

    if secret == "ai-gateway-jwt-secret-change-me" {
        panic!("CRITICAL: Default JWT secret is not allowed in production! Set GATEWAY_JWT_SECRET env var or configure jwt_secret in config.yaml.");
    }

    if secret.starts_with("${") {
        panic!(
            "CRITICAL: JWT secret references env var {} which is not set. \
             Add GATEWAY_JWT_SECRET to your .env file or environment.",
            secret
        );
    }

    secret
}

/// Seed database from config YAML if database is empty.
pub fn seed_from_config(
    config_path: &str,
    db: &Database,
    admin_username: &str,
) -> Result<(), String> {
    let content = fs::read_to_string(Path::new(config_path))
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    #[derive(Deserialize)]
    struct OldTenant {
        id: String,
        name: String,
        keys: Vec<OldApiKey>,
        rate_limits: Option<OldRateLimit>,
    }

    #[derive(Deserialize)]
    struct OldApiKey {
        key: String,
        #[serde(default)]
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        expires_at: Option<String>,
    }

    #[derive(Deserialize)]
    struct OldRateLimit {
        rpm: Option<u64>,
        tpm: Option<u64>,
    }

    #[derive(Deserialize)]
    struct OldEndpoint {
        url: String,
        api_key: String,
        #[serde(default = "default_one")]
        weight: u32,
        timeout_secs: Option<u64>,
    }

    #[derive(Deserialize)]
    struct OldChannel {
        id: String,
        #[serde(default)]
        name: String,
        provider: String,
        #[serde(default = "default_one_i32")]
        priority: i32,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        endpoints: Vec<OldEndpoint>,
    }

    #[derive(Deserialize)]
    struct OldModelChannel {
        channel_id: String,
        #[serde(default = "default_one_i32")]
        priority: i32,
    }

    #[derive(Default, Deserialize)]
    struct OldPricing {
        #[serde(default)]
        prompt_price: f64,
        #[serde(default)]
        completion_price: f64,
        #[serde(default)]
        cache_read_price: f64,
        #[serde(default)]
        cache_write_price: f64,
        #[serde(default)]
        image_input_price: f64,
        #[serde(default)]
        audio_input_price: f64,
        #[serde(default)]
        audio_output_price: f64,
    }

    #[derive(Deserialize)]
    struct OldModel {
        id: String,
        name: String,
        model_pattern: String,
        #[serde(default)]
        pricing: OldPricing,
        #[serde(default)]
        channels: Vec<OldModelChannel>,
    }

    #[derive(Deserialize)]
    struct OldRoutingRule {
        name: String,
        tenant_id: String,
        model_pattern: String,
        channel_id: String,
    }

    #[derive(Deserialize)]
    struct OldGlobalKey {
        key: String,
        #[serde(default)]
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        expires_at: Option<String>,
    }

    #[derive(Deserialize)]
    struct SeedPayload {
        #[serde(default)]
        users: Option<Vec<OldTenant>>,
        #[serde(default)]
        tenants: Option<Vec<OldTenant>>,
        #[serde(default)]
        channels: Option<Vec<OldChannel>>,
        #[serde(default)]
        models: Option<Vec<OldModel>>,
        #[serde(default)]
        routing_rules: Option<Vec<OldRoutingRule>>,
        #[serde(default)]
        api_keys: Option<Vec<OldGlobalKey>>,
    }

    fn default_true() -> bool {
        true
    }
    fn default_one() -> u32 {
        1
    }
    fn default_one_i32() -> i32 {
        1
    }

    let seed: SeedPayload = match serde_yaml::from_str(&content) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Seed parse error: {} — skipping seed", e);
            return Ok(());
        }
    };

    // Seed admin user and its API keys before the early-return check,
    // so admin always exists in DB even when there is no tenant/channel/model data.
    {
        if db.get_user(admin_username).unwrap_or(None).is_none() {
            let admin_user = User {
                id: admin_username.to_string(),
                name: "管理员".to_string(),
                password_hash: None, // admin login uses config, not DB
                rate_limits: None,
                timezone: "UTC".to_string(),
            };
            if let Err(e) = db.create_user(&admin_user) {
                tracing::warn!("Seed admin user: {}", e);
            }
        }
        let admin_id = admin_username;
        if let Some(keys) = &seed.api_keys {
            for k in keys {
                let ak = ApiKey {
                    key: k.key.clone(),
                    user_id: admin_id.to_string(),
                    name: k.name.clone(),
                    enabled: k.enabled,
                    expires_at: k.expires_at.clone(),
                    spend_limit: None,
                    allowed_models: None,
                };
                if let Err(e) = db.create_api_key(&ak) {
                    tracing::warn!("Seed api key {}: {}", k.key, e);
                }
            }
        }
    }

    let tenants = seed.users.or(seed.tenants).unwrap_or_default();
    tracing::info!(
        "Seed: {} tenants, {} channels, {} models",
        tenants.len(),
        seed.channels.as_ref().map_or(0, |c| c.len()),
        seed.models.as_ref().map_or(0, |m| m.len()),
    );
    if tenants.is_empty()
        && seed.channels.as_ref().is_none_or(|c| c.is_empty())
        && seed.models.as_ref().is_none_or(|m| m.is_empty())
    {
        return Ok(()); // Nothing else to seed
    }

    tracing::info!("Seeding database from config YAML...");

    for t in &tenants {
        let user = User {
            id: t.id.clone(),
            name: t.name.clone(),
            password_hash: None,
            rate_limits: t.rate_limits.as_ref().map(|rl| RateLimit {
                rpm: rl.rpm,
                tpm: rl.tpm,
            }),
            timezone: "UTC".to_string(),
        };
        if let Err(e) = db.create_user(&user) {
            tracing::warn!("Seed user {}: {}", t.id, e);
        }
        for k in &t.keys {
            let ak = ApiKey {
                key: k.key.clone(),
                user_id: t.id.clone(),
                name: k.name.clone(),
                enabled: k.enabled,
                expires_at: k.expires_at.clone(),
                spend_limit: None,
                allowed_models: None,
            };
            if let Err(e) = db.create_api_key(&ak) {
                tracing::warn!("Seed api_key for {}: {}", t.id, e);
            }
        }
    }

    if let Some(chs) = &seed.channels {
        for c in chs {
            let channel = Channel {
                id: c.id.clone(),
                name: c.name.clone(),
                provider: c.provider.clone(),
                priority: c.priority,
                enabled: c.enabled,
                endpoints: c
                    .endpoints
                    .iter()
                    .map(|ep| Endpoint {
                        id: None,
                        channel_id: c.id.clone(),
                        url: ep.url.clone(),
                        api_key: ep.api_key.clone(),
                        weight: ep.weight,
                        timeout_secs: ep.timeout_secs,
                        enabled: true,
                    })
                    .collect(),
            };
            if let Err(e) = db.create_channel(&channel) {
                tracing::warn!("Seed channel {}: {}", c.id, e);
            }
        }
    }

    if let Some(ms) = &seed.models {
        for m in ms {
            let model = Model {
                id: m.id.clone(),
                name: m.name.clone(),
                model_pattern: m.model_pattern.clone(),
                pricing: Pricing {
                    prompt_price: m.pricing.prompt_price,
                    completion_price: m.pricing.completion_price,
                    cache_read_price: m.pricing.cache_read_price,
                    cache_write_price: m.pricing.cache_write_price,
                    image_input_price: m.pricing.image_input_price,
                    audio_input_price: m.pricing.audio_input_price,
                    audio_output_price: m.pricing.audio_output_price,
                },
                channels: m
                    .channels
                    .iter()
                    .map(|mc| ModelChannel {
                        model_id: m.id.clone(),
                        channel_id: mc.channel_id.clone(),
                        priority: mc.priority,
                    })
                    .collect(),
                published: false,
                context_length: None,
                category: String::default(),
            };
            if let Err(e) = db.create_model(&model) {
                tracing::warn!("Seed model {}: {}", m.id, e);
            }
        }
    }

    if let Some(rules) = &seed.routing_rules {
        for r in rules {
            let rule = RoutingRule {
                name: r.name.clone(),
                user_id: r.tenant_id.clone(),
                model_pattern: r.model_pattern.clone(),
                channel_id: r.channel_id.clone(),
            };
            if let Err(e) = db.create_rule(&rule) {
                tracing::warn!("Seed rule {}: {}", r.name, e);
            }
        }
    }

    tracing::info!(
        "Database seeded: {} users, {} channels, {} models",
        tenants.len(),
        seed.channels.as_ref().map_or(0, |c| c.len()),
        seed.models.as_ref().map_or(0, |m| m.len()),
    );

    Ok(())
}
