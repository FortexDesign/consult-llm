use super::opencode_db;
use super::stream::{
    ParsedStreamEvent, StreamEvents, first_non_empty_string, parse_json_line, tool_label,
    usage_event_from_keys,
};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities};
use super::{append_file_refs, prepare_cli_request, run_cli_executor_with_env};

pub struct OpenCodeCliExecutor {
    capabilities: LlmExecutorCapabilities,
    /// OpenCode provider prefix (e.g. "minimax", "copilot", "google")
    provider_prefix: String,
    reasoning_effort: Option<String>,
    env: std::collections::BTreeMap<String, String>,
}

impl OpenCodeCliExecutor {
    pub fn new(
        provider_prefix: String,
        reasoning_effort: Option<String>,
        env: std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            capabilities: LlmExecutorCapabilities {
                is_cli: true,
                supports_threads: true,
                supports_file_refs: true,
            },
            provider_prefix,
            reasoning_effort,
            env,
        }
    }
}

pub fn parse_opencode_line(line: &str) -> StreamEvents {
    use smallvec::smallvec;

    let Some(event) = parse_json_line(line) else {
        return smallvec![];
    };

    match event.get("type").and_then(|t| t.as_str()) {
        Some("step_start") => {
            let mut events: StreamEvents = smallvec![];
            if let Some(sid) = event.get("sessionID").and_then(|v| v.as_str()) {
                events.push(ParsedStreamEvent::SessionStarted {
                    id: sid.to_string(),
                });
            }
            events
        }
        Some("text") => {
            if let Some(part) = event.get("part")
                && let Some(text) = part.get("text").and_then(|t| t.as_str())
            {
                smallvec![ParsedStreamEvent::AssistantText {
                    text: text.to_string(),
                }]
            } else {
                smallvec![]
            }
        }
        Some("tool_use") => parse_tool_use(&event),
        Some("step_finish") => {
            if let Some(part) = event.get("part")
                && let Some(tokens) = part.get("tokens")
            {
                let mut usage = usage_event_from_keys(tokens, "input", "output");
                if let ParsedStreamEvent::Usage { cost, .. } = &mut usage {
                    *cost = part.get("cost").and_then(|v| v.as_f64());
                }
                smallvec![usage]
            } else {
                smallvec![]
            }
        }
        Some("error") => {
            if let Some(err) = event.get("error") {
                let msg = err
                    .get("data")
                    .and_then(|d| d.get("message"))
                    .and_then(|m| m.as_str())
                    .or_else(|| err.get("name").and_then(|n| n.as_str()))
                    .unwrap_or("unknown error");
                smallvec![ParsedStreamEvent::AssistantText {
                    text: format!("Error: {msg}"),
                }]
            } else {
                smallvec![]
            }
        }
        _ => smallvec![],
    }
}

fn parse_tool_use(event: &serde_json::Value) -> StreamEvents {
    use smallvec::smallvec;

    let Some(part) = event.get("part") else {
        return smallvec![];
    };
    if part.get("type").and_then(|t| t.as_str()) != Some("tool") {
        return smallvec![];
    }

    let Some(state) = part.get("state") else {
        return smallvec![];
    };
    let status = state.get("status").and_then(|s| s.as_str());
    let call_id = part
        .get("callID")
        .and_then(|id| id.as_str())
        .or_else(|| part.get("id").and_then(|id| id.as_str()))
        .unwrap_or("tool")
        .to_string();

    match status {
        Some("running") => smallvec![tool_started_event(part, state, call_id)],
        Some("completed") => smallvec![
            tool_started_event(part, state, call_id.clone()),
            ParsedStreamEvent::ToolFinished {
                call_id,
                success: true,
                error: None,
            }
        ],
        Some("error") => smallvec![
            tool_started_event(part, state, call_id.clone()),
            ParsedStreamEvent::ToolFinished {
                call_id,
                success: false,
                error: state.get("error").and_then(error_text),
            }
        ],
        _ => smallvec![],
    }
}

fn tool_started_event(
    part: &serde_json::Value,
    state: &serde_json::Value,
    call_id: String,
) -> ParsedStreamEvent {
    let name = part
        .get("tool")
        .and_then(|name| name.as_str())
        .unwrap_or("tool");
    ParsedStreamEvent::ToolStarted {
        call_id,
        label: tool_label(name, tool_detail(state).as_deref()),
    }
}

fn error_text(error: &serde_json::Value) -> Option<String> {
    error
        .as_str()
        .map(|error| error.to_string())
        .or_else(|| {
            error
                .get("message")
                .and_then(|message| message.as_str())
                .map(|message| message.to_string())
        })
        .or_else(|| {
            error
                .get("data")
                .and_then(|data| data.get("message"))
                .and_then(|message| message.as_str())
                .map(|message| message.to_string())
        })
        .or_else(|| Some(error.to_string()).filter(|error| error != "null"))
}

fn tool_detail(state: &serde_json::Value) -> Option<String> {
    state.get("input").and_then(|input| {
        first_non_empty_string(
            input,
            &[
                "filePath",
                "file_path",
                "path",
                "pattern",
                "command",
                "cmd",
                "url",
                "query",
                "description",
            ],
        )
    })
}

impl LlmExecutor for OpenCodeCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "opencode_cli"
    }

    fn reasoning_effort(&self, _model: &str) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    fn execute(&self, req: ExecutionRequest) -> anyhow::Result<ExecuteResult> {
        let prepared = prepare_cli_request(req, append_file_refs);
        let fps = prepared.file_paths.as_deref();
        let tid = prepared.thread_id.as_deref();
        let has_configured_db = self.env.contains_key("OPENCODE_DB");
        let mapped_db = tid.map(opencode_db::load).transpose()?.flatten();
        let fresh_db = if !has_configured_db && tid.is_none() {
            Some(opencode_db::new_db_path()?)
        } else {
            None
        };
        let selected_db = mapped_db.as_ref().or(fresh_db.as_ref());

        let opencode_model = if prepared
            .model
            .starts_with(&format!("{}/", self.provider_prefix))
        {
            // Model already includes the provider prefix
            // (e.g. "openrouter/xiaomi/mimo-v2.5-pro" with provider_prefix "openrouter").
            // Pass through as-is to avoid double-prefixing.
            prepared.model.to_string()
        } else {
            format!("{}/{}", self.provider_prefix, prepared.model)
        };

        let mut args: Vec<String> = vec![
            "run".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            opencode_model,
        ];

        if let Some(effort) = &self.reasoning_effort {
            args.push("--variant".to_string());
            args.push(effort.to_string());
        }

        if let Some(t) = tid {
            args.push("--session".to_string());
            args.push(t.to_string());
        }

        if let Some(fps) = fps
            && !fps.is_empty()
        {
            for fp in fps {
                args.push("--file".to_string());
                args.push(fp.display().to_string());
            }
        }

        let mut env = self.env.clone();
        if let Some(db_path) = selected_db {
            env.entry("OPENCODE_DB".to_string())
                .or_insert_with(|| db_path.display().to_string());
        }

        let mut parser = parse_opencode_line;
        let result = run_cli_executor_with_env(
            "opencode",
            &args,
            Some(&env),
            Some(&prepared.stdin_prompt),
            &prepared.prompt,
            &prepared.system_prompt,
            prepared.spool,
            &mut parser,
        )?;
        if let (Some(thread_id), Some(db_path)) = (&result.thread_id, selected_db) {
            opencode_db::save(thread_id, db_path.clone())?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executors::stream::StreamReducer;

    #[test]
    fn test_parse_opencode_line_step_start() {
        let events = parse_opencode_line(
            r#"{"type":"step_start","timestamp":1234,"sessionID":"ses_abc123","part":{"type":"step-start"}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "ses_abc123")
        );
    }

    #[test]
    fn test_parse_opencode_line_text() {
        let events = parse_opencode_line(
            r#"{"type":"text","sessionID":"ses_abc","part":{"type":"text","text":"Hello world"}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text == "Hello world")
        );
    }

    #[test]
    fn test_opencode_reasoning_effort_reports_variant() {
        let executor = OpenCodeCliExecutor::new(
            "openrouter".into(),
            Some("high".into()),
            std::collections::BTreeMap::new(),
        );
        assert_eq!(
            executor.reasoning_effort("openrouter/z-ai/glm-5.2"),
            Some("high")
        );
    }

    #[test]
    fn test_parse_opencode_line_raw_tool_completed() {
        let events = parse_opencode_line(
            r#"{"type":"tool_use","timestamp":1781248898147,"sessionID":"ses_1454ad458ffergZHQMFMnQGntV","part":{"type":"tool","tool":"read","callID":"call_fea4a3c91e4649298af9d400","state":{"status":"completed","input":{"filePath":"/Users/raine/code/consult-llm__worktrees/opencode-tool-logs/src/executors/opencode_cli.rs","limit":1},"output":"<content>1: use super::stream::{ParsedStreamEvent, StreamEvents, tool_label};</content>","metadata":{"preview":"use super::stream::{ParsedStreamEvent, StreamEvents, tool_label};"},"title":"src/executors/opencode_cli.rs","time":{"start":1781248898137,"end":1781248898145}},"id":"prt_ebab53454001IT9Pl6qWRqKB5E","sessionID":"ses_1454ad458ffergZHQMFMnQGntV","messageID":"msg_ebab52c51001lWvsVrd0dTFoUb"}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "call_fea4a3c91e4649298af9d400" && label == "read /Users/raine/code/consult-llm__worktrees/opencode-tool-logs/src/executors/opencode_cli.rs")
        );
        assert!(
            matches!(&events[1], ParsedStreamEvent::ToolFinished { call_id, success, error } if call_id == "call_fea4a3c91e4649298af9d400" && *success && error.is_none())
        );
    }

    #[test]
    fn test_parse_opencode_line_tool_started() {
        let events = parse_opencode_line(
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"bash","state":{"status":"running","input":{"command":"cargo test"},"time":{"start":123}}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "call_1" && label == "bash cargo test")
        );
    }

    #[test]
    fn test_parse_opencode_line_tool_finished_success() {
        let events = parse_opencode_line(
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"bash","state":{"status":"completed","input":{"command":"cargo test"},"output":"ok","title":"cargo test","metadata":{},"time":{"start":123,"end":456}}}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "call_1" && label == "bash cargo test")
        );
        assert!(
            matches!(&events[1], ParsedStreamEvent::ToolFinished { call_id, success, error } if call_id == "call_1" && *success && error.is_none())
        );
    }

    #[test]
    fn test_parse_opencode_line_tool_finished_error() {
        let events = parse_opencode_line(
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"bash","state":{"status":"error","input":{"command":"cargo test"},"error":"exit 1","time":{"start":123,"end":456}}}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "call_1" && label == "bash cargo test")
        );
        assert!(
            matches!(&events[1], ParsedStreamEvent::ToolFinished { call_id, success, error } if call_id == "call_1" && !*success && error.as_deref() == Some("exit 1"))
        );
    }

    #[test]
    fn test_parse_opencode_line_tool_finished_structured_error() {
        let events = parse_opencode_line(
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"bash","state":{"status":"error","input":{"command":"cargo test"},"error":{"data":{"message":"exit 1"}},"time":{"start":123,"end":456}}}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[1], ParsedStreamEvent::ToolFinished { call_id, success, error } if call_id == "call_1" && !*success && error.as_deref() == Some("exit 1"))
        );
    }

    #[test]
    fn test_parse_opencode_line_step_finish() {
        let events = parse_opencode_line(
            r#"{"type":"step_finish","sessionID":"ses_abc","part":{"type":"step-finish","reason":"stop","tokens":{"input":1000,"output":50,"reasoning":10}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ParsedStreamEvent::Usage {
                prompt_tokens: 1000,
                completion_tokens: 50,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_opencode_line_error() {
        let events = parse_opencode_line(
            r#"{"type":"error","sessionID":"ses_abc","error":{"name":"ProviderAuthError","data":{"message":"API key missing"}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text.contains("API key missing"))
        );
    }

    #[test]
    fn test_parse_opencode_line_empty() {
        assert!(parse_opencode_line("").is_empty());
        assert!(parse_opencode_line("  ").is_empty());
        assert!(parse_opencode_line("not json").is_empty());
    }

    #[test]
    fn test_reducer_full_sequence() {
        let mut reducer = StreamReducer::new(
            std::sync::Arc::new(std::sync::Mutex::new(
                consult_llm_core::monitoring::RunSpool::disabled(),
            )),
            None,
            None,
        );
        let lines = vec![
            r#"{"type":"step_start","sessionID":"ses_abc","part":{"type":"step-start"}}"#,
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"read","state":{"status":"running","input":{"filePath":"src/main.rs"},"time":{"start":123}}}}"#,
            r#"{"type":"tool_use","sessionID":"ses_abc","part":{"id":"prt_1","type":"tool","callID":"call_1","tool":"read","state":{"status":"completed","input":{"filePath":"src/main.rs"},"output":"fn main() {}","title":"src/main.rs","metadata":{},"time":{"start":123,"end":456}}}}"#,
            r#"{"type":"text","sessionID":"ses_abc","part":{"type":"text","text":"4"}}"#,
            r#"{"type":"step_finish","sessionID":"ses_abc","part":{"type":"step-finish","reason":"stop","tokens":{"input":15000,"output":1,"reasoning":0}}}"#,
        ];
        for line in &lines {
            reducer.process(parse_opencode_line(line));
        }
        assert_eq!(reducer.thread_id, Some("ses_abc".to_string()));
        assert_eq!(reducer.response, "4");
        assert!(reducer.usage.is_some());
        let usage = reducer.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 15000);
        assert_eq!(usage.completion_tokens, 1);
    }
}
