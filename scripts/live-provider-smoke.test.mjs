import assert from 'node:assert/strict'
import http from 'node:http'
import test from 'node:test'

import {
  buildGatewayConfig,
  classifyFailure,
  isTransientFailure,
  loadCaseManifest,
  mergeSourceUrlAllowlist,
  parseArguments,
  parseSse,
  runCase,
  selectCases,
  strictlyIncreasing,
} from './live-provider-smoke.mjs'

const cases = loadCaseManifest()

test('default selection excludes high-cost live searches', () => {
  const selected = selectCases(cases, parseArguments([]))
  assert.ok(selected.length > 0)
  assert.ok(selected.every((testCase) => testCase.cost === 'low'))
  assert.ok(selected.some((testCase) => testCase.id === 'kimi.function'))
  assert.ok(!selected.some((testCase) => testCase.id === 'kimi.web_search'))
})

test('explicit case selection is stable and includes high-cost cases', () => {
  const selected = selectCases(cases, parseArguments([
    '--case',
    'deepseek.web_search',
    '--case=kimi.web_search_stream',
  ]))
  assert.deepEqual(selected.map((testCase) => testCase.id), [
    'deepseek.web_search',
    'kimi.web_search_stream',
  ])
})

test('gateway config references credential env names without embedding keys', () => {
  const selected = selectCases(cases, parseArguments([
    '--case=deepseek.function',
    '--case=kimi.function',
    '--case=fallback.deepseek_bai',
  ]))
  const environment = {
    DEEPSEEK_BASE_URL: 'https://deepseek.example',
    KIMI_BASE_URL: 'https://kimi.example/coding',
    B_AI_BASE_URL: 'https://bai.example/v1',
    DEEPSEEK_API_KEY: 'deepseek-secret-sentinel',
    KIMI_API_KEY: 'kimi-secret-sentinel',
    B_AI_API_KEY: 'bai-secret-sentinel',
  }
  const config = buildGatewayConfig(selected, environment, 9876, 'http://127.0.0.1:12345')
  const serialized = JSON.stringify(config)
  assert.doesNotMatch(serialized, /secret-sentinel/)
  assert.match(serialized, /DEEPSEEK_API_KEY/)
  assert.match(serialized, /KIMI_API_KEY/)
  assert.match(serialized, /B_AI_API_KEY/)
  assert.match(serialized, /FALLBACK_PRIMARY_TEST_KEY/)
  assert.equal(config.routes.find((route) => route.id === 'deepseek-bai-fallback-live').fallback_accounts[0], 'bai-live')
})

test('SSE parser preserves event order and monotonic sequence checks', () => {
  const events = parseSse([
    'event: response.created',
    'data: {"type":"response.created","sequence_number":0}',
    '',
    'event: response.completed',
    'data: {"type":"response.completed","sequence_number":1}',
    '',
  ].join('\n'))
  assert.deepEqual(events.map((event) => event.type), [
    'response.created',
    'response.completed',
  ])
  assert.equal(strictlyIncreasing(events.map((event) => event.sequence_number)), true)
  assert.equal(strictlyIncreasing([0, 0]), false)
})

test('source URL allowlist preserves configured Provider hosts and appends localhost once', () => {
  assert.equal(
    mergeSourceUrlAllowlist('api.deepseek.com,api.minimaxi.com,127.0.0.1', true),
    'api.deepseek.com,api.minimaxi.com,127.0.0.1',
  )
  assert.equal(mergeSourceUrlAllowlist('api.kimi.com', false), 'api.kimi.com')
})

test('live failures distinguish Provider availability from transient transport failures', () => {
  assert.equal(classifyFailure({ http_status: 403, error_code: 'permission_error' }), 'provider_unavailable')
  assert.equal(classifyFailure({ http_status: 429, error_code: 'rate_limit_exceeded' }), 'provider_unavailable')
  assert.equal(classifyFailure({ http_status: 502, error_code: 'upstream_request_failed' }), 'failed')
  assert.equal(isTransientFailure({ http_status: 504, error_code: 'upstream_request_failed' }), true)
  assert.equal(isTransientFailure({ http_status: 403, error_code: 'permission_error' }), false)
})

const reportedUsage = {
  input_tokens: 10,
  output_tokens: 4,
  total_tokens: 14,
  input_tokens_details: { cached_tokens: 3 },
  output_tokens_details: { reasoning_tokens: 2 },
}

function persistedUsage(streamed = false) {
  return {
    request_id: 'fixture-request',
    success: true,
    status_code: 200,
    streamed,
    usage_source: streamed ? 'parsed' : 'upstream',
    input_tokens: 10,
    output_tokens: 4,
    total_tokens: 14,
    cached_tokens: 3,
    cache_read_tokens: 3,
    cache_creation_tokens: 0,
    reasoning_tokens: 2,
  }
}

function functionResponse() {
  return {
    status: 'completed',
    output: [{
      id: 'function-item',
      type: 'function_call',
      status: 'completed',
      call_id: 'call-weather',
      name: 'lookup_weather',
      arguments: '{"city":"Tokyo"}',
    }],
    usage: structuredClone(reportedUsage),
  }
}

function messageResponse() {
  return {
    status: 'completed',
    output: [{ type: 'message', content: [{ type: 'output_text', text: 'response-body-sentinel' }] }],
    usage: structuredClone(reportedUsage),
  }
}

function functionStream() {
  const response = functionResponse()
  return [
    { type: 'response.created', response: { status: 'in_progress' } },
    { type: 'response.output_item.added', output_index: 0, item: { ...response.output[0], arguments: '', status: 'in_progress' } },
    { type: 'response.function_call_arguments.delta', item_id: 'function-item', output_index: 0, delta: '{"city":' },
    { type: 'response.function_call_arguments.delta', item_id: 'function-item', output_index: 0, delta: '"Tokyo"}' },
    { type: 'response.function_call_arguments.done', item_id: 'function-item', output_index: 0, arguments: '{"city":"Tokyo"}' },
    { type: 'response.output_item.done', output_index: 0, item: response.output[0] },
    { type: 'response.completed', response },
  ].map((event, sequence_number) => ({ ...event, sequence_number }))
}

// Exercise the production HTTP/SSE parsing, request building and Admin Usage
// comparison together, without a Provider, subprocess or database.
async function fakeGateway(t, {
  payloads = [functionResponse(), messageResponse()],
  events = functionStream(),
  usageEvents = [persistedUsage(), persistedUsage()],
} = {}) {
  const requests = []
  const requestIds = new Map()
  const server = http.createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1')
    let payload
    if (request.method === 'POST' && url.pathname === '/v1/responses') {
      const chunks = []
      for await (const chunk of request) chunks.push(chunk)
      const body = JSON.parse(Buffer.concat(chunks).toString())
      const index = requests.length
      requests.push({ body, headers: request.headers })
      requestIds.set(request.headers['x-client-source'], index)
      if (body.stream) {
        response.writeHead(200, { 'content-type': 'text/event-stream' })
        response.end(events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(''))
        return
      }
      payload = payloads[index]
    } else if (url.pathname === '/admin/usage/events') {
      const index = requestIds.get(url.searchParams.get('client_source'))
      payload = { data: [{ request_id: `request-${index}` }] }
    } else {
      const index = Number(url.pathname.split('request-')[1])
      payload = { data: usageEvents[index], attempts: [] }
    }
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(payload))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => {
    server.closeAllConnections()
    server.close(resolve)
  }))
  const context = {
    gatewayBaseUrl: `http://127.0.0.1:${server.address().port}`,
    gatewayKey: 'gateway-key-sentinel',
    adminKey: 'admin-key-sentinel',
    timeoutMs: 2_000,
    shortRunId: 'unit',
  }
  return { requests, run: (id) => runCase(context, cases.find((testCase) => testCase.id === id)) }
}

function assertMetadataOnly(metadata) {
  const serialized = JSON.stringify(metadata)
  assert.doesNotMatch(serialized, /response-body-sentinel|gateway-key-sentinel|admin-key-sentinel/)
  assert.doesNotMatch(serialized, /Tokyo|temperature_c|Call lookup_weather/)
}

test('Kimi native function round-trip sends supported parameters and preserves the conversation', async (t) => {
  const gateway = await fakeGateway(t)
  const result = await gateway.run('kimi.function')
  assert.equal(gateway.requests.length, 2)
  for (const { body, headers } of gateway.requests) {
    assert.deepEqual(body.reasoning, { effort: 'low' })
    assert.equal(body.tool_choice, 'auto')
    assert.equal(body.max_output_tokens, 512)
    assert.equal(body.parallel_tool_calls, false)
    assert.equal(body.stream, false)
    assert.notEqual(headers['accept-encoding'], 'identity')
  }
  const conversation = gateway.requests[1].body.input
  assert.deepEqual(conversation[1], functionResponse().output[0])
  assert.equal(conversation[2].type, 'function_call_output')
  assert.equal(conversation[2].call_id, 'call-weather')
  assert.deepEqual(JSON.parse(conversation[2].output), { temperature_c: 22, condition: 'clear' })
  assert.equal(result.tool_name, 'lookup_weather')
  assert.equal(result.arguments_valid, true)
  assert.equal(result.usage_events.length, 2)
  assertMetadataOnly(result)
})

test('Kimi function round-trip rejects missing/wrong tools, malformed arguments and incomplete responses', async (t) => {
  const failures = [
    ['missing function', (payloads) => { payloads[0].output = messageResponse().output }, /single function/],
    ['wrong function', (payloads) => { payloads[0].output[0].name = 'other_tool' }, /wrong tool/],
    ['incomplete response', (payloads) => { payloads[0].status = 'incomplete' }, /did not complete/],
    ['incomplete function', (payloads) => { payloads[0].output[0].status = 'incomplete' }, /did not complete/],
    ['missing call id', (payloads) => { delete payloads[0].output[0].call_id }, /call_id/],
    ...['not-json', 'null', '[]', '{}', '{"city":42}'].map((args) => [
      `invalid arguments ${args}`,
      (payloads) => { payloads[0].output[0].arguments = args },
      /expected JSON schema/,
    ]),
    ['repeated tool', (payloads) => { payloads[1].output.push(functionResponse().output[0]) }, /repeated/],
    ['no final message', (payloads) => { payloads[1].output = [] }, /final message/],
  ]
  for (const [name, mutate, expectedError] of failures) {
    await t.test(name, async (t) => {
      const payloads = [functionResponse(), messageResponse()]
      mutate(payloads)
      const gateway = await fakeGateway(t, { payloads })
      await assert.rejects(gateway.run('kimi.function'), expectedError)
    })
  }
})

test('Kimi reported usage rejects estimated/missing sources and every persisted Token mismatch in either phase', async (t) => {
  for (const phase of [0, 1]) {
    const mismatches = [
      ['estimated', (event) => { event.usage_source = 'estimated' }],
      ['missing', (event) => { event.usage_source = 'missing' }],
      ...['input_tokens', 'output_tokens', 'total_tokens', 'cached_tokens', 'cache_read_tokens', 'cache_creation_tokens', 'reasoning_tokens'].map((field) => [field, (event) => { event[field] += 1 }]),
    ]
    for (const [name, mutate] of mismatches) {
      await t.test(`phase ${phase + 1}: ${name}`, async (t) => {
        const usageEvents = [persistedUsage(), persistedUsage()]
        mutate(usageEvents[phase])
        const gateway = await fakeGateway(t, { usageEvents })
        await assert.rejects(gateway.run('kimi.function'), (error) => {
          assert.match(error.message, /reported usage does not match/)
          assert.equal(error.metadata.phase, phase === 0 ? 'first' : 'second')
          assertMetadataOnly(error.metadata)
          return true
        })
      })
    }
  }
})

test('Kimi usage follows the existing total/cache/reasoning contract without adding reasoning twice', async (t) => {
  const payloads = [functionResponse(), messageResponse()]
  delete payloads[0].usage.total_tokens
  payloads[1].usage = {
    input_tokens: 0,
    prompt_tokens: 99,
    output_tokens: 4,
    cache_read_input_tokens: 2,
    cache_creation_input_tokens: 3,
    reasoning_tokens: 2,
  }
  const usageEvents = [persistedUsage(), {
    ...persistedUsage(), input_tokens: 0, total_tokens: 4, cached_tokens: 5,
    cache_read_tokens: 2, cache_creation_tokens: 3,
  }]
  const gateway = await fakeGateway(t, { payloads, usageEvents })
  const result = await gateway.run('kimi.function')
  assert.equal(result.response_usage[0].total_tokens, 14)
  assert.equal(result.response_usage[1].total_tokens, 4)
  assert.equal(result.response_usage[1].cached_tokens, 5)
  assert.equal(result.response_usage[1].reasoning_tokens, 2)
})

test('Kimi omitted upstream usage allows an estimate but does not silently pass missing persisted usage', async (t) => {
  for (const source of ['estimated', 'missing']) {
    await t.test(source, async (t) => {
      const payloads = [functionResponse(), messageResponse()]
      delete payloads[1].usage
      const usageEvents = [persistedUsage(), { ...persistedUsage(), usage_source: source }]
      const gateway = await fakeGateway(t, { payloads, usageEvents })
      if (source === 'estimated') {
        const result = await gateway.run('kimi.function')
        assert.equal(result.response_usage[1], null)
        assert.equal(result.usage_events[1].usage_source, 'estimated')
      } else {
        await assert.rejects(gateway.run('kimi.function'), /no persisted usage/)
      }
    })
  }
})

test('Kimi streaming function case validates supported request, argument reconstruction and parsed Usage', async (t) => {
  const gateway = await fakeGateway(t, { usageEvents: [persistedUsage(true)] })
  const result = await gateway.run('kimi.function_stream')
  const { body, headers } = gateway.requests[0]
  assert.equal(body.stream, true)
  assert.deepEqual(body.reasoning, { effort: 'low' })
  assert.equal(body.tool_choice, 'auto')
  assert.equal(body.max_output_tokens, 512)
  assert.notEqual(headers['accept-encoding'], 'identity')
  assert.equal(result.tool_name, 'lookup_weather')
  assert.equal(result.argument_delta_events, 2)
  assert.equal(result.arguments_done_events, 1)
  assert.equal(result.sequence_monotonic, true)
  assert.equal(result.usage_event.usage_source, 'parsed')
  assertMetadataOnly(result)
})

test('Kimi streaming function case rejects tool, argument, sequence and terminal event regressions', async (t) => {
  const failures = [
    ['wrong tool', (events) => { events.at(-1).response.output[0].name = 'other_tool' }, /wrong tool/],
    ['invalid final arguments', (events) => { events.at(-1).response.output[0].arguments = 'null' }, /expected JSON schema/],
    ['missing delta', (events) => events.filter((event) => !event.type.endsWith('arguments.delta')), /no function argument deltas/],
    ['missing arguments done', (events) => events.filter((event) => !event.type.endsWith('arguments.done')), /arguments done/],
    ['incorrect delta', (events) => { events[3].delta = '"Osaka"}' }, /arguments disagree/],
    ['incorrect done arguments', (events) => { events[4].arguments = '{"city":"Osaka"}' }, /arguments disagree/],
    ['wrong item id', (events) => { events[2].item_id = 'other-item' }, /wrong output item/],
    ['wrong output index', (events) => { events[2].output_index = 1 }, /wrong output item/],
    ['done before delta', (events) => { [events[3], events[4]] = [events[4], events[3]]; return events.map((event, sequence_number) => ({ ...event, sequence_number })) }, /arguments disagree/],
    ['duplicate sequence', (events) => { events[2].sequence_number = 1 }, /sequence/],
    ['missing sequence', (events) => { delete events[2].sequence_number }, /sequence/],
    ['missing created', (events) => events.slice(1), /complete response lifecycle/],
    ['missing completed', (events) => events.slice(0, -1), /complete response lifecycle/],
    ['duplicate completed', (events) => [...events, { ...events.at(-1), sequence_number: 7 }], /complete response lifecycle/],
    ['terminal error', (events) => [...events, { type: 'error', sequence_number: 7 }], /complete response lifecycle/],
    ['incomplete response', (events) => { events.at(-1).response.status = 'incomplete' }, /did not complete/],
  ]
  for (const [name, mutate, expectedError] of failures) {
    await t.test(name, async (t) => {
      const events = functionStream()
      const gateway = await fakeGateway(t, { events: mutate(events) || events, usageEvents: [persistedUsage(true)] })
      await assert.rejects(gateway.run('kimi.function_stream'), expectedError)
    })
  }
})

test('Kimi streaming usage requires parsed source and matching Token counts', async (t) => {
  for (const [name, mutation] of [
    ['estimated', { usage_source: 'estimated' }],
    ['missing', { usage_source: 'missing' }],
    ['wrong total', { total_tokens: 15 }],
    ['wrong reasoning', { reasoning_tokens: 3 }],
    ['wrong cache', { cached_tokens: 4 }],
  ]) {
    await t.test(name, async (t) => {
      const gateway = await fakeGateway(t, { usageEvents: [{ ...persistedUsage(true), ...mutation }] })
      await assert.rejects(gateway.run('kimi.function_stream'), /reported usage does not match/)
    })
  }
})

test('Kimi search cases reuse supported low/auto parameters without changing high-cost selection', async (t) => {
  for (const streamed of [false, true]) {
    await t.test(streamed ? 'SSE' : 'JSON', async (t) => {
      const payload = {
        status: 'completed',
        output: [{ type: 'web_search_call', status: 'completed', action: { type: 'search' } }],
        usage: structuredClone(reportedUsage),
      }
      const events = [
        { type: 'response.created' },
        { type: 'response.web_search_call.in_progress' },
        { type: 'response.web_search_call.searching' },
        { type: 'response.web_search_call.completed' },
        { type: 'response.completed', response: payload },
      ].map((event, sequence_number) => ({ ...event, sequence_number }))
      const gateway = await fakeGateway(t, { payloads: [payload], events, usageEvents: [persistedUsage(streamed)] })
      const id = streamed ? 'kimi.web_search_stream' : 'kimi.web_search'
      const result = await gateway.run(id)
      assert.deepEqual(gateway.requests[0].body.reasoning, { effort: 'low' })
      assert.equal(gateway.requests[0].body.tool_choice, 'auto')
      assert.equal(gateway.requests[0].body.max_output_tokens, 1024)
      assert.equal(cases.find((testCase) => testCase.id === id).cost, 'high')
      assertMetadataOnly(result)
    })
  }
})

test('Kimi reasoning retains high effort and not_triggered when no signature is emitted', async (t) => {
  const gateway = await fakeGateway(t, { payloads: [messageResponse()] })
  const result = await gateway.run('kimi.reasoning_signature')
  assert.equal(result.outcome, 'not_triggered')
  assert.deepEqual(gateway.requests[0].body.reasoning, { effort: 'high' })
  assert.equal(gateway.requests[0].body.tool_choice, undefined)
  assertMetadataOnly(result)
})
