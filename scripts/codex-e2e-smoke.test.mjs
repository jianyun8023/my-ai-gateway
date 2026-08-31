import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildCodexArgs,
  buildCodexConfig,
  evaluateSearchResult,
  evaluateToolResult,
  gatewayAdminBaseUrl,
  loadCaseManifest,
  parseArguments,
  parseCodexJsonLines,
  sanitizeCodexEnvironment,
  selectCases,
  summarizeCodexEvents,
} from './codex-e2e-smoke.mjs'

const cases = loadCaseManifest()

test('default selection keeps high-cost search opt-in', () => {
  const selected = selectCases(cases, parseArguments([]))
  assert.deepEqual(selected.map((item) => item.id), ['codex.tool'])
  assert.deepEqual(selectCases(cases, parseArguments(['--include-search'])).map((item) => item.id), [
    'codex.tool',
    'codex.web_search',
  ])
})

test('Codex config uses an environment key and never embeds the secret', () => {
  const config = buildCodexConfig({
    model: 'k3',
    baseUrl: 'http://127.0.0.1:8788/v1',
    clientSource: 'codex-e2e-run-tool',
  })
  assert.match(config, /env_key = "CODEX_GATEWAY_API_KEY"/)
  assert.match(config, /X-Client-Source/)
  assert.doesNotMatch(config, /gateway-secret/)
})

test('Codex child environment removes provider and admin secrets', () => {
  const sanitized = sanitizeCodexEnvironment({
    PATH: '/usr/bin',
    KIMI_API_KEY: 'kimi-secret',
    KIMI_API_KEY_2: 'kimi-secret-2',
    AWS_ACCESS_KEY_ID: 'aws-secret',
    PRIVATE_KEY: 'private-secret',
    GATEWAY_ADMIN_KEY: 'admin-secret',
    DATABASE_URL: 'postgres://secret',
    CODEX_GATEWAY_API_KEY: 'old-value',
  }, {
    home: '/tmp/codex-home',
    stateDirectory: '/tmp/codex-home/state',
    apiKey: 'gateway-data-key',
  })
  assert.equal(sanitized.PATH, '/usr/bin')
  assert.equal(sanitized.KIMI_API_KEY, undefined)
  assert.equal(sanitized.KIMI_API_KEY_2, undefined)
  assert.equal(sanitized.AWS_ACCESS_KEY_ID, undefined)
  assert.equal(sanitized.PRIVATE_KEY, undefined)
  assert.equal(sanitized.GATEWAY_ADMIN_KEY, undefined)
  assert.equal(sanitized.DATABASE_URL, undefined)
  assert.equal(sanitized.CODEX_GATEWAY_API_KEY, 'gateway-data-key')
})

test('Admin Usage queries strip only the Responses /v1 suffix', () => {
  assert.equal(gatewayAdminBaseUrl('http://127.0.0.1:8788/v1'), 'http://127.0.0.1:8788')
  assert.equal(gatewayAdminBaseUrl('https://gateway.example.test/api/v1/'), 'https://gateway.example.test/api')
  assert.equal(gatewayAdminBaseUrl('http://127.0.0.1:8788'), 'http://127.0.0.1:8788')
})

test('search flag is placed before the exec subcommand', () => {
  const args = buildCodexArgs({
    model: 'k3',
    workspace: '/tmp/codex-workspace',
    outputPath: '/tmp/codex-final.txt',
    prompt: 'search',
    search: true,
    skipGitRepoCheck: true,
  })
  assert.equal(args[0], '--search')
  assert.equal(args[1], '--skip-git-repo-check')
  assert.equal(args[2], 'exec')
  assert.ok(args.includes('--strict-config'))
  assert.ok(args.includes('--skip-git-repo-check'))
})

test('tool events and final canary are summarized without retaining body text', () => {
  const { events, invalidLines } = parseCodexJsonLines([
    '{"type":"item.completed","item":{"type":"command_execution","exit_code":0,"aggregated_output":"codex-gateway-canary\\n"}}',
    '{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":4,"reasoning_output_tokens":2}}',
  ].join('\n'))
  const summary = summarizeCodexEvents(events, 'codex-gateway-canary')
  assert.equal(invalidLines, 0)
  assert.equal(summary.canary_seen, true)
  assert.deepEqual(summary.usage, {
    input_tokens: 10,
    output_tokens: 4,
    reasoning_output_tokens: 2,
    cached_input_tokens: 0,
    total_tokens: 14,
  })
  assert.equal(evaluateToolResult('CODEX_GATEWAY_E2E_OK:codex-gateway-canary', summary, 'codex-gateway-canary').passed, true)
})

test('search requires a completed event and an official Rust Blog source', () => {
  const { events } = parseCodexJsonLines([
    '{"type":"item.completed","item":{"type":"web_search","query":"latest Rust release","action":{"type":"search"}}}',
  ].join('\n'))
  const summary = summarizeCodexEvents(events)
  assert.equal(evaluateSearchResult('SEARCH_E2E_OK:1.98.0:https://blog.rust-lang.org/releases/latest/', summary).passed, true)
  assert.equal(evaluateSearchResult('SEARCH_E2E_OK:1.98.0:https://example.com/', summary).passed, false)
})
