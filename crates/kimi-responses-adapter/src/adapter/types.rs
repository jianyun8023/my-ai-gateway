use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// Go's encoding/json leaves the zero value when a field is JSON null; serde
/// rejects null for non-Option fields. Real Kimi payloads contain nulls
/// (e.g. "stop_reason":null in message_start), so every scalar/slice field
/// we parse must tolerate null the way Go does.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

// OpenAI Responses API inbound types. Only the fields the adapter needs are
// modeled; unknown fields are ignored.

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResponsesRequest {
    #[serde(default, deserialize_with = "null_default")]
    pub model: String,
    /// String or []InputItem.
    #[serde(default)]
    pub input: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "null_default")]
    pub instructions: String,
    #[serde(default, deserialize_with = "null_default")]
    pub tools: Vec<Box<RawValue>>,
    #[serde(default)]
    pub tool_choice: Option<Box<RawValue>>,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<ResponsesReasoning>,
    #[serde(default, deserialize_with = "null_default")]
    pub max_output_tokens: i64,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default, deserialize_with = "null_default")]
    pub stream: bool,
    #[serde(default)]
    pub store: Option<bool>,
    #[serde(default, deserialize_with = "null_default")]
    pub include: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResponsesReasoning {
    #[serde(default, deserialize_with = "null_default")]
    pub effort: String,
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub summary: String,
}

/// InputItem is a union over the Responses input item kinds.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct InputItem {
    /// message | reasoning | function_call | function_call_output |
    /// web_search_call | ...
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    /// For type=message.
    #[serde(default, deserialize_with = "null_default")]
    pub role: String,

    /// String or []ContentPart.
    #[serde(default)]
    pub content: Option<Box<RawValue>>,

    // reasoning
    #[serde(default, deserialize_with = "null_default")]
    pub summary: Vec<SummaryPart>,
    #[serde(default, deserialize_with = "null_default")]
    pub encrypted_content: String,

    // function_call
    #[serde(default, deserialize_with = "null_default")]
    pub call_id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub arguments: String,

    /// function_call_output: string or []ContentPart.
    #[serde(default)]
    pub output: Option<Box<RawValue>>,

    // web_search_call
    #[serde(default)]
    pub action: Option<Box<RawValue>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SummaryPart {
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentPart {
    /// input_text | input_image | output_text | ...
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
    /// String or {"url": ...}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<Box<RawValue>>,
}

// Anthropic-side types (Kimi Code Messages API).

#[derive(Debug, Default, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub stream: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContent>,
}

/// AnthropicContent is a union of all content block kinds used in both
/// directions: text, image, thinking, redacted_thinking, tool_use,
/// tool_result, server_tool_use, web_search_tool_result.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AnthropicContent {
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,

    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource>,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// redacted_thinking payload.
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data: String,

    /// tool_use / server_tool_use.
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// tool_use / server_tool_use.
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// tool_use / server_tool_use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tool_use_id: String,
    /// tool_result / web_search_tool_result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImageSource {
    /// "base64" or "url".
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub media_type: String,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data: String,
    #[serde(default, deserialize_with = "null_default")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
}

#[derive(Debug, Default, Serialize)]
pub struct AnthropicTool {
    /// Set for server tools, e.g. web_search_20250305.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#type: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "is_zero")]
    pub max_uses: i64,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

#[derive(Debug, Serialize)]
pub struct AnthropicToolChoice {
    /// auto | any | none | tool.
    pub r#type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "is_false")]
    pub disable_parallel_tool_use: bool,
}

#[derive(Debug, Serialize)]
pub struct AnthropicThinking {
    /// enabled | disabled.
    pub r#type: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub budget_tokens: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicUsage {
    #[serde(default, deserialize_with = "null_default")]
    pub input_tokens: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub output_tokens: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub cache_creation_input_tokens: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub cache_read_input_tokens: i64,
    #[serde(default)]
    pub output_tokens_details: Option<AnthropicOutputTokenDetails>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicOutputTokenDetails {
    #[serde(default, deserialize_with = "null_default")]
    pub thinking_tokens: i64,
}

/// AnthropicMessageObj is the non-streaming response / message_start payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicMessageObj {
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub role: String,
    #[serde(default, deserialize_with = "null_default")]
    pub content: Vec<AnthropicContent>,
    #[serde(default, deserialize_with = "null_default")]
    #[allow(dead_code)]
    pub model: String,
    #[serde(default, deserialize_with = "null_default")]
    pub stop_reason: String,
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
}

/// AnthropicStreamEvent is the envelope for every SSE event from upstream.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicStreamEvent {
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub index: i64,
    #[serde(default)]
    pub message: Option<AnthropicMessageObj>,
    #[serde(default)]
    pub content_block: Option<AnthropicContent>,
    #[serde(default)]
    pub delta: Option<AnthropicDelta>,
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
    #[serde(default)]
    pub error: Option<AnthropicError>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicDelta {
    /// text_delta | thinking_delta | signature_delta | input_json_delta.
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
    #[serde(default, deserialize_with = "null_default")]
    pub thinking: String,
    #[serde(default, deserialize_with = "null_default")]
    pub signature: String,
    #[serde(default, deserialize_with = "null_default")]
    pub partial_json: String,
    #[serde(default, deserialize_with = "null_default")]
    pub stop_reason: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnthropicError {
    #[serde(default, deserialize_with = "null_default")]
    pub r#type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub message: String,
}
