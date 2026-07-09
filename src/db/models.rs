#![allow(dead_code)]
use rusqlite::{params, Connection};

use crate::domain::model::{Model, ModelChannel, Pricing};

pub fn list(conn: &Connection) -> Result<Vec<Model>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, model_pattern, prompt_price, completion_price, published, context_length FROM models ORDER BY id")?;
    let models: Vec<Model> = stmt
        .query_map([], |row| {
            Ok(Model {
                id: row.get(0)?,
                name: row.get(1)?,
                model_pattern: row.get(2)?,
                pricing: Pricing {
                    prompt_price: row.get(3)?,
                    completion_price: row.get(4)?,
                },
                channels: Vec::new(),
                published: row.get::<_, i32>(5)? != 0,
                context_length: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut result = Vec::new();
    for mut m in models {
        m.channels = list_bindings(conn, &m.id)?;
        result.push(m);
    }
    Ok(result)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Model>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, model_pattern, prompt_price, completion_price, published, context_length FROM models WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Model {
            id: row.get(0)?,
            name: row.get(1)?,
            model_pattern: row.get(2)?,
            pricing: Pricing {
                prompt_price: row.get(3)?,
                completion_price: row.get(4)?,
            },
            channels: Vec::new(),
            published: row.get::<_, i32>(5)? != 0,
            context_length: row.get(6)?,
        })
    })?;
    match rows.next() {
        Some(Ok(mut m)) => {
            m.channels = list_bindings(conn, &m.id)?;
            Ok(Some(m))
        }
        _ => Ok(None),
    }
}

pub fn create(conn: &Connection, m: &Model) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, published, context_length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![m.id, m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.published as i32, m.context_length],
    )?;
    for binding in &m.channels {
        create_binding(conn, &m.id, binding)?;
    }
    Ok(())
}

pub fn update(conn: &Connection, m: &Model) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE models SET name = ?1, model_pattern = ?2, prompt_price = ?3, completion_price = ?4, published = ?5, context_length = ?6 WHERE id = ?7",
        params![m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.published as i32, m.context_length, m.id],
    )?;
    conn.execute("DELETE FROM model_channels WHERE model_id = ?1", params![m.id])?;
    for binding in &m.channels {
        create_binding(conn, &m.id, binding)?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM models WHERE id = ?1", params![id])?;
    Ok(())
}

pub(super) fn list_bindings(conn: &Connection, model_id: &str) -> Result<Vec<ModelChannel>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT model_id, channel_id, priority FROM model_channels WHERE model_id = ?1 ORDER BY priority",
    )?;
    let rows = stmt.query_map(params![model_id], |row| {
        Ok(ModelChannel {
            model_id: row.get(0)?,
            channel_id: row.get(1)?,
            priority: row.get(2)?,
        })
    })?;
    let mut bindings = Vec::new();
    for row in rows {
        bindings.push(row?);
    }
    Ok(bindings)
}

fn create_binding(conn: &Connection, model_id: &str, binding: &ModelChannel) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO model_channels (model_id, channel_id, priority) VALUES (?1, ?2, ?3)",
        params![model_id, binding.channel_id, binding.priority],
    )?;
    Ok(())
}
