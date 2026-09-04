/**
 * normalize-report.mjs — Convert third-party tool output to #114 unified schema.
 *
 * Exports pure functions for programmatic use; also runnable as:
 *   node scripts/conformance/normalize-report.mjs <tool> <raw-report.json> [<output.json>]
 *
 * Rules:
 *  - Tool-uncovered capabilities → SKIPPED + TOOL_LIMITATION (never UNSUPPORTED)
 *  - Original case names are preserved as evidence.notes
 *  - tool_name and tool_version are always present
 */

import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const REPO_ROOT = resolve(__dirname, '../..')
const VERSIONS_PATH = resolve(REPO_ROOT, 'tests/tooling/versions.json')
const SCHEMA_PATH = resolve(REPO_ROOT, 'tests/coverage/test-result.schema.json')

// ── Version loading ─────────────────────────────────────────────────────────

export function loadVersions() {
  return JSON.parse(readFileSync(VERSIONS_PATH, 'utf8'))
}

// ── Git commit ──────────────────────────────────────────────────────────────

export function gatewayCommit() {
  try {
    return execSync('git rev-parse --short HEAD', { cwd: REPO_ROOT, encoding: 'utf8' }).trim()
  } catch {
    return 'unknown'
  }
}

// ── llmprobe normalization ──────────────────────────────────────────────────

const LLMPROBE_SPEC_MAP = {
  'chat-completions': 'openai_chat_completions',
  'responses': 'openai_responses',
  'anthropic-messages': 'anthropic_messages',
}

const LLMPROBE_FEATURE_MAP = {
  text: 'text',
  streaming: 'streaming',
  tool: 'tools',
  tools: 'tools',
  tool_call: 'tools',
  function_call: 'tools',
  structured_output: 'structured_output',
  json_schema: 'structured_output',
  usage: 'usage',
  multi_turn: 'multi_turn',
  error: 'error_envelope',
  reasoning: 'reasoning',
  thinking: 'thinking',
}

function guessFeature(caseName) {
  const lower = caseName.toLowerCase()
  for (const [keyword, feature] of Object.entries(LLMPROBE_FEATURE_MAP)) {
    if (lower.includes(keyword)) return feature
  }
  return 'text'
}

function guessStream(caseName) {
  const lower = caseName.toLowerCase()
  return lower.includes('stream') || lower.includes('sse')
}

function mapResult(status) {
  if (!status) return 'SKIPPED'
  const s = String(status).toUpperCase()
  if (s === 'PASS' || s === 'PASSED' || s === 'OK' || s === 'SUCCESS') return 'PASS'
  if (s === 'FAIL' || s === 'FAILED' || s === 'ERROR') return 'FAIL'
  if (s === 'SKIP' || s === 'SKIPPED' || s === 'NOT_RUN') return 'SKIPPED'
  return 'SKIPPED'
}

export function normalizeLlmprobe(rawReport, spec) {
  const versions = loadVersions()
  const toolVersion = versions.llmprobe
  const commit = gatewayCommit()
  const protocol = LLMPROBE_SPEC_MAP[spec] || 'openai_chat_completions'
  const now = new Date().toISOString()
  const results = []

  const tests = Array.isArray(rawReport)
    ? rawReport
    : rawReport.tests || rawReport.results || rawReport.cases || []

  for (const test of tests) {
    const name = test.name || test.case || test.id || 'unknown'
    const feature = guessFeature(name)
    const caseId = `${protocol.split('_').pop()}.${feature}.llmprobe_${name.replace(/[\s./]+/g, '_').toLowerCase()}`

    results.push({
      case_id: caseId,
      protocol_in: protocol,
      protocol_upstream: protocol,
      mode: 'native',
      feature,
      provider: 'mock',
      requested_model: 'conformance-test-model',
      resolved_model: 'conformance-test-model',
      result: mapResult(test.status || test.result),
      failure_class: mapResult(test.status || test.result) === 'FAIL' ? 'TOOL_LIMITATION' : null,
      http_status: test.http_status || null,
      stream: guessStream(name),
      request_id: null,
      retry_count: 0,
      fallback_count: 0,
      duration_ms: test.duration_ms || test.duration || 0,
      evidence: {
        assertions: [],
        notes: `llmprobe case: ${name}. ${test.message || test.error || ''}`.trim(),
      },
      tool_name: 'llmprobe',
      tool_version: toolVersion,
      timestamp: now,
    })
  }

  return {
    tool: 'llmprobe',
    tool_version: toolVersion,
    gateway_commit: commit,
    started_at: now,
    results,
  }
}

// ── CompatCanary normalization ──────────────────────────────────────────────

const CC_PROBE_PROTOCOL = {
  models: 'openai_chat_completions',
  chat: 'openai_chat_completions',
  streaming: 'openai_chat_completions',
  tool: 'openai_chat_completions',
  structured_output: 'openai_chat_completions',
  responses: 'openai_responses',
  responses_streaming: 'openai_responses',
}

const CC_PROBE_FEATURE = {
  models: 'text',
  list: 'text',
  basic: 'text',
  chat: 'text',
  streaming: 'streaming',
  tool: 'tools',
  tool_call: 'tools',
  structured_output: 'structured_output',
  responses: 'text',
  responses_streaming: 'streaming',
}

export function normalizeCompatcanary(rawReport) {
  const versions = loadVersions()
  const toolVersion = versions.compatcanary
  const commit = gatewayCommit()
  const now = new Date().toISOString()
  const results = []

  const probes = Array.isArray(rawReport)
    ? rawReport
    : rawReport.probes || rawReport.results || rawReport.checks || []

  for (const probe of probes) {
    const name = probe.name || probe.probe || probe.id || 'unknown'
    const normalizedName = name.toLowerCase().replace(/[\s-]+/g, '_')
    // Prefer the stable dotted id (e.g. "chat.tool_call") for classification;
    // display names like "Forced tool call" do not match the mapping tables.
    const rawId = typeof probe.id === 'string' ? probe.id : ''
    const idCategory = rawId.split('.')[0] || ''
    const idLeaf = rawId.split('.').pop() || ''
    const protocol =
      idCategory === 'responses'
        ? 'openai_responses'
        : CC_PROBE_PROTOCOL[idLeaf] || CC_PROBE_PROTOCOL[idCategory] || CC_PROBE_PROTOCOL[normalizedName] || 'openai_chat_completions'
    const feature =
      CC_PROBE_FEATURE[idLeaf] || CC_PROBE_FEATURE[idCategory] || CC_PROBE_FEATURE[normalizedName] || 'text'
    const surface = protocol === 'openai_responses' ? 'responses' : 'chat'
    const caseSuffix = rawId ? rawId.replace(/[.\s-]+/g, '_') : normalizedName
    const caseId = `${surface}.${feature}.cc_${caseSuffix}`
    const errorMessage =
      typeof probe.error === 'object' && probe.error !== null
        ? probe.error.message || JSON.stringify(probe.error)
        : probe.error
    const httpStatus = probe.http_status || probe.error?.httpStatus || null
    const durationMs = probe.duration_ms ?? probe.durationMs ?? probe.duration ?? 0

    results.push({
      case_id: caseId,
      protocol_in: protocol,
      protocol_upstream: protocol,
      mode: 'native',
      feature,
      provider: 'mock',
      requested_model: 'conformance-test-model',
      resolved_model: 'conformance-test-model',
      result: mapResult(probe.status || probe.result),
      failure_class: mapResult(probe.status || probe.result) === 'FAIL' ? 'TOOL_LIMITATION' : null,
      http_status: httpStatus,
      stream: normalizedName.includes('stream'),
      request_id: null,
      retry_count: 0,
      fallback_count: 0,
      duration_ms: durationMs,
      evidence: {
        assertions: [],
        notes: `CompatCanary probe: ${name}. ${probe.message || errorMessage || probe.summary || ''}`.trim(),
      },
      tool_name: 'compatcanary',
      tool_version: toolVersion,
      timestamp: now,
    })
  }

  return {
    tool: 'compatcanary',
    tool_version: toolVersion,
    gateway_commit: commit,
    started_at: now,
    results,
  }
}

// ── SDK smoke normalization ─────────────────────────────────────────────────

export function normalizeSdkSmoke(sdkResults, sdkName, sdkVersion) {
  const commit = gatewayCommit()
  const now = new Date().toISOString()

  return {
    tool: `sdk_smoke_${sdkName}`,
    tool_version: sdkVersion,
    gateway_commit: commit,
    started_at: now,
    results: sdkResults.map((r) => ({
      case_id: r.case_id,
      protocol_in: r.protocol_in,
      protocol_upstream: r.protocol_in,
      mode: 'native',
      feature: r.feature || 'text',
      provider: 'mock',
      requested_model: r.model || 'conformance-test-model',
      resolved_model: r.model || 'conformance-test-model',
      result: r.result,
      failure_class: r.result === 'FAIL' ? (r.failure_class || 'GATEWAY_BUG') : null,
      http_status: r.http_status || null,
      stream: r.stream || false,
      request_id: null,
      retry_count: 0,
      fallback_count: 0,
      duration_ms: r.duration_ms || 0,
      evidence: r.evidence || null,
      tool_name: `sdk_smoke_${sdkName}`,
      tool_version: sdkVersion,
      timestamp: now,
    })),
  }
}

// ── CLI entry ───────────────────────────────────────────────────────────────

if (import.meta.url === `file://${process.argv[1]}`) {
  const [, , tool, inputPath, outputPath] = process.argv

  if (!tool || !inputPath) {
    console.error('Usage: normalize-report.mjs <llmprobe|compatcanary> <input.json> [output.json]')
    process.exit(2)
  }

  if (!existsSync(inputPath)) {
    console.error(`Input file not found: ${inputPath}`)
    process.exit(1)
  }

  const raw = JSON.parse(readFileSync(inputPath, 'utf8'))
  let normalized

  switch (tool) {
    case 'llmprobe':
      normalized = normalizeLlmprobe(raw, process.env.LLMPROBE_SPEC || 'chat-completions')
      break
    case 'compatcanary':
      normalized = normalizeCompatcanary(raw)
      break
    default:
      console.error(`Unknown tool: ${tool}. Supported: llmprobe, compatcanary`)
      process.exit(2)
  }

  const output = JSON.stringify(normalized, null, 2)
  if (outputPath) {
    writeFileSync(outputPath, output)
    console.error(`Wrote ${normalized.results.length} results to ${outputPath}`)
  } else {
    process.stdout.write(output + '\n')
  }
}
