//! Anthropic Messages API provider.
//!
//! Wire format: POST /v1/messages with `input_schema` tool definitions.
//! Tool results are sent as `type: "tool_result"` blocks in user messages.

use crate::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const CONNECT_TIMEOUT_SECS: u64 = 15;
const REQUEST_TIMEOUT_SECS: u64 = 180;

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| ProviderError::Request(e.to_string()))?;

        Ok(Self {
            client,
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<Completion, ProviderError> {
        let wire_messages = messages_to_wire(messages);
        let wire_tools: Vec<AnthropicTool> = tools
            .iter()
            .map(|t| AnthropicTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let body = AnthropicRequest {
            model: self.model.clone(),
            max_tokens,
            system: system.to_string(),
            messages: wire_messages,
            tools: wire_tools,
        };

        let url = format!("{}/v1/messages", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;

        let status = resp.status().as_u16();

        if status == 401 {
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            return Err(ProviderError::Auth(body_text));
        }
        if status == 429 || status == 529 {
            return Err(ProviderError::RateLimit);
        }
        if !resp.status().is_success() {
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            return Err(ProviderError::Http {
                status,
                body: body_text,
            });
        }

        let wire_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        wire_response_to_completion(wire_resp)
    }
}

// ---------------------------------------------------------------------------
// Wire format — Anthropic-specific structs (private to this module)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    tools: Vec<AnthropicTool>,
}

#[derive(Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Serialize, Debug)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicBlock>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        /// Omitted when `false`: `skip_serializing_if = "Not::not"` calls
        /// `not()` on the value and skips the field when that returns `true`
        /// (i.e., when `is_error` is false). Keeps the payload compact;
        /// Anthropic ignores the field when absent.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
    stop_reason: String,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn messages_to_wire(messages: &[Message]) -> Vec<AnthropicMessage> {
    messages
        .iter()
        .map(|msg| AnthropicMessage {
            role: match msg.role {
                Role::User => "user".into(),
                Role::Assistant => "assistant".into(),
            },
            content: msg.content.iter().map(block_to_wire).collect(),
        })
        .collect()
}

fn block_to_wire(block: &ContentBlock) -> AnthropicBlock {
    match block {
        ContentBlock::Text { text } => AnthropicBlock::Text { text: text.clone() },
        ContentBlock::ToolUse { id, name, input } => AnthropicBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => AnthropicBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
    }
}

fn wire_response_to_completion(resp: AnthropicResponse) -> Result<Completion, ProviderError> {
    let mut content = Vec::with_capacity(resp.content.len());
    for block in resp.content {
        match block {
            AnthropicBlock::Text { text } => content.push(ContentBlock::Text { text }),
            AnthropicBlock::ToolUse { id, name, input } => {
                content.push(ContentBlock::ToolUse { id, name, input });
            }
            // tool_result blocks must not appear in assistant responses.
            // Return an error so the caller sees the protocol violation rather
            // than a misleading NoPlanProposed from an empty content vec.
            AnthropicBlock::ToolResult { tool_use_id, .. } => {
                return Err(ProviderError::Parse(format!(
                    "unexpected tool_result block in Anthropic assistant \
                     response (tool_use_id: {tool_use_id}); API protocol violation"
                )));
            }
        }
    }

    let stop_reason = match resp.stop_reason.as_str() {
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        other => {
            return Err(ProviderError::Parse(format!(
                "unexpected stop_reason from Anthropic: '{other}'"
            )));
        }
    };

    Ok(Completion {
        content,
        stop_reason,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_to_wire_user_text() {
        let messages = vec![Message::user_text("hello")];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].role, "user");
        assert_eq!(wire[0].content.len(), 1);
        let json = serde_json::to_value(&wire[0].content[0]).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn messages_to_wire_tool_result() {
        let messages = vec![Message::tool_results(vec![
            crate::provider::ToolResultBlock {
                tool_use_id: "tu_1".into(),
                content: "{\"ok\": true}".into(),
                is_error: false,
            },
        ])];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire[0].role, "user");
        let json = serde_json::to_value(&wire[0].content[0]).unwrap();
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "tu_1");
        // is_error: false should be omitted (skip_serializing_if)
        assert!(json.get("is_error").is_none());
    }

    #[test]
    fn messages_to_wire_tool_result_with_error_flag() {
        let messages = vec![Message::tool_results(vec![
            crate::provider::ToolResultBlock {
                tool_use_id: "tu_err".into(),
                content: "failed".into(),
                is_error: true,
            },
        ])];
        let wire = messages_to_wire(&messages);
        let json = serde_json::to_value(&wire[0].content[0]).unwrap();
        assert_eq!(json["is_error"], true);
    }

    #[test]
    fn stop_reason_tool_use_parsed_correctly() {
        let resp = AnthropicResponse {
            content: vec![AnthropicBlock::ToolUse {
                id: "tu_1".into(),
                name: "get_system_state".into(),
                input: serde_json::json!({}),
            }],
            stop_reason: "tool_use".into(),
        };
        let completion = wire_response_to_completion(resp).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn stop_reason_end_turn_parsed_correctly() {
        let resp = AnthropicResponse {
            content: vec![AnthropicBlock::Text {
                text: "done".into(),
            }],
            stop_reason: "end_turn".into(),
        };
        let completion = wire_response_to_completion(resp).unwrap();
        assert_eq!(completion.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn stop_reason_max_tokens_parsed_correctly() {
        let resp = AnthropicResponse {
            content: vec![AnthropicBlock::Text {
                text: "truncated".into(),
            }],
            stop_reason: "max_tokens".into(),
        };
        let completion = wire_response_to_completion(resp).unwrap();
        assert_eq!(completion.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn unknown_stop_reason_returns_parse_error() {
        let resp = AnthropicResponse {
            content: vec![],
            stop_reason: "something_new_from_anthropic".into(),
        };
        assert!(
            matches!(
                wire_response_to_completion(resp),
                Err(ProviderError::Parse(_))
            ),
            "unknown stop_reason must return Parse error"
        );
    }

    #[test]
    fn assistant_role_message_serialises_correctly() {
        use crate::provider::ContentBlock;
        let messages = vec![Message {
            role: crate::provider::Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "thinking...".into(),
            }],
        }];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].role, "assistant");
        let json = serde_json::to_value(&wire[0].content[0]).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "thinking...");
    }

    #[test]
    fn tool_definition_maps_to_input_schema() {
        let def = ToolDefinition {
            name: "my_tool".into(),
            description: "does stuff".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        };
        let wire = AnthropicTool {
            name: def.name.clone(),
            description: def.description.clone(),
            input_schema: def.input_schema.clone(),
        };
        let json = serde_json::to_value(&wire).unwrap();
        assert!(json.get("input_schema").is_some());
        assert!(json.get("parameters").is_none());
    }

    #[test]
    fn unexpected_tool_result_in_response_returns_parse_error() {
        // An Anthropic assistant response must never contain a tool_result block.
        // If one arrives it is an API protocol violation and must be surfaced as
        // an error rather than silently discarded.
        let resp = AnthropicResponse {
            content: vec![AnthropicBlock::ToolResult {
                tool_use_id: "tu_123".into(),
                content: "some content".into(),
                is_error: false,
            }],
            stop_reason: "tool_use".into(),
        };
        assert!(
            matches!(
                wire_response_to_completion(resp),
                Err(ProviderError::Parse(_))
            ),
            "unexpected ToolResult in assistant response must return Parse error"
        );
    }

    #[test]
    fn stop_sequence_is_parsed_as_end_turn() {
        // Anthropic may return "stop_sequence" when a custom stop sequence fires.
        // It should be treated identically to "end_turn" from the planner's view.
        let resp = AnthropicResponse {
            content: vec![AnthropicBlock::Text {
                text: "stopped".into(),
            }],
            stop_reason: "stop_sequence".into(),
        };
        let completion = wire_response_to_completion(resp).unwrap();
        assert_eq!(completion.stop_reason, StopReason::EndTurn);
    }
}
