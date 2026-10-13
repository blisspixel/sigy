//! Synthetic published metadata, never a worker, provider, or media invocation.
use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::json;
use sigy_service::{
    monitor::MonitorSpec,
    sources::{HttpSource, NetworkScope},
    storage::Store,
    translation::TranslationProfile,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn populate(library: &Path) -> Result {
    let catalog = library.join("catalog.sqlite3");
    let mut store = Store::open(&catalog)?;
    store.register_source(
        "radio:v1",
        &HttpSource::new(
            "Synthetic radio",
            "https://example.com/radio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    drop(store);
    let mut connection = Connection::open(&catalog)?;
    // Fixtures model completed publication. Preserve all trigger definitions and
    // restore them before the ordinary Store::open integrity audits run.
    let triggers = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger'")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let transaction = connection.transaction()?;
    transaction.pragma_update(None, "defer_foreign_keys", true)?;
    for (name, _) in &triggers {
        transaction.execute_batch(&format!("DROP TRIGGER \"{}\"", name.replace('"', "\"\"")))?;
    }
    transaction.execute_batch(include_str!("published.sql"))?;
    insert_translation_profile(&transaction, library)?;
    insert_monitor(&transaction)?;
    for (_, sql) in triggers {
        transaction.execute_batch(&sql)?;
    }
    transaction.commit()?;
    drop(connection);
    // This is deliberately the production reopen path: fixtures must satisfy
    // foreign keys, journal replay, transcript coverage, and catalog audits.
    drop(Store::open(&catalog).map_err(|error| format!("reopen synthetic catalog: {error:?}"))?);
    Ok(())
}

fn insert_monitor(connection: &Connection) -> Result {
    let spec: MonitorSpec = serde_json::from_value(json!({
        "name":"World fair", "goal":"Inspect synthetic multilingual evidence",
        "terms":[{"language":"es","text":"feria"},{"language":"en","text":"fair"}],
        "sources":["radio:v1"],"candidate_sources":[],"schedules":[],
        "daily_audio_seconds":60,"total_audio_seconds":120,
        "recognition_profile":null,"translation_profile":null
    }))?;
    spec.validate()?;
    connection.execute("INSERT INTO monitors VALUES ('fair', 25)", [])?;
    connection.execute(
        "INSERT INTO monitor_versions VALUES ('fair', 1, ?1, ?2, 25)",
        params![serde_json::to_string(&spec)?, spec.digest()?],
    )?;
    Ok(())
}

fn insert_translation_profile(connection: &Connection, library: &Path) -> Result {
    let mut profile = TranslationProfile {
        id: "mt".into(),
        engine: "llama-cpp-completion-v1".into(),
        template: "hy-mt2-plain-v1".into(),
        runtime_dir: library.join("unused-runtime").display().to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "4".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: library.join("unused-model.gguf").display().to_string(),
        model_sha256: "5".repeat(64),
        model_bytes: 1000,
        languages: "es".into(),
        threads: 2,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    profile.validate()?;
    connection.execute("INSERT INTO translation_profiles VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,5)",
        params![profile.id,profile.engine,profile.template,profile.runtime_dir,profile.executable,profile.runtime_sha256,
            profile.runtime_files,i64::try_from(profile.runtime_bytes)?,profile.model_path,profile.model_sha256,i64::try_from(profile.model_bytes)?,
            profile.languages,profile.threads,i64::try_from(profile.memory_bytes)?,i64::try_from(profile.cue_deadline_ms)?,profile.profile_sha256])?;
    connection.execute(
        "UPDATE translation_jobs SET profile_sha256 = ?1",
        [&profile.profile_sha256],
    )?;
    connection.execute(
        "UPDATE translations SET profile_sha256 = ?1",
        [&profile.profile_sha256],
    )?;
    Ok(())
}
