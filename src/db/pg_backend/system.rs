use super::*;
use async_trait::async_trait;

#[async_trait]
impl SystemBackend for PgBackend {
    // ── Settings ─────────────────────────────────────────────────────────

    async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let result: Option<(String,)> =
            query_as("SELECT value FROM balancer_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(result.map(|r| r.0))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        query(
            "INSERT INTO balancer_settings (key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_gateway_config(&self) -> Result<GatewayRuntimeConfig, DbError> {
        match self.get_setting("gateway_config").await? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| DbError(format!("Invalid gateway config JSON: {}", e))),
            None => Ok(GatewayRuntimeConfig::default()),
        }
    }

    async fn set_gateway_config(&self, config: &GatewayRuntimeConfig) -> Result<(), DbError> {
        let json = serde_json::to_string(config)
            .map_err(|e| DbError(format!("Failed to serialize gateway config: {}", e)))?;
        self.set_setting("gateway_config", &json).await
    }

    // ── Content Filter Rules ─────────────────────────────────────────

    async fn list_filter_rules(&self) -> Result<Vec<ContentFilterRule>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String, String, Option<String>, Option<String>, bool, i32, String, String)>(
            "SELECT id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at FROM content_filter_rules ORDER BY priority ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list filter rules: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    pattern_type,
                    pattern,
                    action,
                    scope,
                    channel_id,
                    replacement,
                    enabled,
                    priority,
                    created_at,
                    updated_at,
                )| ContentFilterRule {
                    id,
                    name,
                    pattern_type,
                    pattern,
                    action,
                    scope,
                    channel_id,
                    replacement,
                    enabled,
                    priority,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        query(
            "INSERT INTO content_filter_rules (id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.pattern_type)
        .bind(&rule.pattern)
        .bind(&rule.action)
        .bind(&rule.scope)
        .bind(&rule.channel_id)
        .bind(&rule.replacement)
        .bind(rule.enabled)
        .bind(rule.priority)
        .bind(&rule.created_at)
        .bind(&rule.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to create filter rule: {}", e)))?;
        Ok(())
    }

    async fn update_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        query(
            "UPDATE content_filter_rules SET name=$1, pattern_type=$2, pattern=$3, action=$4, scope=$5, channel_id=$6, replacement=$7, enabled=$8, priority=$9, updated_at=$10 WHERE id=$11"
        )
        .bind(&rule.name)
        .bind(&rule.pattern_type)
        .bind(&rule.pattern)
        .bind(&rule.action)
        .bind(&rule.scope)
        .bind(&rule.channel_id)
        .bind(&rule.replacement)
        .bind(rule.enabled)
        .bind(rule.priority)
        .bind(&rule.updated_at)
        .bind(&rule.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to update filter rule: {}", e)))?;
        Ok(())
    }

    async fn delete_filter_rule(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM content_filter_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("Failed to delete filter rule: {}", e)))?;
        Ok(())
    }

    // ── Health Probe Results ─────────────────────────────────────────

    async fn channel_usage_24h(
        &self,
    ) -> Result<Vec<(String, String, u64, u64, f64, f64)>, DbError> {
        let rows = query_as::<_, (String, String, i64, i64, f64, f64)>(
            "SELECT channel_id, model, COUNT(*)::bigint, SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint, COALESCE(AVG(latency_ms)::float8, 0), COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8, 0)
             FROM billing_events
             WHERE timestamp::timestamptz >= NOW() - INTERVAL '1 day'
             GROUP BY channel_id, model ORDER BY COUNT(*) DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query channel usage: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|(ch, m, req, suc, avg, p95)| (ch, m, req as u64, suc as u64, avg, p95))
            .collect())
    }

    async fn recent_request_paths(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, DbError> {
        let rows = query_as::<_, (String, String, String, Option<i64>, i64, bool)>(
            "SELECT timestamp, model, channel_id, endpoint_id, latency_ms, success FROM billing_events ORDER BY timestamp DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query recent paths: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|(ts, m, ch, eid, lat, suc)| (ts, m, ch, eid, lat as u64, suc))
            .collect())
    }

    async fn routing_flow_snapshot(
        &self,
        hours: u32,
    ) -> Result<Vec<(String, String, Option<i64>, u64)>, DbError> {
        use Row;
        let since = (chrono::Utc::now() - chrono::Duration::hours(hours as i64))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();
        let rows = query("SELECT model, channel_id, endpoint_id, COUNT(*)::bigint FROM billing_events WHERE \"timestamp\"::timestamp >= $1::timestamp GROUP BY model, channel_id, endpoint_id")
            .bind(&since).fetch_all(&self.pool).await.map_err(|e| DbError(format!("routing_flow_snapshot: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<String, _>(1).unwrap_or_default(),
                    r.try_get::<Option<i64>, _>(2).unwrap_or(None),
                    r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                )
            })
            .collect())
    }

    async fn routing_history_buckets(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<crate::db::RoutingHistoryBucket>, DbError> {
        use Row;
        let rows = query(
            "SELECT
                CASE WHEN (EXTRACT(EPOCH FROM $2::timestamp - $1::timestamp)) < 172800
                  THEN date_trunc('hour', \"timestamp\"::timestamp)::text
                  ELSE date_trunc('day',  \"timestamp\"::timestamp)::text
                END AS bucket,
                channel_id,
                COUNT(*)::bigint AS requests,
                SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                AVG(latency_ms)::float8 AS avg_latency
             FROM billing_events
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY bucket, channel_id
             ORDER BY bucket ASC",
        )
        .bind(start)
        .bind(end)
        .bind(model)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("routing_history_buckets: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| crate::db::RoutingHistoryBucket {
                bucket: r.try_get::<String, _>(0).unwrap_or_default(),
                channel_id: r.try_get::<String, _>(1).unwrap_or_default(),
                endpoint_id: None,
                requests: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
                successes: r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                avg_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
            })
            .collect())
    }

    async fn routing_history_endpoint_stats(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<crate::db::RoutingEndpointStat>, DbError> {
        use Row;
        let rows = query(
            "SELECT channel_id,
                    COUNT(*)::bigint AS requests,
                    SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                    AVG(latency_ms)::float8 AS avg_latency,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms), 0)::float8 AS p95_latency
             FROM billing_events
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY channel_id
             ORDER BY requests DESC",
        )
        .bind(start).bind(end).bind(model)
        .fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_stats: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| crate::db::RoutingEndpointStat {
                channel_id: r.try_get::<String, _>(0).unwrap_or_default(),
                endpoint_id: None,
                requests: r.try_get::<i64, _>(1).unwrap_or(0) as u64,
                successes: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
                avg_latency: r.try_get::<f64, _>(3).unwrap_or(0.0),
                p95_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
            })
            .collect())
    }

    async fn routing_history_endpoint_details(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, f64, f64)>, DbError> {
        use Row;
        let rows = query(
            "SELECT ul.channel_id, ul.endpoint_id, e.url,
                    COUNT(*)::bigint, SUM(CASE WHEN ul.success THEN 1 ELSE 0 END)::bigint,
                    AVG(ul.latency_ms)::float8,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY ul.latency_ms),0)::float8
             FROM billing_events ul LEFT JOIN endpoints e ON e.id=ul.endpoint_id
             WHERE \"ul\".\"timestamp\"::timestamp>=$1::timestamp AND \"ul\".\"timestamp\"::timestamp<=$2::timestamp
               AND ($3::text IS NULL OR ul.model=$3)
             GROUP BY ul.channel_id, ul.endpoint_id, e.url ORDER BY ul.channel_id, COUNT(*) DESC",
        ).bind(start).bind(end).bind(model).fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_details: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<Option<i64>, _>(1).unwrap_or(None),
                    r.try_get::<Option<String>, _>(2).unwrap_or(None),
                    r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                    r.try_get::<i64, _>(4).unwrap_or(0) as u64,
                    r.try_get::<f64, _>(5).unwrap_or(0.0),
                    r.try_get::<f64, _>(6).unwrap_or(0.0),
                )
            })
            .collect())
    }

    // ── Announcements ──────────────────────────────────────────────────

    async fn list_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        let rows = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_announcements: {e}")))?;
        Ok(rows.iter().map(map_announcement_row).collect())
    }

    async fn list_published_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        let rows = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements WHERE published = true ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_published_announcements: {e}")))?;
        Ok(rows.iter().map(map_announcement_row).collect())
    }

    async fn get_announcement(&self, id: &str) -> Result<Option<AnnouncementRow>, DbError> {
        let row = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_announcement: {e}")))?;
        Ok(row.as_ref().map(map_announcement_row))
    }

    async fn create_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        query(
            "INSERT INTO announcements (id, title, content, created_by, created_at, updated_at, published) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&a.id)
        .bind(&a.title)
        .bind(&a.content)
        .bind(&a.created_by)
        .bind(&a.created_at)
        .bind(&a.updated_at)
        .bind(a.published)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("create_announcement: {e}")))?;
        Ok(())
    }

    async fn update_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        query(
            "UPDATE announcements SET title = $1, content = $2, updated_at = $3, published = $4 \
             WHERE id = $5",
        )
        .bind(&a.title)
        .bind(&a.content)
        .bind(&a.updated_at)
        .bind(a.published)
        .bind(&a.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("update_announcement: {e}")))?;
        Ok(())
    }

    async fn delete_announcement(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM announcements WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("delete_announcement: {e}")))?;
        Ok(())
    }

    // ── Casbin Policies ──────────────────────────────────────────────────

    async fn casbin_list_policies(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>, DbError> {
        let rows = query("SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_policies ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError(format!("casbin_list_policies: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<String, _>(1).unwrap_or_default(),
                    r.try_get::<String, _>(2).unwrap_or_default(),
                    r.try_get::<String, _>(3).unwrap_or_default(),
                    r.try_get::<String, _>(4).unwrap_or_default(),
                    r.try_get::<String, _>(5).unwrap_or_default(),
                    r.try_get::<String, _>(6).unwrap_or_default(),
                )
            })
            .collect())
    }

    async fn casbin_add_policy(
        &self,
        ptype: &str,
        v0: &str,
        v1: &str,
        v2: &str,
        v3: &str,
        v4: &str,
        v5: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO casbin_policies (ptype, v0, v1, v2, v3, v4, v5) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (ptype, v0, v1, v2, v3, v4, v5) DO NOTHING",
        )
        .bind(ptype)
        .bind(v0)
        .bind(v1)
        .bind(v2)
        .bind(v3)
        .bind(v4)
        .bind(v5)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("casbin_add_policy: {e}")))?;
        Ok(())
    }

    async fn casbin_remove_policy(&self, ptype: &str, v0: &str, v1: &str) -> Result<(), DbError> {
        query("DELETE FROM casbin_policies WHERE ptype = $1 AND v0 = $2 AND v1 = $3")
            .bind(ptype)
            .bind(v0)
            .bind(v1)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("casbin_remove_policy: {e}")))?;
        Ok(())
    }

    async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        let rows = query_as::<_, (String, f64, f64)>(
            "SELECT id, balance, frozen FROM users LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, b, f)| {
                (
                    id,
                    Decimal::try_from(b).unwrap_or(Decimal::ZERO),
                    Decimal::try_from(f).unwrap_or(Decimal::ZERO),
                )
            })
            .collect())
    }
}
