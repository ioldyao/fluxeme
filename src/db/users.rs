use rusqlite::{params, Connection};

use crate::domain::user::{ApiKey, User};

/// List all users (password_hash excluded)
pub fn list(conn: &Connection) -> Result<Vec<User>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, rpm, tpm, timezone FROM users ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: None,
            rate_limits: {
                let rpm: Option<u64> = row.get(2)?;
                let tpm: Option<u64> = row.get(3)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    })?;
    let mut users = Vec::new();
    for row in rows {
        users.push(row?);
    }
    Ok(users)
}

/// Get user by id (password_hash excluded)
pub fn get(conn: &Connection, id: &str) -> Result<Option<User>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, rpm, tpm, timezone FROM users WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: None,
            rate_limits: {
                let rpm: Option<u64> = row.get(2)?;
                let tpm: Option<u64> = row.get(3)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    })?;
    match rows.next() {
        Some(Ok(u)) => Ok(Some(u)),
        _ => Ok(None),
    }
}

/// Get user with password_hash for login verification
pub fn get_with_password(conn: &Connection, id: &str) -> Result<Option<User>, crate::db::DbError> {
    let mut stmt =
        conn.prepare("SELECT id, name, password_hash, rpm, tpm, timezone FROM users WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: Some(row.get::<_, String>(2)?),
            rate_limits: {
                let rpm: Option<u64> = row.get(3)?;
                let tpm: Option<u64> = row.get(4)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    })?;
    match rows.next() {
        Some(Ok(u)) => Ok(Some(u)),
        _ => Ok(None),
    }
}

pub fn create(conn: &Connection, user: &User) -> Result<(), crate::db::DbError> {
    let (rpm, tpm) = user
        .rate_limits
        .as_ref()
        .map(|r| (r.rpm, r.tpm))
        .unwrap_or((None, None));
    let pw_hash = user.password_hash.as_deref().unwrap_or("");
    let tz = if user.timezone.is_empty() { "UTC" } else { &user.timezone };
    conn.execute(
        "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![user.id, user.name, pw_hash, rpm, tpm, tz],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, user: &User) -> Result<(), crate::db::DbError> {
    let (rpm, tpm) = user
        .rate_limits
        .as_ref()
        .map(|r| (r.rpm, r.tpm))
        .unwrap_or((None, None));
    let tz = if user.timezone.is_empty() { "UTC" } else { &user.timezone };

    if let Some(ref pw) = user.password_hash {
        conn.execute(
            "UPDATE users SET name = ?1, password_hash = ?2, rpm = ?3, tpm = ?4, timezone = ?5 WHERE id = ?6",
            params![user.name, pw, rpm, tpm, tz, user.id],
        )?;
    } else {
        conn.execute(
            "UPDATE users SET name = ?1, rpm = ?2, tpm = ?3, timezone = ?4 WHERE id = ?5",
            params![user.name, rpm, tpm, tz, user.id],
        )?;
    }
    Ok(())
}

/// Get only the timezone for a user (lightweight read for chart grouping)
pub fn get_timezone(conn: &Connection, id: &str) -> Result<String, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT timezone FROM users WHERE id = ?1")?;
    let tz: Option<String> = stmt.query_row(params![id], |row| row.get(0)).ok();
    Ok(tz.unwrap_or_else(|| "UTC".to_string()))
}

/// Update only the timezone for a user
pub fn update_timezone(conn: &Connection, id: &str, timezone: &str) -> Result<(), crate::db::DbError> {
    let tz = if timezone.is_empty() { "UTC" } else { timezone };
    conn.execute(
        "UPDATE users SET timezone = ?1 WHERE id = ?2",
        params![tz, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
    Ok(())
}

// ── API keys ──────────────────────────────────────────────────────

fn row_to_api_key_joined(row: &rusqlite::Row, off: usize) -> rusqlite::Result<ApiKey> {
    let allowed_models_str: Option<String> = row.get(off + 6)?;
    Ok(ApiKey {
        key: row.get(off)?,
        user_id: row.get(off + 1)?,
        name: row.get(off + 2)?,
        enabled: row.get::<_, i32>(off + 3)? != 0,
        expires_at: row.get(off + 4)?,
        spend_limit: row.get(off + 5)?,
        allowed_models: allowed_models_str
            .filter(|s| !s.is_empty())
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
    })
}

fn row_to_api_key(row: &rusqlite::Row) -> rusqlite::Result<ApiKey> {
    row_to_api_key_joined(row, 0)
}

pub fn list_api_keys(conn: &Connection, user_id: &str) -> Result<Vec<ApiKey>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models FROM api_keys WHERE user_id = ?1 ORDER BY key",
    )?;
    let rows = stmt.query_map(params![user_id], row_to_api_key)?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(row?);
    }
    Ok(keys)
}

pub fn create_api_key(conn: &Connection, key: &ApiKey) -> Result<(), crate::db::DbError> {
    let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
    conn.execute(
        "INSERT INTO api_keys (key, user_id, name, enabled, expires_at, spend_limit, allowed_models) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![key.key, key.user_id, key.name, key.enabled as i32, key.expires_at, key.spend_limit, allowed],
    )?;
    Ok(())
}

pub fn update_api_key(conn: &Connection, key: &ApiKey) -> Result<(), crate::db::DbError> {
    let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
    conn.execute(
        "UPDATE api_keys SET name = ?1, enabled = ?2, expires_at = ?3, spend_limit = ?4, allowed_models = ?5 WHERE key = ?6",
        params![key.name, key.enabled as i32, key.expires_at, key.spend_limit, allowed, key.key],
    )?;
    Ok(())
}

pub fn delete_api_key(conn: &Connection, key: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM api_keys WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn lookup_key(
    conn: &Connection,
    key: &str,
) -> Result<Option<(User, ApiKey)>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models
         FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = ?1",
    )?;
    let mut rows = stmt.query_map(params![key], |row| {
        let api_key = row_to_api_key_joined(row, 5)?;
        let user = User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: None,
            rate_limits: {
                let rpm: Option<u64> = row.get(2)?;
                let tpm: Option<u64> = row.get(3)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        };
        Ok((user, api_key))
    })?;
    match rows.next() {
        Some(Ok(pair)) => Ok(Some(pair)),
        _ => Ok(None),
    }
}

pub fn all_api_keys(conn: &Connection) -> Result<Vec<(User, ApiKey)>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models
         FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
    )?;
    let rows = stmt.query_map([], |row| {
        let api_key = row_to_api_key_joined(row, 5)?;
        let user = User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: None,
            rate_limits: {
                let rpm: Option<u64> = row.get(2)?;
                let tpm: Option<u64> = row.get(3)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        };
        Ok((user, api_key))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}
