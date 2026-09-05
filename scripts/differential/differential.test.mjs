/**
 * differential.test.mjs — Offline unit tests for the differential testing framework.
 *
 * Uses node:test (no external dependencies). Does not consume real tokens.
 * Tests normalizers, comparator, classifier, argument parsing, and case loading.
 *
 * Coverage:
 *  - Normalizer field stripping (each normalizer individually)
 *  - Result classification matrix:
 *    - Both sides fail → UPSTREAM_LIMITATION
 *    - Direct PASS + Gateway FAIL → GATEWAY_BUG
 *    - Both PASS → PASS
 *    - Gateway drops declared field → GATEWAY_BUG
 *    - Transport error → FLAKY
 *    - Model didn't produce expected output → MODEL_BEHAVIOR
 *  - Argument parsing
 *  - Case loading and filtering (incl. default exclusion of error/high-cost cases)
 *  - Stream normalization and stream-summary assertions (finish_reason / usage drop)
 */

import assert from 'node:assert/strict'
import test from 'node:test'

import {
  strip_dynamic_ids,
  strip_timestamps,
  normalize_finish_reason,
  normalize_usage_shape,
  normalize_tool_call,
  normalize_stream_events,
  applyNormalizers,
  listNormalizers,
} from './normalize.mjs'

import { compareResponses, classifyResult } from './compare.mjs'
import { parseArguments, loadCases, filterCases } from './run.mjs'

// ════════════════════════════════════════════════════════════════════════════
// §1 — Normalizer unit tests
// ════════════════════════════════════════════════════════════════════════════

test('strip_dynamic_ids: strips response id and system_fingerprint', () => {
  const input = {
    id: 'chatcmpl-abc123',
    system_fingerprint: 'fp_xyz789',
    object: 'chat.completion',
    choices: [{ index: 0, message: { content: 'hello' }, finish_reason: 'stop' }],
  }
  const result = strip_dynamic_ids(input)
  assert.equal(result.id, '<stripped:dynamic_id>')
  assert.equal(result.system_fingerprint, '<stripped:system_fingerprint>')
  assert.equal(result.object, 'chat.completion', 'non-dynamic fields preserved')
  assert.equal(result.choices[0].message.content, 'hello', 'content preserved')
})

test('strip_dynamic_ids: strips tool_call ids while preserving structure', () => {
  const input = {
    id: 'chatcmpl-abc',
    choices: [{
      index: 0,
      message: {
        content: null,
        tool_calls: [
          { id: 'call_abc123', type: 'function', function: { name: 'get_weather', arguments: '{}' } },
          { id: 'call_def456', type: 'function', function: { name: 'get_time', arguments: '{}' } },
        ],
      },
      finish_reason: 'tool_calls',
    }],
  }
  const result = strip_dynamic_ids(input)
  assert.equal(result.choices[0].message.tool_calls.length, 2, 'tool call count preserved')
  assert.equal(result.choices[0].message.tool_calls[0].id, '<stripped:tool_call_id>')
  assert.equal(result.choices[0].message.tool_calls[0].function.name, 'get_weather', 'function name preserved')
  assert.equal(result.choices[0].message.tool_calls[1].id, '<stripped:tool_call_id>')
})

test('strip_dynamic_ids: handles null and non-object input', () => {
  assert.equal(strip_dynamic_ids(null), null)
  assert.equal(strip_dynamic_ids(undefined), undefined)
  assert.equal(strip_dynamic_ids('string'), 'string')
})

test('strip_timestamps: strips created field', () => {
  const input = { id: 'abc', created: 1700000000, object: 'chat.completion' }
  const result = strip_timestamps(input)
  assert.equal(result.created, '<stripped:timestamp>')
  assert.equal(result.id, 'abc', 'other fields preserved')
  assert.equal(result.object, 'chat.completion')
})

test('strip_timestamps: no-op when created is absent', () => {
  const input = { id: 'abc', object: 'chat.completion' }
  const result = strip_timestamps(input)
  assert.ok(!('created' in result) || result.created === undefined)
})

test('normalize_finish_reason: maps provider-specific reasons to canonical', () => {
  const tests = [
    { input: 'stop', expected: 'stop' },
    { input: 'end_turn', expected: 'stop' },
    { input: 'end', expected: 'stop' },
    { input: 'length', expected: 'length' },
    { input: 'max_tokens', expected: 'length' },
    { input: 'tool_calls', expected: 'tool_calls' },
    { input: 'tool_use', expected: 'tool_calls' },
    { input: 'content_filter', expected: 'content_filter' },
  ]
  for (const { input, expected } of tests) {
    const resp = { choices: [{ finish_reason: input }] }
    const result = normalize_finish_reason(resp)
    assert.equal(result.choices[0].finish_reason, expected, `${input} → ${expected}`)
  }
})

test('normalize_finish_reason: preserves null (streaming intermediate)', () => {
  const resp = { choices: [{ finish_reason: null }] }
  const result = normalize_finish_reason(resp)
  assert.equal(result.choices[0].finish_reason, null)
})

test('normalize_usage_shape: replaces values with type markers, preserves keys', () => {
  const input = {
    usage: {
      prompt_tokens: 10,
      completion_tokens: 5,
      total_tokens: 15,
      prompt_cache_hit_tokens: 8,
    },
  }
  const result = normalize_usage_shape(input)
  assert.equal(result.usage.prompt_tokens, '<normalized:number>')
  assert.equal(result.usage.completion_tokens, '<normalized:number>')
  assert.equal(result.usage.total_tokens, '<normalized:number>')
  assert.equal(result.usage.prompt_cache_hit_tokens, '<normalized:number>', 'extension field preserved as key')
})

test('normalize_usage_shape: handles nested details', () => {
  const input = {
    usage: {
      prompt_tokens: 10,
      completion_tokens: 5,
      completion_tokens_details: {
        reasoning_tokens: 2,
        accepted_prediction_tokens: 0,
      },
    },
  }
  const result = normalize_usage_shape(input)
  assert.equal(result.usage.completion_tokens_details.reasoning_tokens, '<normalized:number>')
})

test('normalize_usage_shape: no-op when usage is absent', () => {
  const input = { choices: [{ message: { content: 'hi' } }] }
  const result = normalize_usage_shape(input)
  assert.ok(!result.usage)
})

test('normalize_tool_call: canonicalizes JSON key order', () => {
  const input = {
    choices: [{
      message: {
        tool_calls: [{
          id: 'call_1',
          type: 'function',
          function: { name: 'fn', arguments: '{"b":2,"a":1}' },
        }],
      },
    }],
  }
  const result = normalize_tool_call(input)
  assert.equal(result.choices[0].message.tool_calls[0].function.arguments, '{"a":1,"b":2}')
})

test('normalize_tool_call: preserves invalid JSON arguments (for diagnostic)', () => {
  const input = {
    choices: [{
      message: {
        tool_calls: [{
          id: 'call_1',
          type: 'function',
          function: { name: 'fn', arguments: 'not json' },
        }],
      },
    }],
  }
  const result = normalize_tool_call(input)
  assert.equal(result.choices[0].message.tool_calls[0].function.arguments, 'not json')
})

test('normalize_stream_events: extracts structural summary', () => {
  const chunks = [
    { id: 'chunk-1', choices: [{ delta: { role: 'assistant' }, finish_reason: null }] },
    { id: 'chunk-2', choices: [{ delta: { content: 'hello' }, finish_reason: null }] },
    { id: 'chunk-3', choices: [{ delta: { content: ' world' }, finish_reason: null }] },
    { id: 'chunk-4', choices: [{ delta: {}, finish_reason: 'stop' }] },
    '[DONE]',
  ]
  const summary = normalize_stream_events(chunks)
  assert.equal(summary.total_chunks, 4, '4 data chunks excluding [DONE]')
  assert.equal(summary.has_done, true)
  assert.equal(summary.has_content, true)
  assert.equal(summary.final_finish_reason, 'stop')
  assert.equal(summary.role, 'assistant')
  assert.deepEqual(summary.tool_call_names, [])
})

test('normalize_stream_events: detects tool calls in stream', () => {
  const chunks = [
    { choices: [{ delta: { role: 'assistant', tool_calls: [{ index: 0, function: { name: 'get_weather', arguments: '' } }] }, finish_reason: null }] },
    { choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: '{"loc' } }] }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'tool_calls' }] },
    '[DONE]',
  ]
  const summary = normalize_stream_events(chunks)
  assert.deepEqual(summary.tool_call_names, ['get_weather'])
  assert.equal(summary.final_finish_reason, 'tool_calls')
  assert.equal(summary.has_content, false)
})

test('normalize_stream_events: handles empty stream', () => {
  const summary = normalize_stream_events([])
  assert.equal(summary.total_chunks, 0)
  assert.equal(summary.has_done, false)
  assert.equal(summary.has_content, false)
})

test('applyNormalizers: applies multiple normalizers in order', () => {
  const input = {
    id: 'chatcmpl-abc',
    created: 1700000000,
    choices: [{ finish_reason: 'end_turn', message: { content: 'hi' } }],
    usage: { prompt_tokens: 10, completion_tokens: 5 },
  }
  const { normalized, applied } = applyNormalizers(input, [
    'strip_dynamic_ids',
    'strip_timestamps',
    'normalize_finish_reason',
    'normalize_usage_shape',
  ])
  assert.equal(normalized.id, '<stripped:dynamic_id>')
  assert.equal(normalized.created, '<stripped:timestamp>')
  assert.equal(normalized.choices[0].finish_reason, 'stop')
  assert.equal(normalized.usage.prompt_tokens, '<normalized:number>')
  assert.deepEqual(applied, ['strip_dynamic_ids', 'strip_timestamps', 'normalize_finish_reason', 'normalize_usage_shape'])
})

test('applyNormalizers: throws on unknown normalizer', () => {
  assert.throws(() => applyNormalizers({}, ['nonexistent_normalizer']), /Unknown normalizer/)
})

test('applyNormalizers: applies per-chunk normalizers to stream arrays', () => {
  const chunks = [
    { id: 'chunk-1', created: 1700000000, choices: [{ delta: { content: 'hi' } }] },
    { id: 'chunk-2', created: 1700000001, choices: [{ delta: { content: '!' } }] },
    '[DONE]',
  ]
  const { normalized } = applyNormalizers(chunks, ['strip_dynamic_ids', 'strip_timestamps'])
  assert.equal(normalized[0].id, '<stripped:dynamic_id>')
  assert.equal(normalized[0].created, '<stripped:timestamp>')
  assert.equal(normalized[1].id, '<stripped:dynamic_id>')
  assert.equal(normalized[2], '[DONE]', '[DONE] sentinel preserved')
})

test('listNormalizers: returns documented normalizer list', () => {
  const list = listNormalizers()
  assert.ok(list.length >= 6, 'at least 6 normalizers documented')
  for (const n of list) {
    assert.ok(n.name, 'has name')
    assert.ok(Array.isArray(n.fields), 'has fields array')
    assert.ok(n.reason, 'has reason')
  }
})

// ════════════════════════════════════════════════════════════════════════════
// §2 — Classification matrix tests
// ════════════════════════════════════════════════════════════════════════════

test('classification: Both sides fail with same HTTP status → UPSTREAM_LIMITATION', () => {
  const direct = { _meta: { http_status: 429 }, error: { message: 'rate limited', type: 'rate_limit_error' } }
  const gateway = { _meta: { http_status: 429 }, error: { message: 'rate limited', type: 'rate_limit_error' } }
  const assertions = [{ name: 'http_status_match', passed: true, message: null }]
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'UPSTREAM_LIMITATION')
})

test('classification: Both sides fail with same class (5xx) → UPSTREAM_LIMITATION', () => {
  const direct = { _meta: { http_status: 503 } }
  const gateway = { _meta: { http_status: 502 } }
  const assertions = []
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'UPSTREAM_LIMITATION')
})

test('classification: Direct PASS + Gateway FAIL → GATEWAY_BUG', () => {
  const direct = { _meta: { http_status: 200 }, choices: [{ message: { content: 'ok' } }] }
  const gateway = { _meta: { http_status: 500 }, error: { message: 'internal error' } }
  const assertions = [{ name: 'http_status_match', passed: false, message: 'Direct: 200, Gateway: 500' }]
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'GATEWAY_BUG')
})

test('classification: Both PASS, all assertions pass → PASS', () => {
  const direct = { _meta: { http_status: 200 } }
  const gateway = { _meta: { http_status: 200 } }
  const assertions = [
    { name: 'envelope_parseable', passed: true, message: null },
    { name: 'finish_reason_present', passed: true, message: null },
  ]
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'PASS')
  assert.equal(failure_class, null)
})

test('classification: Both PASS but Gateway drops field → GATEWAY_BUG', () => {
  const direct = { _meta: { http_status: 200 }, usage: { prompt_tokens: 5 } }
  const gateway = { _meta: { http_status: 200 } }
  const assertions = [
    { name: 'usage_present', passed: false, message: 'Direct has usage but Gateway does not' },
  ]
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'GATEWAY_BUG')
})

test('classification: Transport error → FLAKY', () => {
  const direct = { _meta: { http_status: 0, transport_error: true, error_message: 'ECONNREFUSED' } }
  const gateway = { _meta: { http_status: 200 } }
  const assertions = []
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'FLAKY')
})

test('classification: Gateway transport error → FLAKY', () => {
  const direct = { _meta: { http_status: 200 } }
  const gateway = { _meta: { http_status: 0, transport_error: true, error_message: 'timeout' } }
  const assertions = []
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'FLAKY')
})

test('classification: Model did not produce expected output → MODEL_BEHAVIOR', () => {
  const direct = { _meta: { http_status: 200 } }
  const gateway = { _meta: { http_status: 200 } }
  const assertions = [
    { name: 'tool_call_present', passed: false, message: 'Direct did not produce tool_calls (model behavior)' },
  ]
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'FAIL')
  assert.equal(failure_class, 'MODEL_BEHAVIOR')
})

test('classification: Direct FAIL + Gateway PASS → PASS (gateway fallback may succeed)', () => {
  const direct = { _meta: { http_status: 503 } }
  const gateway = { _meta: { http_status: 200 } }
  const assertions = []
  const { result, failure_class } = classifyResult(direct, gateway, assertions, {})
  assert.equal(result, 'PASS')
  assert.equal(failure_class, null)
})

// ════════════════════════════════════════════════════════════════════════════
// §3 — compareResponses integration tests (with stubs)
// ════════════════════════════════════════════════════════════════════════════

test('compareResponses: text.basic PASS with matching responses', () => {
  const directRaw = {
    id: 'chatcmpl-direct-123',
    object: 'chat.completion',
    created: 1700000000,
    choices: [{
      index: 0,
      message: { role: 'assistant', content: 'hello' },
      finish_reason: 'stop',
    }],
    usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 },
    _meta: { http_status: 200 },
  }
  const gatewayRaw = {
    id: 'chatcmpl-gateway-456',
    object: 'chat.completion',
    created: 1700000005,
    choices: [{
      index: 0,
      message: { role: 'assistant', content: 'hello' },
      finish_reason: 'stop',
    }],
    usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 },
    _meta: { http_status: 200 },
  }
  const caseSpec = {
    case_id: 'text.basic',
    protocol: 'openai_chat_completions',
    feature: 'text',
    provider: 'deepseek',
    model_default: 'deepseek-chat',
    stream: false,
    normalizers: ['strip_dynamic_ids', 'strip_timestamps', 'normalize_finish_reason', 'normalize_usage_shape'],
    assertions: [
      { name: 'http_status_match' },
      { name: 'envelope_parseable' },
      { name: 'finish_reason_present' },
      { name: 'usage_fields_present' },
      { name: 'choices_non_empty' },
      { name: 'content_non_empty' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 100)
  assert.equal(result.result, 'PASS')
  assert.equal(result.failure_class, null)
  assert.equal(result.case_id, 'differential.text.basic')
  assert.equal(result.protocol_in, 'openai_chat_completions')
  assert.equal(result.feature, 'text')
  assert.equal(result.stream, false)
  assert.ok(result.evidence.assertions.length > 0)
  assert.ok(result.evidence.assertions.every((a) => a.passed))
})

test('compareResponses: Gateway drops usage → GATEWAY_BUG detected', () => {
  const directRaw = {
    id: 'chatcmpl-1',
    created: 1700000000,
    choices: [{ index: 0, message: { content: 'ok' }, finish_reason: 'stop' }],
    usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 },
    _meta: { http_status: 200 },
  }
  const gatewayRaw = {
    id: 'chatcmpl-2',
    created: 1700000001,
    choices: [{ index: 0, message: { content: 'ok' }, finish_reason: 'stop' }],
    _meta: { http_status: 200 },
    // usage intentionally missing
  }
  const caseSpec = {
    case_id: 'usage.basic',
    protocol: 'openai_chat_completions',
    feature: 'usage',
    provider: 'deepseek',
    model_default: 'deepseek-chat',
    stream: false,
    normalizers: ['strip_dynamic_ids', 'strip_timestamps', 'normalize_usage_shape'],
    assertions: [
      { name: 'http_status_match' },
      { name: 'usage_present' },
      { name: 'usage_shape_match' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 50)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
  const usageAssertion = result.evidence.assertions.find((a) => a.name === 'usage_present')
  assert.ok(usageAssertion)
  assert.equal(usageAssertion.passed, false)
})

test('compareResponses: Both sides 429 → UPSTREAM_LIMITATION', () => {
  const directRaw = {
    error: { message: 'Rate limit exceeded', type: 'rate_limit_error' },
    _meta: { http_status: 429 },
  }
  const gatewayRaw = {
    error: { message: 'Rate limit exceeded', type: 'rate_limit_error' },
    _meta: { http_status: 429 },
  }
  const caseSpec = {
    case_id: 'error.rate_limit_429',
    protocol: 'openai_chat_completions',
    feature: 'error_envelope',
    provider: 'deepseek',
    model_default: 'deepseek-chat',
    stream: false,
    normalizers: ['strip_dynamic_ids', 'strip_timestamps'],
    assertions: [
      { name: 'http_status_match' },
      { name: 'error_envelope_present' },
      { name: 'error_type_present' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 50)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'UPSTREAM_LIMITATION')
})

test('compareResponses: stream.basic PASS with matching stream', () => {
  const directChunks = [
    { id: 'd-1', choices: [{ delta: { role: 'assistant' }, finish_reason: null }] },
    { id: 'd-2', choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { id: 'd-3', choices: [{ delta: {}, finish_reason: 'stop' }] },
    '[DONE]',
  ]
  const gatewayChunks = [
    { id: 'g-1', choices: [{ delta: { role: 'assistant' }, finish_reason: null }] },
    { id: 'g-2', choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { id: 'g-3', choices: [{ delta: {}, finish_reason: 'stop' }] },
    '[DONE]',
  ]
  const directRaw = { _meta: { http_status: 200 }, _stream_chunks: directChunks }
  const gatewayRaw = { _meta: { http_status: 200 }, _stream_chunks: gatewayChunks }
  const caseSpec = {
    case_id: 'stream.basic',
    protocol: 'openai_chat_completions',
    feature: 'streaming',
    provider: 'deepseek',
    model_default: 'deepseek-chat',
    stream: true,
    normalizers: ['strip_dynamic_ids', 'strip_timestamps', 'normalize_finish_reason', 'normalize_stream_events'],
    assertions: [
      { name: 'http_status_match' },
      { name: 'stream_has_chunks' },
      { name: 'stream_has_done' },
      { name: 'finish_reason_present' },
      { name: 'content_chunks_present' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 200)
  assert.equal(result.result, 'PASS')
  assert.equal(result.failure_class, null)
  assert.equal(result.stream, true)
})

test('compareResponses: tool.single with matching tool calls', () => {
  const directRaw = {
    id: 'chatcmpl-d',
    created: 1700000000,
    choices: [{
      index: 0,
      message: {
        role: 'assistant',
        content: null,
        tool_calls: [{
          id: 'call_direct_1',
          type: 'function',
          function: { name: 'get_weather', arguments: '{"location":"Tokyo"}' },
        }],
      },
      finish_reason: 'tool_calls',
    }],
    usage: { prompt_tokens: 20, completion_tokens: 15, total_tokens: 35 },
    _meta: { http_status: 200 },
  }
  const gatewayRaw = {
    id: 'chatcmpl-g',
    created: 1700000002,
    choices: [{
      index: 0,
      message: {
        role: 'assistant',
        content: null,
        tool_calls: [{
          id: 'call_gw_99',
          type: 'function',
          function: { name: 'get_weather', arguments: '{"location": "Tokyo"}' },
        }],
      },
      finish_reason: 'tool_calls',
    }],
    usage: { prompt_tokens: 20, completion_tokens: 15, total_tokens: 35 },
    _meta: { http_status: 200 },
  }
  const caseSpec = {
    case_id: 'tool.single',
    protocol: 'openai_chat_completions',
    feature: 'tools',
    provider: 'deepseek',
    model_default: 'deepseek-chat',
    stream: false,
    normalizers: ['strip_dynamic_ids', 'strip_timestamps', 'normalize_finish_reason', 'normalize_usage_shape', 'normalize_tool_call'],
    assertions: [
      { name: 'http_status_match' },
      { name: 'tool_call_present' },
      { name: 'tool_name_match' },
      { name: 'tool_args_valid_json' },
      { name: 'tool_call_id_present' },
      { name: 'finish_reason_tool_calls' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 300)
  assert.equal(result.result, 'PASS')
  assert.equal(result.failure_class, null)
})

// ════════════════════════════════════════════════════════════════════════════
// §4 — Argument parsing tests
// ════════════════════════════════════════════════════════════════════════════

test('parseArguments: defaults', () => {
  const args = parseArguments([])
  assert.deepEqual(args.providers, [])
  assert.equal(args.model, null)
  assert.deepEqual(args.cases, [])
  assert.equal(args.list, false)
  assert.equal(args.timeout, 30_000)
})

test('parseArguments: --provider and --case', () => {
  const args = parseArguments(['--provider', 'deepseek', '--case', 'text.basic', '--case', 'stream.basic'])
  assert.deepEqual(args.providers, ['deepseek'])
  assert.deepEqual(args.cases, ['text.basic', 'stream.basic'])
})

test('parseArguments: --model override', () => {
  const args = parseArguments(['--model', 'deepseek-reasoner'])
  assert.equal(args.model, 'deepseek-reasoner')
})

test('parseArguments: equals-style arguments', () => {
  const args = parseArguments(['--provider=deepseek', '--case=tool.single', '--model=test', '--timeout=60000'])
  assert.deepEqual(args.providers, ['deepseek'])
  assert.deepEqual(args.cases, ['tool.single'])
  assert.equal(args.model, 'test')
  assert.equal(args.timeout, 60000)
})

test('parseArguments: --list flag', () => {
  const args = parseArguments(['--list'])
  assert.equal(args.list, true)
})

test('parseArguments: --include-error and --include-high-cost flags', () => {
  const args = parseArguments(['--include-error', '--include-high-cost'])
  assert.equal(args.includeError, true)
  assert.equal(args.includeHighCost, true)
})

test('parseArguments: include flags default to false', () => {
  const args = parseArguments([])
  assert.equal(args.includeError, false)
  assert.equal(args.includeHighCost, false)
})

// ════════════════════════════════════════════════════════════════════════════
// §5 — Case loading and filtering tests
// ════════════════════════════════════════════════════════════════════════════

test('loadCases: loads and parses cases.json', () => {
  const cases = loadCases()
  assert.ok(Array.isArray(cases))
  assert.ok(cases.length >= 6, 'at least 6 cases defined')
  for (const c of cases) {
    assert.ok(c.case_id, 'each case has case_id')
    assert.ok(c.provider, 'each case has provider')
    assert.ok(c.protocol, 'each case has protocol')
    assert.ok(c.feature, 'each case has feature')
    assert.ok(Array.isArray(c.normalizers), 'each case has normalizers array')
    assert.ok(Array.isArray(c.assertions), 'each case has assertions array')
  }
})

test('loadCases: all case_ids match schema pattern', () => {
  const cases = loadCases()
  const pattern = /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$/
  for (const c of cases) {
    // Prefix 'differential.' is added by compareResponses; raw case_id must be valid base
    assert.match(`differential.${c.case_id}`, pattern, `case_id 'differential.${c.case_id}' must match pattern`)
  }
})

test('loadCases: required 6 case types present', () => {
  const cases = loadCases()
  const ids = cases.map((c) => c.case_id)
  assert.ok(ids.includes('text.basic'), 'text.basic present')
  assert.ok(ids.includes('stream.basic'), 'stream.basic present')
  assert.ok(ids.includes('tool.single'), 'tool.single present')
  assert.ok(ids.includes('tool.roundtrip'), 'tool.roundtrip present')
  assert.ok(ids.includes('usage.basic'), 'usage.basic present')
  assert.ok(ids.includes('error.rate_limit_429'), 'error.rate_limit_429 present')
})

test('filterCases: no filter excludes error_expected cases by default', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: [], cases: [] })
  assert.equal(filtered.length, cases.length - 1, 'error.rate_limit_429 excluded by default')
  assert.ok(!filtered.some((c) => c.error_expected), 'no error_expected case in default selection')
})

test('filterCases: --include-error re-includes error cases', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: [], cases: [], includeError: true })
  assert.equal(filtered.length, cases.length)
})

test('filterCases: explicit --case re-includes an excluded error case', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: [], cases: ['error.rate_limit_429'] })
  assert.equal(filtered.length, 1)
  assert.equal(filtered[0].case_id, 'error.rate_limit_429')
})

test('filterCases: high-cost cases excluded by default unless --include-high-cost', () => {
  const cases = [
    { case_id: 'text.basic', provider: 'p', cost: 'low' },
    { case_id: 'search.deep', provider: 'p', cost: 'high' },
  ]
  const filtered = filterCases(cases, { providers: [], cases: [] })
  assert.deepEqual(filtered.map((c) => c.case_id), ['text.basic'])
  const included = filterCases(cases, { providers: [], cases: [], includeHighCost: true })
  assert.equal(included.length, 2)
})

test('filterCases: filter by provider', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: ['deepseek'], cases: [] })
  assert.ok(filtered.length > 0)
  assert.ok(filtered.every((c) => c.provider === 'deepseek'))
})

test('filterCases: filter by case_id', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: [], cases: ['text.basic', 'stream.basic'] })
  assert.equal(filtered.length, 2)
  assert.deepEqual(filtered.map((c) => c.case_id).sort(), ['stream.basic', 'text.basic'])
})

test('filterCases: unknown provider returns empty', () => {
  const cases = loadCases()
  const filtered = filterCases(cases, { providers: ['nonexistent'], cases: [] })
  assert.equal(filtered.length, 0)
})

// ════════════════════════════════════════════════════════════════════════════
// §6 — Schema conformance of test results
// ════════════════════════════════════════════════════════════════════════════

test('compareResponses output has all required schema fields', () => {
  const directRaw = {
    id: 'test',
    choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }],
    usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    _meta: { http_status: 200 },
  }
  const gatewayRaw = { ...directRaw, id: 'test2' }
  const caseSpec = {
    case_id: 'text.basic',
    protocol: 'openai_chat_completions',
    feature: 'text',
    provider: 'test',
    model_default: 'test-model',
    stream: false,
    normalizers: ['strip_dynamic_ids'],
    assertions: [{ name: 'http_status_match' }],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)

  const requiredFields = [
    'case_id', 'protocol_in', 'result', 'feature',
    'stream', 'duration_ms', 'tool_name', 'tool_version', 'timestamp',
  ]
  for (const field of requiredFields) {
    assert.ok(result[field] !== undefined, `required field '${field}' present`)
  }

  assert.match(result.case_id, /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$/)

  // If FAIL, failure_class must be present
  if (result.result === 'FAIL') {
    assert.ok(result.failure_class, 'failure_class present when result is FAIL')
  }
})

// ════════════════════════════════════════════════════════════════════════════
// §7 — Edge cases and robustness
// ════════════════════════════════════════════════════════════════════════════

test('compareResponses: Gateway parse error detected', () => {
  const directRaw = {
    choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }],
    _meta: { http_status: 200 },
  }
  const gatewayRaw = {
    _meta: { http_status: 200, parse_error: true },
  }
  const caseSpec = {
    case_id: 'text.basic',
    protocol: 'openai_chat_completions',
    feature: 'text',
    provider: 'test',
    model_default: 'test-model',
    stream: false,
    normalizers: [],
    assertions: [{ name: 'envelope_parseable' }],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
})

test('compareResponses: Gateway missing tool_calls when Direct has them → GATEWAY_BUG', () => {
  const directRaw = {
    choices: [{
      message: {
        tool_calls: [{ id: 'c1', type: 'function', function: { name: 'fn', arguments: '{}' } }],
      },
      finish_reason: 'tool_calls',
    }],
    _meta: { http_status: 200 },
  }
  const gatewayRaw = {
    choices: [{
      message: { content: 'I cannot call tools' },
      finish_reason: 'stop',
    }],
    _meta: { http_status: 200 },
  }
  const caseSpec = {
    case_id: 'tool.single',
    protocol: 'openai_chat_completions',
    feature: 'tools',
    provider: 'test',
    model_default: 'test-model',
    stream: false,
    normalizers: ['strip_dynamic_ids'],
    assertions: [
      { name: 'tool_call_present' },
      { name: 'tool_name_match' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
})

test('compareResponses: stream with Gateway missing [DONE] when Direct has it', () => {
  const directChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'stop' }] },
    '[DONE]',
  ]
  const gatewayChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'stop' }] },
    // [DONE] missing
  ]
  const directRaw = { _meta: { http_status: 200 }, _stream_chunks: directChunks }
  const gatewayRaw = { _meta: { http_status: 200 }, _stream_chunks: gatewayChunks }
  const caseSpec = {
    case_id: 'stream.basic',
    protocol: 'openai_chat_completions',
    feature: 'streaming',
    provider: 'test',
    model_default: 'test-model',
    stream: true,
    normalizers: ['strip_dynamic_ids', 'normalize_stream_events'],
    assertions: [
      { name: 'stream_has_done' },
      { name: 'stream_has_chunks' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
  const doneAssertion = result.evidence.assertions.find((a) => a.name === 'stream_has_done')
  assert.equal(doneAssertion.passed, false)
})

test('compareResponses: stream with Gateway dropping final finish_reason → GATEWAY_BUG', () => {
  const directChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'stop' }] },
    '[DONE]',
  ]
  const gatewayChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {} }] }, // final chunk lost finish_reason
    '[DONE]',
  ]
  const directRaw = { _meta: { http_status: 200 }, _stream_chunks: directChunks }
  const gatewayRaw = { _meta: { http_status: 200 }, _stream_chunks: gatewayChunks }
  const caseSpec = {
    case_id: 'stream.basic',
    protocol: 'openai_chat_completions',
    feature: 'streaming',
    provider: 'test',
    model_default: 'test-model',
    stream: true,
    normalizers: ['strip_dynamic_ids', 'normalize_stream_events'],
    assertions: [
      { name: 'finish_reason_present' },
      { name: 'stream_has_done' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
  const frAssertion = result.evidence.assertions.find((a) => a.name === 'finish_reason_present')
  assert.equal(frAssertion.passed, false, 'stream finish_reason drop must be detected via _stream_summary')
})

test('compareResponses: stream with Gateway dropping usage chunk → GATEWAY_BUG', () => {
  const directChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'stop' }] },
    { choices: [], usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 } },
    '[DONE]',
  ]
  const gatewayChunks = [
    { choices: [{ delta: { content: 'hi' }, finish_reason: null }] },
    { choices: [{ delta: {}, finish_reason: 'stop' }] },
    // usage chunk dropped
    '[DONE]',
  ]
  const directRaw = { _meta: { http_status: 200 }, _stream_chunks: directChunks }
  const gatewayRaw = { _meta: { http_status: 200 }, _stream_chunks: gatewayChunks }
  const caseSpec = {
    case_id: 'stream.basic',
    protocol: 'openai_chat_completions',
    feature: 'streaming',
    provider: 'test',
    model_default: 'test-model',
    stream: true,
    normalizers: ['strip_dynamic_ids', 'normalize_stream_events'],
    assertions: [
      { name: 'stream_usage_present' },
      { name: 'finish_reason_present' },
    ],
  }
  const result = compareResponses(directRaw, gatewayRaw, caseSpec, 10)
  assert.equal(result.result, 'FAIL')
  assert.equal(result.failure_class, 'GATEWAY_BUG')
  const usageAssertion = result.evidence.assertions.find((a) => a.name === 'stream_usage_present')
  assert.equal(usageAssertion.passed, false)
})
