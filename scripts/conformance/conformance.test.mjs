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

test('normalizeLlmprobe converts raw report to unified schema', () => {
  const raw = {
    tests: [
      { name: 'basic_text_response', status: 'PASS', duration_ms: 120 },
      { name: 'streaming_text', status: 'FAIL', duration_ms: 340, message: 'timeout' },
      { name: 'tool_call_basic', status: 'SKIP', duration_ms: 0 },
    ],
  }
  const report = normalizeLlmprobe(raw, 'chat-completions')
  assert.equal(report.tool, 'llmprobe')
  assert.ok(report.tool_version)
  assert.equal(report.results.length, 3)

  const pass = report.results.find((r) => r.result === 'PASS')
  assert.ok(pass)
  assert.equal(pass.protocol_in, 'openai_chat_completions')
  assert.equal(pass.tool_name, 'llmprobe')
  assert.equal(pass.failure_class, null)

  const fail = report.results.find((r) => r.result === 'FAIL')
  assert.ok(fail)
  assert.equal(fail.failure_class, 'TOOL_LIMITATION')
  assert.ok(fail.stream, 'streaming case detected as stream')

  const skip = report.results.find((r) => r.result === 'SKIPPED')
  assert.ok(skip)
})

test('normalizeLlmprobe handles empty report', () => {
  const report = normalizeLlmprobe({ tests: [] }, 'responses')
  assert.equal(report.results.length, 0)
  assert.equal(report.tool, 'llmprobe')
})

test('normalizeLlmprobe maps spec to protocol correctly', () => {
  const report = normalizeLlmprobe(
    { tests: [{ name: 'test', status: 'PASS' }] },
    'anthropic-messages',
  )
  assert.equal(report.results[0].protocol_in, 'anthropic_messages')
})

test('normalizeLlmprobe handles array-format raw report', () => {
  const raw = [
    { name: 'test1', status: 'PASS', duration_ms: 50 },
  ]
  const report = normalizeLlmprobe(raw, 'chat-completions')
  assert.equal(report.results.length, 1)
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

test('tool limitation never mapped to UNSUPPORTED', () => {
  const raw = {
    tests: [{ name: 'uncovered_feature', status: 'SKIP' }],
  }
  const report = normalizeLlmprobe(raw, 'chat-completions')
  for (const r of report.results) {
    assert.notEqual(r.result, 'UNSUPPORTED', 'SKIPPED must not become UNSUPPORTED')
    if (r.failure_class) {
      assert.notEqual(r.failure_class, 'GATEWAY_BUG', 'TOOL_LIMITATION, not GATEWAY_BUG for skipped')
    }
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
    { tests: [{ name: 'test_case', status: 'PASS', duration_ms: 100 }] },
    'chat-completions',
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

// ── Failure path: runner preserves report on non-zero exit ──────────────────

test('report includes failure info when tool exits non-zero', () => {
  const raw = {
    tests: [
      { name: 'passing_test', status: 'PASS', duration_ms: 50 },
      { name: 'failing_test', status: 'FAIL', duration_ms: 100, message: 'assertion failed' },
    ],
  }
  const report = normalizeLlmprobe(raw, 'chat-completions')
  assert.equal(report.results.length, 2)
  const fail = report.results.find((r) => r.result === 'FAIL')
  assert.ok(fail, 'failing result preserved')
  assert.ok(fail.evidence.notes.includes('failing_test'), 'failure notes preserved')
})
