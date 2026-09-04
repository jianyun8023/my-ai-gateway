//! Protocol Contract Tests for AI Gateway (#116).
//!
//! Non-streaming basic contract + gateway error envelope tests for
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

#[path = "contract_tests/chat.rs"]
mod chat;

#[path = "contract_tests/responses.rs"]
mod responses;

#[path = "contract_tests/messages.rs"]
mod messages;

#[path = "contract_tests/errors.rs"]
mod errors;
