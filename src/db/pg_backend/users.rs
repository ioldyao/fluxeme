use super::*;
use async_trait::async_trait;

#[async_trait]
impl UsersBackend for PgBackend {
    // ── Users ────────────────────────────────────────────────────────────

    async fn list_users(&self, status: Option<&str>) -> Result<Vec<User>, DbError> {
        let rows = if let Some(status) = status {
            query(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE status = $1 ORDER BY id",
            )
            .bind(status)
            .fetch_all(&self.pool)
            .await?
        } else {
            query(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users ORDER BY id",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                Self::map_user_row(r, &mut idx)
            })
            .collect())
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let mut idx = 0usize;
            Self::map_user_row(r, &mut idx)
        }))
    }

    async fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError> {
        let rows = query(
            "SELECT id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let mut idx = 0usize;
            Self::map_user_with_pw_row(r, &mut idx)
        }))
    }

    async fn create_user(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let pw_hash = user.password_hash.as_deref().unwrap_or("");
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        let role = if user.role.is_empty() {
            "user"
        } else {
            &user.role
        };
        query(
            "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&user.id)
        .bind(&user.name)
        .bind(pw_hash)
        .bind(rpm)
        .bind(tpm)
        .bind(tz)
        .bind(user.token_version)
        .bind(role)
        .bind(user.concurrency_limit as i64)
        .bind(&user.currency)
        .bind(&user.status)
        .bind(&user.suspended_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_initial_admin(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let pw_hash = user.password_hash.as_deref().unwrap_or("");
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        let role = if user.role.is_empty() {
            "user"
        } else {
            &user.role
        };

        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;

        let (admin_count,): (i64,) = query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
            .fetch_one(&mut *tx)
            .await?;
        if admin_count > 0 {
            return Err(DbError("Setup already completed".to_string()));
        }

        query(
            "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&user.id)
        .bind(&user.name)
        .bind(pw_hash)
        .bind(rpm)
        .bind(tpm)
        .bind(tz)
        .bind(user.token_version)
        .bind(role)
        .bind(user.concurrency_limit as i64)
        .bind(&user.currency)
        .bind(&user.status)
        .bind(&user.suspended_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn update_user(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        if let Some(ref pw) = user.password_hash {
            query(
                "UPDATE users SET name = $1, password_hash = $2, rpm = $3, tpm = $4, timezone = $5, token_version = $6, role = $7, concurrency_limit = $8, currency = $9, status = $10, suspended_at = $11 WHERE id = $12",
            )
            .bind(&user.name)
            .bind(pw)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(user.token_version)
            .bind(&user.role)
            .bind(user.concurrency_limit as i64)
            .bind(&user.currency)
            .bind(&user.status)
            .bind(&user.suspended_at)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        } else {
            query(
                "UPDATE users SET name = $1, rpm = $2, tpm = $3, timezone = $4, token_version = $5, role = $6, concurrency_limit = $7, currency = $8, status = $9, suspended_at = $10 WHERE id = $11",
            )
            .bind(&user.name)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(user.token_version)
            .bind(&user.role)
            .bind(user.concurrency_limit as i64)
            .bind(&user.currency)
            .bind(&user.status)
            .bind(&user.suspended_at)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn bump_user_token_version(&self, id: &str) -> Result<(), DbError> {
        let result = query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("User not found".to_string()));
        }
        Ok(())
    }

    async fn update_user_admin_fields(
        &self,
        id: &str,
        name: Option<String>,
        password_hash: Option<String>,
        rate_limits: Option<crate::domain::user::RateLimit>,
        role: Option<String>,
        concurrency_limit: Option<u32>,
    ) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        let next_name = name.unwrap_or(current_user.name.clone());
        let next_role = role.unwrap_or(current_user.role.clone());
        let next_rate_limits = rate_limits.or(current_user.rate_limits.clone());
        let next_concurrency_limit = concurrency_limit.unwrap_or(current_user.concurrency_limit);

        if current_user.role == "admin"
            && current_user.status == USER_STATUS_ACTIVE
            && next_role != current_user.role
        {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot demote the last active admin".to_string()));
            }
        }
        let next_token_version = if next_role != current_user.role || password_hash.is_some() {
            current_user.token_version + 1
        } else {
            current_user.token_version
        };
        let (rpm, tpm) = next_rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let tz = if current_user.timezone.is_empty() {
            "UTC"
        } else {
            current_user.timezone.as_str()
        };

        let updated = if let Some(ref pw) = password_hash {
            let row = query(
                "UPDATE users SET name = $1, password_hash = $2, rpm = $3, tpm = $4, timezone = $5, token_version = $6, role = $7, concurrency_limit = $8, currency = $9, status = $10, suspended_at = $11 WHERE id = $12 RETURNING id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at",
            )
            .bind(&next_name)
            .bind(pw)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(next_token_version)
            .bind(&next_role)
            .bind(next_concurrency_limit as i64)
            .bind(&current_user.currency)
            .bind(&current_user.status)
            .bind(&current_user.suspended_at)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let mut idx = 0usize;
            Self::map_user_row(&row, &mut idx)
        } else {
            let row = query(
                "UPDATE users SET name = $1, rpm = $2, tpm = $3, timezone = $4, token_version = $5, role = $6, concurrency_limit = $7, currency = $8, status = $9, suspended_at = $10 WHERE id = $11 RETURNING id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at",
            )
            .bind(&next_name)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(next_token_version)
            .bind(&next_role)
            .bind(next_concurrency_limit as i64)
            .bind(&current_user.currency)
            .bind(&current_user.status)
            .bind(&current_user.suspended_at)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let mut idx = 0usize;
            Self::map_user_row(&row, &mut idx)
        };

        tx.commit().await?;
        Ok(updated)
    }

    async fn suspend_user(
        &self,
        id: &str,
        suspended_at: &chrono::DateTime<chrono::Utc>,
    ) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.status == USER_STATUS_SUSPENDED {
            return Ok(current_user);
        }

        if current_user.role == "admin" && current_user.status == USER_STATUS_ACTIVE {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot suspend the last active admin".to_string()));
            }
        }

        let suspended_at_str = suspended_at.to_rfc3339();
        query(
            "UPDATE users SET status = $1, suspended_at = $2, token_version = token_version + 1 WHERE id = $3",
        )
        .bind(USER_STATUS_SUSPENDED)
        .bind(&suspended_at_str)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let updated = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let user = Self::map_user_row(updated, &mut idx);
        tx.commit().await?;
        Ok(user)
    }

    async fn restore_user(&self, id: &str) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.status == USER_STATUS_ACTIVE {
            return Ok(current_user);
        }

        query(
            "UPDATE users SET status = $1, suspended_at = NULL, token_version = token_version + 1 WHERE id = $2",
        )
        .bind(USER_STATUS_ACTIVE)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let updated = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let user = Self::map_user_row(updated, &mut idx);
        tx.commit().await?;
        Ok(user)
    }

    async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.role == "admin" && current_user.status == USER_STATUS_ACTIVE {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot delete the last active admin".to_string()));
            }
        }

        query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn count_admins(&self, status: Option<&str>) -> Result<i64, DbError> {
        let (count,): (i64,) = if let Some(status) = status {
            query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                .bind(status)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
                .fetch_one(&self.pool)
                .await?
        };
        Ok(count)
    }

    async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        let result: Option<(String,)> = query_as("SELECT timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result.map(|r| r.0).unwrap_or_else(|| "UTC".to_string()))
    }

    async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        let tz = if timezone.is_empty() { "UTC" } else { timezone };
        query("UPDATE users SET timezone = $1 WHERE id = $2")
            .bind(tz)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_user_currency(&self, id: &str) -> Result<String, DbError> {
        let rows = query_as::<_, (String,)>("SELECT currency FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(rows.map(|r| r.0).unwrap_or_else(|| "usd".to_string()))
    }

    async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError> {
        let cur = if currency.is_empty() { "usd" } else { currency };
        query("UPDATE users SET currency = $1 WHERE id = $2")
            .bind(cur)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
