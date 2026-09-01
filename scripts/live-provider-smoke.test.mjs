import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildGatewayConfig,
  classifyFailure,
  isTransientFailure,
  loadCaseManifest,
  mergeSourceUrlAllowlist,
  parseArguments,
  parseSse,
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
