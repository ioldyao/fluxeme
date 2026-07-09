#![allow(dead_code)]
use rusqlite::{params, Connection};

use crate::domain::channel::{Channel, Endpoint};

#[allow(dead_code)]
pub fn list(conn: &Connection) -> Result<Vec<Channel>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, provider, priority, enabled FROM channels ORDER BY priority, id",
    )?;
    let mut channels: Vec<Channel> = stmt
        .query_map([], |row| {
            Ok(Channel {
                id: row.get(0)?,
                name: row.get(1)?,
                provider: row.get(2)?,
                priority: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                endpoints: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Single batch query for all endpoints
    let mut estmt = conn.prepare(
        "SELECT id, channel_id, url, api_key, weight, timeout_secs FROM endpoints ORDER BY channel_id",
    )?;
    let endpoint_rows = estmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                Endpoint {
                    id: Some(row.get(0)?),
                    channel_id: row.get(1)?,
                    url: row.get(2)?,
                    api_key: row.get(3)?,
                    weight: row.get(4)?,
                    timeout_secs: row.get(5)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut eps_by_channel: std::collections::HashMap<String, Vec<Endpoint>> =
        std::collections::HashMap::new();
    for (ch_id, ep) in endpoint_rows {
        eps_by_channel.entry(ch_id).or_default().push(ep);
    }

    for ch in &mut channels {
        if let Some(eps) = eps_by_channel.remove(&ch.id) {
            ch.endpoints = eps;
        }
    }
    Ok(channels)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Channel>, crate::db::DbError> {
    let mut stmt =
        conn.prepare("SELECT id, name, provider, priority, enabled FROM channels WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Channel {
            id: row.get(0)?,
            name: row.get(1)?,
            provider: row.get(2)?,
            priority: row.get(3)?,
            enabled: row.get::<_, i32>(4)? != 0,
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
        "INSERT INTO channels (id, name, provider, priority, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![ch.id, ch.name, ch.provider, ch.priority, ch.enabled as i32],
    )?;
    for ep in &ch.endpoints {
        create_endpoint(conn, &ch.id, ep)?;
    }
    Ok(())
}

pub fn update(conn: &Connection, ch: &Channel) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE channels SET name = ?1, provider = ?2, priority = ?3, enabled = ?4 WHERE id = ?5",
        params![ch.name, ch.provider, ch.priority, ch.enabled as i32, ch.id],
    )?;
    // Replace endpoints: delete old, insert new
    conn.execute(
        "DELETE FROM endpoints WHERE channel_id = ?1",
        params![ch.id],
    )?;
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

fn list_endpoints(
    conn: &Connection,
    channel_id: &str,
) -> Result<Vec<Endpoint>, crate::db::DbError> {
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

fn create_endpoint(
    conn: &Connection,
    channel_id: &str,
    ep: &Endpoint,
) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![channel_id, ep.url, ep.api_key, ep.weight, ep.timeout_secs],
    )?;
    Ok(())
}
