use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub caldav_url: String,
    pub username: String,
    #[serde(skip_serializing)]
    #[schema(write_only)]
    pub password: String,
    pub ics_path: String,
    pub sync_interval_secs: i64,
    pub last_synced: Option<String>,
    pub last_sync_status: Option<String>,
    pub last_sync_error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSource {
    pub name: String,
    pub caldav_url: String,
    pub username: String,
    pub password: String,
    pub ics_path: String,
    pub sync_interval_secs: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSource {
    pub name: Option<String>,
    pub caldav_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ics_path: Option<String>,
    pub sync_interval_secs: Option<i64>,
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            caldav_url TEXT NOT NULL,
            username TEXT NOT NULL,
            password TEXT NOT NULL,
            ics_path TEXT NOT NULL UNIQUE,
            sync_interval_secs INTEGER NOT NULL DEFAULT 3600,
            last_synced TEXT,
            last_sync_status TEXT,
            last_sync_error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS ics_data (
            source_id INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
            ics_content TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS destinations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            ics_url TEXT NOT NULL,
            caldav_url TEXT NOT NULL,
            calendar_name TEXT NOT NULL,
            username TEXT NOT NULL,
            password TEXT NOT NULL,
            sync_interval_secs INTEGER NOT NULL DEFAULT 3600,
            sync_all INTEGER NOT NULL DEFAULT 0,
            keep_local INTEGER NOT NULL DEFAULT 0,
            last_synced TEXT,
            last_sync_status TEXT,
            last_sync_error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    // Migrate existing DBs: add status columns
    let _ = conn.execute_batch(
        "ALTER TABLE sources ADD COLUMN last_sync_status TEXT;
         ALTER TABLE sources ADD COLUMN last_sync_error TEXT;",
    );
    // Migrate existing DBs: rename sync_interval_minutes -> sync_interval_secs
    let _ = conn.execute_batch(
        "ALTER TABLE sources ADD COLUMN sync_interval_secs INTEGER NOT NULL DEFAULT 3600;
         UPDATE sources SET sync_interval_secs = sync_interval_minutes * 60 WHERE sync_interval_minutes IS NOT NULL;
         ALTER TABLE destinations ADD COLUMN sync_interval_secs INTEGER NOT NULL DEFAULT 3600;
         UPDATE destinations SET sync_interval_secs = sync_interval_minutes * 60 WHERE sync_interval_minutes IS NOT NULL;",
    );
    Ok(())
}

pub fn list_sources(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, caldav_url, username, password, ics_path, sync_interval_secs, last_synced, last_sync_status, last_sync_error, created_at FROM sources ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Source {
            id: row.get(0)?,
            name: row.get(1)?,
            caldav_url: row.get(2)?,
            username: row.get(3)?,
            password: row.get(4)?,
            ics_path: row.get(5)?,
            sync_interval_secs: row.get(6)?,
            last_synced: row.get(7)?,
            last_sync_status: row.get(8)?,
            last_sync_error: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_source(conn: &Connection, id: i64) -> Result<Option<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, caldav_url, username, password, ics_path, sync_interval_secs, last_synced, last_sync_status, last_sync_error, created_at FROM sources WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Source {
            id: row.get(0)?,
            name: row.get(1)?,
            caldav_url: row.get(2)?,
            username: row.get(3)?,
            password: row.get(4)?,
            ics_path: row.get(5)?,
            sync_interval_secs: row.get(6)?,
            last_synced: row.get(7)?,
            last_sync_status: row.get(8)?,
            last_sync_error: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn create_source(conn: &Connection, src: &CreateSource) -> Result<i64> {
    conn.execute(
        "INSERT INTO sources (name, caldav_url, username, password, ics_path, sync_interval_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![src.name, src.caldav_url, src.username, src.password, src.ics_path, src.sync_interval_secs],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_source(conn: &Connection, id: i64, upd: &UpdateSource) -> Result<bool> {
    let existing = match get_source(conn, id)? {
        Some(s) => s,
        None => return Ok(false),
    };
    conn.execute(
        "UPDATE sources SET name = ?1, caldav_url = ?2, username = ?3, password = ?4, ics_path = ?5, sync_interval_secs = ?6 WHERE id = ?7",
        params![
            upd.name.as_deref().unwrap_or(&existing.name),
            upd.caldav_url.as_deref().unwrap_or(&existing.caldav_url),
            upd.username.as_deref().unwrap_or(&existing.username),
            upd.password.as_deref().filter(|s| !s.is_empty()).unwrap_or(&existing.password),
            upd.ics_path.as_deref().unwrap_or(&existing.ics_path),
            upd.sync_interval_secs.unwrap_or(existing.sync_interval_secs),
            id
        ],
    )?;
    Ok(true)
}

pub fn delete_source(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
    Ok(rows > 0)
}

pub fn update_last_synced(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE sources SET last_synced = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn update_sync_status(
    conn: &Connection,
    id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE sources SET last_sync_status = ?1, last_sync_error = ?2 WHERE id = ?3",
        params![status, error, id],
    )?;
    Ok(())
}

pub fn save_ics_data(conn: &Connection, source_id: i64, content: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO ics_data (source_id, ics_content, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(source_id) DO UPDATE SET ics_content = ?2, updated_at = datetime('now')",
        params![source_id, content],
    )?;
    Ok(())
}

pub fn get_ics_data(conn: &Connection, source_id: i64) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT ics_content FROM ics_data WHERE source_id = ?1")?;
    let mut rows = stmt.query_map(params![source_id], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_ics_data_by_path(conn: &Connection, path: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT d.ics_content FROM ics_data d JOIN sources s ON d.source_id = s.id WHERE s.ics_path = ?1",
    )?;
    let mut rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

// --- Destinations (ICS -> CalDAV reverse sync) ---

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Destination {
    pub id: i64,
    pub name: String,
    pub ics_url: String,
    pub caldav_url: String,
    pub calendar_name: String,
    pub username: String,
    #[serde(skip_serializing)]
    #[schema(write_only)]
    pub password: String,
    pub sync_interval_secs: i64,
    pub sync_all: bool,
    pub keep_local: bool,
    pub last_synced: Option<String>,
    pub last_sync_status: Option<String>,
    pub last_sync_error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDestination {
    pub name: String,
    pub ics_url: String,
    pub caldav_url: String,
    pub calendar_name: String,
    pub username: String,
    pub password: String,
    pub sync_interval_secs: i64,
    #[serde(default)]
    pub sync_all: bool,
    #[serde(default)]
    pub keep_local: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDestination {
    pub name: Option<String>,
    pub ics_url: Option<String>,
    pub caldav_url: Option<String>,
    pub calendar_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub sync_interval_secs: Option<i64>,
    pub sync_all: Option<bool>,
    pub keep_local: Option<bool>,
}

pub fn list_destinations(conn: &Connection) -> Result<Vec<Destination>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local, last_synced, last_sync_status, last_sync_error, created_at FROM destinations ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Destination {
            id: row.get(0)?,
            name: row.get(1)?,
            ics_url: row.get(2)?,
            caldav_url: row.get(3)?,
            calendar_name: row.get(4)?,
            username: row.get(5)?,
            password: row.get(6)?,
            sync_interval_secs: row.get(7)?,
            sync_all: row.get(8)?,
            keep_local: row.get(9)?,
            last_synced: row.get(10)?,
            last_sync_status: row.get(11)?,
            last_sync_error: row.get(12)?,
            created_at: row.get(13)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_destination(conn: &Connection, id: i64) -> Result<Option<Destination>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local, last_synced, last_sync_status, last_sync_error, created_at FROM destinations WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Destination {
            id: row.get(0)?,
            name: row.get(1)?,
            ics_url: row.get(2)?,
            caldav_url: row.get(3)?,
            calendar_name: row.get(4)?,
            username: row.get(5)?,
            password: row.get(6)?,
            sync_interval_secs: row.get(7)?,
            sync_all: row.get(8)?,
            keep_local: row.get(9)?,
            last_synced: row.get(10)?,
            last_sync_status: row.get(11)?,
            last_sync_error: row.get(12)?,
            created_at: row.get(13)?,
        })
    })?;
    match rows.next() {
        Some(Ok(d)) => Ok(Some(d)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn create_destination(conn: &Connection, dest: &CreateDestination) -> Result<i64> {
    conn.execute(
        "INSERT INTO destinations (name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![dest.name, dest.ics_url, dest.caldav_url, dest.calendar_name, dest.username, dest.password, dest.sync_interval_secs, dest.sync_all, dest.keep_local],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_destination(conn: &Connection, id: i64, upd: &UpdateDestination) -> Result<bool> {
    let existing = match get_destination(conn, id)? {
        Some(d) => d,
        None => return Ok(false),
    };
    conn.execute(
        "UPDATE destinations SET name = ?1, ics_url = ?2, caldav_url = ?3, calendar_name = ?4, username = ?5, password = ?6, sync_interval_secs = ?7, sync_all = ?8, keep_local = ?9 WHERE id = ?10",
        params![
            upd.name.as_deref().unwrap_or(&existing.name),
            upd.ics_url.as_deref().unwrap_or(&existing.ics_url),
            upd.caldav_url.as_deref().unwrap_or(&existing.caldav_url),
            upd.calendar_name.as_deref().unwrap_or(&existing.calendar_name),
            upd.username.as_deref().unwrap_or(&existing.username),
            upd.password.as_deref().filter(|s| !s.is_empty()).unwrap_or(&existing.password),
            upd.sync_interval_secs.unwrap_or(existing.sync_interval_secs),
            upd.sync_all.unwrap_or(existing.sync_all),
            upd.keep_local.unwrap_or(existing.keep_local),
            id
        ],
    )?;
    Ok(true)
}

pub fn delete_destination(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute("DELETE FROM destinations WHERE id = ?1", params![id])?;
    Ok(rows > 0)
}

pub fn update_destination_sync_status(
    conn: &Connection,
    id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE destinations SET last_sync_status = ?1, last_sync_error = ?2, last_synced = datetime('now') WHERE id = ?3",
        params![status, error, id],
    )?;
    Ok(())
}
