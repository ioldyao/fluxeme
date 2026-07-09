use rusqlite::{params, Connection};

use crate::domain::routing::RoutingRule;

pub fn list(conn: &Connection) -> Result<Vec<RoutingRule>, crate::db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT name, user_id, model_pattern, channel_id FROM routing_rules ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(RoutingRule {
            name: row.get(0)?,
            user_id: row.get(1)?,
            model_pattern: row.get(2)?,
            channel_id: row.get(3)?,
        })
    })?;
    let mut rules = Vec::new();
    for row in rows {
        rules.push(row?);
    }
    Ok(rules)
}

pub fn create(conn: &Connection, rule: &RoutingRule) -> Result<(), crate::db::DbError> {
    conn.execute(
        "INSERT INTO routing_rules (name, user_id, model_pattern, channel_id) VALUES (?1, ?2, ?3, ?4)",
        params![rule.name, rule.user_id, rule.model_pattern, rule.channel_id],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, rule: &RoutingRule) -> Result<(), crate::db::DbError> {
    conn.execute(
        "UPDATE routing_rules SET user_id = ?1, model_pattern = ?2, channel_id = ?3 WHERE name = ?4",
        params![rule.user_id, rule.model_pattern, rule.channel_id, rule.name],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, name: &str) -> Result<(), crate::db::DbError> {
    conn.execute("DELETE FROM routing_rules WHERE name = ?1", params![name])?;
    Ok(())
}
