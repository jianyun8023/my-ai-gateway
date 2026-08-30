use serde::Deserialize;
use serde_json::json;
use serde_json::value::RawValue;

use crate::adapter::config::Config;
use crate::adapter::models::ModelInfo;
use crate::adapter::reasoning::decode_reasoning;
use crate::adapter::types::{
    AnthropicContent, AnthropicMessage, AnthropicRequest, AnthropicThinking, AnthropicTool,
    AnthropicToolChoice, ContentPart, ImageSource, InputItem, ResponsesRequest, SummaryPart,
};

/// Supplies per-model metadata (collected from the Kimi upstream) by model id.
pub type ModelResolver<'a> = dyn Fn(&str) -> Option<ModelInfo> + 'a;

/// Converts an OpenAI Responses request into a Kimi Anthropic Messages
/// request. The conversion is deterministic and stateless: all conversational
/// state arrives in req.input on every call.
///
/// resolve_model optionally supplies per-model metadata (collected from the
/// Kimi upstream). Default max_tokens precedence:
/// client max_output_tokens > model metadata > cfg.max_tokens.
pub fn build_anthropic_request(
    cfg: &Config,
    req: &ResponsesRequest,
    resolve_model: Option<&ModelResolver<'_>>,
) -> Result<AnthropicRequest, String> {
    let mut model = req.model.clone();
    if let Some(mapped) = cfg.model_map.get(&model) {
        model = mapped.clone();
    }

    let mut out = AnthropicRequest {
        model,
        max_tokens: req.max_output_tokens,
        stream: req.stream,
        temperature: req.temperature,
        top_p: req.top_p,
        ..Default::default()
    };
    if out.max_tokens <= 0 {
        out.max_tokens = cfg.max_tokens;
        if let Some(resolve) = resolve_model {
            if let Some(info) = resolve(&out.model) {
                if info.max_output_tokens > 0 {
                    out.max_tokens = info.max_output_tokens;
                }
            }
        }
    }

    let items = parse_input(req.input.as_deref())?;

    let mut thinking_enabled = configure_thinking(cfg, req, &mut out);
    if !thinking_enabled && contains_signed_reasoning(&items) {
        // Signed thinking history requires thinking mode on
        // Anthropic-compatible upstreams; enable it even when the client
        // omitted reasoning.effort or asked for minimal (sub2api#5166).
        out.thinking = Some(AnthropicThinking {
            r#type: "enabled".to_string(),
            budget_tokens: cfg.thinking_budgets.get("medium").copied().unwrap_or(0),
        });
        clamp_thinking_budget(&mut out);
        thinking_enabled = true;
    }
    if thinking_enabled {
        // Anthropic rejects sampling overrides together with extended thinking.
        out.temperature = None;
        out.top_p = None;
    }

    let mut system_parts: Vec<String> = Vec::new();
    if !req.instructions.is_empty() {
        system_parts.push(req.instructions.clone());
    }

    let mut msgs: Vec<AnthropicMessage> = Vec::new();

    for it in &items {
        let mut typ = it.r#type.as_str();
        if typ.is_empty() && !it.role.is_empty() {
            typ = "message";
        }
        match typ {
            "message" => match it.role.as_str() {
                "system" | "developer" => {
                    let s = content_text(it.content.as_deref());
                    if !s.is_empty() {
                        system_parts.push(s);
                    }
                }
                "user" => {
                    for b in user_content_blocks(it.content.as_deref()) {
                        push_block(&mut msgs, "user", b);
                    }
                }
                "assistant" => {
                    for b in assistant_content_blocks(it.content.as_deref()) {
                        push_block(&mut msgs, "assistant", b);
                    }
                }
                _ => {}
            },
            "reasoning" => {
                if !thinking_enabled {
                    continue;
                }
                let Some(p) = decode_reasoning(&it.encrypted_content, &summary_text(&it.summary))
                else {
                    continue;
                };
                if !p.redacted.is_empty() {
                    push_block(
                        &mut msgs,
                        "assistant",
                        AnthropicContent {
                            r#type: "redacted_thinking".to_string(),
                            data: p.redacted,
                            ..Default::default()
                        },
                    );
                } else {
                    push_block(
                        &mut msgs,
                        "assistant",
                        AnthropicContent {
                            r#type: "thinking".to_string(),
                            thinking: p.thinking,
                            signature: p.signature,
                            ..Default::default()
                        },
                    );
                }
            }
            "function_call" => {
                let input =
                    parse_raw_json(&it.arguments).unwrap_or_else(|| parse_raw_json("{}").unwrap());
                push_block(
                    &mut msgs,
                    "assistant",
                    AnthropicContent {
                        r#type: "tool_use".to_string(),
                        id: it.call_id.clone(),
                        name: it.name.clone(),
                        input: Some(input),
                        ..Default::default()
                    },
                );
            }
            "function_call_output" => {
                push_block(
                    &mut msgs,
                    "user",
                    AnthropicContent {
                        r#type: "tool_result".to_string(),
                        tool_use_id: it.call_id.clone(),
                        content: Some(tool_result_content(it.output.as_deref())),
                        ..Default::default()
                    },
                );
            }
            "web_search_call" => replay_web_search(&mut msgs, it),
            _ => {}
        }
    }

    if msgs.is_empty() {
        return Err("input produced no messages".to_string());
    }
    out.messages = msgs;
    out.system = system_parts.join("\n\n");

    out.tools = convert_tools(&req.tools)?;
    out.tool_choice = convert_tool_choice(req.tool_choice.as_deref(), req.parallel_tool_calls);

    Ok(out)
}

fn replay_web_search(msgs: &mut Vec<AnthropicMessage>, item: &InputItem) {
    if item.id.is_empty() {
        return;
    }
    let action = item
        .action
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok())
        .unwrap_or_default();
    let query = action
        .get("query")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let input = parse_raw_json(&json!({"query": query}).to_string()).expect("query serializes");
    let sources = action
        .get("sources")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| source.get("url").and_then(serde_json::Value::as_str))
        .map(|url| json!({"type": "web_search_result", "url": url, "title": url}))
        .collect();
    push_block(
        msgs,
        "assistant",
        AnthropicContent {
            r#type: "server_tool_use".to_string(),
            id: item.id.clone(),
            name: "web_search".to_string(),
            input: Some(input),
            ..Default::default()
        },
    );
    push_block(
        msgs,
        "assistant",
        AnthropicContent {
            r#type: "web_search_tool_result".to_string(),
            tool_use_id: item.id.clone(),
            content: Some(serde_json::Value::Array(sources)),
            ..Default::default()
        },
    );
}

fn push_block(msgs: &mut Vec<AnthropicMessage>, role: &str, block: AnthropicContent) {
    if msgs.last().map(|m| m.role.as_str()) != Some(role) {
        msgs.push(AnthropicMessage {
            role: role.to_string(),
            content: Vec::new(),
        });
    }
    msgs.last_mut().expect("just pushed").content.push(block);
}

/// Maps Responses reasoning effort onto an Anthropic thinking budget,
/// clamped against max_tokens. Returns whether thinking is enabled.
fn configure_thinking(cfg: &Config, req: &ResponsesRequest, out: &mut AnthropicRequest) -> bool {
    let mut effort = "medium".to_string();
    if let Some(r) = &req.reasoning {
        if !r.effort.is_empty() {
            effort = r.effort.to_lowercase();
        }
    }
    if effort == "none" || effort == "minimal" {
        out.thinking = Some(AnthropicThinking {
            r#type: "disabled".to_string(),
            budget_tokens: 0,
        });
        return false;
    }
    let budget = cfg
        .thinking_budgets
        .get(&effort)
        .or_else(|| cfg.thinking_budgets.get("medium"))
        .copied()
        .unwrap_or(0);
    out.thinking = Some(AnthropicThinking {
        r#type: "enabled".to_string(),
        budget_tokens: budget,
    });
    clamp_thinking_budget(out);
    true
}

/// Keeps the thinking budget strictly below max_tokens.
fn clamp_thinking_budget(out: &mut AnthropicRequest) {
    let Some(t) = &out.thinking else {
        return;
    };
    if t.r#type != "enabled" {
        return;
    }
    let mut budget = t.budget_tokens;
    if out.max_tokens <= 2048 {
        out.thinking = Some(AnthropicThinking {
            r#type: "disabled".to_string(),
            budget_tokens: 0,
        });
        return;
    }
    if budget >= out.max_tokens {
        budget = out.max_tokens / 2;
        if budget < 1024 {
            budget = 1024;
        }
    }
    out.thinking.as_mut().expect("checked above").budget_tokens = budget;
}

/// Reports whether any input item carries a replayable thinking signature.
fn contains_signed_reasoning(items: &[InputItem]) -> bool {
    items.iter().any(|it| {
        it.r#type == "reasoning"
            && decode_reasoning(&it.encrypted_content, &summary_text(&it.summary)).is_some()
    })
}

fn parse_raw_json(s: &str) -> Option<Box<RawValue>> {
    serde_json::from_str::<Box<RawValue>>(s).ok()
}

fn parse_input(raw: Option<&RawValue>) -> Result<Vec<InputItem>, String> {
    let Some(raw) = raw else {
        return Err("missing input".to_string());
    };
    if let Ok(s) = serde_json::from_str::<String>(raw.get()) {
        let part = serde_json::to_string(&[ContentPart {
            r#type: "input_text".to_string(),
            text: s,
            image_url: None,
        }])
        .expect("content part serializes");
        return Ok(vec![InputItem {
            r#type: "message".to_string(),
            role: "user".to_string(),
            content: parse_raw_json(&part),
            ..Default::default()
        }]);
    }
    serde_json::from_str::<Vec<InputItem>>(raw.get()).map_err(|e| format!("invalid input: {e}"))
}

fn parse_content_parts(raw: Option<&RawValue>) -> Vec<ContentPart> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if let Ok(s) = serde_json::from_str::<String>(raw.get()) {
        return vec![ContentPart {
            r#type: "input_text".to_string(),
            text: s,
            image_url: None,
        }];
    }
    serde_json::from_str::<Vec<ContentPart>>(raw.get()).unwrap_or_default()
}

fn content_text(raw: Option<&RawValue>) -> String {
    parse_content_parts(raw)
        .iter()
        .filter(|p| !p.text.is_empty())
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn user_content_blocks(raw: Option<&RawValue>) -> Vec<AnthropicContent> {
    let mut blocks = Vec::new();
    for p in parse_content_parts(raw) {
        match p.r#type.as_str() {
            "input_text" | "output_text" | "text" => {
                if !p.text.is_empty() {
                    blocks.push(AnthropicContent {
                        r#type: "text".to_string(),
                        text: p.text,
                        ..Default::default()
                    });
                }
            }
            "input_image" => {
                if let Some(b) = image_block(p.image_url.as_deref()) {
                    blocks.push(b);
                }
            }
            _ => {}
        }
    }
    blocks
}

fn assistant_content_blocks(raw: Option<&RawValue>) -> Vec<AnthropicContent> {
    parse_content_parts(raw)
        .into_iter()
        .filter(|p| {
            (p.r#type == "output_text" || p.r#type == "text" || p.r#type == "input_text")
                && !p.text.is_empty()
        })
        .map(|p| AnthropicContent {
            r#type: "text".to_string(),
            text: p.text,
            ..Default::default()
        })
        .collect()
}

fn image_block(raw: Option<&RawValue>) -> Option<AnthropicContent> {
    let raw = raw?;
    let url = if let Ok(s) = serde_json::from_str::<String>(raw.get()) {
        s
    } else {
        #[derive(Deserialize)]
        struct UrlObj {
            #[serde(default)]
            url: String,
        }
        serde_json::from_str::<UrlObj>(raw.get())
            .map(|o| o.url)
            .unwrap_or_default()
    };
    if url.is_empty() {
        return None;
    }
    if let Some((media_type, data)) = parse_data_uri(&url) {
        return Some(AnthropicContent {
            r#type: "image".to_string(),
            source: Some(ImageSource {
                r#type: "base64".to_string(),
                media_type,
                data,
                url: String::new(),
            }),
            ..Default::default()
        });
    }
    Some(AnthropicContent {
        r#type: "image".to_string(),
        source: Some(ImageSource {
            r#type: "url".to_string(),
            url,
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn parse_data_uri(u: &str) -> Option<(String, String)> {
    let rest = u.strip_prefix("data:")?;
    let i = rest.find(',')?;
    let meta = &rest[..i];
    let data = &rest[i + 1..];
    let mut media_type = meta.split(';').next().unwrap_or("");
    if media_type.is_empty() {
        media_type = "application/octet-stream";
    }
    Some((media_type.to_string(), data.to_string()))
}

fn tool_result_content(raw: Option<&RawValue>) -> serde_json::Value {
    let Some(raw) = raw else {
        return serde_json::Value::String(String::new());
    };
    if let Ok(s) = serde_json::from_str::<String>(raw.get()) {
        return serde_json::Value::String(s);
    }
    if let Ok(parts) = serde_json::from_str::<Vec<ContentPart>>(raw.get()) {
        let blocks: Vec<AnthropicContent> = parts
            .into_iter()
            .filter(|p| !p.text.is_empty())
            .map(|p| AnthropicContent {
                r#type: "text".to_string(),
                text: p.text,
                ..Default::default()
            })
            .collect();
        if !blocks.is_empty() {
            return serde_json::to_value(blocks).expect("blocks serialize");
        }
    }
    serde_json::Value::String(raw.get().to_string())
}

fn convert_tools(raw: &[Box<RawValue>]) -> Result<Vec<AnthropicTool>, String> {
    let mut tools = Vec::new();
    for rt in raw {
        let Ok(probe) = serde_json::from_str::<serde_json::Value>(rt.get()) else {
            continue;
        };
        let Some(typ) = probe.get("type").and_then(|t| t.as_str()) else {
            continue;
        };
        match typ {
            "function" => {
                #[derive(Deserialize)]
                struct FnTool {
                    #[serde(default)]
                    name: String,
                    #[serde(default)]
                    description: String,
                    #[serde(default)]
                    parameters: Option<Box<RawValue>>,
                }
                let f: FnTool = serde_json::from_str(rt.get())
                    .map_err(|e| format!("invalid function tool: {e}"))?;
                let parameters = f.parameters.unwrap_or_else(|| {
                    parse_raw_json(r#"{"type":"object","properties":{}}"#).unwrap()
                });
                tools.push(AnthropicTool {
                    r#type: String::new(),
                    name: f.name,
                    description: f.description,
                    input_schema: Some(parameters),
                    max_uses: 0,
                });
            }
            "web_search" | "web_search_preview" | "web_search_preview_2025_03_11" => {
                tools.push(AnthropicTool {
                    r#type: "web_search_20250305".to_string(),
                    name: "web_search".to_string(),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    Ok(tools)
}

fn convert_tool_choice(
    raw: Option<&RawValue>,
    parallel: Option<bool>,
) -> Option<AnthropicToolChoice> {
    let mut choice: Option<AnthropicToolChoice> = None;
    if let Some(raw) = raw {
        if let Ok(s) = serde_json::from_str::<String>(raw.get()) {
            let mk = |t: &str| AnthropicToolChoice {
                r#type: t.to_string(),
                name: String::new(),
                disable_parallel_tool_use: false,
            };
            match s.as_str() {
                "auto" => choice = Some(mk("auto")),
                "required" => choice = Some(mk("any")),
                "none" => choice = Some(mk("none")),
                _ => {}
            }
        } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(raw.get()) {
            let typ = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if (typ == "function" || typ == "tool") && !name.is_empty() {
                choice = Some(AnthropicToolChoice {
                    r#type: "tool".to_string(),
                    name: name.to_string(),
                    disable_parallel_tool_use: false,
                });
            }
        }
    }
    if parallel == Some(false) {
        if choice.is_none() {
            choice = Some(AnthropicToolChoice {
                r#type: "auto".to_string(),
                name: String::new(),
                disable_parallel_tool_use: false,
            });
        }
        if let Some(c) = choice.as_mut() {
            if c.r#type == "auto" || c.r#type == "any" {
                c.disable_parallel_tool_use = true;
            }
        }
    }
    choice
}

fn summary_text(parts: &[SummaryPart]) -> String {
    parts.iter().map(|p| p.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::reasoning::{
        decode_reasoning, encode_reasoning, encode_redacted_reasoning,
    };
    use crate::adapter::test_config;

    fn must_build(cfg: &Config, req_json: &str) -> AnthropicRequest {
        let req: ResponsesRequest = serde_json::from_str(req_json).expect("unmarshal request");
        build_anthropic_request(cfg, &req, None).expect("build_anthropic_request")
    }

    fn build_err(cfg: &Config, req_json: &str) -> Option<String> {
        let req: ResponsesRequest = serde_json::from_str(req_json).expect("unmarshal request");
        build_anthropic_request(cfg, &req, None).err()
    }

    #[test]
    fn reasoning_round_trip() {
        let enc = encode_reasoning("let me think about this", "sig-kimi-abc123");
        let req_json = format!(
            r#"{{
                "model": "k3-256k",
                "input": [
                    {{"type": "message", "role": "user", "content": [{{"type": "input_text", "text": "hi"}}]}},
                    {{"type": "reasoning", "encrypted_content": "{enc}",
                     "summary": [{{"type": "summary_text", "text": "let me think about this"}}]}},
                    {{"type": "message", "role": "assistant", "content": [{{"type": "output_text", "text": "hello!"}}]}},
                    {{"type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{{\"cmd\":\"ls\"}}"}},
                    {{"type": "function_call_output", "call_id": "call_1", "output": "file.txt"}}
                ]
            }}"#
        );
        let out = must_build(&test_config(), &req_json);

        assert_eq!(out.messages.len(), 3, "expected user/assistant/user");
        let assistant = &out.messages[1];
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 3, "thinking+text+tool_use");
        let th = &assistant.content[0];
        assert_eq!(th.r#type, "thinking");
        assert_eq!(th.thinking, "let me think about this");
        assert_eq!(th.signature, "sig-kimi-abc123");
        assert_eq!(assistant.content[1].r#type, "text");
        assert_eq!(assistant.content[1].text, "hello!");
        let tu = &assistant.content[2];
        assert_eq!(tu.r#type, "tool_use");
        assert_eq!(tu.id, "call_1");
        assert_eq!(tu.name, "shell");
        assert_eq!(tu.input.as_ref().unwrap().get(), r#"{"cmd":"ls"}"#);
        let tr = &out.messages[2].content[0];
        assert_eq!(tr.r#type, "tool_result");
        assert_eq!(tr.tool_use_id, "call_1");
        assert_eq!(
            tr.content,
            Some(serde_json::Value::String("file.txt".into()))
        );
    }

    #[test]
    fn bare_signature_fallback() {
        let p = decode_reasoning("raw-kimi-signature", "thinking text from summary")
            .expect("fallback decode failed");
        assert_eq!(p.signature, "raw-kimi-signature");
        assert_eq!(p.thinking, "thinking text from summary");
    }

    #[test]
    fn foreign_signature_dropped() {
        assert!(
            decode_reasoning("gAAAA-openai-blob", "summary").is_none(),
            "OpenAI gAAAA blob must not be replayed to Anthropic upstreams"
        );
    }

    #[test]
    fn signed_reasoning_forces_thinking_on() {
        // effort=minimal would normally disable thinking, but a signed
        // thinking block in history requires thinking mode upstream
        // (sub2api#5166).
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "reasoning": {"effort": "minimal"},
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                    {"type": "reasoning", "encrypted_content": "kimi-sig-abc",
                     "summary": [{"type": "summary_text", "text": "thought"}]},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "again"}]}
                ]
            }"#,
        );
        let thinking = out
            .thinking
            .expect("signed reasoning should force thinking on");
        assert_eq!(thinking.r#type, "enabled");
        let th = &out.messages[1].content[0];
        assert_eq!(th.r#type, "thinking");
        assert_eq!(th.signature, "kimi-sig-abc");
    }

    #[test]
    fn no_force_when_no_signed_reasoning() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "reasoning": {"effort": "minimal"},
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                    {"type": "reasoning", "encrypted_content": "gAAAA-blob"},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]}
                ]
            }"#,
        );
        let thinking = out.thinking.expect("thinking config present");
        assert_eq!(thinking.r#type, "disabled");
        for m in &out.messages {
            for b in &m.content {
                assert_ne!(b.r#type, "thinking", "foreign reasoning leaked: {m:?}");
            }
        }
    }

    #[test]
    fn instructions_and_system_become_system() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "instructions": "You are Codex.",
                "input": [
                    {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Be terse."}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
                ]
            }"#,
        );
        assert_eq!(out.system, "You are Codex.\n\nBe terse.");
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "user");
    }

    #[test]
    fn tools_and_web_search() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": "hi",
                "tools": [
                    {"type": "function", "name": "shell", "description": "run", "parameters": {"type": "object"}},
                    {"type": "web_search_preview"}
                ],
                "tool_choice": "auto",
                "parallel_tool_calls": false
            }"#,
        );
        assert_eq!(out.tools.len(), 2);
        assert_eq!(out.tools[0].name, "shell");
        assert!(out.tools[0].input_schema.is_some());
        assert_eq!(out.tools[1].r#type, "web_search_20250305");
        assert_eq!(out.tools[1].name, "web_search");
        let tc = out.tool_choice.expect("tool_choice set");
        assert_eq!(tc.r#type, "auto");
        assert!(tc.disable_parallel_tool_use);
    }

    #[test]
    fn thinking_effort_mapping() {
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","reasoning":{"effort":"high"},"max_output_tokens":65536}"#,
        );
        let t = out.thinking.unwrap();
        assert_eq!(t.r#type, "enabled");
        assert_eq!(t.budget_tokens, 32768);

        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","reasoning":{"effort":"minimal"}}"#,
        );
        assert_eq!(out.thinking.unwrap().r#type, "disabled");

        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","reasoning":{"effort":"high"},"max_output_tokens":8000}"#,
        );
        let t = out.thinking.unwrap();
        assert_eq!(t.r#type, "enabled");
        assert!(
            t.budget_tokens < 8000,
            "budget should be clamped below max_tokens"
        );
    }

    #[test]
    fn image_input() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [{"type":"message","role":"user","content":[
                    {"type":"input_text","text":"what is this?"},
                    {"type":"input_image","image_url":"data:image/png;base64,aGk="}
                ]}]
            }"#,
        );
        let blocks = &out.messages[0].content;
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].r#type, "image");
        let source = blocks[1].source.as_ref().unwrap();
        assert_eq!(source.media_type, "image/png");
        assert_eq!(source.data, "aGk=");
    }

    #[test]
    fn model_map() {
        let mut cfg = test_config();
        cfg.model_map.insert(
            "k3-256k".to_string(),
            "kimi-for-coding-highspeed".to_string(),
        );
        let out = must_build(&cfg, r#"{"model":"k3-256k","input":"hi"}"#);
        assert_eq!(out.model, "kimi-for-coding-highspeed");
    }

    // ---- positive cases ----

    #[test]
    fn multiple_tool_results_merge_into_one_user_message() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [
                    {"type":"message","role":"user","content":[{"type":"input_text","text":"run both"}]},
                    {"type":"function_call","call_id":"c1","name":"shell","arguments":"{}"},
                    {"type":"function_call","call_id":"c2","name":"shell","arguments":"{}"},
                    {"type":"function_call_output","call_id":"c1","output":"one"},
                    {"type":"function_call_output","call_id":"c2","output":"two"}
                ]
            }"#,
        );
        assert_eq!(out.messages.len(), 3, "expected user/assistant/user");
        let last = &out.messages[2];
        assert_eq!(last.role, "user");
        assert_eq!(last.content.len(), 2, "tool results should merge");
        assert_eq!(last.content[0].tool_use_id, "c1");
        assert_eq!(last.content[1].tool_use_id, "c2");
    }

    #[test]
    fn function_call_invalid_arguments_become_empty_object() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [
                    {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]},
                    {"type":"function_call","call_id":"c1","name":"shell","arguments":"not-json"}
                ]
            }"#,
        );
        let tu = &out.messages[1].content[0];
        assert_eq!(tu.input.as_ref().unwrap().get(), "{}");
    }

    #[test]
    fn tool_result_with_structured_output() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [
                    {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]},
                    {"type":"function_call","call_id":"c1","name":"shell","arguments":"{}"},
                    {"type":"function_call_output","call_id":"c1","output":[{"type":"output_text","text":"part1"},{"type":"output_text","text":"part2"}]}
                ]
            }"#,
        );
        assert_eq!(out.messages.len(), 3, "expected user/assistant/user");
        let tr = &out.messages[2].content[0];
        let blocks = tr
            .content
            .as_ref()
            .and_then(|c| c.as_array())
            .expect("structured blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["text"], "part1");
        assert_eq!(blocks[1]["text"], "part2");
    }

    #[test]
    fn tool_choice_variants() {
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","tool_choice":"required"}"#,
        );
        assert_eq!(out.tool_choice.unwrap().r#type, "any");
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","tool_choice":"none"}"#,
        );
        assert_eq!(out.tool_choice.unwrap().r#type, "none");
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","tool_choice":{"type":"function","name":"shell"}}"#,
        );
        let tc = out.tool_choice.unwrap();
        assert_eq!(tc.r#type, "tool");
        assert_eq!(tc.name, "shell");
    }

    #[test]
    fn image_url_source() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [{"type":"message","role":"user","content":[
                    {"type":"input_image","image_url":{"url":"https://example.com/x.png"}}
                ]}]
            }"#,
        );
        let b = &out.messages[0].content[0];
        assert_eq!(b.r#type, "image");
        let source = b.source.as_ref().unwrap();
        assert_eq!(source.r#type, "url");
        assert_eq!(source.url, "https://example.com/x.png");
    }

    #[test]
    fn redacted_thinking_round_trip() {
        let enc = encode_redacted_reasoning("opaque-redacted-data");
        let req_json = format!(
            r#"{{
                "model": "k3",
                "input": [
                    {{"type":"message","role":"user","content":[{{"type":"input_text","text":"hi"}}]}},
                    {{"type":"reasoning","encrypted_content":"{enc}"}},
                    {{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"hello"}}]}}
                ]
            }}"#
        );
        let out = must_build(&test_config(), &req_json);
        let th = &out.messages[1].content[0];
        assert_eq!(th.r#type, "redacted_thinking");
        assert_eq!(th.data, "opaque-redacted-data");
    }

    #[test]
    fn sampling_dropped_only_when_thinking_enabled() {
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","temperature":0.5,"top_p":0.9,"reasoning":{"effort":"medium"}}"#,
        );
        assert!(out.temperature.is_none() && out.top_p.is_none());
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","temperature":0.5,"top_p":0.9,"reasoning":{"effort":"minimal"}}"#,
        );
        assert_eq!(out.temperature, Some(0.5));
        assert_eq!(out.top_p, Some(0.9));
    }

    #[test]
    fn tiny_max_tokens_disables_thinking() {
        let out = must_build(
            &test_config(),
            r#"{"model":"k3","input":"hi","max_output_tokens":1024}"#,
        );
        assert_eq!(
            out.thinking.unwrap().r#type,
            "disabled",
            "max_tokens<=2048 should disable thinking"
        );
    }

    #[test]
    fn web_search_call_history_replayed() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [
                    {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]},
                    {"type":"web_search_call","id":"ws_1","status":"completed","action":{"type":"search","query":"x","sources":[{"type":"url","url":"https://example.com"}]}},
                    {"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}
                ]
            }"#,
        );
        let search = &out.messages[1].content;
        assert_eq!(search[0].r#type, "server_tool_use");
        assert_eq!(search[0].id, "ws_1");
        assert_eq!(search[0].name, "web_search");
        assert_eq!(search[0].input.as_ref().unwrap().get(), r#"{"query":"x"}"#);
        assert_eq!(search[1].r#type, "web_search_tool_result");
        assert_eq!(search[1].tool_use_id, "ws_1");
        assert_eq!(
            search[1].content.as_ref().unwrap()[0]["url"],
            "https://example.com"
        );
    }

    #[test]
    fn string_message_content() {
        let out = must_build(
            &test_config(),
            r#"{"model": "k3", "input": [{"type":"message","role":"user","content":"plain string"}]}"#,
        );
        assert_eq!(out.messages[0].content[0].text, "plain string");
    }

    // ---- negative cases ----

    #[test]
    fn missing_input_rejected() {
        assert!(build_err(&test_config(), r#"{"model":"k3"}"#).is_some());
    }

    #[test]
    fn invalid_input_type_rejected() {
        assert!(build_err(&test_config(), r#"{"model":"k3","input":42}"#).is_some());
    }

    #[test]
    fn system_only_input_produces_no_messages() {
        let err = build_err(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [{"type":"message","role":"developer","content":[{"type":"input_text","text":"be nice"}]}]
            }"#,
        );
        assert!(
            err.is_some(),
            "input without user/assistant message must be rejected"
        );
    }

    #[test]
    fn unknown_items_skipped() {
        let out = must_build(
            &test_config(),
            r#"{
                "model": "k3",
                "input": [
                    {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]},
                    {"type":"computer_call","id":"x"},
                    {"type":"local_shell_call","id":"y"}
                ]
            }"#,
        );
        assert_eq!(
            out.messages.len(),
            1,
            "unknown item types should be skipped"
        );
    }
}
