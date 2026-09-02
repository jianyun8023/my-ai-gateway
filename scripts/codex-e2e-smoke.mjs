import { spawn, spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const repositoryRoot = path.resolve(scriptDir, '..')
const defaultManifestPath = path.join(repositoryRoot, 'tests/codex/cases.json')
const defaultResultDirectory = path.join(repositoryRoot, 'target/codex-e2e')
const defaultHome = path.join(defaultResultDirectory, 'home')
const defaultWorkspace = path.join(defaultResultDirectory, 'workspace')

class CodexE2EFailure extends Error {
  constructor(message, metadata = {}) {
    super(message)
    this.name = 'CodexE2EFailure'
    this.metadata = metadata
  }
}

function check(condition, message, metadata = {}) {
  if (!condition) throw new CodexE2EFailure(message, metadata)
}

function nowMilliseconds() {
  return Number(process.hrtime.bigint() / 1_000_000n)
}

function duration(startedAt) {
  return nowMilliseconds() - startedAt
}

function stableRunId() {
  return `${new Date().toISOString().replaceAll(/[:.]/g, '-')}-${randomBytes(4).toString('hex')}`
}

export function loadCaseManifest(manifestPath = defaultManifestPath) {
  const cases = JSON.parse(readFileSync(manifestPath, 'utf8'))
  check(Array.isArray(cases) && cases.length > 0, 'Codex E2E case manifest must be a non-empty array')
  const ids = new Set()
  for (const testCase of cases) {
    check(typeof testCase.id === 'string' && testCase.id.length > 0, 'Codex E2E case id is required')
    check(!ids.has(testCase.id), `duplicate Codex E2E case id: ${testCase.id}`)
    ids.add(testCase.id)
    check(['tool', 'search', 'multi_turn_tool'].includes(testCase.kind), `invalid Codex E2E kind for ${testCase.id}`)
    check(['low', 'high'].includes(testCase.cost), `invalid Codex E2E cost for ${testCase.id}`)
    check(typeof testCase.description === 'string' && testCase.description.length > 0, `description is required for ${testCase.id}`)
  }
  return cases
}

export function parseArguments(argv) {
  const options = {
    cases: [],
    includeSearch: false,
    list: false,
    skipUsageCheck: false,
    keepOutput: false,
    help: false,
    envFile: process.env.CODEX_E2E_ENV_FILE || '.env.codex-e2e',
    outputDirectory: null,
    home: null,
    workspace: null,
    codexCli: null,
    model: null,
    virtualKeyId: null,
    virtualKeyName: null,
    timeoutMs: null,
  }
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index]
    const nextValue = () => {
      index += 1
      check(index < argv.length, `${argument} requires a value`)
      return argv[index]
    }
    if (argument === '--case') options.cases.push(nextValue())
    else if (argument.startsWith('--case=')) options.cases.push(argument.slice('--case='.length))
    else if (argument === '--env-file') options.envFile = nextValue()
    else if (argument.startsWith('--env-file=')) options.envFile = argument.slice('--env-file='.length)
    else if (argument === '--output-dir') options.outputDirectory = nextValue()
    else if (argument.startsWith('--output-dir=')) options.outputDirectory = argument.slice('--output-dir='.length)
    else if (argument === '--home') options.home = nextValue()
    else if (argument.startsWith('--home=')) options.home = argument.slice('--home='.length)
    else if (argument === '--workspace') options.workspace = nextValue()
    else if (argument.startsWith('--workspace=')) options.workspace = argument.slice('--workspace='.length)
    else if (argument === '--codex') options.codexCli = nextValue()
    else if (argument.startsWith('--codex=')) options.codexCli = argument.slice('--codex='.length)
    else if (argument === '--model') options.model = nextValue()
    else if (argument.startsWith('--model=')) options.model = argument.slice('--model='.length)
    else if (argument === '--virtual-key-id') options.virtualKeyId = Number(nextValue())
    else if (argument.startsWith('--virtual-key-id=')) options.virtualKeyId = Number(argument.slice('--virtual-key-id='.length))
    else if (argument === '--virtual-key-name') options.virtualKeyName = nextValue()
    else if (argument.startsWith('--virtual-key-name=')) options.virtualKeyName = argument.slice('--virtual-key-name='.length)
    else if (argument === '--timeout-ms') options.timeoutMs = Number(nextValue())
    else if (argument.startsWith('--timeout-ms=')) options.timeoutMs = Number(argument.slice('--timeout-ms='.length))
    else if (argument === '--include-search') options.includeSearch = true
    else if (argument === '--skip-usage-check') options.skipUsageCheck = true
    else if (argument === '--keep-output') options.keepOutput = true
    else if (argument === '--list') options.list = true
    else if (argument === '--help' || argument === '-h') options.help = true
    else throw new CodexE2EFailure(`unknown argument: ${argument}`)
  }
  return options
}

export function selectCases(cases, options) {
  const knownIds = new Set(cases.map((testCase) => testCase.id))
  for (const id of options.cases) check(knownIds.has(id), `unknown Codex E2E case: ${id}`)
  let selected = cases
  if (options.cases.length > 0) {
    const requested = new Set(options.cases)
    selected = selected.filter((testCase) => requested.has(testCase.id))
  } else if (!options.includeSearch) {
    selected = selected.filter((testCase) => testCase.cost === 'low')
  }
  check(selected.length > 0, 'no Codex E2E cases selected')
  return selected
}

function loadEnvironmentFile(envFile) {
  const resolved = path.resolve(repositoryRoot, envFile)
  if (!existsSync(resolved)) return null
  process.loadEnvFile(resolved)
  return resolved
}

function resolveOptions(options) {
  const timeoutMs = Number(options.timeoutMs ?? process.env.CODEX_E2E_TIMEOUT_MS ?? 300_000)
  return {
    ...options,
    outputDirectory: path.resolve(repositoryRoot, options.outputDirectory || process.env.CODEX_E2E_RESULT_DIR || defaultResultDirectory),
    home: path.resolve(repositoryRoot, options.home || process.env.CODEX_HOME || defaultHome),
    workspace: path.resolve(repositoryRoot, options.workspace || process.env.CODEX_E2E_WORKSPACE || defaultWorkspace),
    codexCli: options.codexCli || process.env.CODEX_CLI || 'codex',
    model: options.model || process.env.CODEX_MODEL || 'k3',
    virtualKeyId: options.virtualKeyId ?? (process.env.CODEX_VIRTUAL_KEY_ID ? Number(process.env.CODEX_VIRTUAL_KEY_ID) : null),
    virtualKeyName: options.virtualKeyName || process.env.CODEX_VIRTUAL_KEY_NAME || null,
    timeoutMs: Number.isFinite(timeoutMs) ? timeoutMs : 300_000,
  }
}

function normalizedGatewayBaseUrl(raw) {
  let url
  try { url = new URL(raw) } catch {}
  check(url && ['http:', 'https:'].includes(url.protocol), 'CODEX_GATEWAY_BASE_URL must be an HTTP(S) URL')
  check(!url.username && !url.password && !url.search && !url.hash, 'CODEX_GATEWAY_BASE_URL must not contain credentials, query, or fragment')
  if (url.pathname === '/' || url.pathname === '') url.pathname = '/v1'
  return url.toString().replace(/\/$/, '')
}

function validateEnvironment(options, skipUsageCheck) {
  check(process.env.CODEX_E2E_TESTS === '1', 'CODEX_E2E_TESTS must be exactly 1 for live Codex E2E')
  check(process.env.CODEX_GATEWAY_BASE_URL, 'CODEX_GATEWAY_BASE_URL is required')
  check(
    process.env.CODEX_GATEWAY_API_KEY || process.env.CODEX_GATEWAY_ADMIN_KEY,
    'CODEX_GATEWAY_ADMIN_KEY is required to resolve a database Virtual Key unless CODEX_GATEWAY_API_KEY is explicitly set',
  )
  if (!skipUsageCheck) check(process.env.CODEX_GATEWAY_ADMIN_KEY, 'CODEX_GATEWAY_ADMIN_KEY is required unless --skip-usage-check is set')
  check(options.model.trim().length > 0, 'Codex model must not be empty')
  check(Number.isFinite(options.timeoutMs) && options.timeoutMs >= 10_000, 'CODEX_E2E_TIMEOUT_MS must be at least 10000')
  check(path.resolve(options.home) !== path.join(os.homedir(), '.codex'), 'refusing to use the default user Codex home; choose an isolated CODEX_HOME')
  const gatewayBaseUrl = normalizedGatewayBaseUrl(process.env.CODEX_GATEWAY_BASE_URL)
  return { gatewayBaseUrl, adminBaseUrl: gatewayAdminBaseUrl(gatewayBaseUrl) }
}

export function gatewayAdminBaseUrl(raw) {
  const url = new URL(raw)
  const pathname = url.pathname.replace(/\/+$/, '')
  if (pathname === '/v1') url.pathname = '/'
  else if (pathname.endsWith('/v1')) url.pathname = pathname.slice(0, -3) || '/'
  return url.toString().replace(/\/$/, '')
}

export function buildCodexConfig({ model, baseUrl, clientSource }) {
  return [
    `model = ${JSON.stringify(model)}`,
    'model_provider = "gateway"',
    'model_reasoning_effort = "low"',
    'model_reasoning_summary = "none"',
    'model_verbosity = "low"',
    'model_context_window = 131072',
    'approval_policy = "never"',
    'sandbox_mode = "read-only"',
    '',
    '[model_providers.gateway]',
    'name = "my-ai-gateway Codex E2E"',
    `base_url = ${JSON.stringify(baseUrl)}`,
    'env_key = "CODEX_GATEWAY_API_KEY"',
    'wire_api = "responses"',
    `http_headers = { "X-Client-Source" = ${JSON.stringify(clientSource)} }`,
    'request_max_retries = 0',
    'stream_max_retries = 0',
    'stream_idle_timeout_ms = 120000',
    '',
  ].join('\n')
}

export function buildCodexArgs({ model, workspace, outputPath, prompt, search = false, skipGitRepoCheck = false }) {
  // Codex CLI >=0.149 rejects global flags placed before the subcommand
  // (`error: unexpected argument '--skip-git-repo-check' found` with
  // `tip: 'exec --skip-git-repo-check' exists`).  Place every global
  // flag (--search, --skip-git-repo-check) immediately after `exec`,
  // before the per-subcommand flag set.
  const args = ['exec']
  if (search) args.push('--search')
  if (skipGitRepoCheck) args.push('--skip-git-repo-check')
  args.push(
    '--strict-config',
    '--ephemeral',
    '--json',
    '--sandbox',
    'read-only',
    '--model',
    model,
    '--cd',
    workspace,
    '--output-last-message',
    outputPath,
  )
  args.push(prompt)
  return args
}

function ensureDirectory(directory) {
  mkdirSync(directory, { recursive: true, mode: 0o700 })
  chmodSync(directory, 0o700)
}

export function secureFile(filePath) {
  if (!existsSync(filePath)) return false
  chmodSync(filePath, 0o600)
  return true
}

function prepareWorkspace(workspace, canary) {
  ensureDirectory(workspace)
  const canaryPath = path.join(workspace, 'CANARY.txt')
  if (existsSync(canaryPath)) {
    const current = readFileSync(canaryPath, 'utf8').trim()
    check(current.startsWith('codex-gateway-'), 'refusing to overwrite an unrelated workspace CANARY.txt')
  }
  writeFileSync(canaryPath, `${canary}\n`, { mode: 0o600 })
  secureFile(canaryPath)
  return canaryPath
}

function isGitWorkspace(workspace) {
  const result = spawnSync('git', ['-C', workspace, 'rev-parse', '--show-toplevel'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })
  return result.status === 0
}

function captureChunk(current, chunk, limit = 8 * 1024 * 1024) {
  if (current.length >= limit) return current
  const next = `${current}${String(chunk)}`
  return next.length > limit ? next.slice(0, limit) : next
}

function runProcess(command, args, environment, cwd, timeoutMs) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      cwd,
      env: environment,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let stdout = ''
    let stderr = ''
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      child.kill('SIGTERM')
      setTimeout(() => {
        if (child.exitCode === null) child.kill('SIGKILL')
      }, 3_000).unref()
    }, timeoutMs)
    child.stdout.on('data', (chunk) => { stdout = captureChunk(stdout, chunk) })
    child.stderr.on('data', (chunk) => { stderr = captureChunk(stderr, chunk) })
    child.on('error', (error) => {
      clearTimeout(timer)
      resolve({ code: null, signal: null, timedOut, stdout, stderr, error: error.code || error.name })
    })
    child.on('close', (code, signal) => {
      clearTimeout(timer)
      resolve({ code, signal, timedOut, stdout, stderr, error: null })
    })
  })
}

export function parseCodexJsonLines(stdout) {
  const events = []
  let invalidLines = 0
  for (const line of String(stdout).split(/\r?\n/)) {
    const trimmed = line.trim()
    if (!trimmed) continue
    try {
      events.push(JSON.parse(trimmed))
    } catch {
      invalidLines += 1
    }
  }
  return { events, invalidLines }
}

const sensitiveEnvironmentPattern = /(?:^|_)(API_KEY|ACCESS_KEY|PRIVATE_KEY|TOKEN|SECRET|PASSWORD|DATABASE_URL|CONFIG_JSON|ADMIN_KEY|CREDENTIAL|AUTH)(?:_|$)/i

export function sanitizeCodexEnvironment(environment, { home, stateDirectory, apiKey }) {
  const sanitized = { ...environment }
  for (const key of Object.keys(sanitized)) {
    if (sensitiveEnvironmentPattern.test(key)) delete sanitized[key]
  }
  sanitized.CODEX_HOME = home
  sanitized.CODEX_SQLITE_HOME = stateDirectory
  sanitized.CODEX_GATEWAY_API_KEY = apiKey
  return sanitized
}

function addUsage(total, usage) {
  if (!usage || typeof usage !== 'object') return total
  total.input_tokens += Number(usage.input_tokens || 0)
  total.output_tokens += Number(usage.output_tokens || 0)
  total.reasoning_output_tokens += Number(usage.reasoning_output_tokens || usage.reasoning_tokens || 0)
  total.cached_input_tokens += Number(usage.cached_input_tokens || usage.cache_read_input_tokens || 0)
  return total
}

export function summarizeCodexEvents(events, expectedCanary = '') {
  const eventTypes = new Set()
  const commandItems = []
  const searchItems = []
  const usage = {
    input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
    cached_input_tokens: 0,
  }
  for (const event of events) {
    if (event?.type) eventTypes.add(event.type)
    if (event?.type === 'turn.completed') addUsage(usage, event.usage)
    const item = event?.item
    if (!item) continue
    if (item.type === 'command_execution') commandItems.push({
      completed: event.type === 'item.completed',
      succeeded: event.type === 'item.completed' && item.exit_code === 0,
      canary_seen: typeof item.aggregated_output === 'string' && item.aggregated_output.includes(expectedCanary),
    })
    if (item.type === 'web_search') searchItems.push({
      completed: event.type === 'item.completed',
      has_query: Boolean(item.query || item.action?.query || item.action?.queries?.length),
      action_type: item.action?.type || null,
    })
  }
  usage.total_tokens = usage.input_tokens + usage.output_tokens
  return {
    event_count: events.length,
    event_types: [...eventTypes].sort(),
    command_execution_count: commandItems.length,
    successful_command_count: commandItems.filter((item) => item.succeeded).length,
    canary_seen: commandItems.some((item) => item.succeeded && item.canary_seen),
    web_search_count: searchItems.length,
    completed_web_search_count: searchItems.filter((item) => item.completed).length,
    nonempty_search_query_count: searchItems.filter((item) => item.has_query).length,
    search_action_types: [...new Set(searchItems.map((item) => item.action_type).filter(Boolean))].sort(),
    usage,
  }
}

export function evaluateToolResult(finalText, summary, expectedCanary) {
  const final = String(finalText).trim()
  return {
    passed: summary.canary_seen && final === `CODEX_GATEWAY_E2E_OK:${expectedCanary}`,
    command_seen: summary.command_execution_count > 0,
    command_succeeded: summary.successful_command_count > 0,
    canary_seen: summary.canary_seen,
    final_exact: final === `CODEX_GATEWAY_E2E_OK:${expectedCanary}`,
  }
}

export function evaluateSearchResult(finalText, summary) {
  const final = String(finalText).trim()
  const match = final.match(/^SEARCH_E2E_OK:([^:\s]+):(https?:\/\/[^\s]+)$/)
  let sourceHost = null
  if (match) {
    try { sourceHost = new URL(match[2]).hostname } catch {}
  }
  return {
    passed: summary.completed_web_search_count > 0
      && summary.nonempty_search_query_count > 0
      && sourceHost === 'blog.rust-lang.org',
    search_seen: summary.web_search_count > 0,
    search_completed: summary.completed_web_search_count > 0,
    nonempty_query_seen: summary.nonempty_search_query_count > 0,
    source_host: sourceHost,
    version_present: Boolean(match?.[1]),
  }
}

export function diagnosticStage(kind, processResult, invalidLines, evaluation, usage) {
  if (processResult.error) return 'cli_process_error'
  if (processResult.timedOut) return 'cli_timeout'
  if (processResult.code !== 0) return 'cli_nonzero_exit'
  if (invalidLines > 0) return 'cli_invalid_jsonl'
  if (usage && !usage.passed) return 'usage_missing_or_empty'
  if (kind === 'tool') {
    if (!evaluation.command_seen) return 'command_event_missing'
    if (!evaluation.command_succeeded) return 'command_failed'
    if (!evaluation.canary_seen) return 'canary_not_observed'
    if (!evaluation.final_exact) return 'final_message_mismatch'
  } else {
    if (!evaluation.search_seen) return 'search_event_missing'
    if (!evaluation.search_completed) return 'search_not_completed'
    if (!evaluation.nonempty_query_seen) return 'search_query_missing'
    if (evaluation.source_host !== 'blog.rust-lang.org') return 'official_source_missing'
  }
  return 'passed'
}

export function classifyCodexFailure(failure) {
  const statuses = failure?.usage?.status_codes || []
  if (statuses.some((status) => [403, 429].includes(Number(status)))) {
    return 'provider_unavailable'
  }
  if (
    failure?.usage?.passed
    && ['command_event_missing', 'search_event_missing'].includes(failure?.diagnostic_stage)
  ) {
    return 'not_triggered'
  }
  return 'failed'
}

function safeFailure(error) {
  if (error instanceof CodexE2EFailure) return { message: error.message, ...error.metadata }
  return { message: error?.name || 'unexpected failure' }
}

async function adminUsageEvent(baseUrl, adminKey, clientSource, timeoutMs) {
  const endpoint = `${baseUrl}/admin/usage/events?client_source=${encodeURIComponent(clientSource)}&limit=50`
  const deadline = Date.now() + Math.min(timeoutMs, 15_000)
  while (Date.now() < deadline) {
    try {
      const response = await fetch(endpoint, {
        headers: { authorization: `Bearer ${adminKey}` },
        signal: AbortSignal.timeout(2_000),
      })
      if (response.ok) {
        const payload = await response.json()
        const events = Array.isArray(payload.data) ? payload.data : []
        if (events.length > 0) {
          const successful = events.filter((event) => event.status_code >= 200 && event.status_code < 400 && event.success)
          return {
            passed: successful.length > 0 && successful.some((event) => Number(event.total_tokens || 0) > 0),
            event_count: events.length,
            successful_count: successful.length,
            unsuccessful_count: events.length - successful.length,
            status_codes: [...new Set(events.map((event) => Number(event.status_code || 0)))].sort((a, b) => a - b),
            total_tokens: events.reduce((sum, event) => sum + Number(event.total_tokens || 0), 0),
            usage_sources: [...new Set(events.map((event) => event.usage_source).filter(Boolean))].sort(),
            modes: [...new Set(events.map((event) => event.mode).filter(Boolean))].sort(),
            streamed: events.some((event) => event.streamed === true),
          }
        }
      }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  return {
    passed: false,
    event_count: 0,
    successful_count: 0,
    unsuccessful_count: 0,
    status_codes: [],
    total_tokens: 0,
    usage_sources: [],
    modes: [],
    streamed: false,
  }
}

export function toolPrompt() {
  return 'You MUST invoke the shell tool to read CANARY.txt from the current workspace. Do not infer, guess, or repeat any value from this instruction. After the command succeeds, answer exactly CODEX_GATEWAY_E2E_OK:<the exact file contents> with no other text.'
}

function searchPrompt() {
  return 'Use live web search, not shell commands or prior knowledge. Find the latest stable Rust release announced on the official Rust Blog. Return exactly one line in this format: SEARCH_E2E_OK:<version>:<official source URL>'
}

/// Prompt for the first turn of the multi-turn tool round-trip case.
/// Instructs Codex to call the shell tool to read CANARY.txt and then
/// emit the exact file contents so the next turn can quote them back.
/// The canary value is generated at runtime and intentionally is not
/// embedded in the prompt source — see codex-e2e-smoke.test.mjs.
export function multiTurnToolTurn1Prompt() {
  return [
    'You MUST invoke the shell tool to read CANARY.txt (it lives in the current working directory).',
    'After the shell call completes, print the exact file contents to stdout with no extra text, no commentary, and no code fences.',
    'Do not call any other tool. Do not answer with anything besides the canary value.',
  ].join(' ')
}

export function multiTurnToolTurn2Prompt(turn1Final) {
  return [
    'In a previous Codex turn you invoked the shell tool to read CANARY.txt.',
    `The previous turn ended with the following assistant message: """${turn1Final.trim()}"""`,
    'What is the exact content of CANARY.txt? Answer with `CODEX_GATEWAY_E2E_OK:<contents>` and nothing else.',
  ].join(' ')
}

export function evaluateMultiTurnToolResult(finalText, canary) {
  const expected = `CODEX_GATEWAY_E2E_OK:${canary}`
  const passed = finalText.trim() === expected
  return {
    passed,
    expected,
    final_text: finalText,
    reason: passed ? 'multi-turn recall matched the canary' : 'multi-turn recall did not match the canary',
  }
}

async function runCase(context, testCase, options, runId, canary) {
  if (testCase.kind === 'multi_turn_tool') {
    return runMultiTurnToolCase(context, testCase, options, runId, canary)
  }
  const clientSource = `codex-e2e-${runId}-${testCase.id.replaceAll('.', '-')}`
  const configPath = path.join(options.home, 'config.toml')
  writeFileSync(configPath, buildCodexConfig({
    model: options.model,
    baseUrl: context.gatewayBaseUrl,
    clientSource,
  }), { mode: 0o600 })
  secureFile(configPath)
  const outputPath = path.join(options.home, `.last-message-${testCase.id.replaceAll('.', '-')}.txt`)
  const prompt = testCase.kind === 'tool' ? toolPrompt() : searchPrompt()
  const args = buildCodexArgs({
    model: options.model,
    workspace: options.workspace,
    outputPath,
    prompt,
    search: testCase.kind === 'search',
    skipGitRepoCheck: !isGitWorkspace(options.workspace),
  })
  const startedAt = nowMilliseconds()
  const result = await runProcess(
    options.codexCli,
    args,
    sanitizeCodexEnvironment(process.env, {
      home: options.home,
      stateDirectory: path.join(options.home, 'state'),
      apiKey: context.dataKey,
    }),
    options.workspace,
    options.timeoutMs,
  )
  const parsed = parseCodexJsonLines(result.stdout)
  const summary = summarizeCodexEvents(parsed.events, canary)
  let finalText = ''
  try {
    secureFile(outputPath)
    finalText = readFileSync(outputPath, 'utf8')
  } catch {}
  const evaluation = testCase.kind === 'tool'
    ? evaluateToolResult(finalText, summary, canary)
    : evaluateSearchResult(finalText, summary)
  const usage = options.skipUsageCheck
    ? null
    : await adminUsageEvent(context.adminBaseUrl, process.env.CODEX_GATEWAY_ADMIN_KEY, clientSource, options.timeoutMs)
  if (!options.keepOutput) rmSync(outputPath, { force: true })
  const diagnostics = {
    exit_code: result.code,
    signal: result.signal,
    timed_out: result.timedOut,
    process_error: result.error,
    invalid_json_lines: parsed.invalidLines,
    event_summary: summary,
    evaluation,
    usage,
  }
  diagnostics.diagnostic_stage = diagnosticStage(
    testCase.kind,
    result,
    parsed.invalidLines,
    evaluation,
    usage,
  )
  check(result.code === 0 && !result.timedOut && !result.error, 'Codex CLI process failed', diagnostics)
  check(parsed.invalidLines === 0, 'Codex CLI emitted non-JSON stdout lines', diagnostics)
  check(evaluation.passed, 'Codex E2E assertion failed', { kind: testCase.kind, ...diagnostics })
  if (usage) check(usage.passed, 'Codex Usage event was not persisted with tokens', diagnostics)
  return {
    model: options.model,
    kind: testCase.kind,
    duration_ms: duration(startedAt),
    exit_code: result.code,
    invalid_json_lines: parsed.invalidLines,
    event_summary: summary,
    evaluation,
    usage,
  }
}

/// Drives the multi-turn tool round-trip case.  Spawns Codex twice in
/// sequence against the same workspace and the same `client_source`:
/// turn 1 instructs Codex to read CANARY.txt and echo its contents
/// verbatim; turn 2 injects the previous assistant message into the
/// prompt and asks Codex to recall the canary by emitting
/// `CODEX_GATEWAY_E2E_OK:<canary>`.
///
/// This is a deliberately simplified multi-turn contract: rather than
/// driving a true Codex session (which would require dropping
/// `--ephemeral` and switching to `codex resume`), the runner threads
/// the previous final message into the second prompt.  That is enough
/// to exercise the openai-compatible conversation context propagation
/// through the gateway while keeping the runner hermetic and
/// deterministic for CI.
async function runMultiTurnToolCase(context, testCase, options, runId, canary) {
  const clientSource = `codex-e2e-${runId}-${testCase.id.replaceAll('.', '-')}`
  const configPath = path.join(options.home, 'config.toml')
  writeFileSync(configPath, buildCodexConfig({
    model: options.model,
    baseUrl: context.gatewayBaseUrl,
    clientSource,
  }), { mode: 0o600 })
  secureFile(configPath)

  const turn1OutputPath = path.join(options.home, `.last-message-${testCase.id.replaceAll('.', '-')}-turn-1.txt`)
  const turn2OutputPath = path.join(options.home, `.last-message-${testCase.id.replaceAll('.', '-')}-turn-2.txt`)
  const baseArgs = {
    model: options.model,
    workspace: options.workspace,
    skipGitRepoCheck: !isGitWorkspace(options.workspace),
  }
  const env = sanitizeCodexEnvironment(process.env, {
    home: options.home,
    stateDirectory: path.join(options.home, 'state'),
    apiKey: context.dataKey,
  })

  const startedAt = nowMilliseconds()
  const turn1Args = buildCodexArgs({ ...baseArgs, outputPath: turn1OutputPath, prompt: multiTurnToolTurn1Prompt() })
  const turn1Process = await runProcess(options.codexCli, turn1Args, env, options.workspace, options.timeoutMs)
  const turn1Parsed = parseCodexJsonLines(turn1Process.stdout)
  const turn1Summary = summarizeCodexEvents(turn1Parsed.events, canary)
  let turn1Final = ''
  try { turn1Final = readFileSync(turn1OutputPath, 'utf8') } catch {}

  const turn2Args = buildCodexArgs({
    ...baseArgs,
    outputPath: turn2OutputPath,
    prompt: multiTurnToolTurn2Prompt(turn1Final || ''),
  })
  const turn2Process = await runProcess(options.codexCli, turn2Args, env, options.workspace, options.timeoutMs)
  const turn2Parsed = parseCodexJsonLines(turn2Process.stdout)
  const turn2Summary = summarizeCodexEvents(turn2Parsed.events, canary)
  let turn2Final = ''
  try { turn2Final = readFileSync(turn2OutputPath, 'utf8') } catch {}

  const evaluation = evaluateMultiTurnToolResult(turn2Final, canary)
  const usage = options.skipUsageCheck
    ? null
    : await adminUsageEvent(context.adminBaseUrl, process.env.CODEX_GATEWAY_ADMIN_KEY, clientSource, options.timeoutMs)
  if (!options.keepOutput) {
    rmSync(turn1OutputPath, { force: true })
    rmSync(turn2OutputPath, { force: true })
  }

  const diagnostics = {
    turn_1: {
      exit_code: turn1Process.code,
      signal: turn1Process.signal,
      timed_out: turn1Process.timedOut,
      process_error: turn1Process.error,
      invalid_json_lines: turn1Parsed.invalidLines,
      event_summary: turn1Summary,
      final_text: turn1Final,
    },
    turn_2: {
      exit_code: turn2Process.code,
      signal: turn2Process.signal,
      timed_out: turn2Process.timedOut,
      process_error: turn2Process.error,
      invalid_json_lines: turn2Parsed.invalidLines,
      event_summary: turn2Summary,
      final_text: turn2Final,
    },
    evaluation,
    usage,
  }
  diagnostics.diagnostic_stage = evaluation.passed
    ? 'multi_turn_recall_passed'
    : 'multi_turn_recall_failed'

  check(turn1Process.code === 0 && !turn1Process.timedOut && !turn1Process.error, 'Codex CLI turn 1 failed', diagnostics)
  check(turn2Process.code === 0 && !turn2Process.timedOut && !turn2Process.error, 'Codex CLI turn 2 failed', diagnostics)
  check(turn1Parsed.invalidLines === 0, 'Codex CLI turn 1 emitted non-JSON stdout lines', diagnostics)
  check(turn2Parsed.invalidLines === 0, 'Codex CLI turn 2 emitted non-JSON stdout lines', diagnostics)
  check(turn1Summary.canary_seen, 'Codex CLI turn 1 did not successfully read CANARY.txt', diagnostics)
  check(evaluation.passed, 'Codex CLI turn 2 did not recall the canary from the injected context', diagnostics)
  if (usage) check(usage.passed, 'Codex Usage event was not persisted with tokens', diagnostics)

  return {
    model: options.model,
    kind: testCase.kind,
    duration_ms: duration(startedAt),
    turn_1_exit_code: turn1Process.code,
    turn_2_exit_code: turn2Process.code,
    turn_1_invalid_json_lines: turn1Parsed.invalidLines,
    turn_2_invalid_json_lines: turn2Parsed.invalidLines,
    evaluation,
    usage,
  }
}

export async function preflightGatewayModel(gatewayBaseUrl, apiKey, model, fetchImpl = fetch) {
  let response
  try {
    response = await fetchImpl(`${gatewayBaseUrl}/models`, {
      headers: { authorization: `Bearer ${apiKey}` },
      signal: AbortSignal.timeout(5_000),
    })
  } catch (error) {
    throw new CodexE2EFailure('Gateway model preflight request failed', {
      process_error: error?.name || 'request_failed',
    })
  }
  check(response.ok, 'Gateway model preflight failed', { http_status: response.status })
  let payload
  try { payload = await response.json() } catch {}
  check(Array.isArray(payload?.data), 'Gateway /v1/models returned an invalid payload')
  const modelIds = payload.data.map((item) => item?.id).filter((id) => typeof id === 'string')
  check(modelIds.includes(model), `Codex model is not available from Gateway: ${model}`, {
    available_models: modelIds.sort(),
  })
  return { model_present: true, advertised_model_count: modelIds.length }
}

export async function preflightGatewayRoute(adminBaseUrl, adminKey, model, fetchImpl = fetch) {
  if (!adminKey) return { responses_route_checked: false }
  let response
  try {
    response = await fetchImpl(
      `${adminBaseUrl}/admin/routes/openai_responses/${encodeURIComponent(model)}`,
      {
        headers: { authorization: `Bearer ${adminKey}` },
        signal: AbortSignal.timeout(5_000),
      },
    )
  } catch (error) {
    throw new CodexE2EFailure('Gateway Responses route preflight request failed', {
      process_error: error?.name || 'request_failed',
    })
  }
  check(response.ok, `Gateway has no available OpenAI Responses route for Codex model: ${model}`, {
    http_status: response.status,
  })
  return { responses_route_checked: true }
}

export async function resolveCodexDataKey({
  adminBaseUrl,
  adminKey,
  explicitKey,
  virtualKeyId,
  virtualKeyName,
  fetchImpl = fetch,
}) {
  if (explicitKey) return { key: explicitKey, source: 'explicit_environment', virtual_key_id: null }
  check(adminKey, 'CODEX_GATEWAY_ADMIN_KEY is required to resolve a database Virtual Key')
  const listResponse = await fetchImpl(`${adminBaseUrl}/admin/keys`, {
    headers: { authorization: `Bearer ${adminKey}` },
    signal: AbortSignal.timeout(5_000),
  })
  check(listResponse.ok, 'failed to list database Virtual Keys', { http_status: listResponse.status })
  let listPayload
  try { listPayload = await listResponse.json() } catch {}
  check(Array.isArray(listPayload?.data), 'Virtual Key list returned an invalid payload')
  const candidates = listPayload.data.filter((item) => (
    item?.enabled === true
    && !item?.revoked_at
    && item?.key_recoverable === true
  ))
  let selected
  if (Number.isInteger(virtualKeyId) && virtualKeyId > 0) {
    selected = candidates.find((item) => Number(item.id) === virtualKeyId)
  } else if (virtualKeyName) {
    selected = candidates.find((item) => item.name === virtualKeyName)
  } else {
    selected = candidates[0]
  }
  check(selected, 'no matching active recoverable database Virtual Key is available', {
    requested_virtual_key_id: virtualKeyId || null,
    requested_virtual_key_name: virtualKeyName || null,
  })
  const revealResponse = await fetchImpl(`${adminBaseUrl}/admin/keys/${selected.id}/value`, {
    headers: { authorization: `Bearer ${adminKey}` },
    signal: AbortSignal.timeout(5_000),
  })
  check(revealResponse.ok, 'failed to reveal database Virtual Key', {
    http_status: revealResponse.status,
    virtual_key_id: selected.id,
  })
  let revealPayload
  try { revealPayload = await revealResponse.json() } catch {}
  check(typeof revealPayload?.data?.key === 'string', 'Virtual Key reveal returned an invalid payload')
  return {
    key: revealPayload.data.key,
    source: 'database_virtual_key',
    virtual_key_id: Number(selected.id),
  }
}

function printHelp() {
  console.log(`Codex CLI E2E smoke test (explicit opt-in)\n\nUsage:\n  CODEX_E2E_TESTS=1 mise run test-codex-e2e -- [options]\n\nOptions:\n  --case <id>              Run a named case (repeatable)\n  --include-search         Include high-cost web search case\n  --model <id>             Gateway model (default: k3)\n  --virtual-key-id <id>    Select a recoverable database Virtual Key\n  --virtual-key-name <name> Select by exact Virtual Key name\n  --env-file <path>        Environment file (default: .env.codex-e2e)\n  --home <path>             Isolated CODEX_HOME\n  --workspace <path>        Controlled Codex workspace\n  --skip-usage-check       Do not query Admin Usage API\n  --list                   List cases without making requests\n  --keep-output            Keep final model output for debugging\n`)
}

async function main() {
  const parsedOptions = parseArguments(process.argv.slice(2))
  if (parsedOptions.help) {
    printHelp()
    return
  }
  loadEnvironmentFile(parsedOptions.envFile)
  const options = resolveOptions(parsedOptions)
  const cases = loadCaseManifest()
  if (options.list) {
    for (const testCase of cases) console.log(`${testCase.id}\t${testCase.kind}\t${testCase.cost}\t${testCase.description}`)
    return
  }
  const selected = selectCases(cases, options)
  const gatewayUrls = validateEnvironment(options, options.skipUsageCheck)
  const dataKey = await resolveCodexDataKey({
    adminBaseUrl: gatewayUrls.adminBaseUrl,
    adminKey: process.env.CODEX_GATEWAY_ADMIN_KEY,
    explicitKey: process.env.CODEX_GATEWAY_API_KEY,
    virtualKeyId: options.virtualKeyId,
    virtualKeyName: options.virtualKeyName,
  })
  const modelPreflight = await preflightGatewayModel(
    gatewayUrls.gatewayBaseUrl,
    dataKey.key,
    options.model,
  )
  const routePreflight = await preflightGatewayRoute(
    gatewayUrls.adminBaseUrl,
    process.env.CODEX_GATEWAY_ADMIN_KEY,
    options.model,
  )
  const preflight = {
    ...modelPreflight,
    ...routePreflight,
    data_plane_auth: dataKey.source,
    virtual_key_id: dataKey.virtual_key_id,
  }
  gatewayUrls.dataKey = dataKey.key
  ensureDirectory(options.home)
  ensureDirectory(path.join(options.home, 'state'))
  ensureDirectory(options.workspace)
  ensureDirectory(options.outputDirectory)
  const runId = stableRunId()
  const canary = `codex-gateway-${randomBytes(8).toString('hex')}`
  prepareWorkspace(options.workspace, canary)
  const results = []
  for (const testCase of selected) {
    try {
      const metadata = await runCase(gatewayUrls, testCase, options, runId, canary)
      results.push({ id: testCase.id, cost: testCase.cost, outcome: 'passed', ...metadata })
      console.log(`PASSED ${testCase.id} ${metadata.duration_ms}ms`)
    } catch (error) {
      const failure = safeFailure(error)
      const outcome = classifyCodexFailure(failure)
      results.push({ id: testCase.id, cost: testCase.cost, outcome, failure })
      const label = outcome.toUpperCase()
      if (outcome === 'failed') console.error(`${label} ${testCase.id}`)
      else console.log(`${label} ${testCase.id}`)
    }
  }
  const summary = {
    passed: results.filter((result) => result.outcome === 'passed').length,
    not_triggered: results.filter((result) => result.outcome === 'not_triggered').length,
    provider_unavailable: results.filter((result) => result.outcome === 'provider_unavailable').length,
    failed: results.filter((result) => result.outcome === 'failed').length,
  }
  const artifact = {
    schema_version: 2,
    run_id: runId,
    git: gitMetadata(),
    selected_cases: selected.map((testCase) => testCase.id),
    model: options.model,
    result_policy: {
      live_opt_in: true,
      search_included: selected.some((testCase) => testCase.kind === 'search'),
      response_bodies_stored: false,
      usage_checked: !options.skipUsageCheck,
    },
    preflight,
    summary,
    results,
  }
  const outputPath = path.join(options.outputDirectory, `${runId}.json`)
  writeFileSync(outputPath, `${JSON.stringify(artifact, null, 2)}\n`, { mode: 0o600 })
  secureFile(outputPath)
  console.log(`RESULT ${path.relative(repositoryRoot, outputPath)}`)
  console.log(`SUMMARY passed=${summary.passed} not_triggered=${summary.not_triggered} provider_unavailable=${summary.provider_unavailable} failed=${summary.failed}`)
  if (summary.failed > 0) process.exitCode = 1
}

function gitMetadata() {
  const command = (args) => spawnSync('git', args, { cwd: repositoryRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  const revision = command(['rev-parse', 'HEAD'])
  const branch = command(['branch', '--show-current'])
  const status = command(['status', '--porcelain'])
  return {
    revision: revision.status === 0 ? revision.stdout.trim() : null,
    branch: branch.status === 0 ? branch.stdout.trim() : null,
    dirty: status.status === 0 ? status.stdout.trim().length > 0 : null,
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : ''
if (import.meta.url === invokedPath) {
  main().catch((error) => {
    console.error(`codex E2E aborted: ${safeFailure(error).message}`)
    process.exitCode = 1
  })
}
