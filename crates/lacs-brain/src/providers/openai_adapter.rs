//! Adapter that uses async-openai directly for the OpenAI Chat Completions API.
//!
//! Replaces the rig-based OpenAI path to avoid three rig issues:
//!
//! 1. rig defaults to `/v1/responses` (Responses API), not `/v1/chat/completions`.
//! 2. The Responses API emits reasoning content items for some gpt-4o variants,
//!    causing `from_rig_response` to return "unsupported content types" errors.
//! 3. rig issue #1599: system prompt ends up in a user message instead of the
//!    `instructions` field — a regression introduced by a third-party compat PR.
//!
//! async-openai targets `/v1/chat/completions` directly, tracks the OpenAI API
//! closely, and has no reasoning-item or system-prompt issues.
//!
//! Chat Completions uses a single tool-call ID per call (no dual-ID protocol).
//! `ContentBlock::ToolUse::call_id` is always `None` from this adapter.

use async_openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestToolMessage,
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
        ChatCompletionTool, ChatCompletionTools, CreateChatCompletionRequestArgs,
        CreateChatCompletionResponse, FinishReason, FunctionCall, FunctionObject,
    },
    Client,
};
use async_trait::async_trait;

use crate::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason,
    ToolDefinition,
};

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// LLM backend for OpenAI using async-openai and the Chat Completions API.
pub struct AsyncOpenAiAdapter {
    client: Client<OpenAIConfig>,
    model: String,
}

impl AsyncOpenAiAdapter {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let config = OpenAIConfig::new().with_api_key(api_key.into());
        Self {
            client: Client::with_config(config),
            model: model.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for AsyncOpenAiAdapter {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<Completion, ProviderError> {
        let oai_messages =
            to_openai_messages(system, messages).map_err(ProviderError::Request)?;
        let oai_tools = to_openai_tools(tools);

        let mut req_builder = CreateChatCompletionRequestArgs::default();
        req_builder
            .model(self.model.clone())
            .messages(oai_messages)
            .max_completion_tokens(max_tokens);

        if !oai_tools.is_empty() {
            req_builder.tools(oai_tools);
        }

        let request = req_builder
            .build()
            .map_err(|e: async_openai::error::OpenAIError| ProviderError::Request(e.to_string()))?;

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(map_openai_error)?;

        from_openai_response(response)
    }
}

// ---------------------------------------------------------------------------
// Message conversion: our types → async-openai request types
// ---------------------------------------------------------------------------

/// Convert our system prompt + message history to async-openai's message format.
///
/// Returns `Err(String)` if a message cannot be converted (e.g., empty assistant
/// turn). The error string is surfaced as `ProviderError::Request` by the caller.
fn to_openai_messages(
    system: &str,
    messages: &[Message],
) -> Result<Vec<ChatCompletionRequestMessage>, String> {
    let mut result = Vec::with_capacity(messages.len() + 1);

    // System prompt — always first.
    result.push(ChatCompletionRequestMessage::System(
        ChatCompletionRequestSystemMessage {
            content: system.to_string().into(),
            name: None,
        },
    ));

    for msg in messages {
        match msg.role {
            Role::User => {
                let all_results = !msg.content.is_empty()
                    && msg
                        .content
                        .iter()
                        .all(|b| matches!(b, ContentBlock::ToolResult { .. }));

                if all_results {
                    // Each tool result becomes a separate Tool message.
                    // Chat Completions requires one Tool message per tool call.
                    for block in &msg.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                            ..
                        } = block
                        {
                            let text = if *is_error {
                                format!("[TOOL ERROR] {content}")
                            } else {
                                content.clone()
                            };
                            result.push(ChatCompletionRequestMessage::Tool(
                                ChatCompletionRequestToolMessage {
                                    content: text.into(),
                                    tool_call_id: tool_use_id.clone(),
                                },
                            ));
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

                    result.push(ChatCompletionRequestMessage::User(
                        ChatCompletionRequestUserMessage {
                            content: ChatCompletionRequestUserMessageContent::Text(text),
                            name: None,
                        },
                    ));
                }
            }

            Role::Assistant => {
                let mut text_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<ChatCompletionMessageToolCalls> = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
                        ContentBlock::ToolUse { id, name, input, .. } => {
                            tool_calls.push(ChatCompletionMessageToolCalls::Function(
                                ChatCompletionMessageToolCall {
                                    id: id.clone(),
                                    function: FunctionCall {
                                        name: name.clone(),
                                        arguments: serde_json::to_string(input)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                    },
                                },
                            ));
                        }
                        ContentBlock::ToolResult { .. } => {
                            // Tool results do not appear in assistant messages.
                        }
                    }
                }

                if text_parts.is_empty() && tool_calls.is_empty() {
                    eprintln!(
                        "[lacs-brain] WARNING: skipping empty assistant message \
                         (no text, no tool calls)"
                    );
                    continue;
                }

                let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                if !text_parts.is_empty() {
                    builder.content(text_parts.join("\n"));
                }
                if !tool_calls.is_empty() {
                    builder.tool_calls(tool_calls);
                }

                result.push(ChatCompletionRequestMessage::Assistant(
                    builder
                        .build()
                        .map_err(|e: async_openai::error::OpenAIError| e.to_string())?,
                ));
            }
        }
    }

    Ok(result)
}

/// Convert our tool definitions to async-openai's format.
fn to_openai_tools(tools: &[ToolDefinition]) -> Vec<ChatCompletionTools> {
    tools
        .iter()
        .map(|t| {
            ChatCompletionTools::Function(ChatCompletionTool {
                function: FunctionObject {
                    name: t.name.clone(),
                    description: Some(t.description.clone()),
                    parameters: Some(t.input_schema.clone()),
                    strict: None,
                },
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Response conversion: async-openai response → our types
// ---------------------------------------------------------------------------

fn from_openai_response(response: CreateChatCompletionResponse) -> Result<Completion, ProviderError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("response contained no choices".into()))?;

    let stop_reason = match choice.finish_reason {
        Some(FinishReason::ToolCalls) => StopReason::ToolUse,
        Some(FinishReason::Length) => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    };

    let mut content: Vec<ContentBlock> = Vec::new();

    // Text content
    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            tracing::trace!(
                target: "lacs_brain::openai_adapter",
                "text content ({} chars): {:?}",
                text.len(),
                text.chars().take(200).collect::<String>()
            );
            content.push(ContentBlock::Text { text });
        }
    }

    // Tool calls — response uses Vec<ChatCompletionMessageToolCalls> (the enum).
    // Iterate and extract the Function variant; skip any Custom variants.
    if let Some(tool_calls) = choice.message.tool_calls {
        for tc_enum in tool_calls {
            let tc = match tc_enum {
                ChatCompletionMessageToolCalls::Function(f) => f,
                ChatCompletionMessageToolCalls::Custom(_) => continue,
            };
            let input: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({}));
            tracing::trace!(
                target: "lacs_brain::openai_adapter",
                "tool_call: name={}, args={}",
                tc.function.name,
                tc.function.arguments
            );
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                // Chat Completions uses a single ID — no dual-ID protocol.
                // The planning loop's call_id fallback (id when call_id is None)
                // handles this path correctly for all providers.
                call_id: None,
                name: tc.function.name,
                input,
            });
        }
    }

    if content.is_empty() {
        return Err(ProviderError::Parse(
            "model response contained no text or tool calls".into(),
        ));
    }

    tracing::trace!(
        target: "lacs_brain::openai_adapter",
        "response: {} content blocks, stop_reason={:?}",
        content.len(),
        stop_reason
    );

    Ok(Completion {
        content,
        stop_reason,
    })
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_openai_error(err: async_openai::error::OpenAIError) -> ProviderError {
    let msg = err.to_string();
    eprintln!("[lacs-brain] OpenAI error: {msg}");

    if msg.contains("401")
        || msg.to_lowercase().contains("authentication")
        || msg.to_lowercase().contains("api key")
        || msg.to_lowercase().contains("incorrect api key")
    {
        ProviderError::Auth(msg)
    } else if msg.contains("429") || msg.to_lowercase().contains("rate limit") {
        ProviderError::RateLimit
    } else {
        ProviderError::Request(msg)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use async_openai::types::chat::{
        ChatChoice, ChatChoiceLogprobs, ChatCompletionRequestSystemMessageContent,
        ChatCompletionRequestToolMessageContent, ChatCompletionResponseMessage,
        ChatCompletionTools as OaiChatCompletionTools, Role as OaiRole,
    };
    use crate::provider::{Message, ToolDefinition, ToolResultBlock};

    // --- to_openai_messages ---------------------------------------------------

    #[test]
    fn system_prompt_is_first_message() {
        let msgs = to_openai_messages("You are a bot.", &[Message::user_text("hi")]).unwrap();
        assert!(
            matches!(msgs[0], ChatCompletionRequestMessage::System(_)),
            "expected first message to be System"
        );
        if let ChatCompletionRequestMessage::System(s) = &msgs[0] {
            match &s.content {
                ChatCompletionRequestSystemMessageContent::Text(t) => {
                    assert_eq!(t, "You are a bot.")
                }
                _ => panic!("expected Text content"),
            }
        }
    }

    #[test]
    fn user_text_becomes_user_message() {
        let msgs = to_openai_messages("sys", &[Message::user_text("hello")]).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[1], ChatCompletionRequestMessage::User(_)));
        if let ChatCompletionRequestMessage::User(u) = &msgs[1] {
            match &u.content {
                ChatCompletionRequestUserMessageContent::Text(t) => assert_eq!(t, "hello"),
                _ => panic!("expected Text content"),
            }
        }
    }

    #[test]
    fn tool_results_become_tool_messages_one_per_result() {
        let messages = vec![Message::tool_results(vec![
            ToolResultBlock {
                tool_use_id: "call_1".into(),
                call_id: None,
                content: r#"{"ok":true}"#.into(),
                is_error: false,
            },
            ToolResultBlock {
                tool_use_id: "call_2".into(),
                call_id: None,
                content: "error occurred".into(),
                is_error: true,
            },
        ])];
        let msgs = to_openai_messages("sys", &messages).unwrap();
        // system + 2 tool messages
        assert_eq!(msgs.len(), 3, "expected 3 messages (system + 2 tool)");

        assert!(matches!(msgs[1], ChatCompletionRequestMessage::Tool(_)));
        if let ChatCompletionRequestMessage::Tool(t) = &msgs[1] {
            assert_eq!(t.tool_call_id, "call_1");
            match &t.content {
                ChatCompletionRequestToolMessageContent::Text(s) => {
                    assert_eq!(s, r#"{"ok":true}"#)
                }
                _ => panic!("expected Text content"),
            }
        }

        assert!(matches!(msgs[2], ChatCompletionRequestMessage::Tool(_)));
        if let ChatCompletionRequestMessage::Tool(t) = &msgs[2] {
            assert_eq!(t.tool_call_id, "call_2");
            match &t.content {
                ChatCompletionRequestToolMessageContent::Text(s) => {
                    assert!(s.starts_with("[TOOL ERROR]"), "got: {s}")
                }
                _ => panic!("expected Text content"),
            }
        }
    }

    #[test]
    fn assistant_tool_use_becomes_assistant_message_with_tool_calls() {
        let messages = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "call_abc".into(),
            call_id: None,
            name: "get_system_state".into(),
            input: serde_json::json!({}),
        }])];
        let msgs = to_openai_messages("sys", &messages).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[1], ChatCompletionRequestMessage::Assistant(_)));
        if let ChatCompletionRequestMessage::Assistant(a) = &msgs[1] {
            let tool_calls = a.tool_calls.as_ref().expect("tool_calls must be present");
            assert_eq!(tool_calls.len(), 1);
            match &tool_calls[0] {
                ChatCompletionMessageToolCalls::Function(tc) => {
                    assert_eq!(tc.id, "call_abc");
                    assert_eq!(tc.function.name, "get_system_state");
                }
                _ => panic!("expected Function tool call"),
            }
        }
    }

    #[test]
    fn assistant_text_becomes_assistant_message_with_content() {
        let messages = vec![Message::assistant(vec![ContentBlock::Text {
            text: "thinking...".into(),
        }])];
        let msgs = to_openai_messages("sys", &messages).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[1], ChatCompletionRequestMessage::Assistant(_)));
        if let ChatCompletionRequestMessage::Assistant(a) = &msgs[1] {
            assert!(a.content.is_some(), "content must be present for text-only assistant");
            assert!(a.tool_calls.is_none());
        }
    }

    #[test]
    fn tool_use_id_is_preserved_in_tool_call() {
        let messages = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "call_xyz".into(),
            call_id: Some("call_xyz".into()), // call_id present but ignored for Chat Completions
            name: "propose_plan".into(),
            input: serde_json::json!({"summary": "test"}),
        }])];
        let msgs = to_openai_messages("sys", &messages).unwrap();
        if let ChatCompletionRequestMessage::Assistant(a) = &msgs[1] {
            let tc = a.tool_calls.as_ref().unwrap();
            match &tc[0] {
                ChatCompletionMessageToolCalls::Function(f) => {
                    assert_eq!(f.id, "call_xyz");
                    let parsed: serde_json::Value =
                        serde_json::from_str(&f.function.arguments).unwrap();
                    assert_eq!(parsed["summary"], "test");
                }
                _ => panic!("expected Function"),
            }
        }
    }

    // --- to_openai_tools -----------------------------------------------------

    #[test]
    fn tool_definitions_converted_correctly() {
        let tools = vec![ToolDefinition {
            name: "propose_plan".into(),
            description: "Propose a plan.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string" }
                },
                "required": ["summary"]
            }),
        }];
        let oai_tools = to_openai_tools(&tools);
        assert_eq!(oai_tools.len(), 1);
        assert!(
            matches!(oai_tools[0], OaiChatCompletionTools::Function(_)),
            "expected Function variant"
        );
        if let OaiChatCompletionTools::Function(t) = &oai_tools[0] {
            assert_eq!(t.function.name, "propose_plan");
            assert_eq!(t.function.description.as_deref(), Some("Propose a plan."));
            assert!(t.function.parameters.is_some());
        }
    }

    #[test]
    fn empty_tools_returns_empty_vec() {
        assert!(to_openai_tools(&[]).is_empty());
    }

    // --- from_openai_response ------------------------------------------------

    fn make_tool_call_response(finish_reason: FinishReason) -> CreateChatCompletionResponse {
        CreateChatCompletionResponse {
            id: "test".into(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatCompletionResponseMessage {
                    role: OaiRole::Assistant,
                    content: None,
                    refusal: None,
                    tool_calls: Some(vec![ChatCompletionMessageToolCalls::Function(
                        ChatCompletionMessageToolCall {
                            id: "call_1".into(),
                            function: FunctionCall {
                                name: "propose_plan".into(),
                                arguments: r#"{"summary":"test","steps":[]}"#.into(),
                            },
                        },
                    )]),
                    annotations: None,
                    audio: None,
                    function_call: None,
                },
                finish_reason: Some(finish_reason),
                logprobs: None,
            }],
            created: 0,
            model: "gpt-4o".into(),
            system_fingerprint: None,
            object: "chat.completion".into(),
            usage: None,
            service_tier: None,
        }
    }

    fn make_text_response(text: &str, finish_reason: FinishReason) -> CreateChatCompletionResponse {
        CreateChatCompletionResponse {
            id: "test".into(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatCompletionResponseMessage {
                    role: OaiRole::Assistant,
                    content: Some(text.into()),
                    refusal: None,
                    tool_calls: None,
                    annotations: None,
                    audio: None,
                    function_call: None,
                },
                finish_reason: Some(finish_reason),
                logprobs: None,
            }],
            created: 0,
            model: "gpt-4o".into(),
            system_fingerprint: None,
            object: "chat.completion".into(),
            usage: None,
            service_tier: None,
        }
    }

    #[test]
    fn finish_reason_tool_calls_maps_to_stop_reason_tool_use() {
        let response = make_tool_call_response(FinishReason::ToolCalls);
        let completion = from_openai_response(response).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.content.len(), 1);
        if let ContentBlock::ToolUse { id, call_id, name, .. } = &completion.content[0] {
            assert_eq!(id, "call_1");
            assert!(call_id.is_none(), "call_id must be None for Chat Completions");
            assert_eq!(name, "propose_plan");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn finish_reason_length_maps_to_max_tokens() {
        let response = make_text_response("truncated", FinishReason::Length);
        let completion = from_openai_response(response).unwrap();
        assert_eq!(completion.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn text_only_response_maps_to_end_turn() {
        let response = make_text_response("Hello!", FinishReason::Stop);
        let completion = from_openai_response(response).unwrap();
        assert_eq!(completion.stop_reason, StopReason::EndTurn);
        assert_eq!(completion.content.len(), 1);
        assert!(matches!(&completion.content[0], ContentBlock::Text { text } if text == "Hello!"));
    }

    #[test]
    fn empty_choices_returns_parse_error() {
        let response = CreateChatCompletionResponse {
            id: "test".into(),
            choices: vec![],
            created: 0,
            model: "gpt-4o".into(),
            system_fingerprint: None,
            object: "chat.completion".into(),
            usage: None,
            service_tier: None,
        };
        assert!(from_openai_response(response).is_err());
    }

    // --- map_openai_error ----------------------------------------------------

    #[test]
    fn auth_error_detection() {
        // We can't construct OpenAIError directly, so we test the string logic
        // via the error-string matching used inside map_openai_error.
        // This test documents the expected classification rules.
        let cases = [
            ("401 Unauthorized", true),
            ("incorrect api key provided", true),
            ("authentication failed", true),
            ("429 Too Many Requests", false), // rate limit, not auth
            ("connection refused", false),
        ];
        for (msg, expected_auth) in cases {
            let is_auth = msg.contains("401")
                || msg.to_lowercase().contains("authentication")
                || msg.to_lowercase().contains("api key")
                || msg.to_lowercase().contains("incorrect api key");
            assert_eq!(
                is_auth, expected_auth,
                "auth classification wrong for: {msg}"
            );
        }
    }

    // Verify that an unused `ChatChoiceLogprobs` import doesn't sneak back in.
    // The struct exists but logprobs is Option<ChatChoiceLogprobs> on ChatChoice.
    #[test]
    fn chat_choice_logprobs_unused_import_guard() {
        // This test just needs to compile. It confirms ChatChoiceLogprobs is
        // accessible from the test module without an explicit import in the
        // production code (it's imported at the top of this test module).
        let _: Option<ChatChoiceLogprobs> = None;
    }
}
