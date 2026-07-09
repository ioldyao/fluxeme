#![allow(dead_code)]
use rusqlite::{params, Connection};

use crate::domain::model::{Model, ModelChannel, Pricing};

pub fn list(conn: &Connection) -> Result<Vec<Model>, crate::db::DbError> {
    let mut stmt = conn.prepare("SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length FROM models ORDER BY id")?;
    let mut models: Vec<Model> = stmt
        .query_map([], |row| {
            Ok(Model {
                id: row.get(0)?,
                name: row.get(1)?,
                model_pattern: row.get(2)?,
                pricing: Pricing {
                    prompt_price: row.get(3)?,
                    completion_price: row.get(4)?,
                    cache_read_price: row.get(5)?,
                    cache_write_price: row.get(6)?,
                    image_input_price: row.get(7)?,
                    audio_input_price: row.get(8)?,
                    audio_output_price: row.get(9)?,
                },
                channels: Vec::new(),
                published: row.get::<_, i32>(10)? != 0,
                context_length: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Single batch query for all bindings
    let mut bstmt = conn.prepare(
        "SELECT model_id, channel_id, priority FROM model_channels ORDER BY model_id, priority",
    )?;
    let binding_rows = bstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ModelChannel {
                    model_id: row.get(0)?,
                    channel_id: row.get(1)?,
                    priority: row.get(2)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut bindings_by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
        std::collections::HashMap::new();
    for (model_id, binding) in binding_rows {
        bindings_by_model.entry(model_id).or_default().push(binding);
    }

    for m in &mut models {
        if let Some(bindings) = bindings_by_model.remove(&m.id) {
            m.channels = bindings;
        }
    }
    Ok(models)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Model>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length FROM models WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Model {
            id: row.get(0)?,
            name: row.get(1)?,
            model_pattern: row.get(2)?,
            pricing: Pricing {
                prompt_price: row.get(3)?,
                completion_price: row.get(4)?,
                cache_read_price: row.get(5)?,
                cache_write_price: row.get(6)?,
                image_input_price: row.get(7)?,
                audio_input_price: row.get(8)?,
                audio_output_price: row.get(9)?,
            },
            channels: Vec::new(),
            published: row.get::<_, i32>(10)? != 0,
            context_length: row.get(11)?,
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
        "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![m.id, m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price, m.pricing.cache_write_price, m.pricing.image_input_price, m.pricing.audio_input_price, m.pricing.audio_output_price, m.published as i32, m.context_length],
    )?;
    for binding in &m.channels {
        create_binding(conn, &m.id, binding)?;
    }
    Ok(())
}

pub fn update(conn: &Connection, m: &Model) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE models SET name = ?1, model_pattern = ?2, prompt_price = ?3, completion_price = ?4, cache_read_price = ?5, cache_write_price = ?6, image_input_price = ?7, audio_input_price = ?8, audio_output_price = ?9, published = ?10, context_length = ?11 WHERE id = ?12",
        params![m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price, m.pricing.cache_write_price, m.pricing.image_input_price, m.pricing.audio_input_price, m.pricing.audio_output_price, m.published as i32, m.context_length, m.id],
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

pub fn update_pricing(conn: &Connection, id: &str, p: &Pricing) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE models SET prompt_price=?1, completion_price=?2, cache_read_price=?3, cache_write_price=?4, image_input_price=?5, audio_input_price=?6, audio_output_price=?7 WHERE id=?8",
        params![p.prompt_price, p.completion_price, p.cache_read_price, p.cache_write_price, p.image_input_price, p.audio_input_price, p.audio_output_price, id],
    )?;
    Ok(())
}

fn create_binding(conn: &Connection, model_id: &str, binding: &ModelChannel) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO model_channels (model_id, channel_id, priority) VALUES (?1, ?2, ?3)",
        params![model_id, binding.channel_id, binding.priority],
    )?;
    Ok(())
}
