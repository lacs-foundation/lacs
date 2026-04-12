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
use rig::completion::{
    CompletionModel, CompletionRequest, ToolDefinition as RigToolDefinition,
};
use rig::message::{
    AssistantContent, Message as RigMessage, Text, ToolCall, ToolFunction, ToolResult,
    ToolResultContent, UserContent,
};
use rig::OneOrMany;

use crate::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, StopReason,
    ToolDefinition,
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
}

impl<M: CompletionModel> RigCompletionAdapter<M> {
    pub fn new(model: M) -> Self {
        Self { model }
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
            additional_params: None,
            output_schema: None,
        };

        let response = self
            .model
            .completion(request)
            .await
            .map_err(map_rig_error)?;

        from_rig_response(response.choice)
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
                                    call_id: None,
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
                            assistant_content.push(AssistantContent::Text(Text {
                                text: text.clone(),
                            }));
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            assistant_content.push(AssistantContent::ToolCall(ToolCall::new(
                                id.clone(),
                                ToolFunction::new(name.clone(), input.clone()),
                            )));
                        }
                        ContentBlock::ToolResult { .. } => {
                            // Tool results don't appear in assistant messages
                        }
                    }
                }

                if !assistant_content.is_empty() {
                    match OneOrMany::many(assistant_content) {
                        Ok(content) => {
                            result.push(RigMessage::Assistant {
                                id: None,
                                content,
                            });
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
fn from_rig_response(choice: OneOrMany<AssistantContent>) -> Result<Completion, ProviderError> {
    let mut content = Vec::new();
    let mut has_tool_calls = false;

    for item in choice.iter() {
        match item {
            AssistantContent::Text(text) => {
                if !text.text.is_empty() {
                    content.push(ContentBlock::Text {
                        text: text.text.clone(),
                    });
                }
            }
            AssistantContent::ToolCall(tc) => {
                has_tool_calls = true;
                content.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input: tc.function.arguments.clone(),
                });
            }
            AssistantContent::Reasoning(_) => {
                // Reasoning blocks are not part of our protocol; skip.
            }
            AssistantContent::Image(_) => {
                // Image blocks are not part of our protocol; skip.
            }
        }
    }

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

fn map_rig_error(err: rig::completion::CompletionError) -> ProviderError {
    let msg = err.to_string();

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
    } else if msg.contains("HttpError") || msg.contains("http") {
        ProviderError::Request(msg)
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
                    UserContent::ToolResult(tr) => assert_eq!(tr.id, "tu_1"),
                    _ => panic!("expected tool result, got {:?}", first),
                }
            }
            _ => panic!("expected User message"),
        }
    }

    #[test]
    fn to_rig_messages_converts_assistant_tool_use() {
        let messages = vec![Message::assistant(vec![ContentBlock::ToolUse {
            id: "tu_1".into(),
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
                    }
                    _ => panic!("expected tool call"),
                }
            }
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
                ToolFunction::new("propose_plan".into(), serde_json::json!({"summary": "test"})),
            )),
        ];
        let choice = OneOrMany::many(items).unwrap();
        let completion = from_rig_response(choice).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.content.len(), 2);
    }
}
