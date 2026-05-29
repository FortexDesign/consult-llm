use crate::config;
use crate::config::types::Backend;
use crate::models::{PROVIDERS, Provider};

fn backend_name_for_model(cfg: &config::Config, model: &str) -> &'static str {
    Provider::from_model(model)
        .map(|p| cfg.backend_for(p).as_str())
        .or_else(|| {
            Provider::from_cursor_model(model).and_then(|p| {
                (cfg.backend_for(p) == &Backend::CursorCli).then_some(Backend::CursorCli.as_str())
            })
        })
        .unwrap_or("unknown")
}

pub fn run() -> anyhow::Result<()> {
    let (cfg, registry) = config::init_config().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("Selectors:");
    for spec in PROVIDERS {
        let Ok(resolved) = registry.resolve_model(Some(spec.id)) else {
            continue;
        };
        let backend = cfg.backend_for(spec.provider).as_str();
        println!("  {:<10} -> {resolved} ({backend})", spec.id);
    }
    println!("\nAllowed models:");
    for m in &cfg.allowed_models {
        let backend = backend_name_for_model(&cfg, m);
        println!("  {m} ({backend})");
    }
    println!("\nDefault models (ordered; duplicates are intentional):");
    if cfg.default_models.is_empty() {
        println!("  (none)");
    } else {
        for m in &cfg.default_models {
            println!("  {m}");
        }
    }
    println!("\nDefault -m args:");
    if cfg.default_models.is_empty() {
        println!("  (none)");
    } else {
        println!(
            "  {}",
            cfg.default_models
                .iter()
                .map(|m| format!("-m {m}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(())
}
