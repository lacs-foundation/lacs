//! Ollama OpenAI-compatible provider.
//!
//! Wire format: POST /v1/chat/completions with OpenAI function-calling format.
//! `function.parameters` instead of `input_schema`.
//! Tool results sent as `role: "tool"` messages with `tool_call_id`.
//! `function.arguments` is a JSON-encoded string (requires second parse).

use crate::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CONNECT_TIMEOUT_SECS: u64 = 10;
const REQUEST_TIMEOUT_SECS: u64 = 300; // local models can be slow

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| ProviderError::Request(e.to_string()))?;

        Ok(Self {
            client,
            base_url: base_url.into(),
            model: model.into(),
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<Completion, ProviderError> {
        let mut wire_messages = vec![OpenAiMessage::system(system)];
        wire_messages.extend(messages_to_wire(messages));

        let wire_tools: Vec<OpenAiTool> = tools
            .iter()
            .map(|t| OpenAiTool {
                r#type: "function".into(),
                function: OpenAiFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        let body = OpenAiRequest {
            model: self.model.clone(),
            messages: wire_messages,
            tools: if wire_tools.is_empty() {
                None
            } else {
                Some(wire_tools)
            },
            max_tokens,
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;

        let status = resp.status().as_u16();

        if !resp.status().is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Http {
                status,
                body: body_text,
            });
        }

        let wire_resp: OpenAiResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        wire_response_to_completion(wire_resp)
    }
}

// ---------------------------------------------------------------------------
// Wire format — OpenAI-compatible structs (private to this module)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl OpenAiMessage {
    fn system(content: &str) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiCalledFunction,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAiCalledFunction {
    name: String,
    /// JSON-encoded string — must be parsed with serde_json::from_str.
    arguments: String,
}

#[derive(Serialize)]
struct OpenAiTool {
    r#type: String,
    function: OpenAiFunction,
}

#[derive(Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    /// JSON Schema — uses `parameters` (not `input_schema`).
    parameters: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize, Debug)]
struct OpenAiChoice {
    message: OpenAiMessage,
    finish_reason: String,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn messages_to_wire(messages: &[Message]) -> Vec<OpenAiMessage> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.role {
            Role::User => {
                // If all blocks are tool results, emit one `tool` message per result.
                let all_results = msg
                    .content
                    .iter()
                    .all(|b| matches!(b, ContentBlock::ToolResult { .. }));

                if all_results {
                    for block in &msg.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = block
                        {
                            result.push(OpenAiMessage {
                                role: "tool".into(),
                                content: Some(content.clone()),
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id.clone()),
                            });
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

                    result.push(OpenAiMessage {
                        role: "user".into(),
                        content: Some(text),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
            }
            Role::Assistant => {
                let text_content: String = msg
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

                let tool_calls: Vec<OpenAiToolCall> = msg
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::ToolUse { id, name, input } = b {
                            Some(OpenAiToolCall {
                                id: id.clone(),
                                r#type: "function".into(),
                                function: OpenAiCalledFunction {
                                    name: name.clone(),
                                    arguments: input.to_string(),
                                },
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                result.push(OpenAiMessage {
                    role: "assistant".into(),
                    content: if text_content.is_empty() {
                        None
                    } else {
                        Some(text_content)
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                });
            }
        }
    }

    result
}

fn wire_response_to_completion(resp: OpenAiResponse) -> Result<Completion, ProviderError> {
    let choice = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("empty choices array".into()))?;

    let stop_reason = match choice.finish_reason.as_str() {
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    };

    let mut content = Vec::new();

    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content.push(ContentBlock::Text { text });
        }
    }

    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let input: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.function.name,
                input,
            });
        }
    }

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
    use crate::provider::ToolResultBlock;

    #[test]
    fn user_text_message_maps_to_user_role() {
        let messages = vec![Message::user_text("hello")];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].role, "user");
        assert_eq!(wire[0].content.as_deref(), Some("hello"));
    }

    #[test]
    fn tool_result_maps_to_tool_role() {
        let messages = vec![Message::tool_results(vec![ToolResultBlock {
            tool_use_id: "call_1".into(),
            content: "{\"ok\": true}".into(),
            is_error: false,
        }])];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].role, "tool");
        assert_eq!(wire[0].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn multiple_tool_results_emit_separate_tool_messages() {
        let messages = vec![Message::tool_results(vec![
            ToolResultBlock {
                tool_use_id: "call_1".into(),
                content: "result 1".into(),
                is_error: false,
            },
            ToolResultBlock {
                tool_use_id: "call_2".into(),
                content: "result 2".into(),
                is_error: false,
            },
        ])];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 2);
        assert_eq!(wire[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(wire[1].tool_call_id.as_deref(), Some("call_2"));
    }

    #[test]
    fn finish_reason_tool_calls_maps_to_tool_use() {
        let resp = OpenAiResponse {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".into(),
                        r#type: "function".into(),
                        function: OpenAiCalledFunction {
                            name: "get_system_state".into(),
                            arguments: "{}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: "tool_calls".into(),
            }],
        };
        let completion = wire_response_to_completion(resp).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.content.len(), 1);
        if let ContentBlock::ToolUse { name, .. } = &completion.content[0] {
            assert_eq!(name, "get_system_state");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn arguments_json_string_is_parsed_to_value() {
        let resp = OpenAiResponse {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".into(),
                        r#type: "function".into(),
                        function: OpenAiCalledFunction {
                            name: "propose_plan".into(),
                            arguments: r#"{"summary":"test","steps":[]}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: "tool_calls".into(),
            }],
        };
        let completion = wire_response_to_completion(resp).unwrap();
        if let ContentBlock::ToolUse { input, .. } = &completion.content[0] {
            assert_eq!(input["summary"], "test");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn tool_definition_uses_parameters_not_input_schema() {
        let tool = OpenAiTool {
            r#type: "function".into(),
            function: OpenAiFunction {
                name: "test".into(),
                description: "test tool".into(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            },
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert!(json["function"].get("parameters").is_some());
        assert!(json["function"].get("input_schema").is_none());
    }
}
