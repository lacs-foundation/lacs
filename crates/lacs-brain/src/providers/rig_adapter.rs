//! Adapter that bridges Rig's `CompletionModel` to our `LlmProvider` trait.
//!
//! This module wraps any Rig provider's completion model so it can be used
//! with our existing planning loop. The adapter converts between our internal
//! message types (`crate::provider::Message`, `ContentBlock`, etc.) and Rig's
//! `rig::completion::Message` / `AssistantContent` types.
//!
//! This gives us all Rig providers (Anthropic, Ollama/OpenAI-compatible, Gemini,
//! Groq, DeepSeek, Mistral, xAI, etc.) for free without hand-rolling HTTP clients.

use async_trait::async_trait;
use futures::StreamExt;
use rig::completion::{CompletionModel, CompletionRequest, ToolDefinition as RigToolDefinition};
use rig::message::{
    AssistantContent, Message as RigMessage, Text, ToolCall, ToolFunction, ToolResult,
    ToolResultContent, UserContent,
};
use rig::OneOrMany;

use crate::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, StopReason, ToolDefinition,
};

// ---------------------------------------------------------------------------
// RigCompletionAdapter
// ---------------------------------------------------------------------------

/// Wraps a Rig `CompletionModel` implementor and presents it as an `LlmProvider`.
///
/// `M` is the concrete Rig model type (e.g. `rig::providers::anthropic::CompletionModel`,
/// `rig::providers::ollama::CompletionModel`, etc.).
pub struct RigCompletionAdapter<M: CompletionModel> {
    model: M,
    /// Extra parameters merged verbatim into every `CompletionRequest`. Used to
    /// pass provider-specific knobs that Rig does not expose as first-class
    /// fields (e.g. `{"keep_alive": "30m"}` for Ollama).
    additional_params: Option<serde_json::Value>,
}

impl<M: CompletionModel> RigCompletionAdapter<M> {
    pub fn new(model: M) -> Self {
        Self {
            model,
            additional_params: None,
        }
    }

    pub fn with_additional_params(mut self, params: serde_json::Value) -> Self {
        self.additional_params = Some(params);
        self
    }
}

#[async_trait]
impl<M> LlmProvider for RigCompletionAdapter<M>
where
    M: CompletionModel + Send + Sync,
{
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<Completion, ProviderError> {
        let rig_messages = to_rig_messages(system, messages);
        let rig_tools = to_rig_tools(tools);

        let chat_history = OneOrMany::many(rig_messages).map_err(|_| {
            ProviderError::Request(
                "message conversion produced an empty chat history; \
                 at minimum a system message is expected"
                    .into(),
            )
        })?;

        let request = CompletionRequest {
            model: None,
            preamble: None, // system prompt is in chat_history as first message
            chat_history,
            documents: vec![],
            tools: rig_tools,
            temperature: None,
            max_tokens: Some(max_tokens as u64),
            tool_choice: None,
            additional_params: self.additional_params.clone(),
            output_schema: None,
        };

        // Use the streaming path so that thinking-mode models (e.g. Qwen3)
        // are handled correctly. Rig's non-streaming Ollama path drops the
        // `thinking` field entirely and fails with "No content provided" when
        // a model returns thinking content alongside an empty `content` field.
        // The streaming path accumulates reasoning + text + tool_calls into
        // `choice` correctly. We drain the stream here and read `choice` once
        // it is populated (at stream end). The `LlmProvider` interface stays
        // single-shot — callers see no difference.
        let mut stream = self.model.stream(request).await.map_err(map_rig_error)?;

        while let Some(item) = stream.next().await {
            item.map_err(map_rig_error)?;
        }

        from_rig_response(stream.choice)
    }
}

// ---------------------------------------------------------------------------
// Message conversion: our types → Rig types
// ---------------------------------------------------------------------------

/// Convert our system prompt + message history to Rig's message format.
fn to_rig_messages(system: &str, messages: &[Message]) -> Vec<RigMessage> {
    let mut result = Vec::with_capacity(messages.len() + 1);

    // System prompt as first message
    result.push(RigMessage::System {
        content: system.to_string(),
    });

    for msg in messages {
        match msg.role {
            crate::provider::Role::User => {
                // Check if all blocks are tool results
                let all_results = !msg.content.is_empty()
                    && msg
                        .content
                        .iter()
                        .all(|b| matches!(b, ContentBlock::ToolResult { .. }));

                if all_results {
                    let tool_results: Vec<UserContent> = msg
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                call_id,
                                content,
                                is_error,
                            } = b
                            {
                                let text = if *is_error {
                                    format!("[TOOL ERROR] {content}")
                                } else {
                                    content.clone()
                                };
                                Some(UserContent::ToolResult(ToolResult {
                                    id: tool_use_id.clone(),
                                    // OpenAI Responses API: call_id (call_xxx) is the
                                    // function-call match key for function_call_output.
                                    // Fall back to tool_use_id for providers without a
                                    // separate call ID (Anthropic, Ollama, Gemini).
                                    call_id: Some(
                                        call_id.clone().unwrap_or_else(|| tool_use_id.clone()),
                                    ),
                                    content: OneOrMany::one(ToolResultContent::text(&text)),
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();

                    match OneOrMany::many(tool_results) {
                        Ok(many) => {
                            result.push(RigMessage::User { content: many });
                        }
                        Err(_) => {
                            eprintln!(
                                "[lacs-brain] WARNING: tool-result user message produced \
                                 zero items after conversion; message dropped"
                            );
                        }
                    }
                } else {
                    let text = msg
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::Text { text } = b {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    result.push(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(&text)),
                    });
                }
            }
            crate::provider::Role::Assistant => {
                let mut assistant_content: Vec<AssistantContent> = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => {
                            assistant_content
                                .push(AssistantContent::Text(Text { text: text.clone() }));
                        }
                        ContentBlock::ToolUse {
                            id,
                            call_id,
                            name,
                            input,
                        } => {
                            let mut tc = ToolCall::new(
                                id.clone(),
                                ToolFunction::new(name.clone(), input.clone()),
                            );
                            // For the OpenAI Responses API, call_id (call_xxx) is required
                            // to match the function_call_output in the next turn. For
                            // providers without a separate call ID (Anthropic, Ollama,
                            // Gemini), fall back to id so the ToolResult.call_id still
                            // has a value.
                            tc.call_id = Some(call_id.clone().unwrap_or_else(|| id.clone()));
                            assistant_content.push(AssistantContent::ToolCall(tc));
                        }
                        ContentBlock::ToolResult { .. } => {
                            // Tool results don't appear in assistant messages
                        }
                    }
                }

                if !assistant_content.is_empty() {
                    match OneOrMany::many(assistant_content) {
                        Ok(content) => {
                            result.push(RigMessage::Assistant { id: None, content });
                        }
                        Err(_) => {
                            eprintln!(
                                "[lacs-brain] WARNING: assistant message produced \
                                 zero content items after conversion; message dropped"
                            );
                        }
                    }
                }
            }
        }
    }

    result
}

/// Convert our tool definitions to Rig's format.
fn to_rig_tools(tools: &[ToolDefinition]) -> Vec<RigToolDefinition> {
    tools
        .iter()
        .map(|t| RigToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.input_schema.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Response conversion: Rig types → our types
// ---------------------------------------------------------------------------

/// Convert a Rig completion response into our `Completion` type.
///
/// Per-turn diagnostics (block counts, text preview, tool-call details)
/// are emitted at TRACE level under the `lacs_brain::rig_adapter`
/// target. Turn them on with
/// `RUST_LOG=lacs_brain::rig_adapter=trace` when debugging planner
/// behaviour; normal runs stay silent.
fn from_rig_response(choice: OneOrMany<AssistantContent>) -> Result<Completion, ProviderError> {
    let mut content = Vec::new();
    let mut has_tool_calls = false;

    if tracing::enabled!(target: "lacs_brain::rig_adapter", tracing::Level::TRACE) {
        let mut counts = (0usize, 0usize, 0usize, 0usize);
        for item in choice.iter() {
            match item {
                AssistantContent::Text(_) => counts.0 += 1,
                AssistantContent::ToolCall(_) => counts.1 += 1,
                AssistantContent::Reasoning(_) => counts.2 += 1,
                AssistantContent::Image(_) => counts.3 += 1,
            }
        }
        tracing::trace!(
            target: "lacs_brain::rig_adapter",
            "response blocks: text={}, tool_call={}, reasoning={}, image={}",
            counts.0, counts.1, counts.2, counts.3
        );
    }

    for item in choice.iter() {
        match item {
            AssistantContent::Text(text) => {
                if !text.text.is_empty() {
                    tracing::trace!(
                        target: "lacs_brain::rig_adapter",
                        "text content ({} chars): {:?}",
                        text.text.len(),
                        text.text.chars().take(200).collect::<String>()
                    );
                    content.push(ContentBlock::Text {
                        text: text.text.clone(),
                    });
                }
            }
            AssistantContent::ToolCall(tc) => {
                has_tool_calls = true;
                tracing::trace!(
                    target: "lacs_brain::rig_adapter",
                    "tool_call: name={}, args={}",
                    tc.function.name, tc.function.arguments
                );
                content.push(ContentBlock::ToolUse {
                    // OpenAI Responses API dual-ID:
                    //   id      = fc_xxx  (response-item ID, echoed in the next input array)
                    //   call_id = call_xxx (function-call match key for function_call_output)
                    // Anthropic/Ollama/Gemini set only `id` and leave `call_id` as None.
                    id: tc.id.clone(),
                    call_id: tc.call_id.clone().filter(|s| !s.is_empty()),
                    name: tc.function.name.clone(),
                    input: tc.function.arguments.clone(),
                });
            }
            AssistantContent::Reasoning(_) => {
                // Expected from thinking models (qwen3, qwq, deepseek-r);
                // not surfaced unless explicitly traced.
                tracing::trace!(
                    target: "lacs_brain::rig_adapter",
                    "skipping Reasoning block"
                );
            }
            AssistantContent::Image(_) => {
                // Image blocks during planning mean something is wrong —
                // surface unconditionally.
                eprintln!(
                    "[lacs-brain] WARNING: skipping Image block \
                     (not supported by planning loop)"
                );
            }
        }
    }

    // If the model returned content but we ended up with nothing after filtering
    // out unsupported block types, that is an error — the planning loop cannot
    // proceed with an empty response.
    if content.is_empty() && choice.iter().next().is_some() {
        return Err(ProviderError::Parse(
            "model response contained only unsupported content types \
             (reasoning/image); no text or tool calls found"
                .into(),
        ));
    }

    // LIMITATION: Rig's CompletionResponse does not expose a stop reason, so we
    // cannot distinguish MaxTokens from EndTurn. We infer ToolUse when tool
    // calls are present and fall back to EndTurn otherwise. This means a
    // response that was truncated due to the token limit will be reported as
    // EndTurn, which causes the planning loop to return NoPlanProposed instead
    // of a more specific "output truncated" error. If Rig exposes stop reasons
    // in a future release, this inference should be replaced with the real value.
    let stop_reason = if has_tool_calls {
        StopReason::ToolUse
    } else {
        StopReason::EndTurn
    };

    Ok(Completion {
        content,
        stop_reason,
    })
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Strip URL query parameters that may contain API keys from error messages.
///
/// Redact common key-bearing query parameters from error messages to prevent
/// API key leakage in logs. Handles both first-position (`?key=`, `?api_key=`)
/// and subsequent-position (`&key=`, `&api_key=`) query params, and redacts
/// all occurrences in the string (not just the first).
fn sanitize_error_msg(msg: &str) -> String {
    // Patterns to find and redact (matched case-insensitively via ASCII lower).
    const KEY_PARAMS: &[&str] = &["?key=", "?api_key=", "&key=", "&api_key="];

    let lower = msg.to_lowercase();
    // Collect redaction ranges in reverse order so offsets stay valid.
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();

    for pattern in KEY_PARAMS {
        let mut search_from = 0;
        while let Some(rel) = lower[search_from..].find(pattern) {
            let start = search_from + rel;
            // Keep the `?` or `&` prefix, redact from `key=` to next whitespace/end.
            let param_start = start + 1; // skip the `?` or `&`
            let end = lower[start..]
                .find(|c: char| c.is_whitespace())
                .map(|s| start + s)
                .unwrap_or(msg.len());
            ranges.push(param_start..end);
            search_from = start + pattern.len();
        }
    }

    if ranges.is_empty() {
        return msg.to_string();
    }

    // Sort by start position, then apply in reverse to preserve offsets.
    ranges.sort_by_key(|r| r.start);
    ranges.dedup_by(|b, a| {
        if a.end >= b.start {
            a.end = a.end.max(b.end);
            true
        } else {
            false
        }
    });

    let mut result = msg.to_string();
    for range in ranges.into_iter().rev() {
        result.replace_range(range, "[REDACTED]");
    }
    result
}

fn map_rig_error(err: rig::completion::CompletionError) -> ProviderError {
    let msg = sanitize_error_msg(&err.to_string());

    // NOTE: This classification relies on string matching against Rig's error
    // messages, which is inherently fragile — a Rig version bump could change
    // the wording and break our categorisation. We accept this trade-off
    // because Rig does not expose structured error variants for HTTP status
    // codes. If Rig adds typed error variants in the future, prefer those.
    eprintln!("[lacs-brain] Rig completion error: {msg}");

    if msg.contains("401") || msg.to_lowercase().contains("auth") {
        ProviderError::Auth(msg)
    } else if msg.contains("429") || msg.to_lowercase().contains("rate") {
        ProviderError::RateLimit
    } else {
        ProviderError::Request(msg)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_rig_messages_includes_system_prompt() {
        let messages = vec![Message::user_text("hello")];
        let rig_msgs = to_rig_messages("You are a bot.", &messages);
        assert_eq!(rig_msgs.len(), 2);
        match &rig_msgs[0] {
            RigMessage::System { content } => assert_eq!(content, "You are a bot."),
            _ => panic!("expected System message"),
        }
    }

    #[test]
    fn to_rig_messages_converts_user_text() {
        let messages = vec![Message::user_text("hello")];
        let rig_msgs = to_rig_messages("sys", &messages);
        assert_eq!(rig_msgs.len(), 2);
        match &rig_msgs[1] {
            RigMessage::User { content } => {
                let first = content.first();
                match first {
                    UserContent::Text(t) => assert_eq!(t.text, "hello"),
                    _ => panic!("expected text"),
                }
            }
            _ => panic!("expected User message"),
        }
    }

    #[test]
    fn to_rig_messages_converts_tool_results() {
        let messages = vec![Message::tool_results(vec![
            crate::provider::ToolResultBlock {
                tool_use_id: "tu_1".into(),
                call_id: None,
                content: r#"{"ok":true}"#.into(),
                is_error: false,
            },
        ])];
        let rig_msgs = to_rig_messages("sys", &messages);
        assert_eq!(rig_msgs.len(), 2);
        match &rig_msgs[1] {
            RigMessage::User { content } => {
                let first = content.first();
                match first {
                    UserContent::ToolResult(tr) => {
                        assert_eq!(tr.id, "tu_1");
                        // call_id falls back to tool_use_id when None is provided.
                        assert_eq!(tr.call_id, Some("tu_1".into()));
                    }
                    _ => panic!("expected tool result, got {:?}", first),
                }
            }
            _ => panic!("expected User message"),
        }
    }

    #[test]
    fn to_rig_messages_tool_result_with_explicit_call_id() {
        // When call_id is explicitly set (OpenAI Responses API), it must be
        // forwarded as-is rather than falling back to tool_use_id.
        let messages = vec![Message::tool_results(vec![
            crate::provider::ToolResultBlock {
                tool_use_id: "fc_abc".into(),
                call_id: Some("call_xyz".into()),
                content: "result".into(),
                is_error: false,
            },
        ])];
        let rig_msgs = to_rig_messages("sys", &messages);
        match &rig_msgs[1] {
            RigMessage::User { content } => match content.first() {
                UserContent::ToolResult(tr) => {
                    assert_eq!(tr.id, "fc_abc");
                    assert_eq!(tr.call_id, Some("call_xyz".into()));
                }
                _ => panic!("expected tool result"),
            },
            _ => panic!("expected User message"),
        }
    }

    #[test]
    fn to_rig_messages_converts_assistant_tool_use() {
        let messages = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "tu_1".into(),
            call_id: None,
            name: "get_system_state".into(),
            input: serde_json::json!({}),
        }])];
        let rig_msgs = to_rig_messages("sys", &messages);
        assert_eq!(rig_msgs.len(), 2);
        match &rig_msgs[1] {
            RigMessage::Assistant { content, .. } => {
                let first = content.first();
                match first {
                    AssistantContent::ToolCall(tc) => {
                        assert_eq!(tc.function.name, "get_system_state");
                        // call_id falls back to id when None is provided.
                        assert_eq!(tc.call_id, Some("tu_1".into()));
                    }
                    _ => panic!("expected tool call"),
                }
            }
            _ => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn to_rig_messages_assistant_tool_use_with_explicit_call_id() {
        // OpenAI dual-ID: id=fc_xxx (response-item), call_id=call_xxx (match key).
        let messages = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "fc_abc".into(),
            call_id: Some("call_xyz".into()),
            name: "propose_plan".into(),
            input: serde_json::json!({}),
        }])];
        let rig_msgs = to_rig_messages("sys", &messages);
        match &rig_msgs[1] {
            RigMessage::Assistant { content, .. } => match content.first() {
                AssistantContent::ToolCall(tc) => {
                    assert_eq!(tc.id, "fc_abc");
                    assert_eq!(tc.call_id, Some("call_xyz".into()));
                }
                _ => panic!("expected tool call"),
            },
            _ => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn to_rig_tools_converts_definitions() {
        let tools = vec![ToolDefinition {
            name: "my_tool".into(),
            description: "does stuff".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let rig_tools = to_rig_tools(&tools);
        assert_eq!(rig_tools.len(), 1);
        assert_eq!(rig_tools[0].name, "my_tool");
        assert_eq!(rig_tools[0].description, "does stuff");
    }

    #[test]
    fn from_rig_response_text_only_returns_end_turn() {
        let choice = OneOrMany::one(AssistantContent::Text(Text {
            text: "Hello!".into(),
        }));
        let completion = from_rig_response(choice).unwrap();
        assert_eq!(completion.stop_reason, StopReason::EndTurn);
        assert_eq!(completion.content.len(), 1);
    }

    #[test]
    fn from_rig_response_tool_call_returns_tool_use() {
        let choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "tu_1".into(),
            ToolFunction::new("get_system_state".into(), serde_json::json!({})),
        )));
        let completion = from_rig_response(choice).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.content.len(), 1);
        if let ContentBlock::ToolUse { name, .. } = &completion.content[0] {
            assert_eq!(name, "get_system_state");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn from_rig_response_mixed_content() {
        let items = vec![
            AssistantContent::Text(Text {
                text: "Thinking...".into(),
            }),
            AssistantContent::ToolCall(ToolCall::new(
                "tu_1".into(),
                ToolFunction::new(
                    "propose_plan".into(),
                    serde_json::json!({"summary": "test"}),
                ),
            )),
        ];
        let choice = OneOrMany::many(items).unwrap();
        let completion = from_rig_response(choice).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.content.len(), 2);
    }

    #[test]
    fn sanitize_error_msg_strips_key_param() {
        let input = "request to https://api.example.com/v1?key=sk-secret123 failed";
        let result = sanitize_error_msg(input);
        assert_eq!(
            result,
            "request to https://api.example.com/v1?[REDACTED] failed"
        );
    }

    #[test]
    fn sanitize_error_msg_strips_api_key_param() {
        let input = "https://api.example.com/v1?api_key=sk-secret123";
        let result = sanitize_error_msg(input);
        assert_eq!(result, "https://api.example.com/v1?[REDACTED]");
    }

    #[test]
    fn sanitize_error_msg_no_key_unchanged() {
        let input = "connection refused: https://api.example.com/v1";
        let result = sanitize_error_msg(input);
        assert_eq!(result, input);
    }

    #[test]
    fn sanitize_error_msg_strips_ampersand_key_param() {
        // Subsequent-position `&key=` must also be redacted.
        let input = "https://api.example.com/v1?foo=bar&key=sk-secret123";
        let result = sanitize_error_msg(input);
        assert_eq!(result, "https://api.example.com/v1?foo=bar&[REDACTED]");
    }

    #[test]
    fn sanitize_error_msg_strips_ampersand_api_key_param() {
        // Subsequent-position `&api_key=` must also be redacted.
        let input = "https://api.example.com/v1?model=gpt-4&api_key=sk-secret123";
        let result = sanitize_error_msg(input);
        assert_eq!(
            result,
            "https://api.example.com/v1?model=gpt-4&[REDACTED]"
        );
    }

    #[test]
    fn sanitize_error_msg_strips_all_occurrences() {
        // Two different key-bearing URLs in the same error string — both must be redacted.
        let input =
            "first: https://api1.com?key=secret1 second: https://api2.com?api_key=secret2";
        let result = sanitize_error_msg(input);
        assert_eq!(
            result,
            "first: https://api1.com?[REDACTED] second: https://api2.com?[REDACTED]"
        );
    }

    /// Verify that the empty-content guard fires: if every block is filtered
    /// out (e.g. all Reasoning), `from_rig_response` returns a Parse error.
    ///
    /// We cannot construct `Reasoning` directly (non-exhaustive), so we
    /// exercise the guard by verifying the error message on a response that
    /// contains only empty text (which is also filtered out).
    #[test]
    fn from_rig_response_empty_text_only_returns_ok_empty() {
        // An empty text block is filtered, producing empty content.
        // The OneOrMany is non-empty, so the guard should fire.
        let choice = OneOrMany::one(AssistantContent::Text(Text { text: "".into() }));
        let err = from_rig_response(choice).unwrap_err();
        match err {
            ProviderError::Parse(msg) => {
                assert!(msg.contains("unsupported content types"));
            }
            other => panic!("expected Parse error, got {other:?}"),
        }
    }
}
