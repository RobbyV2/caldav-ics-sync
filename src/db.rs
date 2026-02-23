use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

fn require_non_empty(field: &str, value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{} cannot be empty", field);
    Ok(())
}

fn require_non_negative(field: &str, value: i64) -> Result<()> {
    ensure!(value >= 0, "{} cannot be negative", field);
    Ok(())
}

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
    pub public_ics: bool,
    pub public_ics_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSource {
    pub name: String,
    pub caldav_url: String,
    pub username: String,
    pub password: String,
    pub ics_path: String,
    pub sync_interval_secs: i64,
    #[serde(default)]
    pub public_ics: bool,
    pub public_ics_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSource {
    pub name: Option<String>,
    pub caldav_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ics_path: Option<String>,
    pub sync_interval_secs: Option<i64>,
    pub public_ics: Option<bool>,
    pub public_ics_path: Option<String>,
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
    let _ =
        conn.execute_batch("ALTER TABLE sources ADD COLUMN public_ics INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE sources ADD COLUMN public_ics_path TEXT;");
    let _ = conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_sources_public_ics_path ON sources(public_ics_path) WHERE public_ics_path IS NOT NULL;",
    );
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS source_paths (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
            path TEXT NOT NULL UNIQUE,
            is_public INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    Ok(())
}

pub fn list_sources(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, caldav_url, username, password, ics_path, sync_interval_secs, last_synced, last_sync_status, last_sync_error, created_at, public_ics, public_ics_path FROM sources ORDER BY id",
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
            public_ics: row.get(11)?,
            public_ics_path: row.get(12)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_source(conn: &Connection, id: i64) -> Result<Option<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, caldav_url, username, password, ics_path, sync_interval_secs, last_synced, last_sync_status, last_sync_error, created_at, public_ics, public_ics_path FROM sources WHERE id = ?1",
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
            public_ics: row.get(11)?,
            public_ics_path: row.get(12)?,
        })
    })?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn validate_ics_path(path: &str) -> Result<()> {
    let trimmed = path.trim();
    ensure!(
        trimmed != "public" && !trimmed.starts_with("public/"),
        "ICS path cannot start with 'public' — reserved for public ICS URLs"
    );
    Ok(())
}

fn validate_public_path(
    conn: &Connection,
    path: Option<&str>,
    exclude_id: Option<i64>,
) -> Result<Option<String>> {
    match path {
        Some(p) if !p.trim().is_empty() => {
            let p = p.trim();
            ensure!(!p.starts_with('/'), "Public ICS path must not start with /");
            ensure!(!p.contains(".."), "Public ICS path must not contain ..");
            validate_ics_path(p)?;
            let count: i64 = match exclude_id {
                Some(id) => conn.query_row(
                    "SELECT count(*) FROM sources WHERE (ics_path = ?1 OR public_ics_path = ?1) AND id != ?2",
                    params![p, id],
                    |row| row.get(0),
                )?,
                None => conn.query_row(
                    "SELECT count(*) FROM sources WHERE ics_path = ?1 OR public_ics_path = ?1",
                    params![p],
                    |row| row.get(0),
                )?,
            };
            ensure!(count == 0, "Duplicate public ICS path is not allowed");
            let sp_count: i64 = conn.query_row(
                "SELECT count(*) FROM source_paths WHERE path = ?1",
                params![p],
                |row| row.get(0),
            )?;
            ensure!(
                sp_count == 0,
                "Public path conflicts with an existing source path"
            );
            Ok(Some(p.to_owned()))
        }
        _ => Ok(None),
    }
}

pub fn create_source(conn: &Connection, src: &CreateSource) -> Result<i64> {
    require_non_empty("Name", &src.name)?;
    require_non_empty("CalDAV URL", &src.caldav_url)?;
    require_non_empty("Username", &src.username)?;
    require_non_empty("Password", &src.password)?;
    require_non_empty("ICS Path", &src.ics_path)?;
    validate_ics_path(&src.ics_path)?;
    require_non_negative("Sync interval", src.sync_interval_secs)?;

    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sources WHERE ics_path = ?1 OR public_ics_path = ?1",
        [&src.ics_path],
        |row| row.get(0),
    )?;
    ensure!(count == 0, "Duplicate ICS Path is not allowed");
    let sp_count: i64 = conn.query_row(
        "SELECT count(*) FROM source_paths WHERE path = ?1",
        params![&src.ics_path],
        |row| row.get(0),
    )?;
    ensure!(
        sp_count == 0,
        "ICS path conflicts with an existing source path"
    );

    let public_path = if src.public_ics {
        validate_public_path(conn, src.public_ics_path.as_deref(), None)?
    } else {
        None
    };
    if let Some(ref pp) = public_path {
        ensure!(
            pp != &src.ics_path,
            "Public ICS path cannot be the same as the ICS path"
        );
    }

    conn.execute(
        "INSERT INTO sources (name, caldav_url, username, password, ics_path, sync_interval_secs, public_ics, public_ics_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![src.name, src.caldav_url, src.username, src.password, src.ics_path, src.sync_interval_secs, src.public_ics, public_path],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_source(conn: &Connection, id: i64, upd: &UpdateSource) -> Result<bool> {
    let existing = match get_source(conn, id)? {
        Some(s) => s,
        None => return Ok(false),
    };

    if let Some(ref v) = upd.name {
        require_non_empty("Name", v)?;
    }
    if let Some(ref v) = upd.caldav_url {
        require_non_empty("CalDAV URL", v)?;
    }
    if let Some(ref v) = upd.username {
        require_non_empty("Username", v)?;
    }
    if let Some(ref v) = upd.ics_path {
        require_non_empty("ICS Path", v)?;
        validate_ics_path(v)?;
    }
    if let Some(v) = upd.sync_interval_secs {
        require_non_negative("Sync interval", v)?;
    }

    if let Some(ref new_path) = upd.ics_path {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM sources WHERE (ics_path = ?1 OR public_ics_path = ?1) AND id != ?2",
            params![new_path, id],
            |row| row.get(0),
        )?;
        ensure!(count == 0, "Duplicate ICS Path is not allowed");
        let sp_count: i64 = conn.query_row(
            "SELECT count(*) FROM source_paths WHERE path = ?1",
            params![new_path],
            |row| row.get(0),
        )?;
        ensure!(
            sp_count == 0,
            "ICS path conflicts with an existing source path"
        );
    }

    let eff_public_ics = upd.public_ics.unwrap_or(existing.public_ics);
    let eff_public_path = if eff_public_ics {
        match &upd.public_ics_path {
            Some(p) if p.trim().is_empty() => None,
            Some(p) => validate_public_path(conn, Some(p.as_str()), Some(id))?,
            None => existing.public_ics_path.clone(),
        }
    } else {
        None
    };
    let eff_ics_path = upd.ics_path.as_deref().unwrap_or(&existing.ics_path);
    if let Some(ref pp) = eff_public_path {
        ensure!(
            pp.as_str() != eff_ics_path,
            "Public ICS path cannot be the same as the ICS path"
        );
    }

    conn.execute(
        "UPDATE sources SET name = ?1, caldav_url = ?2, username = ?3, password = ?4, ics_path = ?5, sync_interval_secs = ?6, public_ics = ?7, public_ics_path = ?8 WHERE id = ?9",
        params![
            upd.name.as_deref().unwrap_or(&existing.name),
            upd.caldav_url.as_deref().unwrap_or(&existing.caldav_url),
            upd.username.as_deref().unwrap_or(&existing.username),
            upd.password.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&existing.password),
            eff_ics_path,
            upd.sync_interval_secs.unwrap_or(existing.sync_interval_secs),
            eff_public_ics,
            eff_public_path,
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
        "SELECT d.ics_content FROM ics_data d JOIN sources s ON d.source_id = s.id
         WHERE s.ics_path = ?1
         UNION ALL
         SELECT d.ics_content FROM ics_data d JOIN source_paths sp ON d.source_id = sp.source_id
         WHERE sp.path = ?1
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_ics_data_by_public_path(conn: &Connection, path: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT d.ics_content FROM ics_data d JOIN sources s ON d.source_id = s.id
         WHERE s.public_ics_path = ?1 AND s.public_ics = 1
         UNION ALL
         SELECT d.ics_content FROM ics_data d JOIN source_paths sp ON d.source_id = sp.source_id
         WHERE sp.path = ?1 AND sp.is_public = 1
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn is_public_standard_ics(conn: &Connection, ics_path: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM (
            SELECT 1 FROM sources WHERE ics_path = ?1 AND public_ics = 1 AND (public_ics_path IS NULL OR public_ics_path = '')
            UNION ALL
            SELECT 1 FROM source_paths WHERE path = ?1 AND is_public = 1
         ) t",
        params![ics_path],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

// --- Source Paths (additional ICS routes per source) ---

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SourcePath {
    pub id: i64,
    pub source_id: i64,
    pub path: String,
    pub is_public: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSourcePath {
    pub path: String,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSourcePath {
    pub path: Option<String>,
    pub is_public: Option<bool>,
}

fn validate_source_path(conn: &Connection, path: &str, exclude_id: Option<i64>) -> Result<String> {
    let trimmed = path.trim();
    require_non_empty("Path", trimmed)?;
    validate_ics_path(trimmed)?;
    ensure!(!trimmed.starts_with('/'), "Path must not start with /");
    ensure!(!trimmed.contains(".."), "Path must not contain ..");

    let sources_count: i64 = conn.query_row(
        "SELECT count(*) FROM sources WHERE ics_path = ?1 OR public_ics_path = ?1",
        params![trimmed],
        |row| row.get(0),
    )?;
    ensure!(
        sources_count == 0,
        "Path conflicts with an existing source ICS path"
    );

    let sp_count: i64 = match exclude_id {
        Some(id) => conn.query_row(
            "SELECT count(*) FROM source_paths WHERE path = ?1 AND id != ?2",
            params![trimmed, id],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT count(*) FROM source_paths WHERE path = ?1",
            params![trimmed],
            |row| row.get(0),
        )?,
    };
    ensure!(sp_count == 0, "Duplicate path is not allowed");

    Ok(trimmed.to_owned())
}

pub fn list_source_paths(conn: &Connection, source_id: i64) -> Result<Vec<SourcePath>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_id, path, is_public, created_at FROM source_paths WHERE source_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![source_id], |row| {
        Ok(SourcePath {
            id: row.get(0)?,
            source_id: row.get(1)?,
            path: row.get(2)?,
            is_public: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_source_path(conn: &Connection, id: i64) -> Result<Option<SourcePath>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_id, path, is_public, created_at FROM source_paths WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(SourcePath {
            id: row.get(0)?,
            source_id: row.get(1)?,
            path: row.get(2)?,
            is_public: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    match rows.next() {
        Some(Ok(sp)) => Ok(Some(sp)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn create_source_path(
    conn: &Connection,
    source_id: i64,
    body: &CreateSourcePath,
) -> Result<i64> {
    ensure!(get_source(conn, source_id)?.is_some(), "Source not found");
    let validated_path = validate_source_path(conn, &body.path, None)?;
    conn.execute(
        "INSERT INTO source_paths (source_id, path, is_public) VALUES (?1, ?2, ?3)",
        params![source_id, validated_path, body.is_public],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_source_path(conn: &Connection, id: i64, upd: &UpdateSourcePath) -> Result<bool> {
    let existing = match get_source_path(conn, id)? {
        Some(sp) => sp,
        None => return Ok(false),
    };

    let eff_path = match &upd.path {
        Some(p) => validate_source_path(conn, p, Some(id))?,
        None => existing.path,
    };
    let eff_public = upd.is_public.unwrap_or(existing.is_public);

    conn.execute(
        "UPDATE source_paths SET path = ?1, is_public = ?2 WHERE id = ?3",
        params![eff_path, eff_public, id],
    )?;
    Ok(true)
}

pub fn delete_source_path(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute("DELETE FROM source_paths WHERE id = ?1", params![id])?;
    Ok(rows > 0)
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

fn map_destination_row(row: &rusqlite::Row) -> rusqlite::Result<Destination> {
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
}

pub fn list_destinations(conn: &Connection) -> Result<Vec<Destination>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local, last_synced, last_sync_status, last_sync_error, created_at FROM destinations ORDER BY id",
    )?;
    let rows = stmt.query_map([], map_destination_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_destination(conn: &Connection, id: i64) -> Result<Option<Destination>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local, last_synced, last_sync_status, last_sync_error, created_at FROM destinations WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], map_destination_row)?;
    match rows.next() {
        Some(Ok(d)) => Ok(Some(d)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn find_overlapping_destinations(
    conn: &Connection,
    caldav_url: &str,
    calendar_name: &str,
    exclude_id: Option<i64>,
) -> Result<Vec<Destination>> {
    let base_sql = "SELECT id, name, ics_url, caldav_url, calendar_name, username, password, sync_interval_secs, sync_all, keep_local, last_synced, last_sync_status, last_sync_error, created_at FROM destinations WHERE caldav_url = ?1 AND calendar_name = ?2";

    match exclude_id {
        Some(id) => {
            let sql = format!("{} AND id != ?3", base_sql);
            let mut stmt = conn.prepare(&sql)?;
            let rows =
                stmt.query_map(params![caldav_url, calendar_name, id], map_destination_row)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
        None => {
            let mut stmt = conn.prepare(base_sql)?;
            let rows = stmt.query_map(params![caldav_url, calendar_name], map_destination_row)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
    }
}

pub fn create_destination(conn: &Connection, dest: &CreateDestination) -> Result<i64> {
    require_non_empty("Name", &dest.name)?;
    require_non_empty("ICS URL", &dest.ics_url)?;
    require_non_empty("CalDAV URL", &dest.caldav_url)?;
    require_non_empty("Calendar name", &dest.calendar_name)?;
    require_non_empty("Username", &dest.username)?;
    require_non_empty("Password", &dest.password)?;
    require_non_negative("Sync interval", dest.sync_interval_secs)?;

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

    if let Some(ref v) = upd.name {
        require_non_empty("Name", v)?;
    }
    if let Some(ref v) = upd.ics_url {
        require_non_empty("ICS URL", v)?;
    }
    if let Some(ref v) = upd.caldav_url {
        require_non_empty("CalDAV URL", v)?;
    }
    if let Some(ref v) = upd.calendar_name {
        require_non_empty("Calendar name", v)?;
    }
    if let Some(ref v) = upd.username {
        require_non_empty("Username", v)?;
    }
    if let Some(v) = upd.sync_interval_secs {
        require_non_negative("Sync interval", v)?;
    }

    let eff_caldav_url = upd.caldav_url.as_deref().unwrap_or(&existing.caldav_url);
    let eff_calendar_name = upd
        .calendar_name
        .as_deref()
        .unwrap_or(&existing.calendar_name);

    conn.execute(
        "UPDATE destinations SET name = ?1, ics_url = ?2, caldav_url = ?3, calendar_name = ?4, username = ?5, password = ?6, sync_interval_secs = ?7, sync_all = ?8, keep_local = ?9 WHERE id = ?10",
        params![
            upd.name.as_deref().unwrap_or(&existing.name),
            upd.ics_url.as_deref().unwrap_or(&existing.ics_url),
            eff_caldav_url,
            eff_calendar_name,
            upd.username.as_deref().unwrap_or(&existing.username),
            upd.password.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&existing.password),
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
