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
    check(['tool', 'search'].includes(testCase.kind), `invalid Codex E2E kind for ${testCase.id}`)
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
  check(process.env.CODEX_GATEWAY_API_KEY, 'CODEX_GATEWAY_API_KEY is required')
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
  const args = []
  if (search) args.push('--search')
  if (skipGitRepoCheck) args.push('--skip-git-repo-check')
  args.push(
    'exec',
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
  return { passed: false, event_count: 0, successful_count: 0, total_tokens: 0, usage_sources: [], modes: [], streamed: false }
}

function toolPrompt(expectedCanary) {
  return `Use the shell tool to read CANARY.txt from the current workspace. Do not infer or guess its contents. After the tool succeeds, answer exactly CODEX_GATEWAY_E2E_OK:${expectedCanary} with no other text.`
}

function searchPrompt() {
  return 'Use live web search, not shell commands or prior knowledge. Find the latest stable Rust release announced on the official Rust Blog. Return exactly one line in this format: SEARCH_E2E_OK:<version>:<official source URL>'
}

async function runCase(context, testCase, options, runId, canary) {
  const clientSource = `codex-e2e-${runId}-${testCase.id.replaceAll('.', '-')}`
  const configPath = path.join(options.home, 'config.toml')
  writeFileSync(configPath, buildCodexConfig({
    model: options.model,
    baseUrl: context.gatewayBaseUrl,
    clientSource,
  }), { mode: 0o600 })
  secureFile(configPath)
  const outputPath = path.join(options.home, `.last-message-${testCase.id.replaceAll('.', '-')}.txt`)
  const prompt = testCase.kind === 'tool' ? toolPrompt(canary) : searchPrompt()
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
      apiKey: process.env.CODEX_GATEWAY_API_KEY,
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

function printHelp() {
  console.log(`Codex CLI E2E smoke test (explicit opt-in)\n\nUsage:\n  CODEX_E2E_TESTS=1 mise run test-codex-e2e -- [options]\n\nOptions:\n  --case <id>              Run a named case (repeatable)\n  --include-search         Include high-cost web search case\n  --model <id>             Gateway model (default: k3)\n  --env-file <path>        Environment file (default: .env.codex-e2e)\n  --home <path>             Isolated CODEX_HOME\n  --workspace <path>        Controlled Codex workspace\n  --skip-usage-check       Do not query Admin Usage API\n  --list                   List cases without making requests\n  --keep-output            Keep final model output for debugging\n`)
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
      results.push({ id: testCase.id, cost: testCase.cost, outcome: 'failed', failure: safeFailure(error) })
      console.error(`FAILED ${testCase.id}`)
    }
  }
  const summary = {
    passed: results.filter((result) => result.outcome === 'passed').length,
    failed: results.filter((result) => result.outcome === 'failed').length,
  }
  const artifact = {
    schema_version: 1,
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
    summary,
    results,
  }
  const outputPath = path.join(options.outputDirectory, `${runId}.json`)
  writeFileSync(outputPath, `${JSON.stringify(artifact, null, 2)}\n`, { mode: 0o600 })
  secureFile(outputPath)
  console.log(`RESULT ${path.relative(repositoryRoot, outputPath)}`)
  console.log(`SUMMARY passed=${summary.passed} failed=${summary.failed}`)
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
