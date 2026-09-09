import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createServer } from 'node:http'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  buildCodexArgs,
  buildCodexConfig,
  classifyCodexFailure,
  diagnosticStage,
  evaluateMultiTurnToolResult,
  evaluateSearchResult,
  evaluateToolResult,
  gatewayAdminBaseUrl,
  loadCaseManifest,
  multiTurnToolTurn1Prompt,
  multiTurnToolTurn2Prompt,
  parseArguments,
  parseCodexJsonLines,
  preflightGatewayModel,
  preflightGatewayRoute,
  resolveCodexDataKey,
  sanitizeCodexEnvironment,
  secureFile,
  selectCases,
  summarizeCodexEvents,
  toolPrompt,
} from './codex-e2e-smoke.mjs'

const cases = loadCaseManifest()

test('default selection keeps high-cost search opt-in', () => {
  const selected = selectCases(cases, parseArguments([]))
  // The default profile is `cost=low`, which now includes both
  // codex.tool and codex.multi_turn_tool; the web_search case stays
  // opt-in behind `--include-search`.
  assert.deepEqual(selected.map((item) => item.id), ['codex.tool', 'codex.multi_turn_tool'])
  assert.deepEqual(selectCases(cases, parseArguments(['--include-search'])).map((item) => item.id), [
    'codex.tool',
    'codex.multi_turn_tool',
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

test('Gateway model preflight requires the selected model to be advertised', async () => {
  const fetchImpl = async () => ({
    ok: true,
    status: 200,
    json: async () => ({ data: [{ id: 'deepseek-chat' }, { id: 'MiniMax-M2.7' }] }),
  })
  assert.deepEqual(
    await preflightGatewayModel('http://127.0.0.1:8787/v1', 'gateway-key', 'deepseek-chat', fetchImpl),
    { model_present: true, advertised_model_count: 2 },
  )
  await assert.rejects(
    preflightGatewayModel('http://127.0.0.1:8787/v1', 'gateway-key', 'k3', fetchImpl),
    /Codex model is not available from Gateway: k3/,
  )
})

test('Gateway route preflight verifies an OpenAI Responses route when Admin Key is available', async () => {
  const calls = []
  const fetchImpl = async (url) => {
    calls.push(url)
    return { ok: true, status: 200 }
  }
  assert.deepEqual(
    await preflightGatewayRoute('http://127.0.0.1:8787', 'admin-key', 'deepseek/chat', fetchImpl),
    { responses_route_checked: true },
  )
  assert.match(calls[0], /openai_responses\/deepseek%2Fchat$/)
  assert.deepEqual(
    await preflightGatewayRoute('http://127.0.0.1:8787', '', 'deepseek-chat', fetchImpl),
    { responses_route_checked: false },
  )
})

test('Codex resolves an active recoverable database Virtual Key without persisting it', async () => {
  const calls = []
  const fetchImpl = async (url) => {
    calls.push(url)
    if (url.endsWith('/admin/keys')) return {
      ok: true,
      status: 200,
      json: async () => ({ data: [
        { id: 9, name: 'legacy', enabled: true, key_recoverable: false },
        { id: 7, name: 'codex-e2e', enabled: true, key_recoverable: true },
      ] }),
    }
    return {
      ok: true,
      status: 200,
      json: async () => ({ data: { id: 7, key: 'gw_database_key' } }),
    }
  }
  assert.deepEqual(await resolveCodexDataKey({
    adminBaseUrl: 'http://127.0.0.1:8787',
    adminKey: 'admin-key',
    virtualKeyName: 'codex-e2e',
    fetchImpl,
  }), {
    key: 'gw_database_key',
    source: 'database_virtual_key',
    virtual_key_id: 7,
  })
  assert.deepEqual(calls, [
    'http://127.0.0.1:8787/admin/keys',
    'http://127.0.0.1:8787/admin/keys/7/value',
  ])
})

test('Codex output files are reduced to owner-only permissions', () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'codex-e2e-permissions-'))
  const filePath = path.join(directory, 'final.txt')
  try {
    writeFileSync(filePath, 'test', { mode: 0o664 })
    assert.equal(secureFile(filePath), true)
    assert.equal(statSync(filePath).mode & 0o777, 0o600)
    assert.equal(secureFile(path.join(directory, 'missing.txt')), false)
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})

test('search is global and skip-git-repo-check belongs to exec', () => {
  const args = buildCodexArgs({
    model: 'k3',
    workspace: '/tmp/codex-workspace',
    outputPath: '/tmp/codex-final.txt',
    prompt: 'search',
    search: true,
    skipGitRepoCheck: true,
  })
  assert.equal(args[0], '--search')
  assert.equal(args[1], 'exec')
  assert.equal(args[2], '--skip-git-repo-check')
  assert.equal(args[3], '--strict-config')
  assert.ok(args.includes('--strict-config'))
  assert.ok(args.includes('--skip-git-repo-check'))
  assert.ok(args.includes('--search'))
})

test('default flags omit search and skip-git-repo-check when not requested', () => {
  const args = buildCodexArgs({
    model: 'k3',
    workspace: '/tmp/codex-workspace',
    outputPath: '/tmp/codex-final.txt',
    prompt: 'tool',
    search: false,
    skipGitRepoCheck: false,
  })
  assert.equal(args[0], 'exec')
  assert.ok(!args.includes('--search'))
  assert.ok(!args.includes('--skip-git-repo-check'))
  assert.ok(args.includes('--strict-config'))
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

test('tool prompt cannot reveal the random canary and missing commands get a precise stage', () => {
  const prompt = toolPrompt()
  assert.doesNotMatch(prompt, /codex-gateway-[a-f0-9]+/)
  assert.match(prompt, /MUST invoke the shell tool/)
  assert.equal(diagnosticStage(
    'tool',
    { code: 0, timedOut: false, error: null },
    0,
    {
      command_seen: false,
      command_succeeded: false,
      canary_seen: false,
      final_exact: false,
    },
    { passed: true },
  ), 'command_event_missing')
})

test('Codex failures separate Provider availability and model tool non-triggering', () => {
  assert.equal(classifyCodexFailure({ usage: { status_codes: [403] } }), 'provider_unavailable')
  assert.equal(classifyCodexFailure({ usage: { status_codes: [200] } }), 'failed')
  assert.equal(classifyCodexFailure({
    diagnostic_stage: 'command_event_missing',
    usage: { passed: true, status_codes: [200] },
  }), 'not_triggered')
  assert.equal(classifyCodexFailure({
    diagnostic_stage: 'command_failed',
    usage: { passed: true, status_codes: [200] },
  }), 'failed')
})

test('search requires a completed event and an official Rust Blog source', () => {
  const { events } = parseCodexJsonLines([
    '{"type":"item.completed","item":{"type":"web_search","query":"latest Rust release","action":{"type":"search"}}}',
  ].join('\n'))
  const summary = summarizeCodexEvents(events)
  assert.equal(evaluateSearchResult('SEARCH_E2E_OK:1.98.0:https://blog.rust-lang.org/releases/latest/', summary).passed, true)
  assert.equal(evaluateSearchResult('SEARCH_E2E_OK:1.98.0:https://example.com/', summary).passed, false)
})

test('multi-turn tool prompts do not embed the runtime canary', () => {
  const turn1 = multiTurnToolTurn1Prompt()
  const turn2 = multiTurnToolTurn2Prompt('placeholder-final-message')
  assert.ok(turn1.includes('CANARY.txt'), 'turn 1 prompt must reference CANARY.txt')
  assert.ok(turn2.includes('CANARY.txt'), 'turn 2 prompt must reference CANARY.txt')
  // The canary shape is `codex-gateway-<8 bytes hex>`; assert neither
  // prompt source contains the prefix.
  assert.ok(!turn1.includes('codex-gateway-'), 'turn 1 prompt must not embed the canary prefix')
  assert.ok(!turn2.includes('codex-gateway-'), 'turn 2 prompt must not embed the canary prefix')
  // Turn 2 wraps the previous final message verbatim — the prompt must
  // not sanitise or redact it; tests rely on the previous final being
  // echoed through so the model can quote it back.
  assert.ok(
    turn2.includes('"""placeholder-final-message"""'),
    'turn 2 prompt must embed the turn-1 final message so context propagation is testable',
  )
})

test('multi-turn tool recall evaluator only accepts the exact canary answer', () => {
  const canary = 'codex-gateway-0123456789abcdef'
  const passing = evaluateMultiTurnToolResult(`CODEX_GATEWAY_E2E_OK:${canary}\n`, canary)
  assert.equal(passing.passed, true)
  assert.equal(passing.reason, 'multi-turn recall matched the canary')

  const trimmed = evaluateMultiTurnToolResult(`CODEX_GATEWAY_E2E_OK:${canary}`, canary)
  assert.equal(trimmed.passed, true)

  const wrong = evaluateMultiTurnToolResult(`CODEX_GATEWAY_E2E_OK:codex-gateway-deadbeef`, canary)
  assert.equal(wrong.passed, false)
  assert.match(wrong.reason, /did not match the canary/)

  const polluted = evaluateMultiTurnToolResult(`CODEX_GATEWAY_E2E_OK:${canary} trailing prose`, canary)
  assert.equal(polluted.passed, false)
})

test('multi-turn tool case is in the manifest and is selected by default', () => {
  const cases = loadCaseManifest()
  const multiTurn = cases.find((c) => c.id === 'codex.multi_turn_tool')
  assert.ok(multiTurn, 'manifest must register codex.multi_turn_tool')
  assert.equal(multiTurn.kind, 'multi_turn_tool')
  assert.equal(multiTurn.cost, 'low')

  const selected = selectCases(cases, { cases: [], includeSearch: false })
  assert.ok(
    selected.some((c) => c.id === 'codex.multi_turn_tool'),
    'multi-turn tool case must be selected under the default low-cost profile',
  )
})

test('multi-turn tool case can be addressed explicitly via --case', () => {
  const cases = loadCaseManifest()
  const selected = selectCases(cases, { cases: ['codex.multi_turn_tool'], includeSearch: false })
  assert.deepEqual(selected.map((c) => c.id), ['codex.multi_turn_tool'])
})

test('multi-turn tool prompt reuse is invariant across calls', () => {
  // Multi-turn case must keep the same prompt shape on every invocation;
  // a flake here would invalidate Codex session continuity contracts.
  assert.equal(multiTurnToolTurn1Prompt(), multiTurnToolTurn1Prompt())
  assert.equal(
    multiTurnToolTurn2Prompt('alpha'),
    multiTurnToolTurn2Prompt('alpha'),
  )
  // The turn-2 prompt must reflect the prior final verbatim, including
  // surrounding whitespace, so callers can rely on identical contract.
  assert.equal(
    multiTurnToolTurn2Prompt('  spaced  '),
    multiTurnToolTurn2Prompt('  spaced  '),
  )
})

for (const scenario of ['passed', 'recall_mismatch', 'missing_command', 'missing_usage']) {
  test(`multi-turn ${scenario} serializes only metadata through the CLI artifact path`, async () => {
    const directory = mkdtempSync(path.join(os.tmpdir(), 'codex-e2e-artifact-'))
    const sentinel = 'PRIVATE_MODEL_BODY_SENTINEL'.repeat(256)
    const fakeCli = path.join(directory, 'fake-codex.mjs')
    const output = path.join(directory, 'results')
    const workspace = path.join(directory, 'workspace')
    let usageQueries = 0
    const server = createServer((request, response) => {
      response.setHeader('content-type', 'application/json')
      if (request.url === '/v1/models') response.end(JSON.stringify({ data: [{ id: 'test-model' }] }))
      else if (request.url.startsWith('/admin/routes/')) response.end('{}')
      else if (request.url.startsWith('/admin/usage/events?')) {
        usageQueries += 1
        response.end(JSON.stringify({ data: [{
          status_code: 200, success: true, total_tokens: scenario === 'missing_usage' ? 0 : 10,
          usage_source: 'parsed', mode: 'native', streamed: true,
        }] }))
      } else { response.statusCode = 404; response.end('{}') }
    })
    try {
      await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
      // Execute a real child CLI twice: turn 2 must receive turn 1's text,
      // while neither text may survive serialization to the result artifact.
      writeFileSync(fakeCli, `#!${process.execPath}
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
const args = process.argv.slice(2)
const output = args[args.indexOf('--output-last-message') + 1]
const canary = readFileSync(path.join(process.cwd(), 'CANARY.txt'), 'utf8').trim()
const first = output.includes('turn-1')
const body = ${JSON.stringify(sentinel)}
if (!first && !args.at(-1).includes(body + canary)) process.exit(3)
writeFileSync(output, first ? body + canary : ${JSON.stringify(scenario)} === 'recall_mismatch' ? body + canary : 'CODEX_GATEWAY_E2E_OK:' + canary)
if (first && ${JSON.stringify(scenario)} !== 'missing_command') console.log(JSON.stringify({ type: 'item.completed', item: { type: 'command_execution', exit_code: 0, aggregated_output: canary } }))
console.log(JSON.stringify({ type: 'turn.completed', usage: { input_tokens: 6, output_tokens: 4 } }))
`, { mode: 0o700 })
      writeFileSync(path.join(directory, 'empty.env'), '')
      const child = spawn(process.execPath, [
        '--',
        new URL('./codex-e2e-smoke.mjs', import.meta.url).pathname,
        '--case', 'codex.multi_turn_tool', '--model', 'test-model', '--codex', fakeCli,
        '--env-file', path.join(directory, 'empty.env'), '--home', path.join(directory, 'home'),
        '--workspace', workspace, '--output-dir', output,
      ], {
        env: {
          PATH: process.env.PATH,
          CODEX_E2E_TESTS: '1',
          CODEX_GATEWAY_BASE_URL: `http://127.0.0.1:${server.address().port}/v1`,
          CODEX_GATEWAY_API_KEY: 'fake-data-key', CODEX_GATEWAY_ADMIN_KEY: 'fake-admin-key',
        },
        stdio: ['ignore', 'pipe', 'pipe'],
      })
      let diagnostics = ''
      child.stdout.on('data', (chunk) => { diagnostics += chunk })
      child.stderr.on('data', (chunk) => { diagnostics += chunk })
      const [code] = await once(child, 'close')
      assert.equal(code, scenario === 'passed' ? 0 : 1, diagnostics)
      assert.equal(usageQueries, 1)
      const files = readdirSync(output)
      assert.equal(files.length, 1)
      const serialized = readFileSync(path.join(output, files[0]), 'utf8')
      const canary = readFileSync(path.join(workspace, 'CANARY.txt'), 'utf8').trim()
      assert.ok(!serialized.includes(sentinel))
      assert.ok(!serialized.includes(canary))
      assert.doesNotMatch(serialized, /"(?:expected|final_text)"/)
      const artifact = JSON.parse(serialized)
      const result = artifact.results[0]
      assert.equal(result.outcome, scenario === 'passed' ? 'passed' : 'failed')
      const metadata = result.failure || result
      assert.equal(metadata.evaluation.passed, scenario !== 'recall_mismatch')
      assert.equal(metadata.evaluation.final_exact, scenario !== 'recall_mismatch')
      assert.equal(typeof metadata.evaluation.final_text_length, 'number')
      if (scenario !== 'passed') {
        assert.equal(metadata.turn_1.exit_code, 0)
        assert.equal(metadata.turn_2.exit_code, 0)
        assert.equal(metadata.turn_1.final_text_length, sentinel.length + canary.length)
        assert.equal(metadata.turn_1.event_summary.canary_seen, scenario !== 'missing_command')
        assert.equal(metadata.usage.passed, scenario !== 'missing_usage')
      }
    } finally {
      await new Promise((resolve) => server.close(resolve))
      rmSync(directory, { recursive: true, force: true })
    }
  })
}
