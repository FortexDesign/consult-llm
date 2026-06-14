use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub fn process_files(files: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let cwd = std::env::current_dir()?;
    let resolved: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let p = PathBuf::from(f);
            if p.is_absolute() { p } else { cwd.join(f) }
        })
        .collect();

    let missing: Vec<&str> = files
        .iter()
        .zip(&resolved)
        .filter(|(_, r)| !r.exists())
        .map(|(orig, _)| orig.as_str())
        .collect();

    if !missing.is_empty() {
        anyhow::bail!("Files not found: {}", missing.join(", "));
    }

    let mut result = Vec::new();
    for (orig, resolved_path) in files.iter().zip(&resolved) {
        let content = fs::read_to_string(resolved_path)?;
        result.push((orig.clone(), content));
    }
    Ok(result)
}

pub(crate) fn cleanup_expired_json_files(dir: &Path, ttl_days: u64) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let cutoff = SystemTime::now() - Duration::from_secs(ttl_days * 86400);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && let Ok(meta) = entry.metadata()
            && let Ok(modified) = meta.modified()
            && modified < cutoff
        {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}
