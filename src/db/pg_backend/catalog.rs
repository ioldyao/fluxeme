use super::*;
use async_trait::async_trait;

#[async_trait]
impl CatalogBackend for PgBackend {
    // ── Channels & Endpoints ─────────────────────────────────────────────

    async fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        let ch_rows = query(
            "SELECT id, name, provider, enabled, anthropic_compat FROM channels ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let ep_rows = query(
            "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled, full_url FROM endpoints ORDER BY channel_id",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut channels: Vec<Channel> = ch_rows
            .iter()
            .map(|r| Channel {
                id: r.get(0),
                name: r.get(1),
                provider: r.get(2),
                enabled: r.get(3),
                anthropic_compat: r.get(4),
                endpoints: Vec::new(),
            })
            .collect();

        let mut eps_by_channel: std::collections::HashMap<String, Vec<Endpoint>> =
            std::collections::HashMap::new();
        for r in &ep_rows {
            let ch_id: String = r.get(1);
            eps_by_channel.entry(ch_id).or_default().push(Endpoint {
                id: Some(r.get(0)),
                channel_id: r.get(1),
                url: r.get(2),
                api_key: r.get(3),
                weight: {
                    let w: i32 = r.get(4);
                    w as u32
                },
                timeout_secs: {
                    let t: Option<i64> = r.get(5);
                    t.map(|v| v as u64)
                },
                enabled: r.get(6),
                full_url: r.get(7),
            });
        }
        for ch in &mut channels {
            if let Some(eps) = eps_by_channel.remove(&ch.id) {
                ch.endpoints = eps;
            }
        }
        Ok(channels)
    }

    async fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError> {
        let rows = query(
            "SELECT id, name, provider, enabled, anthropic_compat FROM channels WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        if let Some(r) = rows.first() {
            let mut ch = Channel {
                id: r.get(0),
                name: r.get(1),
                provider: r.get(2),
                enabled: r.get(3),
                anthropic_compat: r.get(4),
                endpoints: Vec::new(),
            };
            let eps = query(
                "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled, full_url FROM endpoints WHERE channel_id = $1",
            )
            .bind(&ch.id)
            .fetch_all(&self.pool)
            .await?;
            ch.endpoints = eps
                .iter()
                .map(|r| Endpoint {
                    id: Some(r.get(0)),
                    channel_id: r.get(1),
                    url: r.get(2),
                    api_key: r.get(3),
                    weight: {
                        let w: i32 = r.get(4);
                        w as u32
                    },
                    timeout_secs: {
                        let t: Option<i64> = r.get(5);
                        t.map(|v| v as u64)
                    },
                    enabled: r.get(6),
                    full_url: r.get(7),
                })
                .collect();
            Ok(Some(ch))
        } else {
            Ok(None)
        }
    }

    async fn create_channel(&self, ch: &Channel) -> Result<(), DbError> {
        query(
            "INSERT INTO channels (id, name, provider, enabled, anthropic_compat) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&ch.id)
        .bind(&ch.name)
        .bind(&ch.provider)
        .bind(ch.enabled)
        .bind(ch.anthropic_compat)
        .execute(&self.pool)
        .await?;
        for ep in &ch.endpoints {
            query(
                "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled, full_url) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&ch.id)
            .bind(&ep.url)
            .bind(&ep.api_key)
            .bind(ep.weight as i32)
            .bind(ep.timeout_secs.map(|v| v as i64))
            .bind(ep.enabled)
            .bind(ep.full_url)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn update_channel(&self, ch: &Channel) -> Result<(), DbError> {
        query(
            "UPDATE channels SET name = $1, provider = $2, enabled = $3, anthropic_compat = $4 WHERE id = $5",
        )
        .bind(&ch.name)
        .bind(&ch.provider)
        .bind(ch.enabled)
        .bind(ch.anthropic_compat)
        .bind(&ch.id)
        .execute(&self.pool)
        .await?;
        query("DELETE FROM endpoints WHERE channel_id = $1")
            .bind(&ch.id)
            .execute(&self.pool)
            .await?;
        for ep in &ch.endpoints {
            query(
                "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled, full_url) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&ch.id)
            .bind(&ep.url)
            .bind(&ep.api_key)
            .bind(ep.weight as i32)
            .bind(ep.timeout_secs.map(|v| v as i64))
            .bind(ep.enabled)
            .bind(ep.full_url)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_channel(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM channels WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        let rows = query(
            "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled, full_url FROM endpoints WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| Endpoint {
            id: Some(r.get(0)),
            channel_id: r.get(1),
            url: r.get(2),
            api_key: r.get(3),
            weight: {
                let w: i32 = r.get(4);
                w as u32
            },
            timeout_secs: {
                let t: Option<i64> = r.get(5);
                t.map(|v| v as u64)
            },
            enabled: r.get(6),
            full_url: r.get(7),
        }))
    }

    async fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError> {
        query("UPDATE endpoints SET enabled = $1 WHERE id = $2")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_endpoint_api_key(&self, id: i64, api_key: &str) -> Result<(), DbError> {
        query("UPDATE endpoints SET api_key = $1 WHERE id = $2")
            .bind(api_key)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Models ───────────────────────────────────────────────────────────

    async fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = query(
            "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
             FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
             ORDER BY mc.model_id, mc.priority",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut models: Vec<Model> = m_rows
            .iter()
            .map(|r| Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            })
            .collect();

        let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
            std::collections::HashMap::new();
        for r in &b_rows {
            let model_id: String = r.get(0);
            by_model.entry(model_id).or_default().push(ModelChannel {
                model_id: r.get(0),
                channel_id: r.get(1),
                priority: r.get(2),
                provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                upstream_model: r.get::<Option<String>, _>(4),
            });
        }
        for m in &mut models {
            if let Some(bindings) = by_model.remove(&m.id) {
                m.channels = bindings;
            }
        }
        Ok(models)
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>, DbError> {
        let rows = query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        if let Some(r) = rows.first() {
            let mut m = Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            };
            let bindings = query(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
                 FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
                 WHERE mc.model_id = $1 ORDER BY mc.priority",
            )
            .bind(&m.id)
            .fetch_all(&self.pool)
            .await?;
            m.channels = bindings
                .iter()
                .map(|r| ModelChannel {
                    model_id: r.get(0),
                    channel_id: r.get(1),
                    priority: r.get(2),
                    provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                    upstream_model: r.get::<Option<String>, _>(4),
                })
                .collect();
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn create_model(&self, m: &Model) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        query(
            "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&m.id)
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .execute(&mut *tx)
        .await?;

        for binding in &m.channels {
            query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(&m.id)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError> {
        if old_id != m.id {
            return Err(DbError("Model ID cannot be changed".to_string()));
        }
        let mut tx = self.pool.begin().await?;
        query(
            "UPDATE models SET name=$1, model_pattern=$2, prompt_price=$3, completion_price=$4, \
             cache_read_price=$5, cache_write_price=$6, image_input_price=$7, audio_input_price=$8, \
             audio_output_price=$9, published=$10, context_length=$11, category=$12 WHERE id=$13",
        )
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
        // Delete old bindings by old_id (model_channels FK references old model id)
        query("DELETE FROM model_channels WHERE model_id = $1")
            .bind(old_id)
            .execute(&mut *tx)
            .await?;
        for binding in &m.channels {
            query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) VALUES ($1, $2, $3, $4)",
            )
            .bind(&m.id)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM models WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models \
             WHERE published = true ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = query(
            "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
             FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
             ORDER BY mc.model_id, mc.priority",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut models: Vec<Model> = m_rows
            .iter()
            .map(|r| Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
                },
                channels: Vec::new(),
                published: true,
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            })
            .collect();

        let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
            std::collections::HashMap::new();
        for r in &b_rows {
            let model_id: String = r.get(0);
            by_model.entry(model_id).or_default().push(ModelChannel {
                model_id: r.get(0),
                channel_id: r.get(1),
                priority: r.get(2),
                provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                upstream_model: r.get::<Option<String>, _>(4),
            });
        }
        for m in &mut models {
            if let Some(bindings) = by_model.remove(&m.id) {
                m.channels = bindings;
            }
        }
        Ok(models)
    }

    async fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError> {
        query("UPDATE models SET published = $1 WHERE id = $2")
            .bind(published)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        query(
            "UPDATE models SET prompt_price=$1, completion_price=$2, cache_read_price=$3, \
             cache_write_price=$4, image_input_price=$5, audio_input_price=$6, \
             audio_output_price=$7 WHERE id=$8",
        )
        .bind(pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError> {
        query("UPDATE models SET context_length = $1 WHERE id = $2")
            .bind(context_length)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Routing Rules ────────────────────────────────────────────────────

    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules ORDER BY priority, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                id: r.get(0),
                name: r.get(1),
                scope: r.get(2),
                user_id: r.get(3),
                source_model: r.get(4),
                target_model: r.get(5),
                channel_id: r.get(6),
                upstream_model: r.get(7),
                priority: r.get(8),
                enabled: r.get(9),
                description: r.get(10),
                created_at: r.get(11),
                updated_at: r.get(12),
                team_id: r.get(13),
            })
            .collect())
    }

    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        let id = if r.id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            r.id.clone()
        };
        query(
            "INSERT INTO routing_rules \
             (id, name, scope, user_id, source_model, target_model, channel_id, \
              upstream_model, priority, enabled, description, created_at, updated_at, team_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(&id)
        .bind(&r.name)
        .bind(&r.scope)
        .bind(&r.user_id)
        .bind(&r.source_model)
        .bind(&r.target_model)
        .bind(&r.channel_id)
        .bind(&r.upstream_model)
        .bind(r.priority)
        .bind(r.enabled)
        .bind(&r.description)
        .bind(&r.created_at)
        .bind(&r.updated_at)
        .bind(&r.team_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        query(
            "UPDATE routing_rules SET name=$1, scope=$2, user_id=$3, source_model=$4, \
             target_model=$5, channel_id=$6, upstream_model=$7, priority=$8, enabled=$9, \
             description=$10, updated_at=$11, team_id=$12 WHERE id=$13",
        )
        .bind(&r.name)
        .bind(&r.scope)
        .bind(&r.user_id)
        .bind(&r.source_model)
        .bind(&r.target_model)
        .bind(&r.channel_id)
        .bind(&r.upstream_model)
        .bind(r.priority)
        .bind(r.enabled)
        .bind(&r.description)
        .bind(&r.updated_at)
        .bind(&r.team_id)
        .bind(&r.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_rule(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM routing_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_team_rule(&self, team_id: &str, rule_id: &str) -> Result<bool, DbError> {
        let result =
            query("DELETE FROM routing_rules WHERE id = $1 AND scope = 'user' AND team_id = $2")
                .bind(rule_id)
                .bind(team_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_user_rules(&self, user_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules WHERE scope='user' AND user_id=$1 \
             ORDER BY priority, name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                id: r.get(0),
                name: r.get(1),
                scope: r.get(2),
                user_id: r.get(3),
                source_model: r.get(4),
                target_model: r.get(5),
                channel_id: r.get(6),
                upstream_model: r.get(7),
                priority: r.get(8),
                enabled: r.get(9),
                description: r.get(10),
                created_at: r.get(11),
                updated_at: r.get(12),
                team_id: r.get(13),
            })
            .collect())
    }
}
