#![allow(dead_code)]
use rusqlite::{params, Connection};

use crate::domain::channel::{Channel, Endpoint};

#[allow(dead_code)]
pub fn list(conn: &Connection) -> Result<Vec<Channel>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, provider, priority, enabled FROM channels ORDER BY priority, id")?;
    let channels: Vec<Channel> = stmt
        .query_map([], |row| {
            Ok(Channel {
                id: row.get(0)?,
                provider: row.get(1)?,
                priority: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0,
                endpoints: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut result = Vec::new();
    for mut ch in channels {
        ch.endpoints = list_endpoints(conn, &ch.id)?;
        result.push(ch);
    }
    Ok(result)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Channel>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, provider, priority, enabled FROM channels WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Channel {
            id: row.get(0)?,
            provider: row.get(1)?,
            priority: row.get(2)?,
            enabled: row.get::<_, i32>(3)? != 0,
            endpoints: Vec::new(),
        })
    })?;
    match rows.next() {
        Some(Ok(mut ch)) => {
            ch.endpoints = list_endpoints(conn, &ch.id)?;
            Ok(Some(ch))
        }
        _ => Ok(None),
    }
}

pub fn create(conn: &Connection, ch: &Channel) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO channels (id, provider, priority, enabled) VALUES (?1, ?2, ?3, ?4)",
        params![ch.id, ch.provider, ch.priority, ch.enabled as i32],
    )?;
    for ep in &ch.endpoints {
        create_endpoint(conn, &ch.id, ep)?;
    }
    Ok(())
}

pub fn update(conn: &Connection, ch: &Channel) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE channels SET provider = ?1, priority = ?2, enabled = ?3 WHERE id = ?4",
        params![ch.provider, ch.priority, ch.enabled as i32, ch.id],
    )?;
    // Replace endpoints: delete old, insert new
    conn.execute("DELETE FROM endpoints WHERE channel_id = ?1", params![ch.id])?;
    for ep in &ch.endpoints {
        create_endpoint(conn, &ch.id, ep)?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM channels WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Endpoints ─────────────────────────────────────────────────────

fn list_endpoints(conn: &Connection, channel_id: &str) -> Result<Vec<Endpoint>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, channel_id, url, api_key, weight, timeout_secs FROM endpoints WHERE channel_id = ?1",
    )?;
    let rows = stmt.query_map(params![channel_id], |row| {
        Ok(Endpoint {
            id: Some(row.get(0)?),
            channel_id: row.get(1)?,
            url: row.get(2)?,
            api_key: row.get(3)?,
            weight: row.get(4)?,
            timeout_secs: row.get(5)?,
        })
    })?;
    let mut eps = Vec::new();
    for row in rows {
        eps.push(row?);
    }
    Ok(eps)
}

fn create_endpoint(conn: &Connection, channel_id: &str, ep: &Endpoint) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![channel_id, ep.url, ep.api_key, ep.weight, ep.timeout_secs],
    )?;
    Ok(())
}
