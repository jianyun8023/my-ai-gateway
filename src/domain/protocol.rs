use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "gateway_protocol")]
pub enum Protocol {
    #[serde(rename = "openai_chat_completions")]
    #[sqlx(rename = "openai_chat_completions")]
    OpenAiChatCompletions,
    #[serde(rename = "openai_responses")]
    #[sqlx(rename = "openai_responses")]
    OpenAiResponses,
    #[serde(rename = "anthropic_messages")]
    #[sqlx(rename = "anthropic_messages")]
    AnthropicMessages,
}

impl Protocol {
    pub const ALL: [Self; 3] = [
        Self::OpenAiChatCompletions,
        Self::OpenAiResponses,
        Self::AnthropicMessages,
    ];
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::OpenAiChatCompletions => "openai_chat_completions",
            Self::OpenAiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
        };
        f.write_str(value)
    }
}

impl FromStr for Protocol {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai_chat_completions" | "chat_completions" => Ok(Self::OpenAiChatCompletions),
            "openai_responses" | "responses" => Ok(Self::OpenAiResponses),
            "anthropic_messages" | "messages" | "claude" => Ok(Self::AnthropicMessages),
            _ => Err(()),
        }
    }
}
