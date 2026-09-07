//! Protocol Contract Tests for AI Gateway (#116, #117).
//!
//! Non-streaming basic contract, streaming protocol semantics, tool calling,
//! reasoning/thinking, and gateway error envelope tests for
//! OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
//!
//! All tests are fully offline: no real Provider secrets, no network access.
//! Each test constructs an in-process gateway backed by `MockProvider` and
//! verifies both the downstream response and the upstream request mapping.
//!
//! Case IDs follow the `{surface}.{feature}.{scenario}` naming from #114.

mod support;

#[path = "contract_tests/common.rs"]
mod common;

// ── #116: Non-streaming basic contract ──────────────────────────────────────

#[path = "contract_tests/chat.rs"]
mod chat;

#[path = "contract_tests/responses.rs"]
mod responses;

#[path = "contract_tests/messages.rs"]
mod messages;

#[path = "contract_tests/errors.rs"]
mod errors;

// ── #117: Streaming / Tool Calling / Reasoning ──────────────────────────────

#[path = "contract_tests/streaming_chat.rs"]
mod streaming_chat;

#[path = "contract_tests/streaming_responses.rs"]
mod streaming_responses;

#[path = "contract_tests/streaming_messages.rs"]
mod streaming_messages;

#[path = "contract_tests/tools.rs"]
mod tools;

#[path = "contract_tests/reasoning.rs"]
mod reasoning;

#[path = "contract_tests/faults.rs"]
mod faults;
