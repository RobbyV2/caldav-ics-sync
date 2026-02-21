use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::AbortHandle;
use tokio_retry2::strategy::ExponentialBackoff;
use tokio_retry2::{Retry, RetryError};
use tracing::info;

use crate::api::AppState;
use crate::db;

const RETRY_BASE_MS: u64 = 30_000;
const RETRY_MAX_MS: u64 = 300_000;
const MAX_RETRIES: usize = 5;

static GENERATION: AtomicU64 = AtomicU64::new(0);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub enum AutoSyncKey {
    Source(i64),
    Destination(i64),
}

pub type AutoSyncRegistry = Arc<Mutex<HashMap<AutoSyncKey, (u64, AbortHandle)>>>;

pub fn new_registry() -> AutoSyncRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn cancel(registry: &AutoSyncRegistry, key: &AutoSyncKey) {
    let Ok(mut map) = registry.lock() else {
        tracing::error!("Registry mutex poisoned during cancel for {:?}", key);
        return;
    };
    if let Some((_, handle)) = map.remove(key) {
        handle.abort();
        info!("Cancelled auto-sync for {:?}", key);
    }
}

fn try_remove(
    registry: &Mutex<HashMap<AutoSyncKey, (u64, AbortHandle)>>,
    key: &AutoSyncKey,
    generation: u64,
) {
    let Ok(mut map) = registry.lock() else {
        return;
    };
    if let Some(&(current, _)) = map.get(key)
        && current == generation
    {
        map.remove(key);
    }
}

fn handle_sync_error(state: &AppState, key: &AutoSyncKey, msg: &str) -> bool {
    let Ok(db) = state.db.lock() else {
        tracing::error!("DB mutex poisoned, stopping auto-sync for {:?}", key);
        return false;
    };
    match key {
        AutoSyncKey::Source(id) => match db::get_source(&db, *id) {
            Ok(Some(_)) => {
                let _ = db::update_sync_status(&db, *id, "error", Some(msg));
                true
            }
            Ok(None) => {
                info!("Source {} no longer exists, stopping auto-sync", id);
                false
            }
            Err(e) => {
                tracing::error!("DB error checking source {}: {}, stopping auto-sync", id, e);
                false
            }
        },
        AutoSyncKey::Destination(id) => match db::get_destination(&db, *id) {
            Ok(Some(_)) => {
                let _ = db::update_destination_sync_status(&db, *id, "error", Some(msg));
                true
            }
            Ok(None) => {
                info!("Destination {} no longer exists, stopping auto-sync", id);
                false
            }
            Err(e) => {
                tracing::error!(
                    "DB error checking destination {}: {}, stopping auto-sync",
                    id,
                    e
                );
                false
            }
        },
    }
}

fn spawn_sync_task<F, Fut>(
    registry: &AutoSyncRegistry,
    key: AutoSyncKey,
    interval_secs: u64,
    display_name: String,
    state: AppState,
    sync_fn: F,
) where
    F: Fn(AppState) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, RetryError<anyhow::Error>>> + Send,
{
    let generation = next_generation();
    let registry_ref = Arc::clone(registry);
    let key_clone = key.clone();
    let log_name = display_name.clone();

    let handle = tokio::spawn(async move {
        loop {
            let strategy = ExponentialBackoff::from_millis(RETRY_BASE_MS)
                .max_delay(Duration::from_millis(RETRY_MAX_MS))
                .take(MAX_RETRIES);

            let result = Retry::spawn(strategy, || sync_fn(state.clone())).await;

            match result {
                Ok(msg) => info!("{}", msg),
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!(
                        "Auto-sync '{}' failed after {} retries: {}",
                        display_name,
                        MAX_RETRIES,
                        msg
                    );
                    if !handle_sync_error(&state, &key_clone, &msg) {
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
        try_remove(&registry_ref, &key_clone, generation);
    });

    let Ok(mut map) = registry.lock() else {
        tracing::error!("Registry mutex poisoned during register for {:?}", key);
        handle.abort();
        return;
    };
    map.insert(key, (generation, handle.abort_handle()));
    drop(map);
    info!(
        "Auto-sync enabled for '{}' (every {}s)",
        log_name, interval_secs
    );
}

pub fn register_source(registry: &AutoSyncRegistry, state: &AppState, source: &db::Source) {
    let key = AutoSyncKey::Source(source.id);
    cancel(registry, &key);

    if source.sync_interval_secs <= 0 {
        return;
    }

    let id = source.id;
    spawn_sync_task(
        registry,
        key,
        source.sync_interval_secs as u64,
        source.name.clone(),
        state.clone(),
        move |state| async move {
            let (url, user, pass) = {
                let db = state.db.lock().unwrap();
                match db::get_source(&db, id) {
                    Ok(Some(s)) => (s.caldav_url, s.username, s.password),
                    _ => {
                        return Err(RetryError::permanent(anyhow::anyhow!(
                            "Source {} no longer exists",
                            id
                        )));
                    }
                }
            };
            let (events, calendars, ics_data) = crate::api::sync::run_sync(&url, &user, &pass)
                .await
                .map_err(RetryError::transient)?;
            let db = state.db.lock().unwrap();
            db::save_ics_data(&db, id, &ics_data).map_err(RetryError::transient)?;
            db::update_last_synced(&db, id).map_err(RetryError::transient)?;
            db::update_sync_status(&db, id, "ok", None).map_err(RetryError::transient)?;
            Ok(format!(
                "Auto-sync source {}: {} events from {} calendars",
                id, events, calendars
            ))
        },
    );
}

pub fn register_destination(registry: &AutoSyncRegistry, state: &AppState, dest: &db::Destination) {
    let key = AutoSyncKey::Destination(dest.id);
    cancel(registry, &key);

    if dest.sync_interval_secs <= 0 {
        return;
    }

    let id = dest.id;
    spawn_sync_task(
        registry,
        key,
        dest.sync_interval_secs as u64,
        dest.name.clone(),
        state.clone(),
        move |state| async move {
            let d = {
                let db = state.db.lock().unwrap();
                match db::get_destination(&db, id) {
                    Ok(Some(d)) => d,
                    _ => {
                        return Err(RetryError::permanent(anyhow::anyhow!(
                            "Destination {} no longer exists",
                            id
                        )));
                    }
                }
            };
            let (uploaded, total) = crate::api::reverse_sync::run_reverse_sync(
                &d.ics_url,
                &d.caldav_url,
                &d.calendar_name,
                &d.username,
                &d.password,
                d.sync_all,
                d.keep_local,
            )
            .await
            .map_err(RetryError::transient)?;
            let db = state.db.lock().unwrap();
            db::update_destination_sync_status(&db, id, "ok", None)
                .map_err(RetryError::transient)?;
            Ok(format!(
                "Auto-sync destination {}: uploaded {} of {} events",
                id, uploaded, total
            ))
        },
    );
}

pub fn register_all(registry: &AutoSyncRegistry, state: &AppState) {
    let sources = {
        let db = state.db.lock().unwrap();
        db::list_sources(&db).unwrap_or_else(|e| {
            tracing::error!("Failed to load sources for auto-sync: {}", e);
            vec![]
        })
    };
    for source in &sources {
        register_source(registry, state, source);
    }

    let destinations = {
        let db = state.db.lock().unwrap();
        db::list_destinations(&db).unwrap_or_else(|e| {
            tracing::error!("Failed to load destinations for auto-sync: {}", e);
            vec![]
        })
    };
    for dest in &destinations {
        register_destination(registry, state, dest);
    }
}
