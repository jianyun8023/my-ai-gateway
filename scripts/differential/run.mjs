#!/usr/bin/env node
/**
 * run.mjs — Direct vs Gateway differential test runner.
 *
 * Usage:
 *   DIFFERENTIAL_TESTS=1 node scripts/differential/run.mjs [options]
 *
 * Options:
 *   --provider <id>   Only run cases for this provider
 *   --model <id>      Override model for all cases
 *   --case <id>       Only run this case (repeatable; also re-includes
 *                     error/high-cost cases excluded by default)
 *   --include-error   Include error_expected cases (excluded by default)
 *   --include-high-cost  Include cost=high cases (excluded by default)
 *   --env-file <path> Path to env file (default: .env.live)
 *   --list            List matching cases without running
 *   --timeout <ms>    Per-request timeout (default: 30000)
 *
 * Environment:
 *   DIFFERENTIAL_TESTS=1         Required to run real provider tests
 *   DIFFERENTIAL_GATEWAY_URL     Gateway base URL (e.g. http://127.0.0.1:8787)
 *   GATEWAY_API_KEY              Gateway-side credential (Gateway 静态入口 Key)
 *   DEEPSEEK_API_KEY             Provider API key (direct path only)
 *   DEEPSEEK_BASE_URL            Provider base URL
 *
 * Exit codes:
 *   0 = all cases passed
 *   1 = at least one case failed
 *   2 = runner error (missing env, zero cases, parse error)
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { execSync } from 'node:child_process'

import { compareResponses } from './compare.mjs'

const __dirname = dirname(fileURLToPath(import.meta.url))
const REPO_ROOT = resolve(__dirname, '../..')
const CASES_PATH = resolve(REPO_ROOT, 'tests/differential/cases.json')
const REPORT_DIR = resolve(REPO_ROOT, 'target/test-reports/differential')
const TOOL_VERSION = '0.1.0'

// ── Argument parsing ────────────────────────────────────────────────────────

export function parseArguments(argv) {
  const args = {
    providers: [],
    model: null,
    cases: [],
    envFile: process.env.DIFFERENTIAL_ENV_FILE || '.env.live',
    list: false,
    timeout: 30_000,
    includeError: false,
    includeHighCost: false,
  }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if ((arg === '--provider') && argv[i + 1]) {
      args.providers.push(argv[++i])
    } else if (arg.startsWith('--provider=')) {
      args.providers.push(arg.split('=', 2)[1])
    } else if ((arg === '--model') && argv[i + 1]) {
      args.model = argv[++i]
    } else if (arg.startsWith('--model=')) {
      args.model = arg.split('=', 2)[1]
    } else if ((arg === '--case') && argv[i + 1]) {
      args.cases.push(argv[++i])
    } else if (arg.startsWith('--case=')) {
      args.cases.push(arg.split('=', 2)[1])
    } else if ((arg === '--env-file') && argv[i + 1]) {
      args.envFile = argv[++i]
    } else if (arg.startsWith('--env-file=')) {
      args.envFile = arg.split('=', 2)[1]
    } else if (arg === '--include-error') {
      args.includeError = true
    } else if (arg === '--include-high-cost') {
      args.includeHighCost = true
    } else if (arg === '--list') {
      args.list = true
    } else if ((arg === '--timeout') && argv[i + 1]) {
      args.timeout = parseInt(argv[++i], 10)
    } else if (arg.startsWith('--timeout=')) {
      args.timeout = parseInt(arg.split('=', 2)[1], 10)
    }
  }
  return args
}

// ── Case loading and filtering ──────────────────────────────────────────────

export function loadCases() {
  if (!existsSync(CASES_PATH)) {
    console.error(`ERROR: Cases file not found: ${CASES_PATH}`)
    process.exit(2)
  }
  try {
    const raw = readFileSync(CASES_PATH, 'utf8')
    const cases = JSON.parse(raw)
    if (!Array.isArray(cases) || cases.length === 0) {
      console.error('ERROR: cases.json must be a non-empty array')
      process.exit(2)
    }
    return cases
  } catch (err) {
    console.error(`ERROR: Failed to parse cases.json: ${err.message}`)
    process.exit(2)
  }
}

export function filterCases(cases, args) {
  let filtered = cases

  // Default exclusion (Issue #119 安全要求：error/高 token case 默认排除)。
  // Explicitly naming a case via --case, or passing --include-error /
  // --include-high-cost, re-includes it.
  filtered = filtered.filter((c) => {
    const explicitlyNamed = args.cases.includes(c.case_id)
    if (c.error_expected && !args.includeError && !explicitlyNamed) return false
    if (c.cost === 'high' && !args.includeHighCost && !explicitlyNamed) return false
    return true
  })

  if (args.providers.length > 0) {
    filtered = filtered.filter((c) => args.providers.includes(c.provider))
  }

  if (args.cases.length > 0) {
    filtered = filtered.filter((c) => args.cases.includes(c.case_id))
  }

  return filtered
}

// ── Environment validation ──────────────────────────────────────────────────

function validateEnvironment(cases) {
  const gatewayUrl = process.env.DIFFERENTIAL_GATEWAY_URL
  if (!gatewayUrl) {
    console.error('ERROR: DIFFERENTIAL_GATEWAY_URL is not set')
    process.exit(2)
  }
  try {
    const url = new URL(gatewayUrl)
    if (!['http:', 'https:'].includes(url.protocol)) {
      console.error(`ERROR: DIFFERENTIAL_GATEWAY_URL must use http:// or https:// (got ${url.protocol})`)
      process.exit(2)
    }
  } catch {
    console.error(`ERROR: DIFFERENTIAL_GATEWAY_URL is not a valid URL: ${gatewayUrl}`)
    process.exit(2)
  }

  // Gateway 侧使用独立凭据（GATEWAY_API_KEY / Virtual Key），
  // provider API key 只用于直连路径，不能通过 Gateway 鉴权。
  if (!process.env.GATEWAY_API_KEY) {
    console.error('ERROR: GATEWAY_API_KEY is not set.')
    console.error('Gateway requests authenticate with the gateway entry key, not the provider API key.')
    process.exit(2)
  }

  for (const c of cases) {
    for (const envVar of c.required_env || []) {
      if (!process.env[envVar]) {
        console.error(`ERROR: Case '${c.case_id}' requires env var ${envVar} but it is not set`)
        process.exit(2)
      }
    }
    // Validate direct base URL
    const directUrl = process.env[c.direct_base_url_env]
    if (!directUrl) {
      console.error(`ERROR: Case '${c.case_id}' requires ${c.direct_base_url_env} but it is not set`)
      process.exit(2)
    }
  }
}

// ── HTTP request helpers ────────────────────────────────────────────────────

/**
 * Send a chat completion request and return parsed response with metadata.
 * Returns { ...responseBody, _meta: { http_status, transport_error? } }
 * For streaming requests, also includes _stream_chunks.
 */
export async function sendRequest(baseUrl, apiKey, request, timeoutMs) {
  const url = `${baseUrl.replace(/\/+$/, '')}/v1/chat/completions`
  const headers = {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${apiKey}`,
  }

  const startTime = Date.now()

  try {
    const res = await fetch(url, {
      method: 'POST',
      headers,
      body: JSON.stringify(request),
      signal: AbortSignal.timeout(timeoutMs),
    })

    const httpStatus = res.status

    if (request.stream) {
      return await parseStreamResponse(res, httpStatus, startTime)
    }

    // Non-streaming
    const text = await res.text()
    let body
    try {
      body = JSON.parse(text)
    } catch {
      return {
        _meta: { http_status: httpStatus, parse_error: true, duration_ms: Date.now() - startTime },
      }
    }

    return {
      ...body,
      _meta: { http_status: httpStatus, duration_ms: Date.now() - startTime },
    }
  } catch (err) {
    return {
      _meta: {
        http_status: 0,
        transport_error: true,
        error_message: err.message,
        duration_ms: Date.now() - startTime,
      },
    }
  }
}

async function parseStreamResponse(res, httpStatus, startTime) {
  const chunks = []
  try {
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })

      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        const trimmed = line.trim()
        if (!trimmed || trimmed.startsWith(':')) continue
        if (trimmed.startsWith('data: ')) {
          const data = trimmed.slice(6).trim()
          if (data === '[DONE]') {
            chunks.push('[DONE]')
          } else {
            try {
              chunks.push(JSON.parse(data))
            } catch {
              // Malformed chunk — record as string for diagnostics
              chunks.push({ _parse_error: true, raw: data.slice(0, 200) })
            }
          }
        }
      }
    }
  } catch (err) {
    return {
      _meta: { http_status: httpStatus, transport_error: true, error_message: err.message, duration_ms: Date.now() - startTime },
      _stream_chunks: chunks,
    }
  }

  return {
    _meta: { http_status: httpStatus, duration_ms: Date.now() - startTime },
    _stream_chunks: chunks,
  }
}

// ── Report helpers ──────────────────────────────────────────────────────────

function gatewayCommit() {
  try {
    return execSync('git rev-parse --short HEAD', { cwd: REPO_ROOT, encoding: 'utf8' }).trim()
  } catch {
    return 'unknown'
  }
}

function ensureReportDir() {
  mkdirSync(REPORT_DIR, { recursive: true })
  return REPORT_DIR
}

function writeReport(results) {
  const dir = ensureReportDir()
  const report = {
    tool: 'differential',
    tool_version: TOOL_VERSION,
    gateway_commit: gatewayCommit(),
    started_at: new Date().toISOString(),
    result_policy: {
      differential_opt_in: true,
      response_bodies_stored: false,
    },
    results,
  }

  const reportPath = resolve(dir, 'differential-report.json')
  writeFileSync(reportPath, JSON.stringify(report, null, 2))
  console.error(`  → ${reportPath}`)
  return report
}

function printSummary(results) {
  const total = results.length
  const pass = results.filter((r) => r.result === 'PASS').length
  const fail = results.filter((r) => r.result === 'FAIL').length
  const skip = results.filter((r) => r.result === 'SKIPPED').length

  console.error(`\ndifferential v${TOOL_VERSION} — ${total} cases: ${pass} PASS, ${fail} FAIL, ${skip} SKIPPED`)

  if (fail > 0) {
    for (const r of results.filter((r) => r.result === 'FAIL')) {
      const cls = r.failure_class ? ` [${r.failure_class}]` : ''
      console.error(`  ✗ ${r.case_id}${cls}: ${r.evidence?.notes || ''}`)
    }
  }

  return fail
}

// ── Main ────────────────────────────────────────────────────────────────────

async function main() {
  const args = parseArguments(process.argv.slice(2))

  // Load env file
  const envPath = resolve(REPO_ROOT, args.envFile)
  if (existsSync(envPath)) {
    process.loadEnvFile(envPath)
  }

  // Check opt-in
  if (process.env.DIFFERENTIAL_TESTS !== '1') {
    console.error('ERROR: DIFFERENTIAL_TESTS is not set to 1.')
    console.error('Real provider differential tests require explicit opt-in.')
    console.error('Set DIFFERENTIAL_TESTS=1 to run, or use `node --test` for offline self-tests.')
    process.exit(2)
  }

  // Load and filter cases
  const allCases = loadCases()
  const cases = filterCases(allCases, args)

  if (cases.length === 0) {
    console.error('ERROR: No matching cases after filtering.')
    console.error(`  Available cases: ${allCases.map((c) => c.case_id).join(', ')}`)
    if (args.providers.length > 0) console.error(`  --provider filter: ${args.providers.join(', ')}`)
    if (args.cases.length > 0) console.error(`  --case filter: ${args.cases.join(', ')}`)
    process.exit(2)
  }

  if (args.list) {
    console.log('Matching cases:')
    for (const c of cases) {
      console.log(`  ${c.case_id} (${c.provider}/${c.model_default}, ${c.protocol}, cost=${c.cost})`)
    }
    process.exit(0)
  }

  // Validate environment for selected cases
  validateEnvironment(cases)

  console.error(`\n=== Differential: ${cases.length} case(s) ===\n`)

  const results = []

  for (const caseSpec of cases) {
    const model = args.model || process.env[caseSpec.model_env] || caseSpec.model_default
    const directBaseUrl = process.env[caseSpec.direct_base_url_env]
    const gatewayBaseUrl = process.env[caseSpec.gateway_base_url_env]

    // Resolve API keys — provider key for the direct path, GATEWAY_API_KEY for
    // the gateway path (Gateway 鉴权只认入口 Key / Virtual Key)
    const apiKeyEnv = (caseSpec.required_env || []).find((e) => e.endsWith('_API_KEY'))
    const apiKey = apiKeyEnv ? process.env[apiKeyEnv] : ''
    const gatewayApiKey = process.env.GATEWAY_API_KEY

    // Build request with model
    const request = { ...caseSpec.request, model }

    console.error(`  [${caseSpec.case_id}] ${caseSpec.description}`)
    console.error(`    Direct: ${directBaseUrl} | Gateway: ${gatewayBaseUrl} | Model: ${model}`)

    const startTime = Date.now()

    // Send to direct provider
    console.error('    → Sending to Direct provider...')
    const directResponse = await sendRequest(directBaseUrl, apiKey, request, args.timeout)

    // Send to gateway
    console.error('    → Sending to Gateway...')
    const gatewayResponse = await sendRequest(gatewayBaseUrl, gatewayApiKey, request, args.timeout)

    const totalDuration = Date.now() - startTime

    // Compare
    const result = compareResponses(directResponse, gatewayResponse, caseSpec, totalDuration)
    result.requested_model = model
    result.resolved_model = model

    const icon = result.result === 'PASS' ? '✓' : '✗'
    const cls = result.failure_class ? ` [${result.failure_class}]` : ''
    console.error(`    ${icon} ${result.result}${cls}`)

    results.push(result)
  }

  // Write report
  writeReport(results)

  // Summary
  const failCount = printSummary(results)
  process.exit(failCount > 0 ? 1 : 0)
}

// Only run main when executed directly (not imported for testing).
// Use the same pattern as scripts/live-provider-smoke.mjs: resolve argv[1]
// through pathToFileURL so spaces/non-ASCII in the path don't silently skip
// main (which would exit 0 with no output — a fake PASS).
const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : ''
if (import.meta.url === invokedPath) {
  main().catch((err) => {
    console.error(`FATAL: ${err.message}`)
    process.exit(2)
  })
}
