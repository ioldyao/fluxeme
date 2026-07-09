use rusqlite::{params, Connection};

use crate::domain::user::{ApiKey, User};

/// List all users (password_hash excluded)
pub fn list(conn: &Connection) -> Result<Vec<User>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, rpm, tpm FROM users ORDER BY id")?;
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
    let mut stmt = conn.prepare("SELECT id, name, rpm, tpm FROM users WHERE id = ?1")?;
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
        })
    })?;
    match rows.next() {
        Some(Ok(u)) => Ok(Some(u)),
        _ => Ok(None),
    }
}

/// Get user with password_hash for login verification
pub fn get_with_password(conn: &Connection, id: &str) -> Result<Option<User>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, password_hash, rpm, tpm FROM users WHERE id = ?1")?;
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
    conn.execute(
        "INSERT INTO users (id, name, password_hash, rpm, tpm) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user.id, user.name, pw_hash, rpm, tpm],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, user: &User) -> Result<(), crate::db::DbError> {
    let (rpm, tpm) = user
        .rate_limits
        .as_ref()
        .map(|r| (r.rpm, r.tpm))
        .unwrap_or((None, None));

    if let Some(ref pw) = user.password_hash {
        conn.execute(
            "UPDATE users SET name = ?1, password_hash = ?2, rpm = ?3, tpm = ?4 WHERE id = ?5",
            params![user.name, pw, rpm, tpm, user.id],
        )?;
    } else {
        conn.execute(
            "UPDATE users SET name = ?1, rpm = ?2, tpm = ?3 WHERE id = ?4",
            params![user.name, rpm, tpm, user.id],
        )?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
    Ok(())
}

// ── API keys ──────────────────────────────────────────────────────

pub fn list_api_keys(conn: &Connection, user_id: &str) -> Result<Vec<ApiKey>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT key, user_id, name, enabled, expires_at FROM api_keys WHERE user_id = ?1 ORDER BY key",
    )?;
    let rows = stmt.query_map(params![user_id], |row| {
        Ok(ApiKey {
            key: row.get(0)?,
            user_id: row.get(1)?,
            name: row.get(2)?,
            enabled: row.get::<_, i32>(3)? != 0,
            expires_at: row.get(4)?,
        })
    })?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(row?);
    }
    Ok(keys)
}

pub fn create_api_key(conn: &Connection, key: &ApiKey) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO api_keys (key, user_id, name, enabled, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![key.key, key.user_id, key.name, key.enabled as i32, key.expires_at],
    )?;
    Ok(())
}

pub fn update_api_key(conn: &Connection, key: &str, enabled: bool) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE api_keys SET enabled = ?1 WHERE key = ?2",
        params![enabled as i32, key],
    )?;
    Ok(())
}

pub fn delete_api_key(conn: &Connection, key: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM api_keys WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn lookup_key(conn: &Connection, key: &str) -> Result<Option<(User, ApiKey)>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.name, u.rpm, u.tpm, a.key, a.user_id, a.name, a.enabled, a.expires_at
         FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = ?1",
    )?;
    let mut rows = stmt.query_map(params![key], |row| {
        let api_key = ApiKey {
            key: row.get(4)?,
            user_id: row.get(5)?,
            name: row.get(6)?,
            enabled: row.get::<_, i32>(7)? != 0,
            expires_at: row.get(8)?,
        };
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
        "SELECT u.id, u.name, u.rpm, u.tpm, a.key, a.user_id, a.name, a.enabled, a.expires_at
         FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
    )?;
    let rows = stmt.query_map([], |row| {
        let api_key = ApiKey {
            key: row.get(4)?,
            user_id: row.get(5)?,
            name: row.get(6)?,
            enabled: row.get::<_, i32>(7)? != 0,
            expires_at: row.get(8)?,
        };
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
        };
        Ok((user, api_key))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}
