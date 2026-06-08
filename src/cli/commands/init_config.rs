const TEMPLATE: &str = r#"# consult-llm user config
# See: https://github.com/raine/consult-llm#configuration

# default_model: gemini
# default_models: [gemini, openai, openai]
# allowed_models: [gemini, openai]
# extra_models: []

# gemini:
#   backend: gemini-cli
# openai:
#   backend: codex-cli
#   reasoning_effort: high
# opencode:
#   default_provider: copilot

# CLI backend profiles
# cli_profiles:
#   claude:
#     command: claude
#     args: ["-p"]
#     env: {}
#     interface: stream-json
#     prompt: stdin
#     headless: true
# anthropic:
#   backend: claude-cli
#   cli_profile: claude
"#;

pub fn run() -> anyhow::Result<()> {
    let path = crate::paths::user_config_file().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let legacy = crate::paths::legacy_config_dir().map(|d| d.join("config.yaml"));

    if path.exists() {
        println!("{} already exists", path.display());
        return Ok(());
    }
    if let Some(l) = legacy.filter(|p| p.exists()) {
        println!(
            "Legacy config already exists at {}. Remove or migrate it first.",
            l.display()
        );
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, TEMPLATE)?;
    println!("wrote {}", path.display());
    Ok(())
}
