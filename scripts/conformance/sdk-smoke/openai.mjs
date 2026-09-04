#!/usr/bin/env node
/**
 * openai.mjs — OpenAI SDK smoke tests against local Gateway.
 *
 * Tests: Chat basic, Chat stream, Responses basic, Responses stream, Tool basic.
 * Goal: discover SDK parsing issues that curl/custom HTTP clients miss.
 *
 * Usage:
 *   node scripts/conformance/sdk-smoke/openai.mjs [--target local|external]
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

const OpenAI = require('openai').default || require('openai')

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
    case_id: 'chat.text.sdk_basic',
    name: 'Chat Completions basic',
    protocol_in: 'openai_chat_completions',
    feature: 'text',
    stream: false,
    run: async (client, model) => {
      const response = await client.chat.completions.create({
        model,
        messages: [{ role: 'user', content: 'Say hello' }],
      })
      assertDefined(response.id, 'response.id')
      assertDefined(response.choices, 'response.choices')
      assert(response.choices.length > 0, 'choices non-empty')
      assertDefined(response.choices[0].message, 'choices[0].message')
      assertDefined(response.choices[0].message.content, 'choices[0].message.content')
      assertDefined(response.usage, 'response.usage')
      return { assertions: [
        { name: 'has_id', passed: true },
        { name: 'has_choices', passed: true },
        { name: 'has_content', passed: true },
        { name: 'has_usage', passed: true },
      ] }
    },
  },
  {
    case_id: 'chat.streaming.sdk_stream',
    name: 'Chat Completions stream',
    protocol_in: 'openai_chat_completions',
    feature: 'streaming',
    stream: true,
    run: async (client, model) => {
      const stream = await client.chat.completions.create({
        model,
        messages: [{ role: 'user', content: 'Say hello' }],
        stream: true,
      })
      const chunks = []
      for await (const chunk of stream) {
        chunks.push(chunk)
      }
      assert(chunks.length > 0, 'received chunks')
      assertDefined(chunks[0].id, 'first chunk has id')
      const hasFinish = chunks.some((c) =>
        c.choices?.some((ch) => ch.finish_reason != null),
      )
      assert(hasFinish, 'stream has finish_reason')
      return { assertions: [
        { name: 'received_chunks', passed: true },
        { name: 'has_chunk_id', passed: true },
        { name: 'has_finish_reason', passed: true },
      ] }
    },
  },
  {
    case_id: 'responses.text.sdk_basic',
    name: 'Responses basic',
    protocol_in: 'openai_responses',
    feature: 'text',
    stream: false,
    run: async (client, model) => {
      const response = await client.responses.create({
        model,
        input: 'Say hello',
      })
      assertDefined(response.id, 'response.id')
      assert(response.status === 'completed', `status is completed, got ${response.status}`)
      assertDefined(response.output, 'response.output')
      assert(response.output.length > 0, 'output non-empty')
      assertDefined(response.usage, 'response.usage')
      return { assertions: [
        { name: 'has_id', passed: true },
        { name: 'status_completed', passed: true },
        { name: 'has_output', passed: true },
        { name: 'has_usage', passed: true },
      ] }
    },
  },
  {
    case_id: 'responses.streaming.sdk_stream',
    name: 'Responses stream',
    protocol_in: 'openai_responses',
    feature: 'streaming',
    stream: true,
    run: async (client, model) => {
      const stream = await client.responses.create({
        model,
        input: 'Say hello',
        stream: true,
      })
      const events = []
      for await (const event of stream) {
        events.push(event)
      }
      assert(events.length > 0, 'received events')
      const hasCompleted = events.some((e) => e.type === 'response.completed')
      assert(hasCompleted, 'stream has response.completed event')
      return { assertions: [
        { name: 'received_events', passed: true },
        { name: 'has_completed_event', passed: true },
      ] }
    },
  },
  {
    case_id: 'chat.tools.sdk_tool_basic',
    name: 'Chat Completions tool call',
    protocol_in: 'openai_chat_completions',
    feature: 'tools',
    stream: false,
    run: async (client, model) => {
      const response = await client.chat.completions.create({
        model,
        messages: [{ role: 'user', content: 'What is the weather in SF?' }],
        tools: [{
          type: 'function',
          function: {
            name: 'get_weather',
            description: 'Get weather for a location',
            parameters: {
              type: 'object',
              properties: {
                location: { type: 'string' },
                unit: { type: 'string', enum: ['celsius', 'fahrenheit'] },
              },
              required: ['location'],
            },
          },
        }],
      })
      assertDefined(response.choices, 'response.choices')
      const message = response.choices[0].message
      assertDefined(message.tool_calls, 'message.tool_calls')
      assert(message.tool_calls.length > 0, 'tool_calls non-empty')
      const tc = message.tool_calls[0]
      assertDefined(tc.id, 'tool_call.id')
      assert(tc.type === 'function', `tool_call.type is function, got ${tc.type}`)
      assertDefined(tc.function.name, 'tool_call.function.name')
      assertDefined(tc.function.arguments, 'tool_call.function.arguments')
      JSON.parse(tc.function.arguments)
      return { assertions: [
        { name: 'has_tool_calls', passed: true },
        { name: 'tool_call_has_id', passed: true },
        { name: 'tool_call_type_function', passed: true },
        { name: 'arguments_valid_json', passed: true },
      ] }
    },
  },
  {
    case_id: 'chat.usage.sdk_usage',
    name: 'Chat Completions usage tracking',
    protocol_in: 'openai_chat_completions',
    feature: 'usage',
    stream: false,
    run: async (client, model) => {
      const response = await client.chat.completions.create({
        model,
        messages: [{ role: 'user', content: 'Say hello' }],
      })
      assertDefined(response.usage, 'response.usage')
      assert(typeof response.usage.prompt_tokens === 'number', 'prompt_tokens is number')
      assert(typeof response.usage.completion_tokens === 'number', 'completion_tokens is number')
      assert(typeof response.usage.total_tokens === 'number', 'total_tokens is number')
      assert(response.usage.total_tokens > 0, 'total_tokens > 0')
      return { assertions: [
        { name: 'has_usage', passed: true },
        { name: 'has_prompt_tokens', passed: true },
        { name: 'has_completion_tokens', passed: true },
        { name: 'total_tokens_positive', passed: true },
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

  console.error(`=== OpenAI SDK smoke tests (${selectedCases.length} cases) ===`)

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

  // ── Create OpenAI client ──────────────────────────────────────────
  const client = new OpenAI({
    apiKey,
    baseURL: `${baseUrl}/v1`,
  })

  // Detect SDK version
  let sdkVersion = 'unknown'
  try {
    const pkg = JSON.parse(readFileSync(resolve(__dirname, 'node_modules/openai/package.json'), 'utf8'))
    sdkVersion = pkg.version
  } catch { /* ignore */ }

  console.error(`  OpenAI SDK v${sdkVersion}`)

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
  const normalized = normalizeSdkSmoke(results, 'openai', sdkVersion)
  writeReport(reportDir, 'openai-sdk-smoke.json', normalized)

  const failCount = printSummary(normalized)
  process.exit(failCount > 0 ? 1 : 0)
}

export { CASES as openaiCases, assert, assertDefined }

main().catch((err) => {
  console.error(`FATAL: ${err.message}`)
  process.exit(2)
})
