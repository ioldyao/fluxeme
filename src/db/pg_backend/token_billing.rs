use super::*;
use async_trait::async_trait;

#[async_trait]
impl TokenBillingBackend for PgBackend {
    // ── Batch Operations ────────────────────────────────────────────────

    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        let mut tx = self.pool.begin().await?;
        let mut deductions: Vec<(String, Decimal, Decimal)> = Vec::new();

        for record in batch {
            // Reservation settlement is authoritative for monetary fields. If the
            // async finalizer has not committed actual usage yet, defer the whole
            // batch to the durable usage backlog instead of creating a zero-value
            // billing event that can outlive the later settlement.
            if let Some((state, settlement_state)) = query_as::<_, (String, String)>(
                "SELECT state, settlement_state FROM token_request_reservations WHERE request_id = $1",
            )
            .bind(&record.request_id)
            .fetch_optional(&mut *tx)
            .await?
            {
                if state == "reserved" && settlement_state == "reserved" {
                    return Err(DbError("token settlement pending; retry usage billing".to_string()));
                }
            }

            let (prompt_price, completion_price, cache_read_price, cache_write_price) = {
                // Lookup pricing within transaction
                let result = query_as::<_, (f64, f64, f64, f64)>(
                    "SELECT prompt_price, completion_price, cache_read_price, cache_write_price FROM models WHERE name = $1",
                )
                .bind(&record.model)
                .fetch_optional(&mut *tx)
                .await;

                match result {
                    Ok(Some(p)) => p,
                    _ => {
                        // Fallback to pattern matching
                        let rows = query_as::<_, (f64, f64, f64, f64, String)>(
                            "SELECT prompt_price, completion_price, cache_read_price, cache_write_price, model_pattern FROM models",
                        )
                        .fetch_all(&mut *tx)
                        .await
                        .unwrap_or_default();

                        let mut found = (0.0, 0.0, 0.0, 0.0);
                        for (p, c, cr, cw, pattern) in rows {
                            if pattern.ends_with('*') {
                                let prefix = &pattern[..pattern.len() - 1];
                                if record.model.starts_with(prefix) {
                                    found = (p, c, cr, cw);
                                    break;
                                }
                            }
                            if pattern == record.model {
                                found = (p, c, cr, cw);
                                break;
                            }
                        }
                        found
                    }
                }
            };

            // Compute cost_amount (always — used for observability even when billing is off)
            let cost_amount = compute_cost_amount(
                record.prompt_tokens,
                record.completion_tokens,
                record.cache_hit_input_tokens,
                record.cache_write_tokens,
                prompt_price,
                completion_price,
                cache_read_price,
                cache_write_price,
            );

            // Insert only billing metadata into PostgreSQL.
            let account_type = record
                .account_type
                .clone()
                .unwrap_or_else(|| "user".to_string());
            let billing_inserted = query(
                "INSERT INTO billing_events (\
                 timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, cache_write_tokens, \
                 prompt_price, completion_price, cache_read_price, cache_write_price, cost_amount, \
                 api_key_name, api_format, stream, client_ip, endpoint_id, \
                 request_body, response_body, reasoning_body, original_model, \
                 success, status_code, team_id, account_type, activity_status, charge_source, priced_cost_amount) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
                 $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, \
                 $24, $25, $26, $27, $28, $29, $30, $31, $32, $33) \
                 ON CONFLICT (request_id) DO NOTHING",
            )
            .bind(&record.timestamp)
            .bind(&record.request_id)
            .bind(&record.user_id)
            .bind(&record.user_name)
            .bind(&record.channel_id)
            .bind(&record.model)
            .bind(record.prompt_tokens as i64)
            .bind(record.completion_tokens as i64)
            .bind(record.total_tokens as i64)
            .bind(record.latency_ms as i64)
            .bind(record.cache_hit_input_tokens as i64)
            .bind(record.cache_write_tokens as i64)
            .bind(prompt_price)
            .bind(completion_price)
            .bind(cache_read_price)
            .bind(cache_write_price)
            .bind(cost_amount)
            .bind(&record.api_key_name)
            .bind(&record.api_format)
            .bind(record.stream)
            .bind(&record.client_ip)
            .bind(record.endpoint_id)
            .bind(Option::<String>::None)
            .bind(Option::<String>::None)
            .bind(Option::<String>::None)
            .bind(&record.original_model)
            .bind(record.success)
            .bind(record.status_code as i32)
            .bind(&record.team_id)
            .bind(&account_type)
            .bind(if record.status_code == 499 { "interrupted" } else if record.success { "success" } else { "failed" })
            .bind(if cost_amount > 0.0 { "wallet" } else { "unknown" })
            .bind(cost_amount)
            .execute(&mut *tx)
            .await?;
            if billing_inserted.rows_affected() == 0 {
                // A replay is never chargeable, but it may still carry metadata
                // missing from an earlier partial write. Keep that repair
                // separate from the insert result so it cannot re-enter billing.
                query(
                    "UPDATE billing_events
                     SET user_name = COALESCE(NULLIF($1, ''), user_name),
                         api_key_name = COALESCE(NULLIF($2, ''), api_key_name),
                         priced_cost_amount = CASE WHEN priced_cost_amount = 0 THEN $3 ELSE priced_cost_amount END
                     WHERE request_id = $4",
                )
                .bind(&record.user_name)
                .bind(&record.api_key_name)
                .bind(cost_amount)
                .bind(&record.request_id)
                .execute(&mut *tx)
                // A reservation-backed record must continue into the metadata
                // reconciliation below; non-reservation replays must not charge again.
                .await?;
                let has_reservation: bool = query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM token_request_reservations WHERE request_id = $1)",
                )
                .bind(&record.request_id)
                .fetch_one(&mut *tx)
                .await?;
                if !has_reservation {
                    continue;
                }
            }

            // Requests using reserve+settle already performed their authoritative
            // package/wallet charge synchronously. The background usage writer only
            // persists the billing fact and must never debit the wallet again.
            // It also rewrites cost_amount to the actual wallet fallback amount so
            // package-covered requests display zero monetary usage in billing views.
            let has_token_reservation: bool = query_scalar(
                "SELECT EXISTS(SELECT 1 FROM token_request_reservations WHERE request_id = $1)",
            )
            .bind(&record.request_id)
            .fetch_one(&mut *tx)
            .await?;
            if has_token_reservation {
                query(
                    "UPDATE billing_events be
                     SET reservation_id = r.id,
                         package_grant_id = r.package_grant_id,
                         accounting_mode = r.accounting_mode,
                         package_units = COALESCE(r.actual_package_units, 0),
                         wallet_amount = COALESCE(r.actual_wallet_amount, 0),
                         cost_amount = COALESCE(r.actual_wallet_amount, 0),
                         activity_status = CASE WHEN be.status_code = 499 THEN 'interrupted' WHEN be.success = false THEN 'failed' WHEN r.actual_package_units > 0 OR r.actual_wallet_amount > 0 THEN 'success' ELSE be.activity_status END,
                         status_reason = COALESCE(r.reason, be.status_reason),
                         charge_source = CASE WHEN r.billing_payment_mode = 'prepaid' AND r.actual_package_units > 0 THEN 'prepaid_package' WHEN r.billing_payment_mode = 'prepaid' THEN 'prepaid' WHEN r.actual_package_units > 0 AND r.actual_wallet_amount > 0 THEN 'package_and_wallet' WHEN r.actual_package_units > 0 THEN 'package' WHEN r.actual_wallet_amount > 0 THEN 'wallet' ELSE be.charge_source END,
                         api_key_name = COALESCE(NULLIF(r.api_key_name, ''), be.api_key_name),
                         billing_group_id = COALESCE(NULLIF(r.billing_group_id, ''), be.billing_group_id),
                         billing_group_name = COALESCE(NULLIF(r.billing_group_name, ''), be.billing_group_name),
                         billing_payment_mode = COALESCE(NULLIF(r.billing_payment_mode, ''), be.billing_payment_mode),
                         prompt_price = r.prompt_price,
                         completion_price = r.completion_price,
                         cache_read_price = r.cache_read_price,
                         cache_write_price = r.cache_write_price,
                         stream = $2,
                         priced_cost_amount = COALESCE(r.actual_priced_cost_amount, 0)
                     FROM token_request_reservations r
                     WHERE be.request_id = r.request_id AND be.request_id = $1",
                )
                .bind(&record.request_id)
                .bind(record.stream)
                .execute(&mut *tx)
                .await?;
                continue;
            }

            if billing_enabled && cost_amount > 0.0 {
                // Team-scoped records charge the team wallet; personal records
                // charge the user wallet. The personal path is preserved
                // verbatim so existing behavior is unchanged.
                if let Some(team_id) = &record.team_id {
                    let (balance, frozen): (f64, f64) = query_as(
                        "SELECT balance, frozen FROM team_wallets WHERE team_id = $1 FOR UPDATE",
                    )
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;

                    let reserved: f64 = query_scalar(
                        "SELECT token_wallet_reserved FROM team_wallets WHERE team_id = $1",
                    )
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;
                    let spendable = balance - frozen - reserved;
                    if spendable < cost_amount {
                        return Err(DbError("insufficient team wallet balance".to_string()));
                    }

                    let new_balance = balance - cost_amount;
                    query(
                        "UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3",
                    )
                    .bind(new_balance)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;

                    let now = chrono::Utc::now().to_rfc3339();
                    query(
                        "INSERT INTO wallet_transactions (id, user_id, type, amount, \
                         balance_before, balance_after, method, status, note, created_at, \
                         team_id, account_type) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&record.user_id)
                    .bind("deduction")
                    .bind(-cost_amount)
                    .bind(balance)
                    .bind(new_balance)
                    .bind("usage")
                    .bind("completed")
                    .bind(format!("Usage: {}", record.model))
                    .bind(&now)
                    .bind(team_id)
                    .bind("team")
                    .execute(&mut *tx)
                    .await?;

                    deductions.push((
                        team_id.clone(),
                        Decimal::try_from(new_balance).unwrap_or(Decimal::ZERO),
                        Decimal::try_from(frozen).unwrap_or(Decimal::ZERO),
                    ));
                } else {
                    let (balance, frozen, reserved): (f64, f64, f64) =
                        query_as("SELECT balance, frozen, token_wallet_reserved FROM users WHERE id = $1 FOR UPDATE")
                            .bind(&record.user_id)
                            .fetch_one(&mut *tx)
                            .await?;
                    let spendable = balance - frozen - reserved;
                    if spendable < cost_amount {
                        return Err(DbError("insufficient wallet balance".to_string()));
                    }
                    if spendable < cost_amount {
                        tracing::warn!(
                            user_id = &record.user_id,
                            balance,
                            frozen,
                            cost_amount,
                            "Insufficient balance — skipping deduction"
                        );
                        continue;
                    }

                    let new_balance = balance - cost_amount;
                    query("UPDATE users SET balance = $1 WHERE id = $2")
                        .bind(new_balance)
                        .bind(&record.user_id)
                        .execute(&mut *tx)
                        .await?;

                    let now = chrono::Utc::now().to_rfc3339();
                    query(
                        "INSERT INTO wallet_transactions (id, user_id, type, amount, \
                         balance_before, balance_after, method, status, note, created_at, \
                         team_id, account_type) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&record.user_id)
                    .bind("deduction")
                    .bind(-cost_amount)
                    .bind(balance)
                    .bind(new_balance)
                    .bind("usage")
                    .bind("completed")
                    .bind(format!("Usage: {}", record.model))
                    .bind(&now)
                    .bind(Option::<String>::None)
                    .bind("user")
                    .execute(&mut *tx)
                    .await?;

                    deductions.push((
                        record.user_id.clone(),
                        Decimal::try_from(new_balance).unwrap_or(Decimal::ZERO),
                        Decimal::try_from(frozen).unwrap_or(Decimal::ZERO),
                    ));
                }
            }
        }

        tx.commit().await?;
        Ok(deductions)
    }

    // ── Token resource packages ──────────────────────────────────────────

    async fn create_token_package_plan(
        &self,
        id: &str,
        code: &str,
        name: &str,
        accounting_mode: &str,
        display_token_amount: i64,
        total_units: i64,
        input_credit_factor: Decimal,
        output_credit_factor: Decimal,
        cache_credit_factor: Decimal,
        exhaustion_policy: &str,
        priority: i32,
        validity_days: Option<i32>,
        created_by: &str,
    ) -> Result<crate::domain::token_package::TokenPackagePlanRow, DbError> {
        if display_token_amount <= 0 || total_units <= 0 {
            return Err(DbError("package amounts must be positive".to_string()));
        }
        if !matches!(accounting_mode, "raw_tokens" | "standardized_credits") {
            return Err(DbError("unsupported accounting mode".to_string()));
        }
        if !matches!(exhaustion_policy, "package_then_wallet" | "package_only") {
            return Err(DbError("unsupported exhaustion policy".to_string()));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let row = query(
            "INSERT INTO token_package_plans
             (id, code, name, accounting_mode, display_token_amount, total_units,
              input_credit_factor, output_credit_factor, cache_credit_factor,
              exhaustion_policy, priority, validity_days, status, created_by, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'active',$13,$14,$14)
             RETURNING id, code, name, accounting_mode, display_token_amount, total_units,
                       input_credit_factor::text, output_credit_factor::text, cache_credit_factor::text,
                       exhaustion_policy, priority, validity_days, status, created_at, updated_at",
        )
        .bind(id).bind(code).bind(name).bind(accounting_mode).bind(display_token_amount)
        .bind(total_units)
        .bind(input_credit_factor.to_f64().unwrap_or(1.0))
        .bind(output_credit_factor.to_f64().unwrap_or(1.0))
        .bind(cache_credit_factor.to_f64().unwrap_or(0.0))
        .bind(exhaustion_policy).bind(priority).bind(validity_days).bind(created_by).bind(&now)
        .fetch_one(&self.pool)
        .await?;
        Ok(crate::domain::token_package::TokenPackagePlanRow {
            id: row.try_get(0)?,
            code: row.try_get(1)?,
            name: row.try_get(2)?,
            accounting_mode: row.try_get::<String, _>(3)?.parse().map_err(DbError)?,
            display_token_amount: row.try_get::<i64, _>(4)?.max(0) as u64,
            total_units: row.try_get::<i64, _>(5)?.max(0) as u64,
            input_credit_factor: row.try_get::<String, _>(6)?.parse().unwrap_or(Decimal::ONE),
            output_credit_factor: row.try_get::<String, _>(7)?.parse().unwrap_or(Decimal::ONE),
            cache_credit_factor: row
                .try_get::<String, _>(8)?
                .parse()
                .unwrap_or(Decimal::ZERO),
            exhaustion_policy: row.try_get::<String, _>(9)?.parse().map_err(DbError)?,
            priority: row.try_get(10)?,
            validity_days: row.try_get(11)?,
            status: row.try_get(12)?,
            created_at: row.try_get(13)?,
            updated_at: row.try_get(14)?,
        })
    }

    async fn delete_token_package_plan(&self, plan_id: &str) -> Result<(), DbError> {
        let active_grants: i64 = query_scalar(
            "SELECT COUNT(*) FROM token_package_grants WHERE plan_id = $1 AND status = 'active'",
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await?;
        if active_grants > 0 {
            return Err(DbError(
                "cannot delete a plan with active grants".to_string(),
            ));
        }
        let deleted = query("DELETE FROM token_package_plans WHERE id = $1")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(DbError("token package plan not found".to_string()));
        }
        Ok(())
    }

    async fn revoke_token_package_grant(&self, grant_id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let status = query_scalar::<_, String>(
            "SELECT status FROM token_package_grants WHERE id = $1 FOR UPDATE",
        )
        .bind(grant_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError("token package grant not found".to_string()))?;
        if status != "active" {
            return Err(DbError(format!(
                "token package grant cannot be revoked from status '{status}'"
            )));
        }
        let updated = query(
            "UPDATE token_package_grants
             SET status = 'revoked', updated_at = $1
             WHERE id = $2 AND status = 'active'",
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(grant_id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(DbError(
                "token package grant was changed concurrently".to_string(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_token_package_plans(
        &self,
    ) -> Result<Vec<crate::domain::token_package::TokenPackagePlanRow>, DbError> {
        let rows = query(
            "SELECT id, code, name, accounting_mode, display_token_amount, total_units,
                    input_credit_factor::text, output_credit_factor::text, cache_credit_factor::text,
                    exhaustion_policy, priority, validity_days, status, created_at, updated_at
             FROM token_package_plans ORDER BY priority DESC, created_at DESC, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(crate::domain::token_package::TokenPackagePlanRow {
                    id: row.try_get(0)?,
                    code: row.try_get(1)?,
                    name: row.try_get(2)?,
                    accounting_mode: row.try_get::<String, _>(3)?.parse().map_err(DbError)?,
                    display_token_amount: row.try_get::<i64, _>(4)?.max(0) as u64,
                    total_units: row.try_get::<i64, _>(5)?.max(0) as u64,
                    input_credit_factor: row
                        .try_get::<String, _>(6)?
                        .parse()
                        .unwrap_or(Decimal::ONE),
                    output_credit_factor: row
                        .try_get::<String, _>(7)?
                        .parse()
                        .unwrap_or(Decimal::ONE),
                    cache_credit_factor: row
                        .try_get::<String, _>(8)?
                        .parse()
                        .unwrap_or(Decimal::ZERO),
                    exhaustion_policy: row.try_get::<String, _>(9)?.parse().map_err(DbError)?,
                    priority: row.try_get(10)?,
                    validity_days: row.try_get(11)?,
                    status: row.try_get(12)?,
                    created_at: row.try_get(13)?,
                    updated_at: row.try_get(14)?,
                })
            })
            .collect()
    }

    async fn list_token_package_grants(
        &self,
        user_id: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<Vec<crate::domain::token_package::TokenPackageGrantRow>, DbError> {
        let rows = match (user_id, team_id) {
            (Some(user_id), None) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id WHERE g.user_id = $1 ORDER BY g.priority DESC, g.expires_at NULLS LAST, g.created_at, g.id",
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?,
            (None, Some(team_id)) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id WHERE g.team_id = $1 ORDER BY g.priority DESC, g.expires_at NULLS LAST, g.created_at, g.id",
            )
            .bind(team_id)
            .fetch_all(&self.pool)
            .await?,
            (None, None) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id \
                 ORDER BY g.created_at DESC, g.id",
            )
            .fetch_all(&self.pool)
            .await?,
            _ => return Ok(Vec::new()),
        };
        rows.into_iter()
            .map(|row| {
                let mode = row
                    .try_get::<String, _>(6)
                    .unwrap_or_else(|_| "raw_tokens".to_string())
                    .parse()
                    .map_err(|e: String| DbError(e))?;
                let policy = row
                    .try_get::<String, _>(12)
                    .unwrap_or_else(|_| "package_then_wallet".to_string())
                    .parse()
                    .map_err(|e: String| DbError(e))?;
                Ok(crate::domain::token_package::TokenPackageGrantRow {
                    id: row.try_get(0)?,
                    plan_id: row.try_get(1)?,
                    plan_code: row.try_get(2)?,
                    plan_name: row.try_get(3)?,
                    user_id: row.try_get(4)?,
                    team_id: row.try_get(5)?,
                    accounting_mode: mode,
                    display_token_amount: row.try_get::<i64, _>(7)?.max(0) as u64,
                    total_units: row.try_get::<i64, _>(8)?.max(0) as u64,
                    consumed_units: row.try_get::<i64, _>(9)?.max(0) as u64,
                    reserved_units: row.try_get::<i64, _>(10)?.max(0) as u64,
                    priority: row.try_get(11)?,
                    exhaustion_policy: policy,
                    status: row.try_get(13)?,
                    expires_at: row.try_get(14)?,
                    created_at: row.try_get(15)?,
                })
            })
            .collect()
    }

    async fn create_token_package_grant(
        &self,
        grant_id: &str,
        plan_id: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        source: &str,
        note: &str,
        expires_at: Option<&str>,
    ) -> Result<crate::domain::token_package::TokenPackageGrantRow, DbError> {
        if user_id.is_none() == team_id.is_none() {
            return Err(DbError(
                "exactly one of user_id or team_id is required".to_string(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let row = query(
            "INSERT INTO token_package_grants
             (id, plan_id, user_id, team_id, accounting_mode, display_token_amount,
              total_units, priority, exhaustion_policy, source, note, expires_at, created_at, updated_at)
             SELECT $1, p.id, $3, $4, p.accounting_mode, p.display_token_amount,
                    p.total_units, p.priority, p.exhaustion_policy, $5, $6, $7, $8, $8
             FROM token_package_plans p WHERE p.id = $2 AND p.status = 'active'
             RETURNING id, plan_id, user_id, team_id, accounting_mode, display_token_amount,
                       total_units, consumed_units, reserved_units, priority, exhaustion_policy,
                       status, expires_at, created_at",
        )
        .bind(grant_id)
        .bind(plan_id)
        .bind(user_id)
        .bind(team_id)
        .bind(source)
        .bind(note)
        .bind(expires_at)
        .bind(&now)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError("token package plan not found or inactive".to_string()))?;
        let mode = row.try_get::<String, _>(4)?.parse().map_err(DbError)?;
        let policy = row.try_get::<String, _>(10)?.parse().map_err(DbError)?;
        let (plan_code, plan_name): (String, String) =
            query_as("SELECT code, name FROM token_package_plans WHERE id = $1")
                .bind(plan_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(crate::domain::token_package::TokenPackageGrantRow {
            id: row.try_get(0)?,
            plan_id: row.try_get(1)?,
            plan_code,
            plan_name,
            user_id: row.try_get(2)?,
            team_id: row.try_get(3)?,
            accounting_mode: mode,
            display_token_amount: row.try_get::<i64, _>(5)?.max(0) as u64,
            total_units: row.try_get::<i64, _>(6)?.max(0) as u64,
            consumed_units: row.try_get::<i64, _>(7)?.max(0) as u64,
            reserved_units: row.try_get::<i64, _>(8)?.max(0) as u64,
            priority: row.try_get(9)?,
            exhaustion_policy: policy,
            status: row.try_get(11)?,
            expires_at: row.try_get(12)?,
            created_at: row.try_get(13)?,
        })
    }

    async fn reserve_token_request(
        &self,
        request: &crate::domain::token_package::TokenReservationRequest,
    ) -> Result<crate::domain::token_package::TokenReservationHandle, DbError> {
        let mut tx = self.pool.begin().await?;
        if let Some(row) = query(
            "SELECT id, package_grant_id, accounting_mode, reserved_package_units, reserved_total_units, reserved_wallet_amount, factor_snapshot, request_fingerprint, state, prompt_price, completion_price, cache_read_price, cache_write_price \
             FROM token_request_reservations WHERE request_id = $1 FOR UPDATE",
        )
        .bind(&request.request_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            let stored_fingerprint = row.try_get::<String, _>(7).unwrap_or_default();
            if !stored_fingerprint.is_empty() && stored_fingerprint != request.request_fingerprint {
                return Err(DbError("request_id already belongs to different request".to_string()));
            }
            if row.try_get::<String, _>(8).unwrap_or_default() != "reserved" {
                return Err(DbError("request_id has already been finalized".to_string()));
            }
            let mode = row
                .try_get::<Option<String>, _>(2)?
                .and_then(|v| v.parse().ok());
            return Ok(crate::domain::token_package::TokenReservationHandle {
                reservation_id: row.try_get(0)?,
                request_id: request.request_id.clone(),
                package_grant_id: row.try_get(1)?,
                accounting_mode: mode,
                input_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["input_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ONE),
                output_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["output_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ONE),
                cache_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["cache_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ZERO),
                reserved_package_units: row.try_get::<i64, _>(3)?.max(0) as u64,
                reserved_total_units: row.try_get::<i64, _>(4)?.max(0) as u64,
                reserved_wallet_amount: Decimal::try_from(row.try_get::<f64, _>(5)?)
                    .unwrap_or(Decimal::ZERO),
                prompt_price: Decimal::try_from(row.try_get::<f64, _>(9).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                completion_price: Decimal::try_from(row.try_get::<f64, _>(10).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                cache_read_price: Decimal::try_from(row.try_get::<f64, _>(11).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                cache_write_price: Decimal::try_from(row.try_get::<f64, _>(12).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                billing_group_id: request.billing_group_id.clone(),
                billing_group_name: request.billing_group_name.clone(),
                billing_payment_mode: request.billing_payment_mode,
            });
        }

        let requested_raw = request
            .prompt_tokens
            .saturating_add(request.completion_tokens);
        let grants = if let Some(team_id) = request.team_id.as_deref() {
            query(
                "SELECT id, accounting_mode, exhaustion_policy, total_units, consumed_units, reserved_units \
                 FROM token_package_grants \
                 WHERE team_id = $1 AND status = 'active' AND (expires_at IS NULL OR expires_at > $2) \
                   AND total_units - consumed_units - reserved_units > 0 \
                 ORDER BY priority DESC, expires_at NULLS LAST, created_at, id FOR UPDATE",
            )
            .bind(team_id)
            .bind(&request.expires_at)
            .fetch_all(&mut *tx)
            .await?
        } else {
            query(
                "SELECT id, accounting_mode, exhaustion_policy, total_units, consumed_units, reserved_units \
                 FROM token_package_grants \
                 WHERE user_id = $1 AND status = 'active' AND (expires_at IS NULL OR expires_at > $2) \
                   AND total_units - consumed_units - reserved_units > 0 \
                 ORDER BY priority DESC, expires_at NULLS LAST, created_at, id FOR UPDATE",
            )
            .bind(&request.user_id)
            .bind(&request.expires_at)
            .fetch_all(&mut *tx)
            .await?
        };
        let grant = grants.first();
        let grant_id = grant.map(|r| r.try_get::<String, _>(0)).transpose()?;
        let mode_text = grant.and_then(|r| r.try_get::<String, _>(1).ok());
        let (input_factor, output_factor, cache_factor) = if let Some(id) = grant_id.as_deref() {
            query_as::<_, (f64, f64, f64)>(
                "SELECT input_factor, output_factor, cache_factor
                 FROM token_package_model_factors
                 WHERE plan_id = (SELECT plan_id FROM token_package_grants WHERE id = $1)
                   AND ($2 = model_pattern OR ($2 LIKE REPLACE(model_pattern, '*', '%')))
                 ORDER BY CASE WHEN $2 = model_pattern THEN 0 ELSE 1 END, model_pattern
                 LIMIT 1",
            )
            .bind(id)
            .bind(&request.model)
            .fetch_optional(&mut *tx)
            .await?
            .map(|(i, o, c)| {
                (
                    Decimal::try_from(i).unwrap_or(Decimal::ONE),
                    Decimal::try_from(o).unwrap_or(Decimal::ONE),
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                )
            })
            .unwrap_or((Decimal::ONE, Decimal::ONE, Decimal::ZERO))
        } else {
            (Decimal::ONE, Decimal::ONE, Decimal::ZERO)
        };
        let mode = mode_text.as_deref().and_then(|v| v.parse().ok());
        let requested = if mode
            == Some(crate::domain::token_package::TokenAccountingMode::StandardizedCredits)
        {
            let usage = crate::domain::token_package::TokenUsage {
                prompt_tokens: request.prompt_tokens,
                completion_tokens: request.completion_tokens,
                cache_hit_input_tokens: request.cache_hit_input_tokens,
                cache_write_tokens: 0,
            };
            usage.standardized_credits(input_factor, output_factor, cache_factor, Decimal::ONE)
        } else {
            requested_raw
        };
        let mut remaining_package_units = requested;
        let mut allocations: Vec<(String, u64, String)> = Vec::new();
        for row in &grants {
            if remaining_package_units == 0 {
                break;
            }
            let available = (row.try_get::<i64, _>(3).unwrap_or(0)
                - row.try_get::<i64, _>(4).unwrap_or(0)
                - row.try_get::<i64, _>(5).unwrap_or(0))
            .max(0) as u64;
            let units = remaining_package_units.min(available);
            if units > 0 {
                allocations.push((
                    row.try_get(0)?,
                    units,
                    row.try_get::<String, _>(2)
                        .unwrap_or_else(|_| "package_then_wallet".to_string()),
                ));
                remaining_package_units -= units;
            }
        }
        let package_units = requested.saturating_sub(remaining_package_units);
        let wallet_units = remaining_package_units;
        let policy = allocations
            .iter()
            .find(|(_, units, _)| *units > 0)
            .map(|(_, _, policy)| policy.as_str())
            .unwrap_or("package_then_wallet");
        if wallet_units > 0
            && policy == "package_only"
            && request.billing_payment_mode == BillingPaymentMode::Metered
        {
            return Err(DbError("token package quota exceeded".to_string()));
        }
        let wallet_hold =
            if request.billing_payment_mode == BillingPaymentMode::Metered && wallet_units > 0 {
                calculate_settlement(
                    TokenUsage {
                        prompt_tokens: request.prompt_tokens,
                        completion_tokens: request.completion_tokens,
                        cache_hit_input_tokens: request.cache_hit_input_tokens,
                        cache_write_tokens: 0,
                    },
                    0,
                    PriceSnapshot {
                        prompt: request.prompt_price,
                        completion: request.completion_price,
                        cache_read: request.cache_read_price,
                        cache_write: request.cache_write_price,
                    },
                    mode,
                    input_factor,
                    output_factor,
                    cache_factor,
                    package_units,
                    request.billing_payment_mode,
                )
                .wallet_amount
                .max(Decimal::ZERO)
            } else {
                // Prepaid requests are record-only: do not reserve or debit the
                // gateway wallet, even when package units do not cover the request.
                Decimal::ZERO
            };
        if wallet_hold > Decimal::ZERO {
            let amount = wallet_hold.to_f64().unwrap_or(f64::MAX);
            if let Some(team_id) = request.team_id.as_deref() {
                let updated = query(
                    "UPDATE team_wallets SET token_wallet_reserved = token_wallet_reserved + $1, updated_at = $2
                     WHERE team_id = $3 AND balance - frozen - token_wallet_reserved >= $1",
                )
                .bind(amount)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(team_id)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(DbError("insufficient team wallet balance".to_string()));
                }
            } else {
                let updated = query(
                    "UPDATE users SET token_wallet_reserved = token_wallet_reserved + $1
                     WHERE id = $2 AND balance - frozen - token_wallet_reserved >= $1",
                )
                .bind(amount)
                .bind(&request.user_id)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(DbError("insufficient wallet balance".to_string()));
                }
            }
        }
        let reservation_id = uuid::Uuid::new_v4().to_string();
        for (id, units, _) in &allocations {
            query("UPDATE token_package_grants SET reserved_units = reserved_units + $1, updated_at = $2 WHERE id = $3")
                .bind(*units as i64)
                .bind(&request.expires_at)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        query(
            "INSERT INTO token_request_reservations \
             (id, request_id, request_fingerprint, user_id, user_name, api_key_name, team_id, account_type, package_grant_id, model, accounting_mode, \
              reserved_prompt_tokens, reserved_completion_tokens, reserved_package_units, reserved_total_units, reserved_wallet_amount, \
              factor_snapshot, prompt_price, completion_price, cache_read_price, cache_write_price, estimated_priced_cost_amount, billing_group_id, billing_group_name, billing_payment_mode, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)",

        )
        .bind(&reservation_id)
        .bind(&request.request_id)
        .bind(&request.request_fingerprint)
        .bind(&request.user_id)
        .bind(&request.user_name)
        .bind(&request.api_key_name)
        .bind(&request.team_id)
        .bind(if request.team_id.is_some() { "team" } else { "user" })
        .bind(&grant_id)
        .bind(&request.model)
        .bind(&mode_text)
        .bind(request.prompt_tokens as i64)
        .bind(request.completion_tokens as i64)
        .bind(package_units as i64)
        .bind(requested as i64)
        .bind(wallet_hold.to_f64().unwrap_or(f64::MAX))
        .bind(serde_json::json!({
            "input_factor": input_factor,
            "output_factor": output_factor,
            "cache_factor": cache_factor,
        }).to_string())
        .bind(request.prompt_price.to_f64().unwrap_or(0.0))
        .bind(request.completion_price.to_f64().unwrap_or(0.0))
        .bind(request.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(request.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(request.estimated_priced_cost_amount.to_f64().unwrap_or(0.0))
        .bind(&request.billing_group_id)
        .bind(&request.billing_group_name)
        .bind(request.billing_payment_mode.as_str())
        .bind(&request.expires_at)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        for (id, units, _) in &allocations {
            query("INSERT INTO token_package_reservation_allocations (id, reservation_id, package_grant_id, reserved_units, created_at) VALUES ($1, $2, $3, $4, $5)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&reservation_id)
                .bind(id)
                .bind(*units as i64)
                .bind(chrono::Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(crate::domain::token_package::TokenReservationHandle {
            reservation_id,
            request_id: request.request_id.clone(),
            package_grant_id: grant_id,
            accounting_mode: mode_text.and_then(|v| v.parse().ok()),
            input_factor,
            output_factor,
            cache_factor,
            reserved_package_units: package_units,
            reserved_total_units: requested,
            reserved_wallet_amount: wallet_hold,
            prompt_price: request.prompt_price,
            completion_price: request.completion_price,
            cache_read_price: request.cache_read_price,
            cache_write_price: request.cache_write_price,
            billing_group_id: request.billing_group_id.clone(),
            billing_group_name: request.billing_group_name.clone(),
            billing_payment_mode: request.billing_payment_mode,
        })
    }

    async fn settle_token_request(
        &self,
        settlement: &crate::domain::token_package::TokenSettlementRequest,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query("SELECT state, package_grant_id, reserved_package_units, reserved_total_units, reserved_prompt_tokens, reserved_completion_tokens, user_id, team_id, reserved_wallet_amount, accounting_mode, factor_snapshot, billing_payment_mode, billing_group_id, billing_group_name, request_id, prompt_price, completion_price, cache_read_price, cache_write_price FROM token_request_reservations WHERE id = $1 FOR UPDATE")
            .bind(&settlement.reservation_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError("token reservation not found".to_string()))?;
        if row.try_get::<String, _>(0)? != "reserved" {
            return Ok(());
        }
        let grant_id = row.try_get::<Option<String>, _>(1)?;
        let reserved = row.try_get::<i64, _>(2)?.max(0) as u64;
        let user_id = row.try_get::<String, _>(6)?;
        let team_id = row.try_get::<Option<String>, _>(7)?;
        let reserved_wallet = row.try_get::<f64, _>(8)?.max(0.0);
        let request_id = row.try_get::<String, _>(14)?;
        let prompt_price = row.try_get::<f64, _>(15)?;
        let completion_price = row.try_get::<f64, _>(16)?;
        let cache_read_price = row.try_get::<f64, _>(17)?;
        let cache_write_price = row.try_get::<f64, _>(18)?;
        let billing_payment_mode = row
            .try_get::<String, _>(11)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(BillingPaymentMode::Metered);
        let accounting_mode = row.try_get::<Option<String>, _>(9)?;
        let factor_snapshot = row
            .try_get::<String, _>(10)
            .unwrap_or_else(|_| "{}".to_string());
        let (input_factor, output_factor, cache_factor) =
            serde_json::from_str::<serde_json::Value>(&factor_snapshot)
                .ok()
                .map(|v| {
                    (
                        v["input_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ONE),
                        v["output_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ONE),
                        v["cache_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ZERO),
                    )
                })
                .unwrap_or((Decimal::ONE, Decimal::ONE, Decimal::ZERO));
        let actual_usage = TokenUsage {
            prompt_tokens: settlement.actual_prompt_tokens,
            completion_tokens: settlement.actual_completion_tokens,
            cache_hit_input_tokens: settlement.actual_cache_hit_input_tokens,
            cache_write_tokens: settlement.actual_cache_write_tokens,
        };
        let breakdown = calculate_settlement(
            actual_usage,
            settlement.actual_cache_write_tokens,
            PriceSnapshot {
                prompt: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
                completion: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
                cache_read: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
                cache_write: Decimal::try_from(cache_write_price).unwrap_or(Decimal::ZERO),
            },
            accounting_mode
                .as_deref()
                .and_then(|value| value.parse().ok()),
            input_factor,
            output_factor,
            cache_factor,
            reserved,
            billing_payment_mode,
        );
        let actual_units = breakdown.actual_units;
        let mut allocation_rows: Vec<(String, i64)> = query_as(
            "SELECT package_grant_id, reserved_units
             FROM token_package_reservation_allocations
             WHERE reservation_id = $1 ORDER BY created_at, id FOR UPDATE",
        )
        .bind(&settlement.reservation_id)
        .fetch_all(&mut *tx)
        .await?;
        // Legacy reservations predate allocation rows and retain one grant id
        // plus a reservation-level unit count. Keep those rows settleable.
        if allocation_rows.is_empty() {
            if let Some(id) = grant_id.as_deref() {
                allocation_rows.push((id.to_string(), reserved as i64));
            }
        }
        let mut remaining_actual_units = actual_units;
        let mut package_units = 0u64;
        for (id, reserved_for_grant) in allocation_rows {
            let (total_units, consumed_units, reserved_units): (i64, i64, i64) = query_as(
                "SELECT total_units, consumed_units, reserved_units
                 FROM token_package_grants WHERE id = $1 FOR UPDATE",
            )
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
            let held_units = (reserved_for_grant.max(0) as u64).min(reserved_units.max(0) as u64);
            let available_after_hold = (total_units.max(0) as u64)
                .saturating_sub(consumed_units.max(0) as u64)
                .saturating_sub((reserved_units.max(0) as u64).saturating_sub(held_units));
            let consumed = remaining_actual_units.min(available_after_hold);
            query("UPDATE token_package_grants SET consumed_units = consumed_units + $1, reserved_units = GREATEST(0, reserved_units - $2), updated_at = $3 WHERE id = $4")
                .bind(consumed as i64)
                .bind(held_units as i64)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            query("UPDATE token_package_reservation_allocations SET consumed_units = $1 WHERE reservation_id = $2 AND package_grant_id = $3")
                .bind(consumed as i64)
                .bind(&settlement.reservation_id)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            if consumed > 0 {
                query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, created_at) VALUES ($1, $2, $3, $4, 'consume', $5, $6) ON CONFLICT DO NOTHING")
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&id)
                    .bind(&settlement.reservation_id)
                    .bind(&request_id)
                    .bind(consumed as i64)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .execute(&mut *tx)
                    .await?;
            }
            package_units += consumed;
            remaining_actual_units = remaining_actual_units.saturating_sub(consumed);
        }
        // Recalculate package coverage with the actual post-provider package
        // allocation. The theoretical cost remains all four price components.
        let breakdown = calculate_settlement(
            actual_usage,
            settlement.actual_cache_write_tokens,
            PriceSnapshot {
                prompt: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
                completion: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
                cache_read: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
                cache_write: Decimal::try_from(cache_write_price).unwrap_or(Decimal::ZERO),
            },
            accounting_mode
                .as_deref()
                .and_then(|value| value.parse().ok()),
            input_factor,
            output_factor,
            cache_factor,
            package_units,
            billing_payment_mode,
        );
        let mut wallet_amount = breakdown.wallet_amount.to_f64().unwrap_or(f64::MAX);
        let receivable_id = format!("receivable-{}", settlement.reservation_id);
        let mut initial_wallet_transaction_id: Option<String> = None;
        let now = chrono::Utc::now().to_rfc3339();
        // Release the entire authorization hold before checking the final debit.
        // This keeps the hold a reservation-only concern and makes overage
        // settlement use the same atomic available-balance check as any debit.
        if reserved_wallet > 0.0 {
            if let Some(team_id) = team_id.as_deref() {
                query("UPDATE team_wallets SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1), updated_at = $2 WHERE team_id = $3")
                    .bind(reserved_wallet)
                    .bind(&now)
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;
            } else {
                query("UPDATE users SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1) WHERE id = $2")
                    .bind(reserved_wallet)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        if billing_payment_mode == BillingPaymentMode::Metered && wallet_amount > 0.0 {
            if let Some(team_id) = team_id.as_deref() {
                let row = query("SELECT balance, frozen, token_wallet_reserved FROM team_wallets WHERE team_id = $1 FOR UPDATE")
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;
                let balance = row.try_get::<f64, _>(0)?;
                let frozen = row.try_get::<f64, _>(1)?;
                let reserved_other = row.try_get::<f64, _>(2)?;
                let settled_wallet_amount =
                    wallet_amount.min((balance - frozen - reserved_other).max(0.0));
                let deduction = -settled_wallet_amount;
                let new_balance = balance + deduction;
                query("UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3")
                    .bind(new_balance)
                    .bind(&now)
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;
                if settled_wallet_amount > 0.0 {
                    let transaction_id = format!("wallet-initial-{}", settlement.reservation_id);
                    query("INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at, team_id, account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,$8,'team')")
                        .bind(&transaction_id).bind(&user_id).bind(deduction).bind(balance).bind(new_balance)
                        .bind("Token package wallet fallback").bind(&now).bind(team_id).execute(&mut *tx).await?;
                    initial_wallet_transaction_id = Some(transaction_id);
                }
                wallet_amount = settled_wallet_amount;
            } else {
                let row = query("SELECT balance, frozen, token_wallet_reserved FROM users WHERE id = $1 FOR UPDATE")
                    .bind(&user_id)
                    .fetch_one(&mut *tx)
                    .await?;
                let balance = row.try_get::<f64, _>(0)?;
                let frozen = row.try_get::<f64, _>(1)?;
                let reserved_other = row.try_get::<f64, _>(2)?;
                let settled_wallet_amount =
                    wallet_amount.min((balance - frozen - reserved_other).max(0.0));
                let deduction = -settled_wallet_amount;
                let new_balance = balance + deduction;
                query("UPDATE users SET balance = $1 WHERE id = $2")
                    .bind(new_balance)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await?;
                if settled_wallet_amount > 0.0 {
                    let transaction_id = format!("wallet-initial-{}", settlement.reservation_id);
                    query("INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at, team_id, account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,NULL,'user')")
                        .bind(&transaction_id).bind(&user_id).bind(deduction).bind(balance).bind(new_balance)
                        .bind("Token package wallet fallback").bind(&now).execute(&mut *tx).await?;
                    initial_wallet_transaction_id = Some(transaction_id);
                }
                wallet_amount = settled_wallet_amount;
            }
        }
        let wallet_shortfall = if billing_payment_mode == BillingPaymentMode::Metered {
            (breakdown.wallet_amount - Decimal::try_from(wallet_amount).unwrap_or(Decimal::ZERO))
                .max(Decimal::ZERO)
                .to_f64()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let final_reason = if wallet_shortfall > 0.0 {
            format!("{}; wallet shortfall={wallet_shortfall}", settlement.reason)
        } else {
            settlement.reason.clone()
        };
        let settlement_state = if wallet_shortfall > 0.0 {
            "settlement_pending"
        } else if actual_units > 0 {
            "settled"
        } else {
            "released"
        };
        let receivable_state = if wallet_shortfall > 0.0 {
            "partially_settled"
        } else {
            "settled"
        };
        query("INSERT INTO token_settlement_receivables (id, reservation_id, request_id, user_id, team_id, account_type, actual_prompt_tokens, actual_completion_tokens, actual_cache_hit_input_tokens, actual_cache_write_tokens, status_code, success, reason, actual_priced_cost_amount, package_priced_cost_amount, wallet_due_amount, settled_wallet_amount, outstanding_amount, state, created_at, updated_at, settled_at) SELECT $1, r.id, r.request_id, r.user_id, r.team_id, r.account_type, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $15, CASE WHEN $13 = 0 THEN $15 ELSE NULL END FROM token_request_reservations r WHERE r.id = $16 ON CONFLICT (reservation_id) DO UPDATE SET actual_prompt_tokens = EXCLUDED.actual_prompt_tokens, actual_completion_tokens = EXCLUDED.actual_completion_tokens, actual_cache_hit_input_tokens = EXCLUDED.actual_cache_hit_input_tokens, actual_cache_write_tokens = EXCLUDED.actual_cache_write_tokens, status_code = EXCLUDED.status_code, success = EXCLUDED.success, reason = EXCLUDED.reason, actual_priced_cost_amount = EXCLUDED.actual_priced_cost_amount, package_priced_cost_amount = EXCLUDED.package_priced_cost_amount, wallet_due_amount = EXCLUDED.wallet_due_amount, settled_wallet_amount = EXCLUDED.settled_wallet_amount, outstanding_amount = EXCLUDED.outstanding_amount, state = EXCLUDED.state, updated_at = EXCLUDED.updated_at, settled_at = EXCLUDED.settled_at")
            .bind(&receivable_id)
            .bind(settlement.actual_prompt_tokens as i64)
            .bind(settlement.actual_completion_tokens as i64)
            .bind(settlement.actual_cache_hit_input_tokens as i64)
            .bind(settlement.actual_cache_write_tokens as i64)
            .bind(settlement.status_code as i32)
            .bind(settlement.success)
            .bind(&final_reason)
            .bind(breakdown.actual_priced_cost.to_f64().unwrap_or(f64::MAX))
            .bind(breakdown.package_priced_cost.to_f64().unwrap_or(f64::MAX))
            .bind(breakdown.wallet_amount.to_f64().unwrap_or(f64::MAX))
            .bind(wallet_amount)
            .bind(wallet_shortfall)
            .bind(receivable_state)
            .bind(&now)
            .bind(&settlement.reservation_id)
            .execute(&mut *tx)
            .await?;
        {
            query("INSERT INTO token_settlement_payments (id, receivable_id, reservation_id, request_id, payment_sequence, payment_type, idempotency_key, amount, account_type, wallet_transaction_id, created_at) VALUES ($1,$2,$3,$4,0,'initial_settlement',$5,$6,$7,$8,$9) ON CONFLICT (idempotency_key) DO NOTHING")
                .bind(format!("payment-initial-{}", settlement.reservation_id))
                .bind(&receivable_id)
                .bind(&settlement.reservation_id)
                .bind(&request_id)
                .bind(format!("settlement:{}:payment:0", settlement.reservation_id))
                .bind(wallet_amount)
                .bind(if team_id.is_some() { "team" } else { "user" })
                .bind(&initial_wallet_transaction_id)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
        }
        query("UPDATE token_request_reservations SET actual_prompt_tokens = $1, actual_completion_tokens = $2, actual_cache_write_tokens = $3, actual_package_units = $4, actual_wallet_amount = $5, wallet_shortfall_amount = $6, actual_priced_cost_amount = $7, state = $8, settlement_state = $9, reason = $10, settled_at = $11 WHERE id = $12")
            .bind(settlement.actual_prompt_tokens as i64)
            .bind(settlement.actual_completion_tokens as i64)
            .bind(settlement.actual_cache_write_tokens as i64)
            .bind(package_units as i64)
            .bind(wallet_amount)
            .bind(wallet_shortfall)
            .bind(breakdown.actual_priced_cost.to_f64().unwrap_or(f64::MAX))
            .bind(if actual_units > 0 && wallet_shortfall == 0.0 {
                "settled"
            } else if actual_units > 0 {
                "settlement_pending"
            } else {
                "released"
            })
            .bind(settlement_state)
            .bind(&final_reason)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&settlement.reservation_id)
            .execute(&mut *tx)
            .await?;
        query(
            "INSERT INTO billing_events (request_id, user_id, user_name, channel_id, model,
             prompt_tokens, completion_tokens, total_tokens, cache_hit_input_tokens, cache_write_tokens,
             prompt_price, completion_price, cache_read_price, cache_write_price,
             cost_amount, success, status_code, timestamp, reservation_id, package_grant_id,
             accounting_mode, package_units, wallet_amount, team_id, account_type,
             api_key_name, billing_group_id, billing_group_name, billing_payment_mode,
             activity_status, charge_source, priced_cost_amount, settlement_state, settled_amount, outstanding_amount, token_settlement_id)
             SELECT r.request_id, r.user_id, COALESCE(NULLIF(r.user_name, ''), u.name), '', r.model,
                    COALESCE(r.actual_prompt_tokens, 0), COALESCE(r.actual_completion_tokens, 0),
                    COALESCE(r.actual_prompt_tokens, 0) + COALESCE(r.actual_completion_tokens, 0),
                    COALESCE($1, 0), COALESCE(r.actual_cache_write_tokens, 0), r.prompt_price,
                    r.completion_price, r.cache_read_price, r.cache_write_price,
                    COALESCE(r.actual_wallet_amount, 0), $2, $3, COALESCE(r.created_at, $4),
                    r.id, r.package_grant_id, r.accounting_mode, COALESCE(r.actual_package_units, 0),
                    COALESCE(r.actual_wallet_amount, 0), r.team_id, r.account_type, NULLIF(r.api_key_name, ''),
                    r.billing_group_id, r.billing_group_name, r.billing_payment_mode,
                    CASE WHEN $3 = 499 THEN 'interrupted' WHEN $3 >= 400 OR $2 = false THEN 'failed' ELSE 'success' END,
                    CASE WHEN r.billing_payment_mode = 'prepaid' AND COALESCE(r.actual_package_units, 0) > 0 THEN 'prepaid_package'
                         WHEN r.billing_payment_mode = 'prepaid' THEN 'prepaid'
                         WHEN COALESCE(r.actual_package_units, 0) > 0 AND COALESCE(r.actual_wallet_amount, 0) > 0 THEN 'package_and_wallet'
                         WHEN COALESCE(r.actual_package_units, 0) > 0 THEN 'package'
                         WHEN COALESCE(r.actual_wallet_amount, 0) > 0 THEN 'wallet'
                         ELSE 'none' END,
                    COALESCE(r.actual_priced_cost_amount, 0), r.settlement_state,
                    COALESCE(r.actual_wallet_amount, 0), COALESCE(r.wallet_shortfall_amount, 0), r.id
             FROM token_request_reservations r JOIN users u ON u.id = r.user_id
             WHERE r.id = $5
             ON CONFLICT (request_id) DO UPDATE SET
               prompt_tokens = EXCLUDED.prompt_tokens,
               completion_tokens = EXCLUDED.completion_tokens,
               total_tokens = EXCLUDED.total_tokens,
               cache_hit_input_tokens = EXCLUDED.cache_hit_input_tokens,
               cache_write_tokens = EXCLUDED.cache_write_tokens,
               prompt_price = EXCLUDED.prompt_price,
               completion_price = EXCLUDED.completion_price,
               cache_read_price = EXCLUDED.cache_read_price,
               cache_write_price = EXCLUDED.cache_write_price,
               cost_amount = EXCLUDED.cost_amount,
               success = EXCLUDED.success,
               status_code = EXCLUDED.status_code,
               reservation_id = EXCLUDED.reservation_id,
               package_grant_id = EXCLUDED.package_grant_id,
               accounting_mode = EXCLUDED.accounting_mode,
               package_units = EXCLUDED.package_units,
               wallet_amount = EXCLUDED.wallet_amount,
               activity_status = EXCLUDED.activity_status,
               charge_source = EXCLUDED.charge_source,
               billing_group_id = COALESCE(EXCLUDED.billing_group_id, billing_events.billing_group_id),
               billing_group_name = COALESCE(EXCLUDED.billing_group_name, billing_events.billing_group_name),
               billing_payment_mode = COALESCE(EXCLUDED.billing_payment_mode, billing_events.billing_payment_mode),
               priced_cost_amount = EXCLUDED.priced_cost_amount,
               settlement_state = EXCLUDED.settlement_state,
               settled_amount = EXCLUDED.settled_amount,
               outstanding_amount = EXCLUDED.outstanding_amount,
               token_settlement_id = EXCLUDED.token_settlement_id,
               user_name = COALESCE(NULLIF(EXCLUDED.user_name, ''), billing_events.user_name),
               api_key_name = COALESCE(NULLIF(EXCLUDED.api_key_name, ''), billing_events.api_key_name)",
        )
        .bind(settlement.actual_cache_hit_input_tokens as i64)
        .bind(settlement.success)
        .bind(settlement.status_code as i32)
        .bind(now)
        .bind(&settlement.reservation_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn settle_released_token_request(
        &self,
        request_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        _cache_hit_input_tokens: u64,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query_as::<_, (String, String, i64, Option<String>)>(
            "SELECT id, state, reserved_package_units, package_grant_id
             FROM token_request_reservations WHERE request_id = $1 FOR UPDATE",
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((reservation_id, state, reserved, package_grant_id)) = row else {
            return Ok(());
        };
        if state != "released" || package_grant_id.is_none() || reserved <= 0 {
            return Ok(());
        }
        let actual = prompt_tokens.saturating_add(completion_tokens);
        let consume = (actual as i64).min(reserved).max(0);
        if consume == 0 {
            return Ok(());
        }
        let grant_id = package_grant_id.unwrap();
        query("UPDATE token_package_grants SET consumed_units = consumed_units + $1, reserved_units = GREATEST(0, reserved_units - $1), updated_at = $2 WHERE id = $3")
            .bind(consume)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&grant_id)
            .execute(&mut *tx)
            .await?;
        query("UPDATE token_request_reservations SET actual_prompt_tokens = $1, actual_completion_tokens = $2, actual_package_units = $3, state = 'settled', reason = 'client disconnected after partial output', settled_at = $4 WHERE id = $5 AND state = 'released'")
            .bind(prompt_tokens as i64)
            .bind(completion_tokens as i64)
            .bind(consume)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&reservation_id)
            .execute(&mut *tx)
            .await?;
        query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, display_tokens, prompt_tokens, completion_tokens, credits, created_at, note) VALUES ($1,$2,$3,$4,'consume',$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&grant_id).bind(&reservation_id).bind(request_id)
            .bind(consume).bind(prompt_tokens.saturating_add(completion_tokens) as i64).bind(prompt_tokens as i64).bind(completion_tokens as i64).bind(consume)
            .bind(chrono::Utc::now().to_rfc3339()).bind("partial stream output")
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn recover_token_settlement_receivables(
        &self,
        limit: usize,
        worker_id: &str,
    ) -> Result<usize, DbError> {
        if limit == 0 {
            return Ok(0);
        }
        let mut processed = 0usize;
        for _ in 0..limit {
            let mut tx = self.pool.begin().await?;
            let row = query(
                "SELECT id, reservation_id, user_id, team_id, account_type, outstanding_amount,
                        settled_wallet_amount, request_id
                 FROM token_settlement_receivables
                 WHERE state = 'partially_settled'
                   AND outstanding_amount > 0
                   AND (next_attempt_at IS NULL OR next_attempt_at::timestamptz <= NOW())
                   AND (lease_until IS NULL OR lease_until::timestamptz <= NOW())
                 ORDER BY created_at, id
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1",
            )
            .fetch_optional(&mut *tx)
            .await?;
            let Some(row) = row else {
                tx.commit().await?;
                break;
            };
            let receivable_id: String = row.try_get(0)?;
            let reservation_id: String = row.try_get(1)?;
            let user_id: String = row.try_get(2)?;
            let team_id: Option<String> = row.try_get(3)?;
            let account_type: String = row.try_get(4)?;
            let outstanding: f64 = row.try_get(5)?;
            let settled_before: f64 = row.try_get(6)?;
            let request_id: String = row.try_get(7)?;
            let now = chrono::Utc::now().to_rfc3339();
            query(
                "UPDATE token_settlement_receivables
                 SET attempts = attempts + 1, lease_until = ($1::timestamptz + INTERVAL '60 seconds'),
                     updated_at = $1, last_error = ''
                 WHERE id = $2",
            )
            .bind(&now)
            .bind(&receivable_id)
            .execute(&mut *tx)
            .await?;

            let (balance, frozen, reserved): (f64, f64, f64) = if let Some(team) =
                team_id.as_deref()
            {
                query_as("SELECT balance, frozen, token_wallet_reserved FROM team_wallets WHERE team_id = $1 FOR UPDATE")
                    .bind(team).fetch_one(&mut *tx).await?
            } else {
                query_as("SELECT balance, frozen, token_wallet_reserved FROM users WHERE id = $1 FOR UPDATE")
                    .bind(&user_id).fetch_one(&mut *tx).await?
            };
            let payment = outstanding.min((balance - frozen - reserved).max(0.0));
            if payment <= 0.0 {
                query("UPDATE token_settlement_receivables SET lease_until = NULL, next_attempt_at = ($1::timestamptz + INTERVAL '1 hour'), last_error = 'insufficient_funds', updated_at = $1 WHERE id = $2")
                    .bind(&now).bind(&receivable_id).execute(&mut *tx).await?;
                tx.commit().await?;
                processed += 1;
                continue;
            }
            let payment_sequence: i64 = query_scalar("SELECT COALESCE(MAX(payment_sequence), 0) + 1 FROM token_settlement_payments WHERE receivable_id = $1")
                .bind(&receivable_id).fetch_one(&mut *tx).await?;
            let idempotency_key = format!("settlement:{receivable_id}:payment:{payment_sequence}");
            let payment_id = format!("payment-{}-{}", receivable_id, payment_sequence);
            let payment_inserted = query("INSERT INTO token_settlement_payments (id, receivable_id, reservation_id, request_id, payment_sequence, payment_type, idempotency_key, amount, account_type, created_at) VALUES ($1,$2,$3,$4,$5,'recovery',$6,$7,$8,$9) ON CONFLICT (idempotency_key) DO NOTHING")
                .bind(&payment_id).bind(&receivable_id).bind(&reservation_id).bind(&request_id)
                .bind(payment_sequence).bind(&idempotency_key).bind(payment).bind(&account_type).bind(&now)
                .execute(&mut *tx).await?;
            if payment_inserted.rows_affected() != 1 {
                tx.commit().await?;
                processed += 1;
                continue;
            }
            let new_balance = balance - payment;
            let tx_id = format!("wallet-{}", payment_id);
            if let Some(team) = team_id.as_deref() {
                query("UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3")
                    .bind(new_balance)
                    .bind(&now)
                    .bind(team)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO wallet_transactions (id,user_id,type,amount,balance_before,balance_after,method,status,note,created_at,team_id,account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,$8,'team')")
                    .bind(&tx_id).bind(&user_id).bind(-payment).bind(balance).bind(new_balance)
                    .bind(format!("Receivable repayment by {worker_id}")).bind(&now).bind(team).execute(&mut *tx).await?;
            } else {
                query("UPDATE users SET balance = $1 WHERE id = $2")
                    .bind(new_balance)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO wallet_transactions (id,user_id,type,amount,balance_before,balance_after,method,status,note,created_at,team_id,account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,NULL,'user')")
                    .bind(&tx_id).bind(&user_id).bind(-payment).bind(balance).bind(new_balance)
                    .bind(format!("Receivable repayment by {worker_id}")).bind(&now).execute(&mut *tx).await?;
            }
            query("UPDATE token_settlement_payments SET wallet_transaction_id = $1 WHERE id = $2")
                .bind(&tx_id)
                .bind(&payment_id)
                .execute(&mut *tx)
                .await?;
            let new_outstanding = (outstanding - payment).max(0.0);
            let new_settled = settled_before + payment;
            let state = if new_outstanding <= 0.0 {
                "settled"
            } else {
                "partially_settled"
            };
            query("UPDATE token_settlement_receivables SET settled_wallet_amount=$1, outstanding_amount=$2, state=$3, lease_until=NULL, next_attempt_at=CASE WHEN $2 > 0 THEN ($4::timestamptz + INTERVAL '1 hour') ELSE NULL END, settled_at=CASE WHEN $2 <= 0 THEN $4 ELSE settled_at END, updated_at=$4 WHERE id=$5")
                .bind(new_settled).bind(new_outstanding).bind(state).bind(&now).bind(&receivable_id)
                .execute(&mut *tx).await?;
            query("UPDATE token_request_reservations SET actual_wallet_amount=$1, wallet_shortfall_amount=$2, state=$3, settlement_state=$3, settled_at=CASE WHEN $2 <= 0 THEN $4 ELSE settled_at END WHERE id=$5")
                .bind(new_settled).bind(new_outstanding).bind(state).bind(&now).bind(&reservation_id)
                .execute(&mut *tx).await?;
            query("UPDATE billing_events SET wallet_amount=$1, cost_amount=$1, settled_amount=$1, outstanding_amount=$2, settlement_state=$3, charge_source=CASE WHEN $2 > 0 THEN 'wallet_shortfall' ELSE 'wallet' END WHERE request_id=$4")
                .bind(new_settled).bind(new_outstanding).bind(state).bind(&request_id)
                .execute(&mut *tx).await?;
            tx.commit().await?;
            processed += 1;
        }
        Ok(processed)
    }

    async fn apply_token_settlement_payment(
        &self,
        receivable_id: &str,
        payment_sequence: i64,
        payment_type: &str,
        idempotency_key: &str,
        amount: Decimal,
    ) -> Result<bool, DbError> {
        if amount < Decimal::ZERO {
            return Err(DbError(
                "settlement payment amount cannot be negative".to_string(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let row = query("SELECT reservation_id, request_id, user_id, team_id, account_type, wallet_due_amount, settled_wallet_amount, outstanding_amount, state FROM token_settlement_receivables WHERE id = $1 FOR UPDATE")
            .bind(receivable_id).fetch_optional(&mut *tx).await?
            .ok_or_else(|| DbError("settlement receivable not found".to_string()))?;
        let reservation_id: String = row.try_get(0)?;
        let request_id: String = row.try_get(1)?;
        let user_id: String = row.try_get(2)?;
        let team_id: Option<String> = row.try_get(3)?;
        let account_type: String = row.try_get(4)?;
        let due: f64 = row.try_get(5)?;
        let settled: f64 = row.try_get(6)?;
        let outstanding: f64 = row.try_get(7)?;
        let existing: Option<String> =
            query_scalar("SELECT id FROM token_settlement_payments WHERE idempotency_key = $1")
                .bind(idempotency_key)
                .fetch_optional(&mut *tx)
                .await?;
        if existing.is_some() {
            tx.commit().await?;
            return Ok(false);
        }
        if payment_sequence == 0 && payment_type != "initial_settlement" {
            return Err(DbError("payment sequence/type mismatch".to_string()));
        }
        if payment_sequence >= 1 && payment_type != "recovery" {
            return Err(DbError("payment sequence/type mismatch".to_string()));
        }
        let value = amount.to_f64().unwrap_or(0.0);
        if value
            > (outstanding
                + if payment_sequence == 0 {
                    due - settled - outstanding
                } else {
                    0.0
                })
                + 1e-15
        {
            return Err(DbError(
                "settlement payment exceeds receivable due".to_string(),
            ));
        }
        let payment_id = format!("payment-command-{}-{}", receivable_id, payment_sequence);
        query("INSERT INTO token_settlement_payments (id, receivable_id, reservation_id, request_id, payment_sequence, payment_type, idempotency_key, amount, account_type, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(&payment_id).bind(receivable_id).bind(&reservation_id).bind(&request_id)
            .bind(payment_sequence).bind(payment_type).bind(idempotency_key).bind(value).bind(&account_type).bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *tx).await?;
        if value > 0.0 {
            let transaction_id = format!(
                "wallet-payment-command-{}-{}",
                receivable_id, payment_sequence
            );
            if let Some(team) = team_id.as_deref() {
                let (balance, frozen, reserved): (f64, f64, f64) = query_as("SELECT balance,frozen,token_wallet_reserved FROM team_wallets WHERE team_id=$1 FOR UPDATE").bind(team).fetch_one(&mut *tx).await?;
                let available = (balance - frozen - reserved).max(0.0);
                if available + 1e-15 < value {
                    return Err(DbError(
                        "insufficient wallet balance for payment command".to_string(),
                    ));
                }
                query("UPDATE team_wallets SET balance=$1 WHERE team_id=$2")
                    .bind(balance - value)
                    .bind(team)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO wallet_transactions (id,user_id,type,amount,balance_before,balance_after,method,status,note,created_at,team_id,account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,$8,'team')").bind(&transaction_id).bind(&user_id).bind(-value).bind(balance).bind(balance-value).bind("Payment command").bind(chrono::Utc::now().to_rfc3339()).bind(team).execute(&mut *tx).await?;
            } else {
                let (balance, frozen, reserved): (f64, f64, f64) = query_as(
                    "SELECT balance,frozen,token_wallet_reserved FROM users WHERE id=$1 FOR UPDATE",
                )
                .bind(&user_id)
                .fetch_one(&mut *tx)
                .await?;
                let available = (balance - frozen - reserved).max(0.0);
                if available + 1e-15 < value {
                    return Err(DbError(
                        "insufficient wallet balance for payment command".to_string(),
                    ));
                }
                query("UPDATE users SET balance=$1 WHERE id=$2")
                    .bind(balance - value)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO wallet_transactions (id,user_id,type,amount,balance_before,balance_after,method,status,note,created_at,account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_settlement','completed',$6,$7,'user')").bind(&transaction_id).bind(&user_id).bind(-value).bind(balance).bind(balance-value).bind("Payment command").bind(chrono::Utc::now().to_rfc3339()).execute(&mut *tx).await?;
            }
            query("UPDATE token_settlement_payments SET wallet_transaction_id=$1 WHERE id=$2")
                .bind(&transaction_id)
                .bind(&payment_id)
                .execute(&mut *tx)
                .await?;
        }
        let new_settled = settled + value;
        let new_outstanding = (outstanding - value).max(0.0);
        let next_state = if new_outstanding <= 1e-15 {
            "settled"
        } else {
            "partially_settled"
        };
        query("UPDATE token_settlement_receivables SET settled_wallet_amount=$1,outstanding_amount=$2,state=$3,updated_at=$4,settled_at=CASE WHEN $2<=1e-15 THEN $4 ELSE settled_at END WHERE id=$5").bind(new_settled).bind(new_outstanding).bind(next_state).bind(chrono::Utc::now().to_rfc3339()).bind(receivable_id).execute(&mut *tx).await?;
        query("UPDATE token_request_reservations SET actual_wallet_amount=$1,wallet_shortfall_amount=$2,state=$3,settlement_state=$3 WHERE id=$4").bind(new_settled).bind(new_outstanding).bind(next_state).bind(&reservation_id).execute(&mut *tx).await?;
        query("UPDATE billing_events SET wallet_amount=$1,cost_amount=$1,settled_amount=$1,outstanding_amount=$2,settlement_state=$3 WHERE request_id=$4").bind(new_settled).bind(new_outstanding).bind(next_state).bind(&request_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn token_request_billing_amount(
        &self,
        request_id: &str,
    ) -> Result<Option<(bool, Decimal, String, Option<String>, Option<String>)>, DbError> {
        let row = query_as::<_, (Option<String>, Option<f64>, Option<String>, Option<String>, Option<String>)>(
            "SELECT package_grant_id, actual_wallet_amount, billing_payment_mode, billing_group_id, billing_group_name
             FROM token_request_reservations
             WHERE request_id = $1 AND state IN ('reserved', 'settled', 'released', 'expired')
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(package_id, amount, mode, group_id, group_name)| {
            (
                package_id.is_some(),
                Decimal::try_from(amount.unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                mode.unwrap_or_else(|| "metered".to_string()),
                group_id,
                group_name,
            )
        }))
    }

    async fn reclaim_expired_token_reservations(&self, limit: usize) -> Result<usize, DbError> {
        if limit == 0 {
            return Ok(0);
        }

        // Selection is deliberately narrow. `release_token_request` performs
        // the state transition under a row lock, so concurrent settlement or
        // another instance can only make a candidate a harmless no-op.
        let reservation_ids = query_scalar::<_, String>(
            "SELECT id
             FROM token_request_reservations
             WHERE state = 'reserved'
               AND expires_at::timestamptz <= NOW()
             ORDER BY expires_at, id
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut reclaimed = 0;
        for reservation_id in reservation_ids {
            self.release_token_request(&reservation_id, "reservation expired")
                .await?;
            reclaimed += 1;
        }
        Ok(reclaimed)
    }

    async fn release_token_request(
        &self,
        reservation_id: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query("SELECT state, package_grant_id, reserved_package_units, reserved_prompt_tokens, reserved_completion_tokens, user_id, team_id, reserved_wallet_amount, request_id FROM token_request_reservations WHERE id = $1 FOR UPDATE")
            .bind(reservation_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            return Ok(());
        };
        if row.try_get::<String, _>(0)? != "reserved" {
            return Ok(());
        }
        let user_id = row.try_get::<String, _>(5)?;
        let team_id = row.try_get::<Option<String>, _>(6)?;
        let reserved_wallet = row.try_get::<f64, _>(7)?;
        let request_id = row.try_get::<String, _>(8)?;
        if reserved_wallet > 0.0 {
            if let Some(team_id) = team_id.as_deref() {
                query("UPDATE team_wallets SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1), updated_at = $2 WHERE team_id = $3")
                    .bind(reserved_wallet).bind(chrono::Utc::now().to_rfc3339()).bind(team_id)
                    .execute(&mut *tx).await?;
            } else {
                query("UPDATE users SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1) WHERE id = $2")
                    .bind(reserved_wallet).bind(&user_id).execute(&mut *tx).await?;
            }
        }
        let allocation_rows: Vec<(String, i64)> = query_as(
            "SELECT package_grant_id, reserved_units
             FROM token_package_reservation_allocations
             WHERE reservation_id = $1 FOR UPDATE",
        )
        .bind(reservation_id)
        .fetch_all(&mut *tx)
        .await?;
        if allocation_rows.is_empty() {
            if let Some(grant_id) = row.try_get::<Option<String>, _>(1)? {
                let reserved = row.try_get::<i64, _>(2)?.max(0);
                query("UPDATE token_package_grants SET reserved_units = GREATEST(0, reserved_units - $1), updated_at = $2 WHERE id = $3")
                    .bind(reserved)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(&grant_id)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, created_at, note) VALUES ($1, $2, $3, $4, 'release', $5, $6, $7) ON CONFLICT DO NOTHING")
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&grant_id)
                    .bind(reservation_id)
                    .bind(&request_id)
                    .bind(-reserved)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(reason)
                    .execute(&mut *tx)
                    .await?;
            }
        } else {
            for (grant_id, reserved) in allocation_rows {
                query("UPDATE token_package_grants SET reserved_units = GREATEST(0, reserved_units - $1), updated_at = $2 WHERE id = $3")
                    .bind(reserved.max(0))
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(&grant_id)
                    .execute(&mut *tx)
                    .await?;
                query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, created_at, note) VALUES ($1, $2, $3, $4, 'release', $5, $6, $7) ON CONFLICT DO NOTHING")
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&grant_id)
                    .bind(reservation_id)
                    .bind(&request_id)
                    .bind(-reserved.max(0))
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(reason)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        query("UPDATE token_request_reservations SET state = 'released', reason = $1, settled_at = $2 WHERE id = $3")
            .bind(reason)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(reservation_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
