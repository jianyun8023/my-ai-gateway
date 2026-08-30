use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// ReasoningPayload is the self-contained representation of a Kimi thinking
/// block that travels inside Responses reasoning.encrypted_content. Keeping
/// both the thinking text and the signature makes the round-trip independent
/// of how the client treats summaries.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReasoningPayload {
    pub v: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub redacted: String,
}

pub fn encode_reasoning(thinking: &str, signature: &str) -> String {
    let p = ReasoningPayload {
        v: 1,
        thinking: thinking.to_string(),
        signature: signature.to_string(),
        redacted: String::new(),
    };
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&p).expect("payload serializes"))
}

pub fn encode_redacted_reasoning(data: &str) -> String {
    let p = ReasoningPayload {
        v: 1,
        thinking: String::new(),
        signature: String::new(),
        redacted: data.to_string(),
    };
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&p).expect("payload serializes"))
}

/// Recovers a Kimi thinking block from encrypted_content.
/// Fallback: if the payload is not our envelope, treat the raw string as a
/// bare signature and use the summary text as the thinking content, matching
/// the simple {"encrypted_content": "<signature>"} convention.
///
/// OpenAI/Codex "gAAAA..." blobs are foreign ciphertext: Anthropic-compatible
/// upstreams reject them with 400, so they are never replayed (mirrors
/// sub2api#5166).
pub fn decode_reasoning(encrypted_content: &str, summary_text: &str) -> Option<ReasoningPayload> {
    if encrypted_content.is_empty() {
        return None;
    }
    if let Ok(raw) = URL_SAFE_NO_PAD.decode(encrypted_content) {
        if let Ok(p) = serde_json::from_slice::<ReasoningPayload>(&raw) {
            if p.v == 1 {
                return Some(p);
            }
        }
    }
    if encrypted_content.starts_with("gAAAA") {
        return None;
    }
    // Bare signature fallback.
    Some(ReasoningPayload {
        v: 1,
        thinking: summary_text.to_string(),
        signature: encrypted_content.to_string(),
        redacted: String::new(),
    })
}
