/**
 * conformance.test.mjs — Unit tests for conformance runner helpers and normalizers.
 *
 * Uses node:test (no external dependencies). Does not consume real tokens.
 * Tests the runner helper functions, report normalization, and failure paths.
 */

import assert from 'node:assert/strict'
import test from 'node:test'

import {
  loadVersions,
  loadConfig,
  parseArguments,
  REPO_ROOT,
} from './runner-helpers.mjs'

import {
  normalizeLlmprobe,
  normalizeCompatcanary,
  normalizeSdkSmoke,
  gatewayCommit,
} from './normalize-report.mjs'

import {
  buildLlmprobeArgs,
  filterNormalizedResults,
} from './run-llmprobe.mjs'

// ── Version loading ─────────────────────────────────────────────────────────

test('loadVersions returns all required tool versions', () => {
  const versions = loadVersions()
  assert.ok(versions.llmprobe, 'llmprobe version present')
  assert.ok(versions.compatcanary, 'compatcanary version present')
  assert.ok(versions.k6, 'k6 version present')
  assert.ok(versions.xk6_sse, 'xk6_sse version present')
  assert.ok(!versions.llmprobe.includes('latest'), 'no latest in llmprobe')
  assert.ok(!versions.compatcanary.includes('latest'), 'no latest in compatcanary')
})

test('loadConfig returns valid conformance config', () => {
  const config = loadConfig()
  assert.ok(config.targets, 'has targets')
  assert.ok(config.targets.local, 'has local target')
  assert.ok(config.targets.external, 'has external target')
  assert.equal(config.default_target, 'local')
  assert.ok(config.report_dir, 'has report_dir')
  assert.ok(config.targets.local.managed === true, 'local target is managed')
  assert.ok(config.targets.external.managed === false, 'external target is not managed')
})

// ── Argument parsing ────────────────────────────────────────────────────────

test('parseArguments defaults to local target', () => {
  const args = parseArguments([])
  assert.equal(args.target, 'local')
  assert.deepEqual(args.specs, [])
  assert.equal(args.filter, null)
  assert.equal(args.dryRun, false)
})

test('parseArguments handles --target and --spec', () => {
  const args = parseArguments([
    '--target', 'external',
    '--spec', 'chat-completions',
    '--spec=responses',
    '--dry-run',
  ])
  assert.equal(args.target, 'external')
  assert.deepEqual(args.specs, ['chat-completions', 'responses'])
  assert.equal(args.dryRun, true)
})

test('parseArguments handles --filter', () => {
  const args = parseArguments(['--filter=streaming'])
  assert.equal(args.filter, 'streaming')
})

// ── llmprobe normalization ──────────────────────────────────────────────────

/** Minimal raw report mirroring the real llmprobe 0.6.x `--json` shape. */
function llmprobeRaw(results, phases = {}) {
  return {
    version: 2,
    run: {
      depth: 'quick',
      startedAt: '2026-09-04T00:00:00.000Z',
      phases: {
        coverage: { status: 'measured' },
        conformance: { status: 'partial', reason: 'quick depth omits slow conformance checks' },
        capability: { status: 'not-run', reason: 'quick depth omits capability evals' },
        ...phases,
      },
    },
    target: { baseUrl: 'http://127.0.0.1:1', model: 'conformance-test-model' },
    conformance: {
      passed: results.filter((r) => r.outcome === 'pass').length,
      total: results.length,
      results,
    },
  }
}

test('normalizeLlmprobe converts real-shape report to unified schema', () => {
  const raw = llmprobeRaw([
    {
      id: 'chat-basic',
      name: 'chat/completions: basic completion',
      surface: 'chat',
      outcome: 'pass',
      durationMs: 42,
      assertions: [
        { id: 'chat-schema', label: 'response matches the spec schema', severity: 'MUST', passed: true },
      ],
      failures: [],
    },
    {
      id: 'chat-streaming',
      name: 'chat/completions: streaming',
      surface: 'chat',
      outcome: 'fail',
      durationMs: 57,
      assertions: [
        { id: 'sse-missing-done', label: 'SSE framing', severity: 'MUST', passed: false, message: 'no [DONE]' },
      ],
      failures: [
        { id: 'sse-missing-done', label: 'SSE framing', severity: 'MUST', message: 'no [DONE]' },
      ],
    },
    {
      id: 'chat-unicode',
      name: 'chat/completions: unicode',
      surface: 'chat',
      outcome: 'skipped',
      reason: 'not run at depth "quick"',
      durationMs: 0,
      assertions: [],
      failures: [],
    },
  ])
  const report = normalizeLlmprobe(raw)
  assert.equal(report.tool, 'llmprobe')
  assert.ok(report.tool_version)
  assert.equal(report.results.length, 3)

  const pass = report.results.find((r) => r.result === 'PASS')
  assert.ok(pass)
  assert.equal(pass.case_id, 'chat.text.basic')
  assert.equal(pass.protocol_in, 'openai_chat_completions')
  assert.equal(pass.failure_class, null)
  assert.equal(pass.duration_ms, 42)
  assert.deepEqual(pass.evidence.assertions, [
    { name: 'MUST:chat-schema', passed: true, message: null },
  ])

  const fail = report.results.find((r) => r.result === 'FAIL')
  assert.ok(fail)
  assert.equal(fail.case_id, 'chat.streaming.streaming')
  assert.equal(fail.failure_class, 'GATEWAY_BUG', 'MUST failure on a shipped surface is a gateway bug until triaged')
  assert.ok(fail.stream, 'streaming case detected as stream')
  assert.ok(fail.evidence.notes.includes('no [DONE]'))

  const skip = report.results.find((r) => r.result === 'SKIPPED')
  assert.ok(skip)
  assert.equal(skip.failure_class, 'TOOL_LIMITATION')
})

test('normalizeLlmprobe maps surfaces to gateway protocols', () => {
  const raw = llmprobeRaw([
    { id: 'models-list', name: 'models', surface: 'models', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
    { id: 'responses-basic', name: 'responses: basic', surface: 'responses', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
    { id: 'messages-basic', name: 'messages: basic', surface: 'messages', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
    { id: 'count-tokens', name: 'count_tokens', surface: 'count-tokens', outcome: 'unsupported', reason: 'endpoint not found', durationMs: 0, assertions: [], failures: [] },
  ])
  const report = normalizeLlmprobe(raw)
  const byCase = Object.fromEntries(report.results.map((r) => [r.case_id, r]))
  assert.equal(byCase['models.text.list'].protocol_in, 'openai_chat_completions')
  assert.equal(byCase['responses.text.basic'].protocol_in, 'openai_responses')
  assert.equal(byCase['messages.text.basic'].protocol_in, 'anthropic_messages')
  assert.equal(byCase['count_tokens.text.count_tokens'].protocol_in, 'anthropic_messages')
})

test('normalizeLlmprobe: unsupported on claimed surface is FAIL, optional surface is UNSUPPORTED', () => {
  const raw = llmprobeRaw([
    { id: 'chat-basic', name: 'chat', surface: 'chat', outcome: 'unsupported', reason: 'chat not implemented', durationMs: 0, assertions: [], failures: [] },
    { id: 'completions-basic', name: 'legacy completions', surface: 'completions', outcome: 'unsupported', reason: 'endpoint not found', durationMs: 0, assertions: [], failures: [] },
  ])
  const report = normalizeLlmprobe(raw)
  const chat = report.results.find((r) => r.case_id === 'chat.text.basic')
  assert.equal(chat.result, 'FAIL', 'gateway claims chat; llmprobe calling it unimplemented means the endpoint broke')
  assert.equal(chat.failure_class, 'GATEWAY_BUG')
  const legacy = report.results.find((r) => r.case_id === 'completions.text.basic')
  assert.equal(legacy.result, 'UNSUPPORTED')
  assert.equal(legacy.failure_class, null)
  assert.equal(legacy.mode, 'unsupported')
})

test('normalizeLlmprobe: out-of-scope surfaces become metadata, not cases', () => {
  const raw = llmprobeRaw([
    { id: 'embeddings-basic', name: 'embeddings', surface: 'embeddings', outcome: 'unsupported', durationMs: 0, assertions: [], failures: [] },
    { id: 'images-generate', name: 'images', surface: 'images', outcome: 'unsupported', durationMs: 0, assertions: [], failures: [] },
    { id: 'chat-basic', name: 'chat', surface: 'chat', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
  ])
  const report = normalizeLlmprobe(raw)
  assert.equal(report.results.length, 1)
  assert.equal(report.out_of_scope_surfaces.length, 2)
  assert.ok(report.out_of_scope_surfaces.every((s) => s.outcome === 'unsupported'))
})

test('normalizeLlmprobe: phase status is preserved as metadata', () => {
  const report = normalizeLlmprobe(llmprobeRaw([]))
  assert.equal(report.phases.capability.status, 'not-run')
  assert.equal(report.results.length, 0)
})

test('normalizeLlmprobe applies assertion-level known gaps without masking new failures', () => {
  // tests/conformance/config.json declares the responses [DONE] over-assertion
  // as a known gap keyed on failure id responses-sse-missing-done.
  const onlyGapFailure = llmprobeRaw([
    {
      id: 'responses-streaming',
      name: 'responses: streaming',
      surface: 'responses',
      outcome: 'fail',
      durationMs: 10,
      assertions: [
        { id: 'responses-sse-missing-done', label: 'SSE framing', severity: 'MUST', passed: false, message: 'no [DONE]' },
      ],
      failures: [
        { id: 'responses-sse-missing-done', label: 'SSE framing', severity: 'MUST', message: 'no [DONE]' },
      ],
    },
  ])
  const gapped = normalizeLlmprobe(onlyGapFailure).results[0]
  assert.equal(gapped.result, 'SKIPPED')
  assert.equal(gapped.failure_class, 'TOOL_LIMITATION')
  assert.ok(gapped.evidence.notes.includes('known gap'))
  assert.ok(gapped.evidence.notes.includes('original outcome FAIL'))

  // An additional, unrelated MUST failure must stay FAIL.
  const extraFailure = llmprobeRaw([
    {
      id: 'responses-streaming',
      name: 'responses: streaming',
      surface: 'responses',
      outcome: 'fail',
      durationMs: 10,
      assertions: [
        { id: 'responses-sse-missing-done', label: 'SSE framing', severity: 'MUST', passed: false, message: 'no [DONE]' },
        { id: 'responses-stream-chunk-schema', label: 'chunk schema', severity: 'MUST', passed: false, message: 'bad frame' },
      ],
      failures: [],
    },
  ])
  const kept = normalizeLlmprobe(extraFailure).results[0]
  assert.equal(kept.result, 'FAIL', 'known gap must not mask an unrelated MUST failure')
  assert.equal(kept.failure_class, 'GATEWAY_BUG')
})

// ── CompatCanary normalization ──────────────────────────────────────────────

test('normalizeCompatcanary converts raw probes to unified schema', () => {
  const raw = {
    probes: [
      { name: 'models', status: 'PASS', duration_ms: 50 },
      { name: 'chat', status: 'PASS', duration_ms: 100 },
      { name: 'streaming', status: 'FAIL', duration_ms: 200, error: 'SSE parse error' },
      { name: 'tool', status: 'PASS', duration_ms: 150 },
      { name: 'structured_output', status: 'SKIP', duration_ms: 0 },
      { name: 'responses', status: 'PASS', duration_ms: 120 },
      { name: 'responses_streaming', status: 'PASS', duration_ms: 180 },
    ],
  }
  const report = normalizeCompatcanary(raw)
  assert.equal(report.tool, 'compatcanary')
  assert.ok(report.tool_version)
  assert.equal(report.results.length, 7)

  const streaming = report.results.find((r) => r.case_id.includes('streaming') && !r.case_id.includes('responses'))
  assert.ok(streaming)
  assert.equal(streaming.result, 'FAIL')
  assert.equal(streaming.stream, true)
  assert.equal(streaming.failure_class, 'TOOL_LIMITATION')

  const responses = report.results.find((r) => r.case_id === 'responses.text.cc_responses')
  assert.ok(responses)
  assert.equal(responses.protocol_in, 'openai_responses')
})

test('normalizeCompatcanary handles empty probes', () => {
  const report = normalizeCompatcanary({ probes: [] })
  assert.equal(report.results.length, 0)
})

// ── SDK smoke normalization ─────────────────────────────────────────────────

test('normalizeSdkSmoke converts SDK results to unified schema', () => {
  const results = [
    {
      case_id: 'chat.text.sdk_basic',
      protocol_in: 'openai_chat_completions',
      feature: 'text',
      stream: false,
      model: 'test-model',
      result: 'PASS',
      duration_ms: 50,
      evidence: { assertions: [{ name: 'has_id', passed: true }], notes: null },
    },
    {
      case_id: 'chat.streaming.sdk_stream',
      protocol_in: 'openai_chat_completions',
      feature: 'streaming',
      stream: true,
      model: 'test-model',
      result: 'FAIL',
      failure_class: 'GATEWAY_BUG',
      duration_ms: 200,
      evidence: { assertions: [{ name: 'stream', passed: false, message: 'no chunks' }], notes: 'SDK error: no chunks' },
    },
  ]
  const report = normalizeSdkSmoke(results, 'openai', '4.73.0')
  assert.equal(report.tool, 'sdk_smoke_openai')
  assert.equal(report.tool_version, '4.73.0')
  assert.equal(report.results.length, 2)
  assert.equal(report.results[0].result, 'PASS')
  assert.equal(report.results[0].failure_class, null)
  assert.equal(report.results[1].result, 'FAIL')
  assert.equal(report.results[1].failure_class, 'GATEWAY_BUG')
})

// ── SKIPPED/TOOL_LIMITATION must not map to UNSUPPORTED ─────────────────────

test('depth-skipped llmprobe cases never map to UNSUPPORTED', () => {
  const raw = llmprobeRaw([
    { id: 'chat-unicode', name: 'unicode', surface: 'chat', outcome: 'skipped', reason: 'not run at depth "quick"', durationMs: 0, assertions: [], failures: [] },
  ])
  const report = normalizeLlmprobe(raw)
  for (const r of report.results) {
    assert.notEqual(r.result, 'UNSUPPORTED', 'SKIPPED must not become UNSUPPORTED')
    assert.notEqual(r.failure_class, 'GATEWAY_BUG', 'TOOL_LIMITATION, not GATEWAY_BUG for skipped')
  }
})

test('compatcanary skip never mapped to UNSUPPORTED', () => {
  const raw = {
    probes: [{ name: 'structured_output', status: 'SKIP' }],
  }
  const report = normalizeCompatcanary(raw)
  for (const r of report.results) {
    assert.notEqual(r.result, 'UNSUPPORTED', 'SKIPPED must not become UNSUPPORTED')
  }
})

// ── Gateway commit helper ───────────────────────────────────────────────────

test('gatewayCommit returns a non-empty string', () => {
  const commit = gatewayCommit()
  assert.ok(commit.length > 0, 'commit is non-empty')
})

// ── Report structure: all required fields per test-result.schema.json ───────

test('normalized results have all required schema fields', () => {
  const report = normalizeLlmprobe(
    llmprobeRaw([
      { id: 'chat-basic', name: 'chat', surface: 'chat', outcome: 'pass', durationMs: 100, assertions: [], failures: [] },
    ]),
  )
  const requiredFields = [
    'case_id', 'protocol_in', 'result', 'feature',
    'stream', 'duration_ms', 'tool_name', 'tool_version', 'timestamp',
  ]
  for (const r of report.results) {
    for (const field of requiredFields) {
      assert.ok(
        r[field] !== undefined,
        `required field '${field}' missing in result ${r.case_id}`,
      )
    }
    assert.match(
      r.case_id,
      /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$/,
      `case_id '${r.case_id}' matches pattern`,
    )
  }
})

// ── Failure path: failures preserved as evidence ────────────────────────────

test('report includes failure info when cases fail', () => {
  const raw = llmprobeRaw([
    { id: 'chat-basic', name: 'chat: basic', surface: 'chat', outcome: 'pass', durationMs: 50, assertions: [], failures: [] },
    {
      id: 'chat-errors',
      name: 'chat: error codes and shapes',
      surface: 'chat',
      outcome: 'fail',
      durationMs: 100,
      assertions: [
        { id: 'chat-error-status', label: '4xx on malformed', severity: 'MUST', passed: false, message: 'assertion failed' },
      ],
      failures: [
        { id: 'chat-error-status', label: '4xx on malformed', severity: 'MUST', message: 'assertion failed' },
      ],
    },
  ])
  const report = normalizeLlmprobe(raw)
  assert.equal(report.results.length, 2)
  const fail = report.results.find((r) => r.result === 'FAIL')
  assert.ok(fail, 'failing result preserved')
  assert.ok(fail.evidence.notes.includes('chat-errors') || fail.evidence.notes.includes('error codes'), 'failure notes preserved')
  assert.ok(fail.evidence.notes.includes('assertion failed'), 'failure message preserved')
})

// ── llmprobe runner argument construction & result filtering ────────────────

test('buildLlmprobeArgs uses the real llmprobe CLI contract', () => {
  const args = buildLlmprobeArgs({
    version: '0.6.1',
    baseUrl: 'http://127.0.0.1:8080',
    apiKey: 'sk-test',
    model: 'test-model',
  })
  assert.equal(args[0], 'llmprobe@0.6.1', 'version pinned')
  assert.equal(args[1], 'http://127.0.0.1:8080', 'base URL is positional')
  assert.ok(args.includes('--json'), 'JSON output requested')
  assert.ok(args.includes('--no-color'), 'ANSI disabled for parseable stdout')
  assert.ok(args.includes('--no-save'), 'library writes disabled')
  assert.ok(args.includes('--quick'), 'quick depth')
  assert.ok(args.includes('--no-bench'), 'benchmark skipped')
  assert.ok(!args.includes('--spec'), 'llmprobe has no --spec flag; filtering is post-hoc')
  assert.ok(!args.includes('--base-url'), 'llmprobe has no --base-url flag')
  assert.ok(!args.includes('--format'), 'llmprobe has no --format flag')
})

test('filterNormalizedResults filters by spec surfaces and case_id substring', () => {
  const report = normalizeLlmprobe(
    llmprobeRaw([
      { id: 'chat-basic', name: 'chat', surface: 'chat', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
      { id: 'chat-streaming', name: 'chat stream', surface: 'chat', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
      { id: 'responses-basic', name: 'responses', surface: 'responses', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
      { id: 'messages-basic', name: 'messages', surface: 'messages', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
      { id: 'models-list', name: 'models', surface: 'models', outcome: 'pass', durationMs: 1, assertions: [], failures: [] },
    ]),
  )
  assert.equal(filterNormalizedResults(report, [], null).results.length, 5)

  const chatOnly = filterNormalizedResults(report, ['chat-completions'], null)
  assert.deepEqual(
    chatOnly.results.map((r) => r.case_id).sort(),
    ['chat.streaming.streaming', 'chat.text.basic', 'models.text.list'],
    'chat spec keeps chat + shared models surface',
  )

  const messagesOnly = filterNormalizedResults(report, ['anthropic-messages'], null)
  assert.deepEqual(
    messagesOnly.results.map((r) => r.case_id).sort(),
    ['messages.text.basic', 'models.text.list'],
  )

  const filtered = filterNormalizedResults(report, [], 'streaming')
  assert.deepEqual(filtered.results.map((r) => r.case_id), ['chat.streaming.streaming'])
})
