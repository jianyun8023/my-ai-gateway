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
const CONFIG_PATH = resolve(REPO_ROOT, 'tests/conformance/config.json')
const SCHEMA_PATH = resolve(REPO_ROOT, 'tests/coverage/test-result.schema.json')

// ── Version loading ─────────────────────────────────────────────────────────

export function loadVersions() {
  return JSON.parse(readFileSync(VERSIONS_PATH, 'utf8'))
}

function loadKnownGaps(tool) {
  try {
    const config = JSON.parse(readFileSync(CONFIG_PATH, 'utf8'))
    return config?.tools?.[tool]?.known_gaps ?? []
  } catch {
    return []
  }
}

/**
 * Apply declared known gaps to a normalized result.
 *
 * A gap matches when `case_pattern` (regex) matches the case_id and, when
 * `failure_id_pattern` is present, every failing MUST assertion id of the
 * case matches it — so a gap can never mask a new, unrelated failure.
 */
function applyKnownGaps(entry, gaps) {
  for (const gap of gaps) {
    if (!new RegExp(gap.case_pattern).test(entry.case_id)) continue
    if (gap.failure_id_pattern) {
      const failing = (entry.evidence?.assertions || [])
        .filter((a) => a.passed === false && a.name.startsWith('MUST:'))
        .map((a) => a.name.slice('MUST:'.length))
      if (failing.length === 0 || !failing.every((id) => new RegExp(gap.failure_id_pattern).test(id))) {
        continue
      }
    }
    return {
      ...entry,
      result: gap.expected_result,
      failure_class: gap.expected_result === 'SKIPPED' ? 'TOOL_LIMITATION' : null,
      evidence: {
        ...entry.evidence,
        notes: `${entry.evidence?.notes || ''} [known gap: ${gap.reason}; original outcome ${entry.result}]`.trim(),
      },
    }
  }
  return entry
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
//
// Real llmprobe 0.6.x JSON shape (from `--json --quick --no-bench`):
//   {
//     run: { phases: { coverage|conformance|capability|agentic|…: {status, reason} } },
//     coverage: { byTier, entries: [{id, label, kind, tier, supported, …}] },
//     conformance: {
//       passed, total, bySurface,
//       results: [{ id: "chat-basic", name, surface: "chat",
//                   outcome: "pass"|"fail"|"unsupported"|"skipped"|"inconclusive"|"unreachable",
//                   reason?, durationMs, failures: [],
//                   assertions: [{id, label, severity: "MUST"|"SHOULD"|"MAY", passed, message?}] }]
//     }
//   }

/** spec filter → llmprobe surfaces covered by that northbound protocol. */
export const LLMPROPE_SPEC_SURFACES = {
  'chat-completions': ['chat', 'completions', 'models'],
  'responses': ['responses', 'models'],
  'anthropic-messages': ['messages', 'count-tokens', 'models'],
}

/** llmprobe surface → gateway protocol.  Only surfaces in the gateway
 * product scope appear here; anything else is recorded as out-of-scope
 * metadata instead of becoming a test case. */
const LLMPROBE_SURFACE_PROTOCOL = {
  chat: 'openai_chat_completions',
  completions: 'openai_chat_completions',
  models: 'openai_chat_completions',
  responses: 'openai_responses',
  messages: 'anthropic_messages',
  'count-tokens': 'anthropic_messages',
}

/** Surfaces the gateway claims to implement.  llmprobe reporting
 * `unsupported` for one of these means the endpoint is broken → FAIL. */
const LLMPROBE_CLAIMED_SURFACES = new Set(['chat', 'responses', 'messages', 'models'])

const LLMPROBE_FEATURE_KEYWORDS = [
  ['usage', 'usage'],
  ['tool_result', 'tool_result'],
  ['parallel_tools', 'parallel_tools'],
  ['tool_choice', 'tool_choice'],
  ['tool', 'tools'],
  ['structured', 'structured_output'],
  ['json', 'structured_output'],
  ['error', 'error_envelope'],
  ['reasoning', 'reasoning'],
  ['thinking', 'thinking'],
  ['stream', 'streaming'],
]

function llmprobeFeature(scenario) {
  const key = scenario.replace(/-/g, '_')
  for (const [keyword, feature] of LLMPROBE_FEATURE_KEYWORDS) {
    if (key.includes(keyword)) return feature
  }
  return 'text'
}

function mapLlmprobeOutcome(outcome, surface) {
  switch (outcome) {
    case 'pass':
      return { result: 'PASS', failure_class: null }
    case 'fail':
    case 'unreachable':
      // A failed MUST assertion (or an unreachable target) on a surface we
      // ship is a real conformance defect until triaged otherwise.
      return { result: 'FAIL', failure_class: 'GATEWAY_BUG' }
    case 'unsupported':
      // Gateway intentionally does not serve this surface (e.g. legacy
      // completions, count-tokens) — honest UNSUPPORTED, never a failure.
      // On a claimed surface it means the endpoint is missing → FAIL.
      if (LLMPROBE_CLAIMED_SURFACES.has(surface)) {
        return { result: 'FAIL', failure_class: 'GATEWAY_BUG' }
      }
      return { result: 'UNSUPPORTED', failure_class: null }
    case 'skipped': // not run at this depth ("quick" omits slow checks)
    case 'inconclusive':
    default:
      return { result: 'SKIPPED', failure_class: 'TOOL_LIMITATION' }
  }
}

export function normalizeLlmprobe(rawReport) {
  const versions = loadVersions()
  const toolVersion = versions.llmprobe
  const knownGaps = loadKnownGaps('llmprobe')
  const commit = gatewayCommit()
  const now = new Date().toISOString()
  const results = []
  const outOfScope = []

  const conformance = rawReport?.conformance ?? {}
  const tests = Array.isArray(conformance.results) ? conformance.results : []

  for (const test of tests) {
    const surface = typeof test.surface === 'string' ? test.surface : ''
    const protocol = LLMPROBE_SURFACE_PROTOCOL[surface]
    if (!protocol) {
      // embeddings / images / audio / …: outside the gateway product surface.
      outOfScope.push({ id: test.id, surface, outcome: test.outcome })
      continue
    }

    const rawId = typeof test.id === 'string' ? test.id : 'unknown'
    const scenario = (rawId.startsWith(`${surface}-`) ? rawId.slice(surface.length + 1) : rawId)
      .replace(/-/g, '_')
      .replace(/[^a-z0-9_]/g, '')
      .toLowerCase() || 'unknown'
    const surfaceAlias = surface.replace(/-/g, '_')
    const feature = llmprobeFeature(scenario)
    const { result, failure_class } = mapLlmprobeOutcome(test.outcome, surface)

    // llmprobe puts failed assertions in `failures`; `assertions` may be
    // empty on failing cases.  Fall back so gap matching and evidence never
    // lose the failing assertion ids.
    const rawAssertions = Array.isArray(test.assertions) && test.assertions.length > 0
      ? test.assertions
      : Array.isArray(test.failures)
        ? test.failures.map((f) => ({ ...f, passed: false }))
        : []
    const assertions = rawAssertions.map((a) => ({
      name: `${a.severity || 'MUST'}:${a.id || a.label || 'assertion'}`,
      passed: a.passed === true,
      message: a.message ?? null,
    }))
    const failures = Array.isArray(test.failures)
      ? test.failures
          .map((f) => (typeof f === 'string' ? f : f?.message))
          .filter(Boolean)
      : []

    results.push(applyKnownGaps({
      case_id: `${surfaceAlias}.${feature}.${scenario}`,
      protocol_in: protocol,
      protocol_upstream: protocol,
      mode: result === 'UNSUPPORTED' ? 'unsupported' : 'native',
      feature,
      provider: 'mock',
      requested_model: rawReport?.target?.model || 'conformance-test-model',
      resolved_model: rawReport?.target?.model || 'conformance-test-model',
      result,
      failure_class,
      http_status: null,
      stream: scenario.includes('stream'),
      request_id: null,
      retry_count: 0,
      fallback_count: 0,
      duration_ms: test.durationMs || 0,
      evidence: {
        assertions,
        notes: [
          `llmprobe case: ${test.name || rawId}`,
          test.reason,
          failures.join('; '),
        ]
          .filter(Boolean)
          .join('. '),
      },
      tool_name: 'llmprobe',
      tool_version: toolVersion,
      timestamp: now,
      // llmprobe surface retained for --spec filtering in the runner.
      llmprobe_surface: surface,
    }, knownGaps))
  }

  return {
    tool: 'llmprobe',
    tool_version: toolVersion,
    gateway_commit: commit,
    started_at: rawReport?.run?.startedAt || now,
    // Phase status (capability/agentic/eval are not-run at --quick depth) and
    // out-of-scope surfaces are metadata, not fabricated test cases.
    phases: rawReport?.run?.phases ?? null,
    out_of_scope_surfaces: outOfScope,
    results,
  }
}

// ── CompatCanary normalization ──────────────────────────────────────────────

/** Generic status mapper shared by tool normalizers with PASS/FAIL/SKIP-style
 * statuses (CompatCanary).  llmprobe has its own outcome vocabulary and is
 * mapped by mapLlmprobeOutcome instead. */
function mapResult(status) {
  if (!status) return 'SKIPPED'
  const s = String(status).toUpperCase()
  if (s === 'PASS' || s === 'PASSED' || s === 'OK' || s === 'SUCCESS') return 'PASS'
  if (s === 'FAIL' || s === 'FAILED' || s === 'ERROR') return 'FAIL'
  if (s === 'SKIP' || s === 'SKIPPED' || s === 'NOT_RUN') return 'SKIPPED'
  return 'SKIPPED'
}

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
      normalized = normalizeLlmprobe(raw)
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
