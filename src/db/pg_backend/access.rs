use super::*;
use async_trait::async_trait;

#[async_trait]
impl AccessBackend for PgBackend {
    // ── Billing groups ────────────────────────────────────────────────

    async fn list_billing_groups(
        &self,
        active_only: bool,
    ) -> Result<Vec<BillingGroupRow>, DbError> {
        let rows = if active_only {
            query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups WHERE status = 'active' ORDER BY is_default DESC, name, id")
                .fetch_all(&self.pool)
                .await?
        } else {
            query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups ORDER BY is_default DESC, name, id")
                .fetch_all(&self.pool)
                .await?
        };
        rows.iter().map(map_billing_group_row).collect()
    }

    async fn get_billing_group(&self, id: &str) -> Result<Option<BillingGroupRow>, DbError> {
        let row = query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(map_billing_group_row).transpose()
    }

    async fn create_billing_group(
        &self,
        id: &str,
        name: &str,
        payment_mode: BillingPaymentMode,
        created_by: &str,
    ) -> Result<BillingGroupRow, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let row = query("INSERT INTO billing_groups (id, name, payment_mode, status, is_default, created_by, created_at, updated_at) VALUES ($1, $2, $3, 'active', false, $4, $5, $5) RETURNING id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by")
            .bind(id)
            .bind(name)
            .bind(payment_mode.as_str())
            .bind(created_by)
            .bind(now)
            .fetch_one(&self.pool)
            .await?;
        map_billing_group_row(&row)
    }

    async fn set_billing_group_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        if !matches!(status, "active" | "inactive") {
            return Err(DbError("invalid billing group status".to_string()));
        }
        let result = query("UPDATE billing_groups SET status = $1, updated_at = $2 WHERE id = $3 AND is_default = false AND deleted_at IS NULL")
            .bind(status)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("billing group not found or protected".to_string()));
        }
        Ok(())
    }

    async fn delete_billing_group(
        &self,
        id: &str,
        actor_id: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row =
            query("SELECT is_default, deleted_at FROM billing_groups WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError("billing group not found".to_string()))?;
        if row.try_get::<bool, _>(0)? {
            return Err(DbError("billing group protected".to_string()));
        }
        if row.try_get::<Option<String>, _>(1)?.is_some() {
            return Err(DbError("billing group already deleted".to_string()));
        }
        // Deleting a group immediately unbinds every API key from it. The key
        // remains present for audit/UI purposes, but its null billing group
        // makes subsequent reservations fail closed until an administrator
        // explicitly assigns a new active group.
        let key_count: i64 =
            query_scalar("SELECT COUNT(*) FROM api_keys WHERE billing_group_id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        query("UPDATE api_keys SET billing_group_id = NULL, billing_payment_mode = NULL WHERE billing_group_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let reservation_count: i64 = query_scalar("SELECT COUNT(*) FROM token_request_reservations WHERE billing_group_id = $1 AND state = 'reserved'")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();
        query("UPDATE billing_groups SET status = 'inactive', deleted_at = $1, deleted_by = $2, deletion_reason = $3, updated_at = $1 WHERE id = $4 AND is_default = false AND deleted_at IS NULL")
            .bind(&now)
            .bind(actor_id)
            .bind(reason)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        query("CREATE TABLE IF NOT EXISTS billing_group_audit_log (id TEXT PRIMARY KEY, billing_group_id TEXT NOT NULL, action TEXT NOT NULL, actor_id TEXT NOT NULL, occurred_at TEXT NOT NULL, reason TEXT NOT NULL, affected_api_key_count BIGINT NOT NULL, affected_reservation_count BIGINT NOT NULL)")
            .execute(&mut *tx).await?;
        query("INSERT INTO billing_group_audit_log (id, billing_group_id, action, actor_id, occurred_at, reason, affected_api_key_count, affected_reservation_count) VALUES ($1,$2,'delete',$3,$4,$5,$6,$7)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(id).bind(actor_id).bind(&now).bind(reason).bind(key_count).bind(reservation_count)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── API Keys ─────────────────────────────────────────────────────────

    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        // Personal key list only: team-scoped keys (team_id NOT NULL) are
        // managed via the team endpoints (list_team_api_keys).
        let rows = query(
            "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode FROM api_keys WHERE user_id = $1 AND team_id IS NULL ORDER BY key",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        let mut keys: Vec<ApiKey> = rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(6);
                ApiKey {
                    key: r.get(0),
                    user_id: r.get(1),
                    name: r.get(2),
                    enabled: r.get(3),
                    expires_at: r.get(4),
                    spend_limit: r
                        .get::<Option<f64>, _>(5)
                        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                    team_id: r.get(7),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(9)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Prepaid),
                }
            })
            .collect();
        // 填充每个 key 的访问范围（资源类型，来自 api_key_scopes 表）
        let scope_rows = query(
            "SELECT api_key_id, resource_type FROM api_key_scopes \
             WHERE action='invoke' AND api_key_id = ANY($1)",
        )
        .bind(keys.iter().map(|k| k.key.clone()).collect::<Vec<String>>())
        .fetch_all(&self.pool)
        .await?;
        let mut scope_map: std::collections::HashMap<String, Vec<String>> = Default::default();
        for row in scope_rows.iter() {
            scope_map.entry(row.get(0)).or_default().push(row.get(1));
        }
        for k in &mut keys {
            k.scopes = scope_map.get(&k.key).cloned();
        }
        Ok(keys)
    }

    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        query(
            "INSERT INTO api_keys (key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&key.key)
        .bind(&key.user_id)
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit.map(|v| v.to_f64().unwrap_or(0.0)))
        .bind(allowed)
        .bind(&key.team_id)
        .bind(&key.billing_group_id)
        .bind(key.billing_payment_mode.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        query("DELETE FROM api_keys WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        query(
            "UPDATE api_keys SET name = $1, enabled = $2, expires_at = $3, spend_limit = $4, allowed_models = $5, team_id = $6, billing_group_id = $7, billing_payment_mode = $8 WHERE key = $9",
        )
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit.map(|v| v.to_f64().unwrap_or(0.0)))
        .bind(allowed)
        .bind(&key.team_id)
        .bind(&key.billing_group_id)
        .bind(key.billing_payment_mode.as_str())
        .bind(&key.key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        let rows = query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, u.status, u.suspended_at, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models, a.team_id, a.billing_group_id, a.billing_payment_mode \
             FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = $1",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let allowed_models_str: Option<String> = r.get(17);
            let api_key = ApiKey {
                key: r.get(11),
                user_id: r.get(12),
                name: r.get(13),
                enabled: r.get(14),
                expires_at: r.get(15),
                spend_limit: r
                    .get::<Option<f64>, _>(16)
                    .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                allowed_models: allowed_models_str
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                team_id: r.get(18),
                scopes: None,
                billing_group_id: r.get::<Option<String>, _>(19).unwrap_or_default(),
                billing_payment_mode: r
                    .get::<Option<String>, _>(20)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(BillingPaymentMode::Prepaid),
            };
            let user = {
                let rpm: Option<i64> = r.get(2);
                let tpm: Option<i64> = r.get(3);
                User {
                    id: r.get(0),
                    name: r.get(1),
                    password_hash: None,
                    rate_limits: {
                        let rpm = rpm.map(|v| v as u64);
                        let tpm = tpm.map(|v| v as u64);
                        if rpm.is_some() || tpm.is_some() {
                            Some(crate::domain::user::RateLimit { rpm, tpm })
                        } else {
                            None
                        }
                    },
                    timezone: r.get::<Option<String>, _>(4).unwrap_or_default(),
                    token_version: r.get::<i64, _>(5),
                    role: r.get::<Option<String>, _>(6).unwrap_or_default(),
                    concurrency_limit: r.get::<i64, _>(7) as u32,
                    currency: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    status: r
                        .get::<Option<String>, _>(9)
                        .unwrap_or_else(|| USER_STATUS_ACTIVE.to_string()),
                    suspended_at: r.get(10),
                }
            };
            (user, api_key)
        }))
    }

    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        let rows = query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, u.status, u.suspended_at, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models, a.team_id, a.billing_group_id, a.billing_payment_mode \
             FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(17);
                let api_key = ApiKey {
                    key: r.get(11),
                    user_id: r.get(12),
                    name: r.get(13),
                    enabled: r.get(14),
                    expires_at: r.get(15),
                    spend_limit: r
                        .get::<Option<f64>, _>(16)
                        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                    team_id: r.get(18),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(19).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(20)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Prepaid),
                };
                let user = {
                    let rpm: Option<i64> = r.get(2);
                    let tpm: Option<i64> = r.get(3);
                    User {
                        id: r.get(0),
                        name: r.get(1),
                        password_hash: None,
                        rate_limits: {
                            let rpm = rpm.map(|v| v as u64);
                            let tpm = tpm.map(|v| v as u64);
                            if rpm.is_some() || tpm.is_some() {
                                Some(crate::domain::user::RateLimit { rpm, tpm })
                            } else {
                                None
                            }
                        },
                        timezone: r.get::<Option<String>, _>(4).unwrap_or_default(),
                        token_version: r.get::<i64, _>(5),
                        role: r.get::<Option<String>, _>(6).unwrap_or_default(),
                        concurrency_limit: r.get::<i64, _>(7) as u32,
                        currency: r.get::<Option<String>, _>(8).unwrap_or_default(),
                        status: r
                            .get::<Option<String>, _>(9)
                            .unwrap_or_else(|| USER_STATUS_ACTIVE.to_string()),
                        suspended_at: r.get(10),
                    }
                };
                (user, api_key)
            })
            .collect())
    }

    // ── API Key Scopes（Platform API Key） ─────────────────────────────

    async fn add_api_key_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO api_key_scopes (id, api_key_id, resource_type, resource_id, action, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT (api_key_id, resource_type, resource_id, action) DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(api_key_id)
        .bind(resource_type)
        .bind(resource_id)
        .bind(action)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn api_key_has_resource_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<bool, DbError> {
        let row = query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM api_key_scopes \
             WHERE api_key_id=$1 AND resource_type=$2 AND action=$3 \
             AND (resource_id='*' OR resource_id=$4)",
        )
        .bind(api_key_id)
        .bind(resource_type)
        .bind(action)
        .bind(resource_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row > 0)
    }
}
