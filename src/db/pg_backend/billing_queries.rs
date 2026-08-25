use super::*;
use async_trait::async_trait;

#[async_trait]
impl BillingQueryBackend for PgBackend {
    // ── Usage Logs ───────────────────────────────────────────────────────

    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (cost, count, tokens): (f64, i64, i64) = if let Some(uid) = user_id {
            query_as(
                "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok((
            Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
            count as u64,
            tokens as u64,
        ))
    }

    async fn period_wallet_amount(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let amount: f64 = if let Some(uid) = user_id {
            query_scalar("SELECT COALESCE(SUM(wallet_amount), 0) FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3")
                .bind(&start).bind(&end).bind(uid).fetch_one(&self.pool).await?
        } else {
            query_scalar("SELECT COALESCE(SUM(wallet_amount), 0) FROM billing_events WHERE timestamp >= $1 AND timestamp < $2")
                .bind(&start).bind(&end).fetch_one(&self.pool).await?
        };
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn period_summary_since(
        &self,
        start: &str,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError> {
        let cost: f64 = if let Some(uid) = user_id {
            query_scalar("SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) FROM billing_events WHERE timestamp >= $1 AND user_id = $2")
                .bind(start)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_scalar(
                "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) FROM billing_events WHERE timestamp >= $1",
            )
            .bind(start)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(Decimal::try_from(cost).unwrap_or(Decimal::ZERO))
    }

    async fn billing_event_modes(
        &self,
        request_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (String, Option<String>)>, DbError> {
        if request_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = query("SELECT request_id, COALESCE(billing_payment_mode, 'metered'), billing_group_name FROM billing_events WHERE request_id = ANY($1)")
            .bind(request_ids)
            .fetch_all(&self.pool)
            .await?;
        let mut result = std::collections::HashMap::new();
        for row in rows {
            result.insert(row.try_get(0)?, (row.try_get(1)?, row.try_get(2)?));
        }
        Ok(result)
    }

    async fn usage_billing(
        &self,
        user_id: &str,
        request_ids: &[String],
    ) -> Result<Vec<crate::db::UsageBillingRow>, DbError> {
        if request_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = query(
            "SELECT request_id, COALESCE(wallet_amount, 0), settlement_state, account_type, billing_payment_mode, reservation_id, COALESCE(outstanding_amount, 0)
             FROM billing_events
             WHERE user_id = $1 AND request_id = ANY($2)",
        )
        .bind(user_id)
        .bind(request_ids)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let request_id = row.try_get(0)?;
                let wallet_amount = Decimal::try_from(row.try_get::<f64, _>(1)?)
                    .map_err(|error| DbError(format!("Invalid wallet amount: {error}")))?;
                let settlement_state: String = row.try_get(2)?;
                let reservation_id: Option<String> = row.try_get(5)?;
                let outstanding_amount = Decimal::try_from(row.try_get::<f64, _>(6)?)
                    .map_err(|error| DbError(format!("Invalid outstanding amount: {error}")))?;
                let wallet_debit_status = if matches!(
                    settlement_state.as_str(),
                    "reserved"
                        | "settlement_pending"
                        | "awaiting_actuals"
                        | "pending"
                        | "partially_settled"
                ) || outstanding_amount > Decimal::ZERO
                {
                    "pending"
                } else if reservation_id.is_none() {
                    if wallet_amount > Decimal::ZERO {
                        "charged"
                    } else {
                        "unavailable"
                    }
                } else if wallet_amount > Decimal::ZERO {
                    "charged"
                } else {
                    "no_charge"
                };
                Ok(crate::db::UsageBillingRow {
                    request_id,
                    wallet_amount,
                    wallet_debit_status: wallet_debit_status.to_string(),
                    account_type: row.try_get(3)?,
                    billing_payment_mode: row.try_get(4)?,
                })
            })
            .collect()
    }

    async fn list_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<crate::db::BillingActivityRow>, DbError> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"SELECT timestamp, request_id, user_id, user_name, model, channel_id,
             CASE WHEN status_code = 499 THEN 'interrupted' WHEN status_code >= 400 OR success = false THEN 'failed'
             WHEN activity_status IN ('unknown', '') THEN CASE WHEN cost_amount = 0
             AND package_units = 0 THEN 'zero_cost' ELSE 'success' END ELSE activity_status END,
             COALESCE(status_reason, ''), status_code, success, prompt_tokens, completion_tokens,
             cache_hit_input_tokens, cache_write_tokens, total_tokens, package_units, package_grant_id,
             COALESCE(wallet_amount, 0), COALESCE(priced_cost_amount, cost_amount, 0),
             CASE WHEN billing_payment_mode = 'prepaid' AND package_units > 0 THEN 'prepaid_package'
             WHEN billing_payment_mode = 'prepaid' THEN 'prepaid'
             WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source
             WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet'
             WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet'
             WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END,
             account_type, team_id, api_key_name, latency_ms, reservation_id,
             billing_group_id, billing_group_name, COALESCE(billing_payment_mode, 'metered')
             FROM billing_events WHERE timestamp >= "#,
        );
        builder
            .push_bind(start)
            .push(" AND timestamp < ")
            .push_bind(end);
        if let Some(uid) = user_id {
            builder.push(" AND user_id = ").push_bind(uid);
        }
        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", search);
            builder
                .push(" AND (request_id ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR COALESCE(api_key_name, '未命名 Key') ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR model ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        if let Some(api_key_name) = &filter.api_key_name {
            builder
                .push(" AND COALESCE(api_key_name, '未命名 Key') = ")
                .push_bind(api_key_name);
        }
        if let Some(model) = &filter.model {
            builder.push(" AND model = ").push_bind(model);
        }
        if let Some(source) = &filter.charge_source {
            builder.push(" AND (CASE WHEN billing_payment_mode = 'prepaid' AND package_units > 0 THEN 'prepaid_package' WHEN billing_payment_mode = 'prepaid' THEN 'prepaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END) = ").push_bind(source);
        }
        builder
            .push(" ORDER BY timestamp DESC LIMIT ")
            .push_bind(limit as i64)
            .push(" OFFSET ")
            .push_bind(offset as i64);
        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(crate::db::BillingActivityRow {
                    timestamp: row.try_get(0)?,
                    request_id: row.try_get(1)?,
                    user_id: row.try_get(2)?,
                    user_name: row.try_get(3)?,
                    model: row.try_get(4)?,
                    channel_id: row.try_get(5)?,
                    activity_status: row.try_get(6)?,
                    status_reason: row.try_get(7)?,
                    status_code: row.try_get::<i32, _>(8)? as u16,
                    success: row.try_get(9)?,
                    prompt_tokens: row.try_get::<i64, _>(10)?.max(0) as u64,
                    completion_tokens: row.try_get::<i64, _>(11)?.max(0) as u64,
                    cache_hit_input_tokens: row.try_get::<i64, _>(12)?.max(0) as u64,
                    cache_write_tokens: row.try_get::<i64, _>(13)?.max(0) as u64,
                    total_tokens: row.try_get::<i64, _>(14)?.max(0) as u64,
                    package_units: row.try_get::<i64, _>(15)?.max(0) as u64,
                    package_grant_id: row.try_get(16)?,
                    wallet_amount: Decimal::try_from(row.try_get::<f64, _>(17)?)
                        .unwrap_or(Decimal::ZERO),
                    priced_cost_amount: Decimal::try_from(row.try_get::<f64, _>(18)?)
                        .unwrap_or(Decimal::ZERO),
                    charge_source: row.try_get(19)?,
                    account_type: row.try_get(20)?,
                    team_id: row.try_get(21)?,
                    api_key_name: row.try_get(22)?,
                    latency_ms: row.try_get::<i64, _>(23)?.max(0) as u64,
                    reservation_id: row.try_get(24)?,
                    billing_group_id: row.try_get(25)?,
                    billing_group_name: row.try_get(26)?,
                    billing_payment_mode: row.try_get(27)?,
                })
            })
            .collect()
    }

    async fn count_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
    ) -> Result<usize, DbError> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT COUNT(*)::bigint FROM billing_events WHERE timestamp >= ",
        );
        builder
            .push_bind(start)
            .push(" AND timestamp < ")
            .push_bind(end);
        if let Some(uid) = user_id {
            builder.push(" AND user_id = ").push_bind(uid);
        }
        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", search);
            builder
                .push(" AND (request_id ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR COALESCE(api_key_name, '未命名 Key') ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR model ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        if let Some(api_key_name) = &filter.api_key_name {
            builder
                .push(" AND COALESCE(api_key_name, '未命名 Key') = ")
                .push_bind(api_key_name);
        }
        if let Some(model) = &filter.model {
            builder.push(" AND model = ").push_bind(model);
        }
        if let Some(source) = &filter.charge_source {
            builder.push(" AND (CASE WHEN billing_payment_mode = 'prepaid' AND package_units > 0 THEN 'prepaid_package' WHEN billing_payment_mode = 'prepaid' THEN 'prepaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END) = ").push_bind(source);
        }
        let (count,) = builder
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?;
        Ok(count.max(0) as usize)
    }

    async fn billing_activity_summary(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivitySummary, DbError> {
        let predicate = if user_id.is_some() {
            " AND user_id = $3"
        } else {
            ""
        };
        let sql = format!(
            "WITH classified AS (\
                SELECT *, CASE \
                    WHEN status_code = 499 OR activity_status = 'interrupted' THEN 'interrupted' \
                    WHEN status_code >= 400 OR success = false THEN 'failed' \
                    WHEN COALESCE(package_units, 0) = 0 AND COALESCE(wallet_amount, 0) = 0 \
                         AND COALESCE(priced_cost_amount, cost_amount, 0) = 0 THEN 'zero_cost' \
                    ELSE 'success' END AS derived_status \
                FROM billing_events WHERE timestamp >= $1 AND timestamp < $2{predicate}) \
             SELECT COUNT(*)::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'success')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'failed')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'interrupted')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'zero_cost')::bigint, \
                    COALESCE(SUM(total_tokens), 0)::bigint, \
                    COALESCE(SUM(package_units), 0)::bigint, \
                    COALESCE(SUM(wallet_amount), 0), \
                    COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                    COUNT(DISTINCT COALESCE(api_key_name, '未命名 Key'))::bigint, \
                    COUNT(DISTINCT model)::bigint FROM classified"
        );
        let mut query =
            query_as::<_, (i64, i64, i64, i64, i64, i64, i64, f64, f64, i64, i64)>(&sql)
                .bind(start)
                .bind(end);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let row = query.fetch_one(&self.pool).await?;
        Ok(crate::db::BillingActivitySummary {
            activity_count: row.0.max(0) as u64,
            success_count: row.1.max(0) as u64,
            failed_count: row.2.max(0) as u64,
            interrupted_count: row.3.max(0) as u64,
            zero_cost_count: row.4.max(0) as u64,
            total_tokens: row.5.max(0) as u64,
            package_units: row.6.max(0) as u64,
            wallet_amount: Decimal::try_from(row.7).unwrap_or(Decimal::ZERO),
            priced_cost_amount: Decimal::try_from(row.8).unwrap_or(Decimal::ZERO),
            api_key_count: row.9.max(0) as u64,
            model_count: row.10.max(0) as u64,
        })
    }

    async fn billing_activity_dimensions(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivityDimensions, DbError> {
        let predicate = if user_id.is_some() {
            " AND user_id = $3"
        } else {
            ""
        };
        let source_expr = "CASE WHEN billing_payment_mode = 'prepaid' AND package_units > 0 THEN 'prepaid_package' WHEN billing_payment_mode = 'prepaid' THEN 'prepaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END";
        let filtered = format!(
            "WITH filtered AS (SELECT COALESCE(api_key_name, '未命名 Key') AS api_key_name, model, package_units, COALESCE(wallet_amount, 0) AS wallet_amount, COALESCE(priced_cost_amount, cost_amount, 0) AS priced_cost_amount, cost_amount, billing_payment_mode, {source_expr} AS charge_source, total_tokens FROM billing_events WHERE timestamp >= $1 AND timestamp < $2{predicate})"
        );

        let api_key_sql = format!(
            "{filtered} SELECT api_key_name, COUNT(*)::bigint, 1::bigint, COUNT(DISTINCT model)::bigint, ARRAY_AGG(DISTINCT model ORDER BY model), ARRAY_AGG(DISTINCT charge_source ORDER BY charge_source), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY api_key_name ORDER BY 2 DESC, 1"
        );
        let model_sql = format!(
            "{filtered} SELECT model, COUNT(*)::bigint, COUNT(DISTINCT api_key_name)::bigint, 1::bigint, ARRAY_AGG(DISTINCT api_key_name ORDER BY api_key_name), ARRAY_AGG(DISTINCT charge_source ORDER BY charge_source), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY model ORDER BY 2 DESC, 1"
        );
        let source_sql = format!(
            "{filtered} SELECT charge_source, COUNT(*)::bigint, COUNT(DISTINCT api_key_name)::bigint, COUNT(DISTINCT model)::bigint, ARRAY_AGG(DISTINCT api_key_name ORDER BY api_key_name), ARRAY_AGG(DISTINCT model ORDER BY model), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY charge_source ORDER BY 2 DESC, 1"
        );

        type DimensionTuple = (
            String,
            i64,
            i64,
            i64,
            Vec<String>,
            Vec<String>,
            i64,
            i64,
            f64,
            f64,
        );
        async fn fetch_dimension_rows(
            pool: &PgPool,
            sql: &str,
            start: &str,
            end: &str,
            user_id: Option<&str>,
        ) -> Result<Vec<DimensionTuple>, DbError> {
            let mut statement = query_as::<_, DimensionTuple>(sql).bind(start).bind(end);
            if let Some(uid) = user_id {
                statement = statement.bind(uid);
            }
            statement.fetch_all(pool).await.map_err(DbError::from)
        }

        let api_keys = fetch_dimension_rows(&self.pool, &api_key_sql, start, end, user_id).await?;
        let models = fetch_dimension_rows(&self.pool, &model_sql, start, end, user_id).await?;
        let sources = fetch_dimension_rows(&self.pool, &source_sql, start, end, user_id).await?;
        let map_row = |row: DimensionTuple| crate::db::BillingActivityDimensionRow {
            name: row.0,
            activity_count: row.1.max(0) as u64,
            key_count: row.2.max(0) as u64,
            model_count: row.3.max(0) as u64,
            related_names: row.4,
            source_names: row.5,
            total_tokens: row.6.max(0) as u64,
            package_units: row.7.max(0) as u64,
            wallet_amount: Decimal::try_from(row.8).unwrap_or(Decimal::ZERO),
            priced_cost_amount: Decimal::try_from(row.9).unwrap_or(Decimal::ZERO),
        };

        Ok(crate::db::BillingActivityDimensions {
            api_keys: api_keys.into_iter().map(&map_row).collect(),
            models: models.into_iter().map(&map_row).collect(),
            sources: sources.into_iter().map(map_row).collect(),
        })
    }

    async fn period_token_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: (i64, i64, f64) = if let Some(uid) = user_id {
            query_as("SELECT COALESCE(SUM(prompt_tokens),0)::bigint, COALESCE(SUM(cache_hit_input_tokens),0)::bigint, COALESCE(SUM(completion_tokens),0)::double precision FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3")
                .bind(&start).bind(&end).bind(uid).fetch_one(&self.pool).await?
        } else {
            query_as("SELECT COALESCE(SUM(prompt_tokens),0)::bigint, COALESCE(SUM(cache_hit_input_tokens),0)::bigint, COALESCE(SUM(completion_tokens),0)::double precision FROM billing_events WHERE timestamp >= $1 AND timestamp < $2")
                .bind(&start).bind(&end).fetch_one(&self.pool).await?
        };
        Ok(vec![
            ("input".into(), rows.0.max(0) as u64, Decimal::ZERO),
            ("cache_hit".into(), rows.1.max(0) as u64, Decimal::ZERO),
            ("output".into(), rows.2.max(0.0) as u64, Decimal::ZERO),
        ])
    }

    async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64)> = if let Some(uid) = user_id {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(m, c)| (m, Decimal::try_from(c).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, String, f64)> = if let Some(uid) = user_id {
            query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(COALESCE(ul.priced_cost_amount, ul.cost_amount, 0)), 0) \
                 FROM billing_events ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 AND ul.user_id = $3 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(COALESCE(ul.priced_cost_amount, ul.cost_amount, 0)), 0) \
                 FROM billing_events ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(ch, n, c)| (ch, n, Decimal::try_from(c).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
    }

    async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (count,): (i64,) = if let Some(uid) = user_id {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(count as usize)
    }

    async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC LIMIT $3 OFFSET $4",
            )
            .bind(&start)
            .bind(&end)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
    }

    async fn billing_months(&self) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM billing_events ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM billing_events WHERE user_id = $1 ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn period_summary_all(&self) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM billing_events GROUP BY month ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn period_summary_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM billing_events WHERE user_id = $1 GROUP BY month ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_active_counts(
        &self,
        year: i32,
        month: u32,
    ) -> Result<(u64, u64), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (active_teams, active_users): (i64, i64) = query_as(
            "SELECT \
             COUNT(DISTINCT CASE WHEN account_type = 'team' AND team_id IS NOT NULL THEN team_id END)::bigint, \
             COUNT(DISTINCT user_id)::bigint \
             FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
        )
        .bind(&start)
        .bind(&end)
        .fetch_one(&self.pool)
        .await?;
        Ok((active_teams as u64, active_users as u64))
    }

    async fn admin_billing_team_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<Vec<(String, String, Decimal, u64, u64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = query_as::<_, (String, String, f64, i64, i64, i64)>(
            "SELECT \
             be.team_id, \
             COALESCE(t.name, be.team_id) AS team_name, \
             COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
             COUNT(*)::bigint AS total_requests, \
             COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
             COUNT(DISTINCT be.user_id)::bigint AS active_users \
             FROM billing_events be \
             LEFT JOIN teams t ON t.id = be.team_id \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.account_type = 'team' AND be.team_id IS NOT NULL \
             GROUP BY be.team_id, t.name \
             ORDER BY total_cost DESC \
             LIMIT $3",
        )
        .bind(&start)
        .bind(&end)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(team_id, team_name, total_cost, total_requests, total_tokens, active_users)| {
                    (
                        team_id,
                        team_name,
                        Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                        total_requests as u64,
                        total_tokens as u64,
                        active_users as u64,
                    )
                },
            )
            .collect())
    }

    async fn admin_billing_teams_page(
        &self,
        year: i32,
        month: u32,
        search: Option<&str>,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                String,
                String,
                String,
                Decimal,
                u64,
                u64,
                u64,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let search_term = search
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("%{}%", value.trim()));
        let (sort_expr, sort_dir) = match sort_by.unwrap_or("total_cost") {
            "team_name" => (
                "team_name",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "total_requests" => (
                "total_requests",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "total_tokens" => (
                "total_tokens",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "active_users" => (
                "active_users",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "last_billed_at" => (
                "last_billed_at",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            _ => (
                "total_cost",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
        };

        let base_cte = "WITH team_billing AS ( \
            SELECT \
            be.team_id, \
            COALESCE(t.name, be.team_id) AS team_name, \
            COALESCE(t.owner_id, '') AS owner_id, \
            COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
            COUNT(*)::bigint AS total_requests, \
            COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
            COUNT(DISTINCT be.user_id)::bigint AS active_users, \
            MAX(be.timestamp)::text AS last_billed_at \
            FROM billing_events be \
            LEFT JOIN teams t ON t.id = be.team_id \
            WHERE be.timestamp >= $1 AND be.timestamp < $2 \
              AND be.account_type = 'team' AND be.team_id IS NOT NULL \
            GROUP BY be.team_id, t.name, t.owner_id \
        ) ";

        let count_sql = if search_term.is_some() {
            format!("{} SELECT COUNT(*)::bigint FROM team_billing WHERE team_name ILIKE $3 OR team_id ILIKE $3 OR owner_id ILIKE $3", base_cte)
        } else {
            format!("{} SELECT COUNT(*)::bigint FROM team_billing", base_cte)
        };

        let total: i64 = if let Some(pattern) = &search_term {
            query_scalar::<_, i64>(&count_sql)
                .bind(&start)
                .bind(&end)
                .bind(pattern)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_scalar::<_, i64>(&count_sql)
                .bind(&start)
                .bind(&end)
                .fetch_one(&self.pool)
                .await?
        };

        let page_sql = if search_term.is_some() {
            format!(
                "{} SELECT team_id, team_name, owner_id, total_cost, total_requests, total_tokens, active_users, last_billed_at \
                 FROM team_billing \
                 WHERE team_name ILIKE $3 OR team_id ILIKE $3 OR owner_id ILIKE $3 \
                 ORDER BY {} {} LIMIT $4 OFFSET $5",
                base_cte, sort_expr, sort_dir
            )
        } else {
            format!(
                "{} SELECT team_id, team_name, owner_id, total_cost, total_requests, total_tokens, active_users, last_billed_at \
                 FROM team_billing \
                 ORDER BY {} {} LIMIT $3 OFFSET $4",
                base_cte, sort_expr, sort_dir
            )
        };

        let rows = if let Some(pattern) = &search_term {
            query_as::<_, (String, String, String, f64, i64, i64, i64, Option<String>)>(&page_sql)
                .bind(&start)
                .bind(&end)
                .bind(pattern)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            query_as::<_, (String, String, String, f64, i64, i64, i64, Option<String>)>(&page_sql)
                .bind(&start)
                .bind(&end)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
        };

        Ok((
            rows.into_iter()
                .map(
                    |(
                        team_id,
                        team_name,
                        owner_id,
                        total_cost,
                        total_requests,
                        total_tokens,
                        active_users,
                        last_billed_at,
                    )| {
                        (
                            team_id,
                            team_name,
                            owner_id,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            active_users as u64,
                            last_billed_at,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn admin_billing_team_users_page(
        &self,
        team_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(String, String, Decimal, u64, u64, Option<String>)>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let total: i64 = query_scalar(
            "SELECT COUNT(*)::bigint FROM ( \
             SELECT be.user_id \
             FROM billing_events be \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.team_id = $3 AND be.account_type = 'team' \
             GROUP BY be.user_id, be.user_name \
            ) users",
        )
        .bind(&start)
        .bind(&end)
        .bind(team_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = query_as::<_, (String, String, f64, i64, i64, Option<String>)>(
            "SELECT \
             be.user_id, \
             be.user_name, \
             COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
             COUNT(*)::bigint AS total_requests, \
             COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
             MAX(be.timestamp)::text AS last_billed_at \
             FROM billing_events be \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.team_id = $3 AND be.account_type = 'team' \
             GROUP BY be.user_id, be.user_name \
             ORDER BY total_cost DESC LIMIT $4 OFFSET $5",
        )
        .bind(&start)
        .bind(&end)
        .bind(team_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok((
            rows.into_iter()
                .map(
                    |(
                        user_id,
                        user_name,
                        total_cost,
                        total_requests,
                        total_tokens,
                        last_billed_at,
                    )| {
                        (
                            user_id,
                            user_name,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            last_billed_at,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn admin_billing_scoped_period_summary(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64, Vec<(String, u64, Decimal)>), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (
            cost,
            count,
            tokens,
            prompt_tokens,
            prompt_cost,
            cache_tokens,
            cache_cost,
            completion_tokens,
            completion_cost,
        ): (f64, i64, i64, i64, f64, i64, f64, i64, f64) = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as(
                    "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                     COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                     COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                     COALESCE(SUM(completion_tokens),0)::bigint, \
                     COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team'",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            } else {
                query_as(
                    "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                     COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                     COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                     COALESCE(SUM(completion_tokens),0)::bigint, \
                     COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as(
                "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team'",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok((
            Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
            count as u64,
            tokens as u64,
            vec![
                (
                    "input".to_string(),
                    prompt_tokens as u64,
                    Decimal::try_from(prompt_cost).unwrap_or(Decimal::ZERO),
                ),
                (
                    "cache_hit".to_string(),
                    cache_tokens as u64,
                    Decimal::try_from(cache_cost).unwrap_or(Decimal::ZERO),
                ),
                (
                    "output".to_string(),
                    completion_tokens as u64,
                    Decimal::try_from(completion_cost).unwrap_or(Decimal::ZERO),
                ),
            ],
        ))
    }

    async fn admin_billing_scoped_model_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64)>(
                    "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY model ORDER BY 2 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64)>(
                    "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY model ORDER BY 2 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(model, cost)| (model, Decimal::try_from(cost).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn admin_billing_scoped_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, String, f64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, String, f64)>(
                    "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                     FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                     WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                       AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                     GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, String, f64)>(
                    "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                     FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                     WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                     GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, String, f64)>(
                "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                 FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                   AND be.team_id = $3 AND be.account_type = 'team' \
                 GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, String, f64)>(
                "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                 FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                 GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(channel_id, name, cost)| {
                (
                    channel_id,
                    name,
                    Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
                )
            })
            .collect())
    }

    async fn admin_billing_daily_trend(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64, i64, i64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, \
                     COALESCE(SUM(total_tokens), 0)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY day ORDER BY day ASC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, \
                     COALESCE(SUM(total_tokens), 0)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY day ORDER BY day ASC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, \
                 COALESCE(SUM(total_tokens), 0)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY day ORDER BY day ASC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, \
                 COALESCE(SUM(total_tokens), 0)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day ASC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(date, total_cost, total_requests, total_tokens)| {
                (
                    date,
                    Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                    total_requests as u64,
                    total_tokens as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_scoped_count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (count,): (i64,) = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as(
                    "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team'",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            } else {
                query_as(
                    "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team'",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(count as usize)
    }

    async fn admin_billing_scoped_daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY day ORDER BY day DESC LIMIT $5 OFFSET $6",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC LIMIT $3 OFFSET $4",
            )
            .bind(&start)
            .bind(&end)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
    }

    async fn admin_billing_scoped_period_summary_all(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 7) AS month, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                     FROM billing_events \
                     WHERE team_id = $1 AND user_id = $2 AND account_type = 'team' \
                     GROUP BY month ORDER BY month DESC",
                )
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 7) AS month, \
                     COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                     FROM billing_events WHERE user_id = $1 \
                     GROUP BY month ORDER BY month DESC",
                )
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 7) AS month, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events \
                 WHERE team_id = $1 AND account_type = 'team' \
                 GROUP BY month ORDER BY month DESC",
            )
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 7) AS month, \
                 COALESCE(SUM(COALESCE(priced_cost_amount, cost_amount, 0)), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events GROUP BY month ORDER BY month DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_user_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<
        Vec<(
            Option<String>,
            Option<String>,
            u64,
            bool,
            String,
            String,
            Decimal,
            u64,
            u64,
            u64,
            Option<String>,
        )>,
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = query_as::<_, (Option<String>, Option<String>, i64, bool, String, Option<String>, f64, i64, i64, i64, Option<String>)>(
            "WITH filtered AS ( \
                SELECT be.* \
                FROM billing_events be \
                WHERE be.timestamp >= $1 AND be.timestamp < $2 \
            ), user_totals AS ( \
                SELECT \
                    be.user_id, \
                    COALESCE(MAX(NULLIF(be.user_name, '')), be.user_id) AS user_name, \
                    COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                    COUNT(*)::bigint AS total_requests, \
                    COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                    COUNT(DISTINCT be.api_key_name)::bigint AS api_key_count, \
                    MAX(be.timestamp)::text AS last_billed_at, \
                    COUNT(DISTINCT CASE WHEN be.team_id IS NOT NULL THEN be.team_id END)::bigint AS team_count \
                FROM filtered be \
                GROUP BY be.user_id \
            ), user_team_rank AS ( \
                SELECT \
                    be.user_id, \
                    be.team_id, \
                    t.name AS team_name, \
                    ROW_NUMBER() OVER ( \
                        PARTITION BY be.user_id \
                        ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.team_id ASC NULLS LAST \
                    ) AS rank_no \
                FROM filtered be \
                LEFT JOIN teams t ON t.id = be.team_id \
                WHERE be.team_id IS NOT NULL \
                GROUP BY be.user_id, be.team_id, t.name \
            ) \
            SELECT \
                utr.team_id, \
                utr.team_name, \
                ut.team_count, \
                (ut.team_count > 1) AS multi_team, \
                ut.user_id, \
                ut.user_name, \
                ut.total_cost, \
                ut.total_requests, \
                ut.total_tokens, \
                ut.api_key_count, \
                ut.last_billed_at \
            FROM user_totals ut \
            LEFT JOIN user_team_rank utr \
              ON utr.user_id = ut.user_id AND utr.rank_no = 1 \
            ORDER BY ut.total_cost DESC, ut.total_requests DESC, ut.user_id ASC \
            LIMIT $3",
        )
        .bind(&start)
        .bind(&end)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    team_id,
                    team_name,
                    team_count,
                    multi_team,
                    user_id,
                    user_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    api_key_count,
                    last_billed_at,
                )| {
                    let fallback_user_name = user_id.clone();
                    (
                        team_id,
                        team_name,
                        team_count as u64,
                        multi_team,
                        user_id,
                        user_name.unwrap_or(fallback_user_name),
                        Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                        total_requests as u64,
                        total_tokens as u64,
                        api_key_count as u64,
                        last_billed_at,
                    )
                },
            )
            .collect())
    }

    async fn admin_billing_user_api_keys_page(
        &self,
        team_id: Option<&str>,
        user_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                Option<String>,
                Decimal,
                u64,
                u64,
                u64,
                u64,
                u64,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<bool>,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let total: i64 = if let Some(tid) = team_id {
            query_scalar(
                "SELECT COUNT(*)::bigint FROM ( \
                 SELECT be.api_key_name \
                 FROM billing_events be \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                   AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                 GROUP BY be.api_key_name \
                ) api_keys",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_scalar(
                "SELECT COUNT(*)::bigint FROM ( \
                 SELECT be.api_key_name \
                 FROM billing_events be \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                 GROUP BY be.api_key_name \
                ) api_keys",
            )
            .bind(&start)
            .bind(&end)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
        };

        let rows = if let Some(tid) = team_id {
            query_as::<_, (Option<String>, f64, i64, i64, i64, i64, i64, Option<String>, Option<String>, Option<String>, Option<bool>, Option<String>)>(
                "WITH key_stats AS ( \
                    SELECT \
                        be.api_key_name, \
                        COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                        COUNT(*)::bigint AS total_requests, \
                        COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                        COALESCE(SUM(be.prompt_tokens), 0)::bigint AS prompt_tokens, \
                        COALESCE(SUM(be.completion_tokens), 0)::bigint AS completion_tokens, \
                        COALESCE(SUM(be.cache_hit_input_tokens), 0)::bigint AS cache_hit_input_tokens, \
                        MAX(be.timestamp)::text AS last_request_at, \
                        MAX(be.team_id) AS team_id \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                      AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                    GROUP BY be.api_key_name \
                ), key_models AS ( \
                    SELECT \
                        be.api_key_name, \
                        be.model, \
                        ROW_NUMBER() OVER ( \
                            PARTITION BY be.api_key_name \
                            ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.model ASC \
                        ) AS rank_no \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                      AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                    GROUP BY be.api_key_name, be.model \
                ) \
                SELECT \
                    ks.api_key_name, \
                    ks.total_cost, \
                    ks.total_requests, \
                    ks.total_tokens, \
                    ks.prompt_tokens, \
                    ks.completion_tokens, \
                    ks.cache_hit_input_tokens, \
                    km.model AS primary_model, \
                    ks.last_request_at, \
                    ks.team_id, \
                    ak.enabled AS api_key_enabled, \
                    ak.key AS api_key \
                FROM key_stats ks \
                LEFT JOIN key_models km \
                  ON km.api_key_name IS NOT DISTINCT FROM ks.api_key_name AND km.rank_no = 1 \
                LEFT JOIN api_keys ak \
                  ON ak.name = ks.api_key_name AND ak.user_id = $4 AND ak.team_id = $3 \
                ORDER BY ks.total_cost DESC, ks.total_requests DESC, ks.api_key_name ASC NULLS LAST \
                LIMIT $5 OFFSET $6",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(user_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (Option<String>, f64, i64, i64, i64, i64, i64, Option<String>, Option<String>, Option<String>, Option<bool>, Option<String>)>(
                "WITH key_stats AS ( \
                    SELECT \
                        be.api_key_name, \
                        COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                        COUNT(*)::bigint AS total_requests, \
                        COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                        COALESCE(SUM(be.prompt_tokens), 0)::bigint AS prompt_tokens, \
                        COALESCE(SUM(be.completion_tokens), 0)::bigint AS completion_tokens, \
                        COALESCE(SUM(be.cache_hit_input_tokens), 0)::bigint AS cache_hit_input_tokens, \
                        MAX(be.timestamp)::text AS last_request_at, \
                        MAX(be.team_id) AS team_id \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                    GROUP BY be.api_key_name \
                ), key_models AS ( \
                    SELECT \
                        be.api_key_name, \
                        be.model, \
                        ROW_NUMBER() OVER ( \
                            PARTITION BY be.api_key_name \
                            ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.model ASC \
                        ) AS rank_no \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                    GROUP BY be.api_key_name, be.model \
                ) \
                SELECT \
                    ks.api_key_name, \
                    ks.total_cost, \
                    ks.total_requests, \
                    ks.total_tokens, \
                    ks.prompt_tokens, \
                    ks.completion_tokens, \
                    ks.cache_hit_input_tokens, \
                    km.model AS primary_model, \
                    ks.last_request_at, \
                    ks.team_id, \
                    ak.enabled AS api_key_enabled, \
                    ak.key AS api_key \
                FROM key_stats ks \
                LEFT JOIN key_models km \
                  ON km.api_key_name IS NOT DISTINCT FROM ks.api_key_name AND km.rank_no = 1 \
                LEFT JOIN api_keys ak \
                  ON ak.name = ks.api_key_name AND ak.user_id = $3 \
                ORDER BY ks.total_cost DESC, ks.total_requests DESC, ks.api_key_name ASC NULLS LAST \
                LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(user_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok((
            rows.into_iter()
                .map(
                    |(
                        api_key_name,
                        total_cost,
                        total_requests,
                        total_tokens,
                        prompt_tokens,
                        completion_tokens,
                        cache_hit_input_tokens,
                        primary_model,
                        last_request_at,
                        team_id,
                        api_key_enabled,
                        api_key,
                    )| {
                        (
                            api_key_name,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            prompt_tokens as u64,
                            completion_tokens as u64,
                            cache_hit_input_tokens as u64,
                            primary_model,
                            last_request_at,
                            team_id,
                            api_key_enabled,
                            api_key,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal, Decimal), DbError> {
        let (prompt_price, completion_price, cache_read_price, cache_write_price) =
            self.pricing_lookup(model_name).await;
        Ok((
            prompt_price,
            completion_price,
            cache_read_price,
            cache_write_price,
        ))
    }
}
