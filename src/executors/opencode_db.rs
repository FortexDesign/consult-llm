use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Mapping {
    thread_id: String,
    db_path: PathBuf,
}

fn root_dir() -> PathBuf {
    consult_llm_core::paths::state_home().join("consult-llm/opencode-db")
}

fn mappings_dir() -> PathBuf {
    root_dir().join("threads")
}

fn db_dir() -> PathBuf {
    root_dir().join("db")
}

fn mapping_path(thread_id: &str) -> PathBuf {
    mappings_dir().join(format!("{thread_id}.json"))
}

pub fn new_db_path() -> anyhow::Result<PathBuf> {
    let dir = db_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{}.db", uuid::Uuid::new_v4().simple())))
}

pub fn load(thread_id: &str) -> anyhow::Result<Option<PathBuf>> {
    let path = mapping_path(thread_id);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let mapping: Mapping = serde_json::from_str(&data)?;
    Ok(Some(mapping.db_path))
}

pub fn save(thread_id: &str, db_path: PathBuf) -> anyhow::Result<()> {
    let dir = mappings_dir();
    std::fs::create_dir_all(&dir)?;
    let path = mapping_path(thread_id);
    let tmp = tempfile::NamedTempFile::new_in(&dir)?;
    let mapping = Mapping {
        thread_id: thread_id.to_string(),
        db_path,
    };
    serde_json::to_writer(&tmp, &mapping)?;
    tmp.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_mapping() {
        let _guard = crate::test_util::XDG_STATE_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }

        let db_path = new_db_path().unwrap();
        assert!(db_path.starts_with(tmp.path()));
        assert_eq!(db_path.extension().and_then(|ext| ext.to_str()), Some("db"));

        save("ses_test", db_path.clone()).unwrap();
        assert_eq!(load("ses_test").unwrap(), Some(db_path));
        assert_eq!(load("ses_missing").unwrap(), None);

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }
}
