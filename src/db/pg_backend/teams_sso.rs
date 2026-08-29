use super::*;
use async_trait::async_trait;

#[async_trait]
impl TeamsSsoBackend for PgBackend {
    // ── Teams ─────────────────────────────────────────────────────────────

    async fn create_team(&self, team: &Team, owner_id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        query(
            "INSERT INTO teams (id, name, owner_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&team.id)
        .bind(&team.name)
        .bind(&team.owner_id)
        .bind(team.created_at.to_rfc3339())
        .bind(team.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        // Owner is a member with role 'owner'.
        query(
            "INSERT INTO team_members (team_id, user_id, role, joined_at) VALUES ($1, $2, 'owner', $3)",
        )
        .bind(&team.id)
        .bind(owner_id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        // Initial zero-balance team wallet.
        query(
            "INSERT INTO team_wallets (team_id, balance, frozen, updated_at) \
             VALUES ($1, 0.0, 0.0, $2)",
        )
        .bind(&team.id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get_team(&self, team_id: &str) -> Result<Option<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT id, name, owner_id, created_at, updated_at FROM teams WHERE id = $1",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team: {e}")))?;
        Ok(rows
            .first()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id: id.clone(),
                name: name.clone(),
                owner_id: owner_id.clone(),
                created_at: chrono::DateTime::parse_from_rfc3339(created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            }))
    }

    async fn list_teams_for_user(&self, user_id: &str) -> Result<Vec<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT t.id, t.name, t.owner_id, t.created_at, t.updated_at \
             FROM teams t \
             JOIN team_members m ON m.team_id = t.id \
             WHERE m.user_id = $1 \
             ORDER BY t.created_at",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_teams_for_user: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id,
                name,
                owner_id,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn list_all_teams(&self) -> Result<Vec<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT id, name, owner_id, created_at, updated_at FROM teams ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_all_teams: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id,
                name,
                owner_id,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn update_team(&self, team_id: &str, name: &str) -> Result<(), DbError> {
        query("UPDATE teams SET name = $1, updated_at = $2 WHERE id = $3")
            .bind(name)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("update_team: {e}")))?;
        Ok(())
    }

    async fn delete_team(&self, team_id: &str) -> Result<(), DbError> {
        query("DELETE FROM teams WHERE id = $1")
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("delete_team: {e}")))?;
        Ok(())
    }

    async fn add_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO team_members (team_id, user_id, role, joined_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(role)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("add_team_member: {e}")))?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: &str, user_id: &str) -> Result<(), DbError> {
        // Refuse to remove the owner.
        let (current_role,): (String,) =
            query_as("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
                .bind(team_id)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError(format!("remove_team_member: {e}")))?;
        if current_role == "owner" {
            return Err(DbError("Cannot remove the team owner".to_string()));
        }
        query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("remove_team_member: {e}")))?;
        Ok(())
    }

    async fn set_team_member_role(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        // Forbid changing the owner's role away from owner.
        let (current_role,): (String,) =
            query_as("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
                .bind(team_id)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError(format!("set_team_member_role: {e}")))?;
        if current_role == "owner" && role != "owner" {
            return Err(DbError("Cannot demote the team owner".to_string()));
        }
        query("UPDATE team_members SET role = $1 WHERE team_id = $2 AND user_id = $3")
            .bind(role)
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("set_team_member_role: {e}")))?;
        Ok(())
    }

    async fn list_team_members(&self, team_id: &str) -> Result<Vec<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members \
             WHERE team_id = $1 ORDER BY joined_at",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_team_members: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id,
                user_id,
                role,
                joined_at: chrono::DateTime::parse_from_rfc3339(&joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn get_team_member(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Option<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members \
             WHERE team_id = $1 AND user_id = $2",
        )
        .bind(team_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team_member: {e}")))?;
        Ok(rows
            .first()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id: team_id.clone(),
                user_id: user_id.clone(),
                role: role.clone(),
                joined_at: chrono::DateTime::parse_from_rfc3339(joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            }))
    }

    async fn get_team_wallet(&self, team_id: &str) -> Result<Option<(f64, f64)>, DbError> {
        let rows = query_as::<_, (f64, f64)>(
            "SELECT balance, frozen FROM team_wallets WHERE team_id = $1",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team_wallet: {e}")))?;
        Ok(rows.first().map(|(balance, frozen)| (*balance, *frozen)))
    }

    async fn all_team_members(&self) -> Result<Vec<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members ORDER BY team_id, user_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("all_team_members: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id,
                user_id,
                role,
                joined_at: chrono::DateTime::parse_from_rfc3339(&joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn add_team_wallet_balance(&self, team_id: &str, amount: f64) -> Result<(), DbError> {
        query("UPDATE team_wallets SET balance = balance + $1, updated_at = $2 WHERE team_id = $3")
            .bind(amount)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("add_team_wallet_balance: {e}")))?;
        Ok(())
    }

    async fn list_team_wallet_transactions(
        &self,
        team_id: &str,
        page: usize,
        size: usize,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        let offset = (page.saturating_sub(1)) * size;
        let rows = query(
            "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
             FROM wallet_transactions WHERE team_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(team_id)
        .bind(size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
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
            .collect::<Vec<_>>();
        let (count,): (i64,) =
            query_as("SELECT COUNT(*) FROM wallet_transactions WHERE team_id = $1")
                .bind(team_id)
                .fetch_one(&self.pool)
                .await?;
        Ok((items, count as usize))
    }

    async fn list_team_api_keys(&self, team_id: &str) -> Result<Vec<ApiKey>, DbError> {
        let rows = query(
            "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode, key_kind \
             FROM api_keys WHERE team_id = $1 ORDER BY key",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
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
                    key_kind: r.get(10),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(9)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Metered),
                }
            })
            .collect())
    }

    async fn list_team_rules(&self, team_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules WHERE scope='user' AND team_id=$1 \
             ORDER BY priority, name",
        )
        .bind(team_id)
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

    // ── SSO Configs ─────────────────────────────────────────────────────────

    async fn list_sso_configs(&self) -> Result<Vec<SsoConfigRow>, DbError> {
        let rows = query_as::<
            _,
            (
                String,
                Option<String>,
                String,
                String,
                String,
                String,
                String,
                bool,
                bool,
                Option<String>,
                String,
                String,
                String,
            ),
        >(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list sso configs: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    team_id,
                    provider_name,
                    issuer_url,
                    client_id,
                    client_secret_encrypted,
                    redirect_url,
                    enabled,
                    auto_create_user,
                    domain_restrictions,
                    default_role,
                    created_at,
                    updated_at,
                )| SsoConfigRow {
                    id,
                    team_id,
                    provider_name,
                    issuer_url,
                    client_id,
                    client_secret_encrypted: Some(client_secret_encrypted),
                    redirect_url,
                    enabled,
                    auto_create_user,
                    domain_restrictions,
                    default_role,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    async fn get_sso_config(&self, id: &str) -> Result<Option<SsoConfigRow>, DbError> {
        let row: Option<(
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            bool,
            bool,
            Option<String>,
            String,
            String,
            String,
        )> = query_as(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to get sso config: {}", e)))?;

        Ok(row.map(
            |(
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted,
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            )| SsoConfigRow {
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted: Some(client_secret_encrypted),
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            },
        ))
    }

    async fn get_sso_config_by_team(&self, team_id: &str) -> Result<Option<SsoConfigRow>, DbError> {
        let row: Option<(
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            bool,
            bool,
            Option<String>,
            String,
            String,
            String,
        )> = query_as(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs WHERE team_id = $1",
        )
        .bind(team_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to get sso config by team: {}", e)))?;

        Ok(row.map(
            |(
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted,
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            )| SsoConfigRow {
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted: Some(client_secret_encrypted),
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            },
        ))
    }

    async fn create_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        query(
            "INSERT INTO sso_configs (id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&config.id)
        .bind(&config.team_id)
        .bind(&config.provider_name)
        .bind(&config.issuer_url)
        .bind(&config.client_id)
        .bind(&config.client_secret_encrypted)
        .bind(&config.redirect_url)
        .bind(config.enabled)
        .bind(config.auto_create_user)
        .bind(&config.domain_restrictions)
        .bind(&config.default_role)
        .bind(&config.created_at)
        .bind(&config.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to create sso config: {}", e)))?;
        Ok(())
    }

    async fn update_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        query(
            "UPDATE sso_configs SET team_id=$2, provider_name=$3, issuer_url=$4, \
             client_id=$5, client_secret_encrypted=$6, redirect_url=$7, \
             enabled=$8, auto_create_user=$9, domain_restrictions=$10, \
             default_role=$11, updated_at=$12 \
             WHERE id=$1",
        )
        .bind(&config.id)
        .bind(&config.team_id)
        .bind(&config.provider_name)
        .bind(&config.issuer_url)
        .bind(&config.client_id)
        .bind(&config.client_secret_encrypted)
        .bind(&config.redirect_url)
        .bind(config.enabled)
        .bind(config.auto_create_user)
        .bind(&config.domain_restrictions)
        .bind(&config.default_role)
        .bind(&config.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to update sso config: {}", e)))?;
        Ok(())
    }

    async fn delete_sso_config(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM sso_configs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("Failed to delete sso config: {}", e)))?;
        Ok(())
    }

    async fn list_sso_user_orgs(&self) -> Result<Vec<(String, String)>, DbError> {
        let rows = query_as::<_, (String, String)>(
            "SELECT user_id, orgs FROM sso_user_orgs ORDER BY user_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list sso user orgs: {}", e)))?;
        Ok(rows)
    }

    async fn upsert_sso_user_orgs(&self, user_id: &str, orgs_json: &str) -> Result<(), DbError> {
        query(
            "INSERT INTO sso_user_orgs (user_id, orgs, updated_at) VALUES ($1, $2, now() AT TIME ZONE 'utc') \
             ON CONFLICT (user_id) DO UPDATE SET orgs = EXCLUDED.orgs, updated_at = now() AT TIME ZONE 'utc'",
        )
        .bind(user_id)
        .bind(orgs_json)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to upsert sso user orgs: {}", e)))?;
        Ok(())
    }
}
