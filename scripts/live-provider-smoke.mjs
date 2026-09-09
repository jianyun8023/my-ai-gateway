import { spawn, spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const repositoryRoot = path.resolve(scriptDir, '..')
const defaultManifestPath = path.join(repositoryRoot, 'tests/live/provider-smoke.cases.json')
const defaultOutputDirectory = path.join(repositoryRoot, 'target/live-provider-smoke')

class SmokeFailure extends Error {
  constructor(message, metadata = {}) {
    super(message)
    this.name = 'SmokeFailure'
    this.metadata = metadata
  }
}

function check(condition, message, metadata = {}) {
  if (!condition) throw new SmokeFailure(message, metadata)
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
  check(Array.isArray(cases) && cases.length > 0, 'live case manifest must be a non-empty array')
  const ids = new Set()
  for (const testCase of cases) {
    check(typeof testCase.id === 'string' && testCase.id.length > 0, 'live case id is required')
    check(!ids.has(testCase.id), `duplicate live case id: ${testCase.id}`)
    ids.add(testCase.id)
    check(['low', 'high'].includes(testCase.cost), `invalid cost for ${testCase.id}`)
    check(Array.isArray(testCase.required_env), `required_env must be an array for ${testCase.id}`)
  }
  return cases
}

export function parseArguments(argv) {
  const options = {
    cases: [],
    providers: [],
    includeHighCost: false,
    keepSchema: false,
    list: false,
    strictKnownIssues: false,
    envFile: process.env.LIVE_ENV_FILE || '.env.live',
    outputDirectory: process.env.LIVE_RESULT_DIR || defaultOutputDirectory,
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
    else if (argument === '--provider') options.providers.push(nextValue())
    else if (argument.startsWith('--provider=')) options.providers.push(argument.slice('--provider='.length))
    else if (argument === '--env-file') options.envFile = nextValue()
    else if (argument.startsWith('--env-file=')) options.envFile = argument.slice('--env-file='.length)
    else if (argument === '--output-dir') options.outputDirectory = nextValue()
    else if (argument.startsWith('--output-dir=')) options.outputDirectory = argument.slice('--output-dir='.length)
    else if (argument === '--include-high-cost') options.includeHighCost = true
    else if (argument === '--keep-schema') options.keepSchema = true
    else if (argument === '--strict-known-issues') options.strictKnownIssues = true
    else if (argument === '--list') options.list = true
    else throw new SmokeFailure(`unknown argument: ${argument}`)
  }
  return options
}

export function selectCases(cases, options) {
  const knownIds = new Set(cases.map((testCase) => testCase.id))
  for (const id of options.cases) check(knownIds.has(id), `unknown live case: ${id}`)
  const knownProviders = new Set(cases.map((testCase) => testCase.provider))
  for (const provider of options.providers) {
    check(knownProviders.has(provider), `unknown live provider: ${provider}`)
  }

  let selected = cases
  if (options.cases.length > 0) {
    const requested = new Set(options.cases)
    selected = selected.filter((testCase) => requested.has(testCase.id))
  } else if (!options.includeHighCost) {
    selected = selected.filter((testCase) => testCase.cost === 'low')
  }
  if (options.providers.length > 0) {
    const providers = new Set(options.providers)
    selected = selected.filter((testCase) => providers.has(testCase.provider))
  }
  check(selected.length > 0, 'no live cases selected')
  return selected
}

export function mergeSourceUrlAllowlist(existing, allowLocalSource = false) {
  const entries = String(existing || '')
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean)
  if (allowLocalSource) entries.push('127.0.0.1')
  return [...new Set(entries)].join(',')
}

export function classifyFailure(failure) {
  const status = Number(failure?.http_status || 0)
  const code = String(failure?.error_code || '').toLowerCase()
  if (
    [403, 429].includes(status)
    || ['permission_error', 'insufficient_quota', 'billing_error', 'rate_limit_exceeded'].includes(code)
  ) return 'provider_unavailable'
  return 'failed'
}

export function isTransientFailure(failure) {
  const status = Number(failure?.http_status || 0)
  const code = String(failure?.error_code || '').toLowerCase()
  return [502, 504].includes(status) || code === 'upstream_request_failed'
}

function protocolCapabilities(protocols) {
  return Object.fromEntries(
    ['openai_chat_completions', 'openai_responses', 'anthropic_messages'].map((protocol) => [
      protocol,
      protocols[protocol] || { mode: 'unsupported' },
    ]),
  )
}

function featureCapabilities() {
  return {
    streaming: 'native',
    tools: 'native',
    tool_streaming: 'native',
    thinking: 'native',
    web_search: 'native',
    usage: 'native',
  }
}

function nativeProvider({ id, name, baseUrl, model, endpoints }) {
  const nativeProtocols = Object.keys(endpoints)
  return {
    id,
    name,
    base_url: baseUrl,
    models: [model],
    native_protocols: nativeProtocols,
    endpoints,
    capabilities: featureCapabilities(),
    protocol_capabilities: protocolCapabilities(
      Object.fromEntries(nativeProtocols.map((protocol) => [protocol, { mode: 'native' }])),
    ),
    model_overrides: {},
  }
}

function account(id, providerId, credentialEnv, modelMap = {}) {
  return {
    id,
    provider_id: providerId,
    display_name: id,
    credential_env: credentialEnv,
    enabled: true,
    weight: 100,
    model_map: modelMap,
  }
}

function route(id, model, providerId, protocols, primaryAccountId, extra = {}) {
  return {
    id,
    model,
    provider_id: providerId,
    protocols,
    primary_account_id: primaryAccountId,
    fallback_accounts: [],
    strategy: 'primary_then_weighted_fallback',
    mode: 'native',
    allow_lossy_conversion: false,
    ...extra,
  }
}

export function buildGatewayConfig(selectedCases, environment, port, fallbackMockUrl) {
  const selectedIds = new Set(selectedCases.map((testCase) => testCase.id))
  const uses = (prefix) => [...selectedIds].some((id) => id.startsWith(prefix))
  const providers = []
  const accounts = []
  const routes = []

  if (uses('deepseek.')) {
    providers.push(nativeProvider({
      id: 'deepseek-live',
      name: 'DeepSeek Live Smoke',
      baseUrl: environment.DEEPSEEK_BASE_URL,
      model: 'deepseek-v4-flash',
      endpoints: {
        openai_chat_completions: '/chat/completions',
        openai_responses: '/responses',
      },
    }))
    accounts.push(account('deepseek-live', 'deepseek-live', 'DEEPSEEK_API_KEY'))
    routes.push(route(
      'deepseek-live',
      'deepseek-v4-flash',
      'deepseek-live',
      ['openai_chat_completions', 'openai_responses'],
      'deepseek-live',
    ))
  }

  if (uses('minimax.')) {
    providers.push(nativeProvider({
      id: 'minimax-live',
      name: 'MiniMax Live Smoke',
      baseUrl: environment.MINIMAX_BASE_URL,
      model: 'MiniMax-M3',
      endpoints: {
        openai_chat_completions: '/v1/chat/completions',
        openai_responses: '/v1/responses',
      },
    }))
    accounts.push(account('minimax-live', 'minimax-live', 'MINIMAX_API_KEY'))
    routes.push(route(
      'minimax-live',
      'MiniMax-M3',
      'minimax-live',
      ['openai_chat_completions', 'openai_responses'],
      'minimax-live',
    ))
  }

  if (uses('kimi.')) {
    // Kimi Code serves OpenAI Responses natively at /v1/responses (issue
    // #157); the embedded Responses→Anthropic adapter was removed.
    providers.push(nativeProvider({
      id: 'kimi-live',
      name: 'Kimi Live Smoke',
      baseUrl: environment.KIMI_BASE_URL,
      model: 'k3',
      endpoints: {
        openai_chat_completions: '/v1/chat/completions',
        openai_responses: '/v1/responses',
        anthropic_messages: '/v1/messages',
      },
    }))
    accounts.push(account('kimi-live', 'kimi-live', 'KIMI_API_KEY'))
    routes.push(route(
      'kimi-live',
      'k3',
      'kimi-live',
      ['openai_responses'],
      'kimi-live',
    ))
  }

  if (selectedIds.has('fallback.deepseek_bai')) {
    check(fallbackMockUrl, 'fallback mock URL is required')
    const logicalModel = 'deepseek-v4-flash-fallback-smoke'
    providers.push(nativeProvider({
      id: 'deepseek-failure-live',
      name: 'DeepSeek Injected Failure',
      baseUrl: fallbackMockUrl,
      model: logicalModel,
      endpoints: { openai_chat_completions: '/chat/completions' },
    }))
    providers.push(nativeProvider({
      id: 'bai-live',
      name: 'b.ai Live Fallback',
      baseUrl: environment.B_AI_BASE_URL,
      model: logicalModel,
      endpoints: { openai_chat_completions: '/chat/completions' },
    }))
    accounts.push(account(
      'deepseek-failure-live',
      'deepseek-failure-live',
      'FALLBACK_PRIMARY_TEST_KEY',
      { [logicalModel]: 'deepseek-v4-flash' },
    ))
    accounts.push(account(
      'bai-live',
      'bai-live',
      'B_AI_API_KEY',
      { [logicalModel]: 'deepseek-v4-flash' },
    ))
    routes.push(route(
      'deepseek-bai-fallback-live',
      logicalModel,
      'deepseek-failure-live',
      ['openai_chat_completions'],
      'deepseek-failure-live',
      { fallback_accounts: ['bai-live'] },
    ))
  }

  return {
    listen_addr: `127.0.0.1:${port}`,
    providers,
    accounts,
    routes,
  }
}

export function parseSse(payload) {
  const events = []
  for (const line of payload.split(/\r?\n/)) {
    if (!line.startsWith('data:')) continue
    const data = line.slice('data:'.length).trim()
    if (!data || data === '[DONE]') continue
    try {
      events.push(JSON.parse(data))
    } catch {
      throw new SmokeFailure('SSE data line is not valid JSON')
    }
  }
  return events
}

export function strictlyIncreasing(numbers) {
  for (let index = 1; index < numbers.length; index += 1) {
    if (!(numbers[index] > numbers[index - 1])) return false
  }
  return true
}

function responseUsage(usage) {
  if (!usage || typeof usage !== 'object') return null
  const input = usage.input_tokens ?? usage.prompt_tokens ?? 0
  const output = usage.output_tokens ?? usage.completion_tokens ?? 0
  const inputDetails = usage.input_tokens_details ?? usage.prompt_tokens_details
  const outputDetails = usage.output_tokens_details ?? usage.completion_tokens_details
  const cacheRead = inputDetails
    ? (inputDetails.cached_tokens ?? 0)
    : (usage.cache_read_input_tokens ?? 0) + (usage.cached_tokens ?? 0)
  const cacheCreation = inputDetails ? 0 : (usage.cache_creation_input_tokens ?? 0)
  return {
    input_tokens: input,
    output_tokens: output,
    total_tokens: usage.total_tokens ?? input + output,
    cached_tokens: cacheRead + cacheCreation,
    cache_read_tokens: cacheRead,
    cache_creation_tokens: cacheCreation,
    reasoning_tokens: outputDetails ? (outputDetails.reasoning_tokens ?? 0) : (usage.reasoning_tokens ?? 0),
  }
}

function eventMetadata(detail) {
  const event = detail?.data || {}
  return {
    request_id: event.request_id || null,
    provider_id: event.provider_id || null,
    source_id: event.source_id || null,
    account_id: event.account_id || null,
    protocol_in: event.protocol_in || null,
    protocol_upstream: event.protocol_upstream || null,
    mode: event.mode || null,
    status_code: event.status_code || 0,
    success: Boolean(event.success),
    retry_count: event.retry_count || 0,
    streamed: Boolean(event.streamed),
    usage_source: event.usage_source || 'missing',
    input_tokens: event.input_tokens || 0,
    output_tokens: event.output_tokens || 0,
    reasoning_tokens: event.reasoning_tokens || 0,
    cached_tokens: event.cached_tokens || 0,
    cache_read_tokens: event.cache_read_tokens || 0,
    cache_creation_tokens: event.cache_creation_tokens || 0,
    total_tokens: event.total_tokens || 0,
    attempts: (detail?.attempts || []).map((attempt) => ({
      attempt_no: attempt.attempt_no,
      provider_id: attempt.provider_id || null,
      source_id: attempt.source_id || null,
      account_id: attempt.account_id,
      upstream_model_id: attempt.upstream_model_id || null,
      status_code: attempt.status_code,
      success: Boolean(attempt.success),
    })),
  }
}

function providerErrorMetadata(payload) {
  const error = payload?.error || {}
  return {
    error_code: error.code || error.type || 'unknown',
    error_type: error.type || null,
  }
}

async function fetchWithTimeout(url, options, timeoutMs) {
  const startedAt = nowMilliseconds()
  const response = await fetch(url, { ...options, signal: AbortSignal.timeout(timeoutMs) })
  const text = await response.text()
  return { response, text, duration_ms: duration(startedAt) }
}

async function postGatewayJson(context, endpoint, body, clientSource) {
  const { response, text, duration_ms } = await fetchWithTimeout(
    `${context.gatewayBaseUrl}${endpoint}`,
    {
      method: 'POST',
      headers: {
        authorization: `Bearer ${context.gatewayKey}`,
        'content-type': 'application/json',
        'x-client-source': clientSource,
      },
      body: JSON.stringify(body),
    },
    context.timeoutMs,
  )
  let payload
  try {
    payload = JSON.parse(text)
  } catch {
    throw new SmokeFailure('gateway returned non-JSON response', {
      http_status: response.status,
      duration_ms,
    })
  }
  if (!response.ok) {
    throw new SmokeFailure('gateway request failed', {
      http_status: response.status,
      duration_ms,
      ...providerErrorMetadata(payload),
    })
  }
  return { payload, duration_ms, http_status: response.status }
}

async function postGatewaySse(context, endpoint, body, clientSource) {
  const { response, text, duration_ms } = await fetchWithTimeout(
    `${context.gatewayBaseUrl}${endpoint}`,
    {
      method: 'POST',
      headers: {
        authorization: `Bearer ${context.gatewayKey}`,
        'content-type': 'application/json',
        'x-client-source': clientSource,
      },
      body: JSON.stringify(body),
    },
    context.timeoutMs,
  )
  if (!response.ok) {
    let error = {}
    try { error = providerErrorMetadata(JSON.parse(text)) } catch {}
    throw new SmokeFailure('gateway SSE request failed', {
      http_status: response.status,
      duration_ms,
      ...error,
    })
  }
  return { events: parseSse(text), raw: text, duration_ms, http_status: response.status }
}

async function adminJson(context, endpoint) {
  const { response, text } = await fetchWithTimeout(
    `${context.gatewayBaseUrl}${endpoint}`,
    { headers: { authorization: `Bearer ${context.adminKey}` } },
    10_000,
  )
  check(response.ok, 'admin request failed', { http_status: response.status })
  try {
    return JSON.parse(text)
  } catch {
    throw new SmokeFailure('admin endpoint returned non-JSON response')
  }
}

async function usageEvent(context, clientSource) {
  const deadline = Date.now() + 8_000
  while (Date.now() < deadline) {
    const events = await adminJson(
      context,
      `/admin/usage/events?client_source=${encodeURIComponent(clientSource)}&limit=5`,
    )
    const requestId = events.data?.[0]?.request_id
    if (requestId) {
      const detail = await adminJson(context, `/admin/usage/events/${encodeURIComponent(requestId)}`)
      return eventMetadata(detail)
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  throw new SmokeFailure('usage event was not persisted', { client_source: clientSource })
}

function toolDefinitionChat() {
  return {
    type: 'function',
    function: {
      name: 'lookup_weather',
      description: 'Look up weather for a city',
      parameters: {
        type: 'object',
        properties: { city: { type: 'string' } },
        required: ['city'],
      },
    },
  }
}

function toolDefinitionResponses() {
  return {
    type: 'function',
    name: 'lookup_weather',
    description: 'Look up weather for a city',
    parameters: {
      type: 'object',
      properties: { city: { type: 'string' } },
      required: ['city'],
    },
  }
}

function clientSource(context, testCase, phase) {
  return `live-${context.shortRunId}-${testCase.id.replaceAll('.', '-')}-${phase}`
}

async function chatFunctionRoundTrip(context, testCase, provider) {
  const model = provider === 'deepseek' ? 'deepseek-v4-flash' : 'MiniMax-M3'
  const user = 'Call lookup_weather for Tokyo. Do not answer directly.'
  const firstSource = clientSource(context, testCase, 'first')
  const firstBody = {
    model,
    messages: [{ role: 'user', content: user }],
    tools: [toolDefinitionChat()],
    tool_choice: 'required',
    stream: false,
  }
  if (provider === 'deepseek') {
    firstBody.thinking = { type: 'disabled' }
    firstBody.max_tokens = 256
  } else {
    firstBody.max_completion_tokens = 256
  }
  const first = await postGatewayJson(context, '/v1/chat/completions', firstBody, firstSource)
  const firstChoice = first.payload.choices?.[0]
  const toolCall = firstChoice?.message?.tool_calls?.[0]
  check(firstChoice?.finish_reason === 'tool_calls', 'provider did not stop for a tool call')
  check(toolCall?.function?.name === 'lookup_weather', 'provider called the wrong tool')
  let argumentsObject
  try { argumentsObject = JSON.parse(toolCall.function.arguments) } catch {}
  check(argumentsObject && typeof argumentsObject === 'object', 'tool arguments are not valid JSON')

  const secondSource = clientSource(context, testCase, 'second')
  const secondBody = {
    model,
    messages: [
      { role: 'user', content: user },
      firstChoice.message,
      {
        role: 'tool',
        tool_call_id: toolCall.id,
        content: JSON.stringify({ temperature_c: 22, condition: 'clear' }),
      },
    ],
    tools: [toolDefinitionChat()],
    tool_choice: 'none',
    stream: false,
  }
  if (provider === 'deepseek') {
    secondBody.thinking = { type: 'disabled' }
    secondBody.max_tokens = 256
  } else {
    secondBody.max_completion_tokens = 256
  }
  const second = await postGatewayJson(context, '/v1/chat/completions', secondBody, secondSource)
  const finalChoice = second.payload.choices?.[0]
  check(finalChoice?.finish_reason === 'stop', 'provider did not finish after tool result')
  check((finalChoice?.message?.content || '').length > 0, 'provider returned no final content')
  check((finalChoice?.message?.tool_calls || []).length === 0, 'provider repeated the tool call')

  return {
    provider,
    tool_name: toolCall.function.name,
    arguments_valid: true,
    first_duration_ms: first.duration_ms,
    second_duration_ms: second.duration_ms,
    response_usage: [responseUsage(first.payload.usage), responseUsage(second.payload.usage)],
    usage_events: [await usageEvent(context, firstSource), await usageEvent(context, secondSource)],
  }
}

function responseSearchMetadata(payload) {
  const output = payload.output || []
  const searchCalls = output.filter((item) => item.type === 'web_search_call')
  const citations = output
    .filter((item) => item.type === 'message')
    .flatMap((item) => item.content || [])
    .flatMap((part) => part.annotations || [])
  return {
    status: payload.status || null,
    output_types: output.map((item) => item.type),
    search_calls: searchCalls.length,
    completed_search_calls: searchCalls.filter((item) => item.status === 'completed').length,
    action_types: searchCalls.map((item) => item.action?.type || null),
    query_count: searchCalls.reduce(
      (total, item) => total + (item.action?.query ? 1 : 0) + (item.action?.queries?.length || 0),
      0,
    ),
    source_count: searchCalls.reduce((total, item) => total + (item.action?.sources?.length || 0), 0),
    citation_count: citations.length,
    usage: responseUsage(payload.usage),
  }
}

async function nativeWebSearch(context, testCase, provider) {
  const model = provider === 'deepseek' ? 'deepseek-v4-flash' : 'MiniMax-M3'
  const source = clientSource(context, testCase, 'request')
  // `web_search_call.action.sources` is opt-in in OpenAI Responses: the
  // gateway forwards the request body verbatim, so the client must declare
  // the include list to surface source URLs (issue #85).  `url_citation`
  // annotations on `message.output_text` should appear by default.
  const include = ['web_search_call.action.sources']
  const body = {
    model,
    input: 'Use web search to find the current stable Rust release from an official Rust source. Return a concise answer with a source.',
    tools: [{ type: 'web_search' }],
    tool_choice: { type: 'web_search' },
    include,
    max_output_tokens: 512,
    stream: false,
  }
  if (provider === 'deepseek') body.reasoning = { effort: 'low' }
  const response = await postGatewayJson(context, '/v1/responses', body, source)
  const metadata = responseSearchMetadata(response.payload)
  check(metadata.status === 'completed', 'search response did not complete')
  check(metadata.search_calls > 0, 'provider emitted no web_search_call')
  check(metadata.completed_search_calls === metadata.search_calls, 'search call did not complete')
  check(metadata.usage?.total_tokens > 0, 'provider search response has no usage')
  return {
    provider,
    duration_ms: response.duration_ms,
    ...metadata,
    include_sent: include,
    usage_event: await usageEvent(context, source),
    known_issue_checks: [
      {
        id: 'web_search_sources_visible',
        issue: 85,
        passed: metadata.source_count > 0,
        note:
          'Native Providers (MiniMax / DeepSeek) currently return empty `web_search_call.action.sources` even with `include`. The capability matrix marks this as Unsupported (#85).',
      },
      {
        id: 'web_search_citations_visible',
        issue: 85,
        passed: metadata.citation_count > 0,
        note:
          'Native Providers currently return no `url_citation` annotations on `message.output_text`. The capability matrix marks this as Unsupported (#85).',
      },
    ],
  }
}

function kimiResponseBody(options) {
  return {
    model: 'k3',
    reasoning: { effort: 'low' },
    ...(options.tools ? { tool_choice: 'auto' } : {}),
    max_output_tokens: 512,
    stream: false,
    ...options,
  }
}

function checkKimiUsage(usage, event, streamed, phase) {
  const expected = responseUsage(usage)
  if (!expected) {
    check(
      event.usage_source !== 'missing' && event.total_tokens > 0,
      'Kimi response has no persisted usage',
      { phase, usage_event: event },
    )
    return
  }
  const expectedSource = streamed ? 'parsed' : 'upstream'
  const mismatchedFields = Object.keys(expected).filter((field) => event[field] !== expected[field])
  check(
    event.usage_source === expectedSource && mismatchedFields.length === 0,
    'Kimi reported usage does not match the persisted usage event',
    {
      phase,
      expected_usage_source: expectedSource,
      usage_source: event.usage_source,
      mismatched_fields: mismatchedFields,
      response_usage: expected,
      usage_event: event,
    },
  )
}

function checkKimiFunctionCall(payload) {
  check(payload?.status === 'completed', 'Kimi function response did not complete')
  const functionCalls = payload.output?.filter((item) => item.type === 'function_call') || []
  check(functionCalls.length === 1, 'Kimi response has no single function call')
  const functionCall = functionCalls[0]
  check(functionCall.name === 'lookup_weather', 'Kimi called the wrong tool')
  check(functionCall.status == null || functionCall.status === 'completed', 'Kimi function call did not complete')
  check(typeof functionCall.call_id === 'string' && functionCall.call_id.length > 0, 'Kimi function call has no call_id')
  let args
  try { args = JSON.parse(functionCall.arguments) } catch {}
  check(
    args && typeof args === 'object' && !Array.isArray(args)
      && typeof args.city === 'string' && args.city.trim().length > 0,
    'Kimi tool arguments do not match the expected JSON schema',
  )
  return functionCall
}

function checkKimiStream(events) {
  const sequence = events.map((event) => event.sequence_number)
  check(
    sequence.length > 0 && sequence.every(Number.isInteger) && strictlyIncreasing(sequence),
    'Kimi SSE sequence is not monotonic or is missing sequence numbers',
  )
  check(
    events[0]?.type === 'response.created'
      && events.filter((event) => event.type === 'response.created').length === 1
      && events.at(-1)?.type === 'response.completed'
      && events.filter((event) => event.type === 'response.completed').length === 1
      && !events.some((event) => ['error', 'response.failed', 'response.incomplete'].includes(event.type)),
    'Kimi SSE did not preserve a complete response lifecycle',
  )
  const finalResponse = completedResponse(events)
  check(finalResponse?.status === 'completed', 'Kimi SSE final response did not complete')
  return finalResponse
}

async function kimiFunctionRoundTrip(context, testCase) {
  const user = 'Call lookup_weather for Tokyo. Do not answer directly.'
  const tools = [toolDefinitionResponses()]
  const firstSource = clientSource(context, testCase, 'first')
  const first = await postGatewayJson(context, '/v1/responses', kimiResponseBody({
    input: user,
    tools,
    parallel_tool_calls: false,
  }), firstSource)
  const functionCall = checkKimiFunctionCall(first.payload)

  const conversation = [
    { type: 'message', role: 'user', content: [{ type: 'input_text', text: user }] },
    ...first.payload.output,
    {
      type: 'function_call_output',
      call_id: functionCall.call_id,
      output: JSON.stringify({ temperature_c: 22, condition: 'clear' }),
    },
  ]
  const secondSource = clientSource(context, testCase, 'second')
  const second = await postGatewayJson(context, '/v1/responses', kimiResponseBody({
    instructions: 'Use the supplied tool result and answer briefly. Do not call the tool again.',
    input: conversation,
    tools,
    parallel_tool_calls: false,
  }), secondSource)
  check(second.payload.status === 'completed', 'Kimi tool result response did not complete')
  check(second.payload.output?.some((item) => item.type === 'message'), 'Kimi returned no final message')
  check(!second.payload.output?.some((item) => item.type === 'function_call'), 'Kimi repeated the function call')

  const firstEvent = await usageEvent(context, firstSource)
  const secondEvent = await usageEvent(context, secondSource)
  checkKimiUsage(first.payload.usage, firstEvent, false, 'first')
  checkKimiUsage(second.payload.usage, secondEvent, false, 'second')
  return {
    tool_name: functionCall.name,
    arguments_valid: true,
    first_duration_ms: first.duration_ms,
    second_duration_ms: second.duration_ms,
    response_usage: [responseUsage(first.payload.usage), responseUsage(second.payload.usage)],
    usage_events: [firstEvent, secondEvent],
  }
}

function completedResponse(events) {
  return [...events].reverse().find((event) => event.type === 'response.completed')?.response
}

async function kimiFunctionStream(context, testCase) {
  const source = clientSource(context, testCase, 'request')
  const response = await postGatewaySse(context, '/v1/responses', kimiResponseBody({
    input: 'Call lookup_weather for Tokyo. Do not answer directly.',
    tools: [toolDefinitionResponses()],
    parallel_tool_calls: false,
    stream: true,
  }), source)
  const finalResponse = checkKimiStream(response.events)
  const functionCall = checkKimiFunctionCall(finalResponse)
  const argumentDeltas = response.events.filter(
    (event) => event.type === 'response.function_call_arguments.delta',
  )
  const argumentsDone = response.events.filter(
    (event) => event.type === 'response.function_call_arguments.done',
  )
  check(argumentDeltas.length > 0, 'Kimi emitted no function argument deltas')
  check(argumentsDone.length === 1, 'Kimi emitted no single function arguments done event')
  const done = argumentsDone[0]
  check(
    typeof functionCall.id === 'string' && functionCall.id.length > 0
      && [...argumentDeltas, done].every((event) => event.item_id === functionCall.id
        && event.output_index === finalResponse.output.indexOf(functionCall)),
    'Kimi function argument events reference the wrong output item',
  )
  check(
    argumentDeltas.every((event) => typeof event.delta === 'string'
      && event.sequence_number < done.sequence_number)
      && argumentDeltas.map((event) => event.delta).join('') === functionCall.arguments
      && done.arguments === functionCall.arguments,
    'Kimi function argument deltas, done event and final arguments disagree',
  )
  const event = await usageEvent(context, source)
  checkKimiUsage(finalResponse.usage, event, true, 'request')
  return {
    duration_ms: response.duration_ms,
    event_count: response.events.length,
    sequence_monotonic: true,
    argument_delta_events: argumentDeltas.length,
    arguments_done_events: argumentsDone.length,
    arguments_valid: true,
    tool_name: functionCall.name,
    usage: responseUsage(finalResponse?.usage),
    usage_event: event,
  }
}

async function kimiWebSearch(context, testCase, streaming) {
  const source = clientSource(context, testCase, 'request')
  const body = kimiResponseBody({
    input: streaming
      ? 'Use web search to find the official MiniMax Server Tools documentation page. Return a concise answer with a source.'
      : 'Use web search to find the current stable Rust release from an official Rust source. Return a concise answer with a source.',
    tools: [{ type: 'web_search' }],
    max_output_tokens: 1024,
    stream: streaming,
  })
  if (!streaming) {
    const response = await postGatewayJson(context, '/v1/responses', body, source)
    const metadata = responseSearchMetadata(response.payload)
    check(metadata.status === 'completed', 'Kimi search response did not complete')
    check(metadata.search_calls === 1 && metadata.completed_search_calls === 1, 'Kimi search call did not complete')
    // Native Responses citation/source visibility is unverified (preset keeps
    // it unknown), so source_count is reported as evidence, not asserted.
    const event = await usageEvent(context, source)
    checkKimiUsage(response.payload.usage, event, false, 'request')
    return {
      duration_ms: response.duration_ms,
      ...metadata,
      usage_event: event,
    }
  }

  const response = await postGatewaySse(context, '/v1/responses', body, source)
  const finalResponse = checkKimiStream(response.events)
  const metadata = responseSearchMetadata(finalResponse || {})
  const keyEventOrder = []
  for (const event of response.events) {
    if (!keyEventOrder.includes(event.type)) keyEventOrder.push(event.type)
  }
  check(response.events.filter((event) => event.type === 'response.web_search_call.in_progress').length === 1, 'missing search in_progress event')
  check(response.events.filter((event) => event.type === 'response.web_search_call.searching').length === 1, 'missing search searching event')
  check(response.events.filter((event) => event.type === 'response.web_search_call.completed').length === 1, 'missing search completed event')
  const event = await usageEvent(context, source)
  checkKimiUsage(finalResponse.usage, event, true, 'request')
  return {
    duration_ms: response.duration_ms,
    event_count: response.events.length,
    sequence_monotonic: true,
    key_event_order: keyEventOrder.filter((type) => /^(response\.(created|in_progress|output_item|web_search_call|completed))/.test(type)),
    ...metadata,
    usage_event: event,
  }
}

async function kimiReasoningSignature(context, testCase) {
  const user = 'Solve carefully: how many trailing zeros are in 100 factorial? Give a concise final answer.'
  const firstSource = clientSource(context, testCase, 'first')
  const first = await postGatewayJson(context, '/v1/responses', kimiResponseBody({
    input: user,
    reasoning: { effort: 'high' },
    max_output_tokens: 2048,
  }), firstSource)
  const firstEvent = await usageEvent(context, firstSource)
  checkKimiUsage(first.payload.usage, firstEvent, false, 'first')
  const reasoning = first.payload.output?.find(
    (item) => item.type === 'reasoning' && (item.encrypted_content || '').length > 0,
  )
  if (!reasoning) {
    return {
      outcome: 'not_triggered',
      reason: 'upstream emitted no thinking/signature block',
      duration_ms: first.duration_ms,
      response_usage: responseUsage(first.payload.usage),
      usage_event: firstEvent,
    }
  }

  const secondSource = clientSource(context, testCase, 'second')
  const conversation = [
    { type: 'message', role: 'user', content: [{ type: 'input_text', text: user }] },
    ...first.payload.output,
    { type: 'message', role: 'user', content: [{ type: 'input_text', text: 'Acknowledge the prior result briefly.' }] },
  ]
  const second = await postGatewayJson(context, '/v1/responses', kimiResponseBody({
    input: conversation,
    reasoning: { effort: 'high' },
    max_output_tokens: 2048,
  }), secondSource)
  check(second.payload.status === 'completed', 'Kimi signed reasoning follow-up did not complete')
  const secondEvent = await usageEvent(context, secondSource)
  checkKimiUsage(second.payload.usage, secondEvent, false, 'second')
  return {
    outcome: 'passed',
    encrypted_content_present: true,
    encrypted_content_length: reasoning.encrypted_content.length,
    first_duration_ms: first.duration_ms,
    second_duration_ms: second.duration_ms,
    response_usage: [responseUsage(first.payload.usage), responseUsage(second.payload.usage)],
    usage_events: [firstEvent, secondEvent],
  }
}

async function fallbackDeepSeekBai(context, testCase) {
  const source = clientSource(context, testCase, 'request')
  const response = await postGatewayJson(context, '/v1/chat/completions', {
    model: 'deepseek-v4-flash-fallback-smoke',
    messages: [{ role: 'user', content: 'ping' }],
    max_tokens: 3,
    stream: false,
  }, source)
  check(response.payload.choices?.length > 0, 'b.ai fallback returned no choice')
  const event = await usageEvent(context, source)
  check(event.success && event.status_code === 200, 'fallback logical event did not succeed')
  check(event.retry_count === 1, 'fallback logical event has wrong retry_count')
  check(event.provider_id === 'custom', 'fallback logical event has wrong provider attribution')
  check(event.source_id === 'bai-live', 'fallback logical event was not attributed to b.ai')
  check(event.attempts.length === 2, 'fallback did not persist two attempts')
  check(
    event.attempts[0].provider_id === 'custom'
      && event.attempts[0].source_id === 'deepseek-failure-live'
      && event.attempts[0].status_code === 503,
    'primary fallback attempt is incorrect',
  )
  check(
    event.attempts[1].provider_id === 'custom'
      && event.attempts[1].source_id === 'bai-live'
      && event.attempts[1].status_code === 200,
    'b.ai fallback attempt is incorrect',
  )
  return {
    duration_ms: response.duration_ms,
    response_usage: responseUsage(response.payload.usage),
    usage_event: event,
  }
}

export async function runCase(context, testCase) {
  switch (testCase.id) {
    case 'deepseek.function': return chatFunctionRoundTrip(context, testCase, 'deepseek')
    case 'deepseek.web_search': return nativeWebSearch(context, testCase, 'deepseek')
    case 'minimax.function': return chatFunctionRoundTrip(context, testCase, 'minimax')
    case 'minimax.web_search': return nativeWebSearch(context, testCase, 'minimax')
    case 'kimi.function': return kimiFunctionRoundTrip(context, testCase)
    case 'kimi.function_stream': return kimiFunctionStream(context, testCase)
    case 'kimi.web_search': return kimiWebSearch(context, testCase, false)
    case 'kimi.web_search_stream': return kimiWebSearch(context, testCase, true)
    case 'kimi.reasoning_signature': return kimiReasoningSignature(context, testCase)
    case 'fallback.deepseek_bai': return fallbackDeepSeekBai(context, testCase)
    default: throw new SmokeFailure(`no runner implemented for ${testCase.id}`)
  }
}

function validateEnvironment(selectedCases, environment) {
  check(environment.LIVE_PROVIDER_TESTS === '1', 'set LIVE_PROVIDER_TESTS=1 to authorize real Provider calls')
  check(environment.LIVE_TEST_DATABASE_URL, 'LIVE_TEST_DATABASE_URL is required')
  if (environment.DATABASE_URL) {
    check(
      environment.DATABASE_URL !== environment.LIVE_TEST_DATABASE_URL,
      'LIVE_TEST_DATABASE_URL must not equal runtime DATABASE_URL',
    )
  }
  const required = new Set(selectedCases.flatMap((testCase) => testCase.required_env))
  for (const name of required) check(environment[name], `${name} is required by selected live cases`)
  for (const name of [...required].filter((item) => item.endsWith('_BASE_URL'))) {
    let url
    try { url = new URL(environment[name]) } catch {}
    check(url && ['http:', 'https:'].includes(url.protocol), `${name} must be an HTTP(S) URL`)
    check(!url.username && !url.password && !url.search && !url.hash, `${name} must not contain credentials, query, or fragment`)
  }
}

function runPsql(databaseUrl, statement) {
  const result = spawnSync(
    'psql',
    [databaseUrl, '--no-psqlrc', '--set', 'ON_ERROR_STOP=1', '--command', statement],
    { cwd: repositoryRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  )
  if (result.status !== 0) {
    throw new SmokeFailure('PostgreSQL schema command failed', {
      exit_code: result.status,
      process_error: result.error?.code || null,
    })
  }
}

function schemaDatabaseUrl(databaseUrl, schema) {
  const url = new URL(databaseUrl)
  url.searchParams.set('options', `-csearch_path=${schema}`)
  return url.toString()
}

async function unusedPort() {
  const server = net.createServer()
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const port = server.address().port
  await new Promise((resolve) => server.close(resolve))
  return port
}

async function startFailureServer() {
  const server = http.createServer((request, response) => {
    request.resume()
    response.writeHead(503, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ error: { code: 'forced_primary_failure' } }))
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

function redact(text, secrets) {
  let redacted = String(text)
  for (const secret of secrets) {
    if (secret) redacted = redacted.replaceAll(secret, '<redacted>')
  }
  return redacted
}

async function startGateway({
  binary,
  port,
  databaseUrl,
  config,
  adminKey,
  masterKey,
  environment,
  allowLocalSource,
}) {
  const logs = []
  const child = spawn(binary, [], {
    cwd: repositoryRoot,
    env: {
      ...environment,
      DATABASE_URL: databaseUrl,
      GATEWAY_LISTEN_ADDR: `127.0.0.1:${port}`,
      GATEWAY_CONFIG_JSON: JSON.stringify(config),
      GATEWAY_CONFIG_IMPORT: 'false',
      GATEWAY_API_KEY: '',
      GATEWAY_ADMIN_KEY: adminKey,
      GATEWAY_CREDENTIAL_MASTER_KEY: masterKey,
      GATEWAY_SOURCE_URL_ALLOWLIST: mergeSourceUrlAllowlist(
        environment.GATEWAY_SOURCE_URL_ALLOWLIST,
        allowLocalSource,
      ),
      FALLBACK_PRIMARY_TEST_KEY: 'injected-primary-test-key',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  const capture = (chunk) => {
    logs.push(String(chunk))
    if (logs.join('').length > 64 * 1024) logs.shift()
  }
  child.stdout.on('data', capture)
  child.stderr.on('data', capture)
  const secrets = [
    environment.DEEPSEEK_API_KEY,
    environment.MINIMAX_API_KEY,
    environment.KIMI_API_KEY,
    environment.B_AI_API_KEY,
    adminKey,
    masterKey,
  ]
  const diagnostics = () => redact(logs.join('').slice(-4000), secrets)

  const baseUrl = `http://127.0.0.1:${port}`
  const deadline = Date.now() + 30_000
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new SmokeFailure('gateway exited during startup', {
        exit_code: child.exitCode,
        logs: diagnostics(),
      })
    }
    try {
      const response = await fetch(`${baseUrl}/healthz`, { signal: AbortSignal.timeout(1_000) })
      if (response.ok) return { child, baseUrl, diagnostics }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  child.kill('SIGTERM')
  throw new SmokeFailure('gateway did not become healthy')
}

async function provisionVirtualKey(baseUrl, adminKey, name) {
  const response = await fetch(`${baseUrl}/admin/keys`, {
    method: 'POST',
    headers: {
      authorization: `Bearer ${adminKey}`,
      'content-type': 'application/json',
    },
    body: JSON.stringify({ name, allowed_models: [] }),
    signal: AbortSignal.timeout(10_000),
  })
  let payload
  try { payload = await response.json() } catch {}
  check(response.ok && typeof payload?.key === 'string', 'failed to provision database Virtual Key', {
    http_status: response.status,
    error_code: payload?.error?.code || null,
  })
  return payload.key
}

async function stopGateway(child) {
  if (!child || child.exitCode !== null) return
  child.kill('SIGTERM')
  await Promise.race([
    new Promise((resolve) => child.once('exit', resolve)),
    new Promise((resolve) => setTimeout(resolve, 3_000)),
  ])
  if (child.exitCode === null) child.kill('SIGKILL')
}

function gitCommand(args) {
  return spawnSync('git', args, {
    cwd: repositoryRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })
}

function gitMetadata() {
  const revision = gitCommand(['rev-parse', 'HEAD'])
  const branch = gitCommand(['branch', '--show-current'])
  const status = gitCommand(['status', '--porcelain'])
  return {
    revision: revision.status === 0 ? revision.stdout.trim() : null,
    branch: branch.status === 0 ? branch.stdout.trim() : null,
    dirty: status.status === 0 ? status.stdout.trim().length > 0 : null,
  }
}

function safeFailure(error) {
  if (error instanceof SmokeFailure) {
    return { message: error.message, ...error.metadata }
  }
  return { message: error?.name || 'unexpected failure' }
}

function preflight(binary) {
  check(existsSync(binary), 'gateway binary is missing; run cargo build first')
  const psql = spawnSync('psql', ['--version'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  check(psql.status === 0, 'psql is required for live Provider smoke tests', {
    process_error: psql.error?.code || null,
  })
  return {
    gateway_binary: true,
    psql: true,
    case_isolation: true,
    data_plane_auth: 'database_virtual_key',
  }
}

async function runIsolatedCaseAttempt({
  testCase,
  binary,
  timeoutMs,
  attemptNo,
  keepSchema,
}) {
  const shortRunId = `${randomBytes(4).toString('hex')}-${attemptNo}`
  const schema = `live_provider_${shortRunId.replaceAll('-', '_')}`
  const adminKey = `admin_live_${randomBytes(24).toString('hex')}`
  const masterKey = `master_live_${randomBytes(32).toString('hex')}`
  let gateway
  let failureServer
  let schemaCreated = false
  try {
    if (testCase.id === 'fallback.deepseek_bai') failureServer = await startFailureServer()
    runPsql(process.env.LIVE_TEST_DATABASE_URL, `CREATE SCHEMA "${schema}"`)
    schemaCreated = true
    const port = await unusedPort()
    const config = buildGatewayConfig([testCase], process.env, port, failureServer?.url)
    gateway = await startGateway({
      binary,
      port,
      databaseUrl: schemaDatabaseUrl(process.env.LIVE_TEST_DATABASE_URL, schema),
      config,
      adminKey,
      masterKey,
      environment: process.env,
      allowLocalSource: Boolean(failureServer),
    })
    const gatewayKey = await provisionVirtualKey(
      gateway.baseUrl,
      adminKey,
      `live-${testCase.id}-${shortRunId}`,
    )
    return await runCase({
      gatewayBaseUrl: gateway.baseUrl,
      gatewayKey,
      adminKey,
      timeoutMs,
      shortRunId,
    }, testCase)
  } catch (error) {
    if (error instanceof SmokeFailure && gateway?.diagnostics) {
      error.metadata.gateway_logs = gateway.diagnostics()
    }
    throw error
  } finally {
    await stopGateway(gateway?.child)
    if (failureServer) await failureServer.close()
    if (schemaCreated && !keepSchema) {
      runPsql(process.env.LIVE_TEST_DATABASE_URL, `DROP SCHEMA "${schema}" CASCADE`)
    }
  }
}

async function runIsolatedCase({ testCase, binary, timeoutMs, retries, keepSchema }) {
  const startedAt = nowMilliseconds()
  const attempts = []
  for (let attemptNo = 0; attemptNo <= retries; attemptNo += 1) {
    try {
      const metadata = await runIsolatedCaseAttempt({
        testCase,
        binary,
        timeoutMs,
        attemptNo,
        keepSchema,
      })
      attempts.push({ attempt_no: attemptNo, outcome: metadata.outcome || 'passed' })
      return {
        outcome: metadata.outcome || 'passed',
        duration_ms: duration(startedAt),
        metadata,
        attempts,
      }
    } catch (error) {
      const failure = safeFailure(error)
      const outcome = classifyFailure(failure)
      attempts.push({ attempt_no: attemptNo, outcome, failure })
      if (outcome === 'failed' && attemptNo < retries && isTransientFailure(failure)) continue
      return { outcome, duration_ms: duration(startedAt), failure, attempts }
    }
  }
  throw new SmokeFailure('unreachable live case retry state')
}

async function main() {
  const options = parseArguments(process.argv.slice(2))
  const envPath = path.resolve(repositoryRoot, options.envFile)
  if (existsSync(envPath)) process.loadEnvFile(envPath)
  const cases = loadCaseManifest()
  if (options.list) {
    for (const testCase of cases) {
      console.log(`${testCase.id}\t${testCase.provider}\t${testCase.cost}\t${testCase.description}`)
    }
    return
  }

  const selected = selectCases(cases, options)
  validateEnvironment(selected, process.env)
  const binary = path.resolve(repositoryRoot, process.env.GATEWAY_BIN || 'target/debug/my-ai-gateway')
  const preflightResult = preflight(binary)

  const runId = stableRunId()
  const startedAt = new Date().toISOString()
  const timeoutMs = Number(process.env.LIVE_REQUEST_TIMEOUT_MS || 300_000)
  check(Number.isFinite(timeoutMs) && timeoutMs >= 1_000, 'LIVE_REQUEST_TIMEOUT_MS is invalid')
  const retries = Number(process.env.LIVE_TRANSIENT_RETRIES ?? 1)
  check(Number.isInteger(retries) && retries >= 0 && retries <= 3, 'LIVE_TRANSIENT_RETRIES must be an integer between 0 and 3')

  const results = []
  for (const testCase of selected) {
    const result = await runIsolatedCase({
      testCase,
      binary,
      timeoutMs,
      retries,
      keepSchema: options.keepSchema,
    })
    results.push({
      id: testCase.id,
      provider: testCase.provider,
      cost: testCase.cost,
      ...result,
    })
    const label = result.outcome.toUpperCase()
    const log = result.outcome === 'failed' ? console.error : console.log
    log(`${label} ${testCase.id} ${result.duration_ms}ms attempts=${result.attempts.length}`)
  }

  const knownIssueFailures = results.flatMap((result) =>
    (result.metadata?.known_issue_checks || [])
      .filter((item) => !item.passed)
      .map((item) => ({ case_id: result.id, ...item })),
  )
  const summary = {
    passed: results.filter((result) => result.outcome === 'passed').length,
    not_triggered: results.filter((result) => result.outcome === 'not_triggered').length,
    provider_unavailable: results.filter((result) => result.outcome === 'provider_unavailable').length,
    failed: results.filter((result) => result.outcome === 'failed').length,
    known_issue_failures: knownIssueFailures.length,
  }
  const artifact = {
    schema_version: 2,
    run_id: runId,
    git: gitMetadata(),
    started_at: startedAt,
    finished_at: new Date().toISOString(),
    selected_cases: selected.map((testCase) => testCase.id),
    result_policy: {
      live_opt_in: true,
      high_cost_included: selected.some((testCase) => testCase.cost === 'high'),
      response_bodies_stored: false,
      known_issues_are_strict: options.strictKnownIssues,
      transient_retries: retries,
    },
    preflight: preflightResult,
    summary,
    known_issue_failures: knownIssueFailures,
    results,
  }
  const outputDirectory = path.resolve(repositoryRoot, options.outputDirectory)
  mkdirSync(outputDirectory, { recursive: true, mode: 0o700 })
  const outputPath = path.join(outputDirectory, `${runId}.json`)
  writeFileSync(outputPath, `${JSON.stringify(artifact, null, 2)}\n`, { mode: 0o600 })
  console.log(`RESULT ${path.relative(repositoryRoot, outputPath)}`)
  console.log(`SUMMARY passed=${summary.passed} not_triggered=${summary.not_triggered} provider_unavailable=${summary.provider_unavailable} failed=${summary.failed} known_issue_failures=${summary.known_issue_failures}`)

  if (summary.failed > 0 || (options.strictKnownIssues && summary.known_issue_failures > 0)) {
    process.exitCode = 1
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : ''
if (import.meta.url === invokedPath) {
  main().catch((error) => {
    console.error(`live provider smoke aborted: ${safeFailure(error).message}`)
    process.exitCode = 1
  })
}
