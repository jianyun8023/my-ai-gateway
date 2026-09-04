#!/usr/bin/env node
/**
 * anthropic.mjs — Anthropic SDK smoke tests against local Gateway.
 *
 * Tests: Messages basic, Messages stream, Tool basic.
 * Goal: discover SDK parsing issues that curl/custom HTTP clients miss.
 *
 * Usage:
 *   node scripts/conformance/sdk-smoke/anthropic.mjs [--target local|external]
 *       [--case <case_id>]
 *
 * Requires: npm install in scripts/conformance/sdk-smoke/ first.
 */

import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const require = createRequire(resolve(__dirname, 'node_modules/'))

const Anthropic = require('@anthropic-ai/sdk').default || require('@anthropic-ai/sdk')

import {
  loadConfig,
  startConformanceTarget,
  waitForReady,
  ensureReportDir,
  writeReport,
  parseArguments,
  printSummary,
  optionalEnv,
} from '../runner-helpers.mjs'
import { normalizeSdkSmoke } from '../normalize-report.mjs'

// ── Test case definitions ───────────────────────────────────────────────────

const CASES = [
  {
    case_id: 'messages.text.sdk_basic',
    name: 'Messages basic',
    protocol_in: 'anthropic_messages',
    feature: 'text',
    stream: false,
    run: async (client, model) => {
      const message = await client.messages.create({
        model,
        max_tokens: 256,
        messages: [{ role: 'user', content: 'Say hello' }],
      })
      assertDefined(message.id, 'message.id')
      assert(message.type === 'message', `type is message, got ${message.type}`)
      assert(message.role === 'assistant', `role is assistant, got ${message.role}`)
      assertDefined(message.content, 'message.content')
      assert(message.content.length > 0, 'content non-empty')
      assert(message.content[0].type === 'text', `content[0].type is text`)
      assertDefined(message.content[0].text, 'content[0].text')
      assertDefined(message.stop_reason, 'message.stop_reason')
      assertDefined(message.usage, 'message.usage')
      return { assertions: [
        { name: 'has_id', passed: true },
        { name: 'type_message', passed: true },
        { name: 'role_assistant', passed: true },
        { name: 'has_text_content', passed: true },
        { name: 'has_stop_reason', passed: true },
        { name: 'has_usage', passed: true },
      ] }
    },
  },
  {
    case_id: 'messages.streaming.sdk_stream',
    name: 'Messages stream',
    protocol_in: 'anthropic_messages',
    feature: 'streaming',
    stream: true,
    run: async (client, model) => {
      const stream = client.messages.stream({
        model,
        max_tokens: 256,
        messages: [{ role: 'user', content: 'Say hello' }],
      })
      const events = []
      for await (const event of stream) {
        events.push(event)
      }
      assert(events.length > 0, 'received events')
      const hasStart = events.some((e) => e.type === 'message_start')
      const hasStop = events.some((e) => e.type === 'message_stop')
      assert(hasStart, 'has message_start event')
      assert(hasStop, 'has message_stop event')

      const finalMessage = await stream.finalMessage()
      assertDefined(finalMessage.id, 'finalMessage.id')
      return { assertions: [
        { name: 'received_events', passed: true },
        { name: 'has_message_start', passed: true },
        { name: 'has_message_stop', passed: true },
        { name: 'final_message_has_id', passed: true },
      ] }
    },
  },
  {
    case_id: 'messages.tools.sdk_tool_basic',
    name: 'Messages tool call',
    protocol_in: 'anthropic_messages',
    feature: 'tools',
    stream: false,
    run: async (client, model) => {
      const message = await client.messages.create({
        model,
        max_tokens: 256,
        messages: [{ role: 'user', content: 'What is the weather in SF?' }],
        tools: [{
          name: 'get_weather',
          description: 'Get weather for a location',
          input_schema: {
            type: 'object',
            properties: {
              location: { type: 'string' },
              unit: { type: 'string', enum: ['celsius', 'fahrenheit'] },
            },
            required: ['location'],
          },
        }],
      })
      assertDefined(message.content, 'message.content')
      assert(message.content.length > 0, 'content non-empty')
      const toolUse = message.content.find((c) => c.type === 'tool_use')
      assertDefined(toolUse, 'has tool_use block')
      assertDefined(toolUse.id, 'tool_use.id')
      assertDefined(toolUse.name, 'tool_use.name')
      assertDefined(toolUse.input, 'tool_use.input')
      assert(message.stop_reason === 'tool_use', `stop_reason is tool_use, got ${message.stop_reason}`)
      return { assertions: [
        { name: 'has_tool_use_content', passed: true },
        { name: 'tool_use_has_id', passed: true },
        { name: 'tool_use_has_name', passed: true },
        { name: 'tool_use_has_input', passed: true },
        { name: 'stop_reason_tool_use', passed: true },
      ] }
    },
  },
  {
    case_id: 'messages.usage.sdk_usage',
    name: 'Messages usage tracking',
    protocol_in: 'anthropic_messages',
    feature: 'usage',
    stream: false,
    run: async (client, model) => {
      const message = await client.messages.create({
        model,
        max_tokens: 256,
        messages: [{ role: 'user', content: 'Say hello' }],
      })
      assertDefined(message.usage, 'message.usage')
      assert(typeof message.usage.input_tokens === 'number', 'input_tokens is number')
      assert(typeof message.usage.output_tokens === 'number', 'output_tokens is number')
      return { assertions: [
        { name: 'has_usage', passed: true },
        { name: 'has_input_tokens', passed: true },
        { name: 'has_output_tokens', passed: true },
      ] }
    },
  },
]

// ── Assertion helpers ───────────────────────────────────────────────────────

function assert(condition, message) {
  if (!condition) throw new Error(`Assertion failed: ${message}`)
}

function assertDefined(value, name) {
  if (value === undefined || value === null) {
    throw new Error(`Expected ${name} to be defined, got ${value}`)
  }
}

// ── Main ────────────────────────────────────────────────────────────────────

async function main() {
  const args = parseArguments(process.argv.slice(2))
  const config = loadConfig()
  const reportDir = ensureReportDir('sdk-smoke')

  // Parse --case filters
  const caseFilters = []
  for (let i = 2; i < process.argv.length; i++) {
    if (process.argv[i] === '--case' && process.argv[i + 1]) {
      caseFilters.push(process.argv[++i])
    } else if (process.argv[i].startsWith('--case=')) {
      caseFilters.push(process.argv[i].split('=', 2)[1])
    }
  }

  const selectedCases =
    caseFilters.length > 0
      ? CASES.filter((c) => caseFilters.includes(c.case_id))
      : CASES

  console.error(`=== Anthropic SDK smoke tests (${selectedCases.length} cases) ===`)

  // ── Resolve target ────────────────────────────────────────────────
  let baseUrl, model, cleanup

  if (args.target === 'local') {
    console.error('  Starting conformance-target…')
    const target = await startConformanceTarget()
    baseUrl = target.baseUrl
    model = target.model
    cleanup = target.cleanup
    await waitForReady(baseUrl)
    console.error(`  Gateway ready at ${baseUrl}`)
  } else {
    baseUrl = optionalEnv(
      config.targets.external.base_url_env,
      'http://127.0.0.1:8787',
    )
    model = optionalEnv(config.targets.external.model_env, 'conformance-test-model')
    cleanup = null
  }

  const apiKey = optionalEnv(
    config.targets[args.target]?.api_key_env || 'CONFORMANCE_API_KEY',
    config.targets.local.api_key_default,
  )

  // ── Create Anthropic client ───────────────────────────────────────
  // Anthropic SDK appends /v1/messages internally, so baseURL should NOT include /v1
  const client = new Anthropic({
    apiKey,
    baseURL: baseUrl,
  })

  // Detect SDK version
  let sdkVersion = 'unknown'
  try {
    const pkg = JSON.parse(readFileSync(resolve(__dirname, 'node_modules/@anthropic-ai/sdk/package.json'), 'utf8'))
    sdkVersion = pkg.version
  } catch { /* ignore */ }

  console.error(`  Anthropic SDK v${sdkVersion}`)

  // ── Run cases ─────────────────────────────────────────────────────
  const results = []

  try {
    for (const testCase of selectedCases) {
      const start = Date.now()
      let result = 'PASS'
      let failureClass = null
      let evidence = null

      try {
        const caseEvidence = await testCase.run(client, model)
        evidence = { assertions: caseEvidence.assertions, notes: null }
      } catch (err) {
        result = 'FAIL'
        failureClass = 'GATEWAY_BUG'
        evidence = {
          assertions: [{ name: 'sdk_call', passed: false, message: err.message }],
          notes: `SDK error: ${err.message}`,
        }
        console.error(`  ✗ ${testCase.case_id}: ${err.message}`)
      }

      const durationMs = Date.now() - start

      results.push({
        case_id: testCase.case_id,
        protocol_in: testCase.protocol_in,
        feature: testCase.feature,
        stream: testCase.stream,
        model,
        result,
        failure_class: failureClass,
        duration_ms: durationMs,
        evidence,
      })

      if (result === 'PASS') {
        console.error(`  ✓ ${testCase.case_id} (${durationMs}ms)`)
      }
    }
  } finally {
    if (cleanup) {
      console.error('\n  Stopping conformance-target…')
      await cleanup()
    }
  }

  // ── Normalize and write report ────────────────────────────────────
  const normalized = normalizeSdkSmoke(results, 'anthropic', sdkVersion)
  writeReport(reportDir, 'anthropic-sdk-smoke.json', normalized)

  const failCount = printSummary(normalized)
  process.exit(failCount > 0 ? 1 : 0)
}

export { CASES as anthropicCases, assert, assertDefined }

main().catch((err) => {
  console.error(`FATAL: ${err.message}`)
  process.exit(2)
})
