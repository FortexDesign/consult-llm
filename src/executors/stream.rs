use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use consult_llm_core::monitoring::{ProgressStage, RunSpool};
use smallvec::SmallVec;

use super::types::Usage;
pub use consult_llm_core::stream_events::ParsedStreamEvent;

/// Most parsed lines produce 0-2 events; SmallVec avoids heap allocation
/// for the common case.
pub type StreamEvents = SmallVec<[ParsedStreamEvent; 2]>;

/// Trim a CLI JSON line and parse it, ignoring empty lines and malformed JSON.
pub fn parse_json_line(line: &str) -> Option<serde_json::Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// Build a usage event from prompt and completion token fields on a JSON object.
pub fn usage_event_from_keys(
    value: &serde_json::Value,
    prompt_key: &str,
    completion_key: &str,
) -> ParsedStreamEvent {
    ParsedStreamEvent::Usage {
        prompt_tokens: value.get(prompt_key).and_then(|v| v.as_u64()).unwrap_or(0),
        completion_tokens: value
            .get(completion_key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    }
}

/// Return the first non-empty string value for any of the given keys.
pub fn first_non_empty_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(key).and_then(|v| v.as_str())
            && !text.is_empty()
        {
            return Some(text.to_string());
        }
    }
    None
}

/// Format a tool label with an optional detail (file path, pattern, etc.)
/// e.g. ("read", Some("src/main.rs")) → "read src/main.rs"
pub fn tool_label(name: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => format!("{name} {d}"),
        None => name.to_string(),
    }
}

/// Accumulates stream events into a final result and forwards them to the spool.
pub struct StreamReducer {
    pub thread_id: Option<String>,
    pub response: String,
    pub usage: Option<Usage>,
    spool: Arc<Mutex<RunSpool>>,
    active_tools: HashMap<String, String>,
}

impl StreamReducer {
    pub fn new(
        spool: Arc<Mutex<RunSpool>>,
        prompt: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Self {
        {
            let mut s = spool.lock().unwrap();
            if let Some(text) = system_prompt {
                s.stream_event(ParsedStreamEvent::SystemPrompt {
                    text: text.to_string(),
                });
            }
            if let Some(text) = prompt {
                s.stream_event(ParsedStreamEvent::Prompt {
                    text: text.to_string(),
                });
            }
        }
        Self {
            thread_id: None,
            response: String::with_capacity(4096),
            usage: None,
            spool,
            active_tools: HashMap::new(),
        }
    }

    pub fn process(&mut self, events: StreamEvents) {
        let mut s = self.spool.lock().unwrap();
        for event in events {
            match event.clone() {
                ParsedStreamEvent::SessionStarted { id } => {
                    self.thread_id = Some(id.clone());
                    s.resolve_thread_id(id);
                }
                ParsedStreamEvent::Thinking { .. } => {
                    s.set_stage(ProgressStage::Thinking);
                }
                ParsedStreamEvent::AssistantText { text } => {
                    self.response.push_str(&text);
                    s.set_stage(ProgressStage::Responding);
                }
                ParsedStreamEvent::ToolStarted {
                    ref call_id,
                    ref label,
                } => {
                    if !self.active_tools.contains_key(call_id) {
                        s.set_stage(ProgressStage::ToolUse {
                            tool: label.clone(),
                        });
                    }
                    self.active_tools.insert(call_id.clone(), label.clone());
                }
                ParsedStreamEvent::ToolFinished {
                    ref call_id,
                    success,
                    ..
                } => {
                    if let Some(label) = self.active_tools.remove(call_id) {
                        s.set_stage(ProgressStage::ToolResult {
                            tool: label,
                            success,
                        });
                    }
                }
                ParsedStreamEvent::Prompt { .. }
                | ParsedStreamEvent::SystemPrompt { .. }
                | ParsedStreamEvent::FilesContext { .. } => {}
                ParsedStreamEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                } => {
                    self.usage = Some(Usage {
                        prompt_tokens,
                        completion_tokens,
                    });
                }
            }
            s.stream_event(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consult_llm_core::monitoring::RunSpool;
    use smallvec::smallvec;

    fn reducer() -> StreamReducer {
        StreamReducer::new(Arc::new(Mutex::new(RunSpool::disabled())), None, None)
    }

    #[test]
    fn complete_stream_session_text_usage() {
        // Happy path: SessionStarted resolves thread_id, AssistantText chunks
        // accumulate, Usage is captured.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::SessionStarted {
            id: "api_thread_xyz".into(),
        }]);
        r.process(smallvec![
            ParsedStreamEvent::AssistantText {
                text: "Hello ".into()
            },
            ParsedStreamEvent::AssistantText {
                text: "world".into()
            },
        ]);
        r.process(smallvec![ParsedStreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
        }]);
        assert_eq!(r.thread_id.as_deref(), Some("api_thread_xyz"));
        assert_eq!(r.response, "Hello world");
        let u = r.usage.expect("usage captured");
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 5);
    }

    #[test]
    fn heartbeat_only_chunks_are_noop() {
        // Empty event batches (e.g. SSE heartbeats that produced no parsed
        // events) must not change reducer state.
        let mut r = reducer();
        r.process(smallvec![]);
        r.process(smallvec![]);
        assert!(r.thread_id.is_none());
        assert!(r.response.is_empty());
        assert!(r.usage.is_none());
    }

    #[test]
    fn usage_event_captured_standalone() {
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
        }]);
        let u = r.usage.expect("usage present");
        assert_eq!(u.prompt_tokens, 1);
        assert_eq!(u.completion_tokens, 2);
        assert!(r.response.is_empty());
        assert!(r.thread_id.is_none());
    }

    #[test]
    fn assistant_text_before_session_started_leaves_thread_id_unset() {
        // Failure-path / out-of-order case: a backend that streams text
        // before announcing its session ID still has its text accumulated,
        // but thread_id stays None until SessionStarted arrives.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::AssistantText {
            text: "leak".into(),
        }]);
        assert!(r.thread_id.is_none());
        assert_eq!(r.response, "leak");
        // Late SessionStarted still resolves.
        r.process(smallvec![ParsedStreamEvent::SessionStarted {
            id: "api_late".into(),
        }]);
        assert_eq!(r.thread_id.as_deref(), Some("api_late"));
    }

    #[test]
    fn duplicate_tool_started_does_not_prevent_finish() {
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::ToolStarted {
            call_id: "c1".into(),
            label: "read a".into(),
        }]);
        r.process(smallvec![ParsedStreamEvent::ToolStarted {
            call_id: "c1".into(),
            label: "read a".into(),
        }]);
        r.process(smallvec![ParsedStreamEvent::ToolFinished {
            call_id: "c1".into(),
            success: true,
            error: None,
        }]);
        assert!(r.response.is_empty());
        assert!(r.thread_id.is_none());
    }

    #[test]
    fn parse_json_line_ignores_empty_and_malformed() {
        assert!(parse_json_line("").is_none());
        assert!(parse_json_line("  ").is_none());
        assert!(parse_json_line("not json").is_none());
    }

    #[test]
    fn parse_json_line_parses_valid_json() {
        let value = parse_json_line(r#"{"type":"init"}"#).expect("parsed");
        assert_eq!(value.get("type").and_then(|t| t.as_str()), Some("init"));
    }

    #[test]
    fn usage_event_from_keys_reads_token_fields() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"input_tokens":10,"output_tokens":5}"#).unwrap();
        assert!(matches!(
            usage_event_from_keys(&value, "input_tokens", "output_tokens"),
            ParsedStreamEvent::Usage {
                prompt_tokens: 10,
                completion_tokens: 5
            }
        ));
    }

    #[test]
    fn first_non_empty_string_skips_empty_values() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"path":"","file_path":"src/main.rs"}"#).unwrap();
        assert_eq!(
            first_non_empty_string(&value, &["path", "file_path"]).as_deref(),
            Some("src/main.rs")
        );
    }

    #[test]
    fn tool_lifecycle_drops_unmatched_finish() {
        // ToolFinished without a prior ToolStarted is silently ignored —
        // pin this so refactors can't turn it into a panic.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::ToolFinished {
            call_id: "missing".into(),
            success: false,
            error: None,
        }]);
        // No assertion target other than "did not panic"; reducer state
        // remains pristine.
        assert!(r.response.is_empty());
        assert!(r.thread_id.is_none());
    }
}
