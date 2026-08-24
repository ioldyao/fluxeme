use super::*;
use async_trait::async_trait;

#[async_trait]
impl WalletBackend for PgBackend {
    // ── Wallet ───────────────────────────────────────────────────────────

    async fn get_wallet_balance(&self, user_id: &str) -> Result<(Decimal, Decimal), DbError> {
        let row: (f64, f64) = query_as("SELECT balance, frozen FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok((
            Decimal::try_from(row.0).unwrap_or(Decimal::ZERO),
            Decimal::try_from(row.1).unwrap_or(Decimal::ZERO),
        ))
    }

    async fn add_wallet_transaction(
        &self,
        id: &str,
        user_id: &str,
        tx_type: &str,
        amount: Decimal,
        balance_before: Decimal,
        balance_after: Decimal,
        method: &str,
        status: &str,
        note: &str,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        query(
            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(user_id)
        .bind(tx_type)
        .bind(amount.to_f64().unwrap_or(0.0))
        .bind(balance_before.to_f64().unwrap_or(0.0))
        .bind(balance_after.to_f64().unwrap_or(0.0))
        .bind(method)
        .bind(status)
        .bind(note)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError> {
        let offset = (page.saturating_sub(1)) * size;
        let rows = query(
            "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
             FROM wallet_transactions WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(user_id)
        .bind(size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| WalletTransactionRow {
                id: r.get(0),
                user_id: r.get(1),
                tx_type: r.get(2),
                amount: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                balance_before: Decimal::try_from(r.get::<f64, _>(4)).unwrap_or(Decimal::ZERO),
                balance_after: Decimal::try_from(r.get::<f64, _>(5)).unwrap_or(Decimal::ZERO),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect())
    }

    async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError> {
        let (count,): (i64,) =
            query_as("SELECT COUNT(*) FROM wallet_transactions WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count as usize)
    }

    async fn list_wallet_tx_by_dates(
        &self,
        user_id: Option<&str>,
        page: usize,
        size: usize,
        since: Option<&str>,
        until: Option<&str>,
        tx_type: Option<&str>,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        // Build dynamic WHERE clause for wallet_transactions
        // Use a helper macro to avoid borrowing issues with the closure capturing &str
        macro_rules! add_filters {
            ($b:expr) => {
                if let Some(uid) = user_id {
                    $b.push(" AND user_id = ");
                    $b.push_bind(uid);
                }
                if let Some(s) = since {
                    $b.push(" AND created_at >= ");
                    $b.push_bind(s);
                }
                if let Some(u) = until {
                    $b.push(" AND created_at <= ");
                    $b.push_bind(u);
                }
                if let Some(t) = tx_type {
                    $b.push(" AND type = ");
                    $b.push_bind(t);
                }
            };
        }

        let mut count_builder: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT COUNT(DISTINCT LEFT(created_at::text, 10)) FROM wallet_transactions WHERE 1=1",
        );

        let mut data_builder: QueryBuilder<'_, Postgres> =
            QueryBuilder::new(
                "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
                 FROM wallet_transactions WHERE 1=1",
            );

        let mut date_builder: QueryBuilder<'_, Postgres> =
            QueryBuilder::new(
                "SELECT DISTINCT LEFT(created_at::text, 10) as tx_date FROM wallet_transactions WHERE 1=1",
            );

        add_filters!(count_builder);
        add_filters!(data_builder);
        add_filters!(date_builder);

        // Total distinct dates
        let (total_dates,): (i64,) = count_builder.build_query_as().fetch_one(&self.pool).await?;
        let total_dates = total_dates as usize;

        // Paginated dates
        let page_offset = (page.saturating_sub(1)) * size;
        date_builder.push(" ORDER BY tx_date DESC LIMIT ");
        date_builder.push_bind(size as i64);
        date_builder.push(" OFFSET ");
        date_builder.push_bind(page_offset as i64);

        let dates: Vec<String> = date_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

        if dates.is_empty() {
            return Ok((Vec::new(), total_dates));
        }

        // Fetch transactions for those dates using IN clause
        data_builder.push(" AND LEFT(created_at::text, 10) IN (");
        for (i, _) in dates.iter().enumerate() {
            if i > 0 {
                data_builder.push(", ");
            }
            data_builder.push_bind(&dates[i]);
        }
        data_builder.push(") ORDER BY created_at DESC");

        let rows = data_builder.build().fetch_all(&self.pool).await?;
        let transactions = rows
            .iter()
            .map(|r| WalletTransactionRow {
                id: r.get(0),
                user_id: r.get(1),
                tx_type: r.get(2),
                amount: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                balance_before: Decimal::try_from(r.get::<f64, _>(4)).unwrap_or(Decimal::ZERO),
                balance_after: Decimal::try_from(r.get::<f64, _>(5)).unwrap_or(Decimal::ZERO),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect();

        Ok((transactions, total_dates))
    }

    async fn get_wallet_request_reserved(&self, user_id: &str) -> Result<Decimal, DbError> {
        let amount: f64 =
            query_scalar("SELECT COALESCE(token_wallet_reserved, 0) FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn get_total_wallet_consumed(&self, user_id: &str) -> Result<Decimal, DbError> {
        let amount: f64 = query_scalar(
            "SELECT COALESCE(SUM(ABS(amount)), 0) FROM wallet_transactions \
             WHERE user_id = $1 AND type = 'deduction' AND status = 'completed'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn get_total_recharged(&self, user_id: &str) -> Result<Decimal, DbError> {
        let (amount,): (f64,) = query_as(
            "SELECT COALESCE(SUM(amount), 0) FROM wallet_transactions \
             WHERE user_id = $1 AND type = 'recharge' AND status = 'completed'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<Decimal>, DbError> {
        let thirty_days_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let (total_cost,): (f64,) = query_as(
            "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
             FROM billing_events WHERE user_id = $1 AND timestamp >= $2",
        )
        .bind(user_id)
        .bind(&thirty_days_ago)
        .fetch_one(&self.pool)
        .await?;

        let (balance,): (f64,) = query_as("SELECT balance FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        let daily_avg = Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO) / Decimal::from(30);
        if daily_avg <= Decimal::ZERO {
            return Ok(None);
        }
        let bal = Decimal::try_from(balance).unwrap_or(Decimal::ZERO);
        Ok(Some(bal / daily_avg))
    }

    // ── Recharge Keys ────────────────────────────────────────────────────

    async fn create_recharge_key(
        &self,
        key: &str,
        amount: Decimal,
        created_by: &str,
        expires_at: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        query(
            "INSERT INTO recharge_keys (key, amount, created_by, created_at, expires_at, team_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(key)
        .bind(amount.to_f64().unwrap_or(0.0))
        .bind(created_by)
        .bind(&now)
        .bind(expires_at)
        .bind(team_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn redeem_recharge_key(
        &self,
        key: &str,
        user_id: &str,
    ) -> Result<(Decimal, Option<String>), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;

        // Atomically mark as used — only if not already used/revoked/expired
        let updated = query(
            "UPDATE recharge_keys SET used_by = $1, used_at = $2 \
             WHERE key = $3 AND used_by IS NULL \
             AND (revoked IS NULL OR revoked = false) \
             AND (expires_at IS NULL OR expires_at > $4)",
        )
        .bind(user_id)
        .bind(&now)
        .bind(key)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // Key doesn't exist or was already used/revoked — fetch details for error message
            let existing =
                query("SELECT used_by, revoked, expires_at FROM recharge_keys WHERE key = $1")
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await?;
            let msg = match existing {
                None => "Invalid recharge key".to_string(),
                Some(r) => {
                    let used_by: Option<String> = r.get(0);
                    let revoked: bool = r.get(1);
                    let expires_at: Option<String> = r.get(2);
                    if used_by.is_some() {
                        "Recharge key already used".to_string()
                    } else if revoked {
                        "Recharge key has been revoked".to_string()
                    } else if let Some(exp) = &expires_at {
                        if let Ok(exp_time) = chrono::DateTime::parse_from_rfc3339(exp) {
                            if chrono::Utc::now() > exp_time {
                                "Recharge key has expired".to_string()
                            } else {
                                "Invalid recharge key".to_string()
                            }
                        } else {
                            "Invalid recharge key".to_string()
                        }
                    } else {
                        "Invalid recharge key".to_string()
                    }
                }
            };
            return Err(DbError(msg));
        }

        // Get amount & team_id from the key
        let (amount, team_id): (f64, Option<String>) =
            query_as("SELECT amount, team_id FROM recharge_keys WHERE key = $1")
                .bind(key)
                .fetch_one(&mut *tx)
                .await?;

        if let Some(ref tid) = team_id {
            // ── Team wallet branch ──
            let (balance,): (f64,) =
                query_as("SELECT balance FROM team_wallets WHERE team_id = $1")
                    .bind(tid)
                    .fetch_one(&mut *tx)
                    .await?;
            let new_balance = balance + amount;
            query("UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3")
                .bind(new_balance)
                .bind(&now)
                .bind(tid)
                .execute(&mut *tx)
                .await?;
            // Record team wallet transaction
            query(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, \
                 balance_after, method, status, note, created_at, team_id, account_type) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(user_id)
            .bind("recharge")
            .bind(amount)
            .bind(balance)
            .bind(new_balance)
            .bind("recharge_key")
            .bind("completed")
            .bind(format!("Key recharge: {} for team: {}", key, tid))
            .bind(&now)
            .bind(tid)
            .bind("team")
            .execute(&mut *tx)
            .await?;
        } else {
            // ── Personal wallet branch (existing behavior) ──
            let (balance,): (f64,) = query_as("SELECT balance FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| DbError("User not found".to_string()))?;
            let new_balance = balance + amount;
            query("UPDATE users SET balance = $1 WHERE id = $2")
                .bind(new_balance)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            query(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, \
                 balance_after, method, status, note, created_at, team_id, account_type) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(user_id)
            .bind("recharge")
            .bind(amount)
            .bind(balance)
            .bind(new_balance)
            .bind("recharge_key")
            .bind("completed")
            .bind(format!("Key recharge: {}", key))
            .bind(&now)
            .bind(Option::<String>::None)
            .bind("user")
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok((Decimal::try_from(amount).unwrap_or(Decimal::ZERO), team_id))
    }

    async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError> {
        let result = query(
            "UPDATE recharge_keys SET revoked = true WHERE key = $1 \
             AND used_by IS NULL AND (revoked IS NULL OR revoked = false)",
        )
        .bind(key)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("Key not found or already used/revoked".to_string()));
        }
        Ok(())
    }

    async fn list_recharge_keys(&self) -> Result<Vec<RechargeKeyRow>, DbError> {
        let rows = query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
             FROM recharge_keys ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
            })
            .collect())
    }

    async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let rows = query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
             FROM recharge_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
            })
            .collect())
    }

    async fn count_recharge_keys_filtered(
        &self,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<usize, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut builder: QueryBuilder<'_, Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM recharge_keys WHERE 1=1");

        Self::apply_recharge_key_filters(&mut builder, search, status, user_search, &now);

        let (count,): (i64,) = builder.build_query_as().fetch_one(&self.pool).await?;
        Ok(count as usize)
    }

    async fn list_recharge_keys_filtered(
        &self,
        limit: usize,
        offset: usize,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut builder: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
             FROM recharge_keys WHERE 1=1",
        );

        Self::apply_recharge_key_filters(&mut builder, search, status, user_search, &now);

        builder.push(" ORDER BY created_at DESC LIMIT ");
        builder.push_bind(limit as i64);
        builder.push(" OFFSET ");
        builder.push_bind(offset as i64);

        let rows = builder.build().fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
            })
            .collect())
    }

    // ── Settings ─────────────────────────────────────────────────────────
}
