/**
 * compare.mjs — Compare normalized Direct vs Gateway responses and classify results.
 *
 * Result classification matrix (from Issue #119):
 *
 *  Direct FAIL + Gateway FAIL, same error class  → UPSTREAM_LIMITATION
 *  Direct PASS + Gateway FAIL                    → GATEWAY_BUG
 *  Both PASS, Gateway drops declared field        → GATEWAY_BUG or DEGRADED
 *  Model randomness prevents stable judgment      → MODEL_BEHAVIOR
 *  Network/transport intermittent failure          → FLAKY
 *
 * This module does NOT produce false PASS results. If comparison cannot be
 * determined, the result is FAIL with the most conservative classification.
 */

import { applyNormalizers, normalize_stream_events } from './normalize.mjs'

// ── Assertion runners ───────────────────────────────────────────────────────

/**
 * Run a single named assertion against direct and gateway responses.
 *
 * @param {string} assertionName
 * @param {object} direct - Direct provider response (parsed, not normalized)
 * @param {object} gateway - Gateway response (parsed, not normalized)
 * @param {object} caseSpec - Case definition from cases.json
 * @returns {{ name: string, passed: boolean, message: string|null }}
 */
function runAssertion(assertionName, direct, gateway, caseSpec) {
  try {
    switch (assertionName) {
      case 'http_status_match':
        return assertHttpStatusMatch(direct, gateway)
      case 'envelope_parseable':
        return assertEnvelopeParseable(direct, gateway)
      case 'finish_reason_present':
        return assertFinishReasonPresent(direct, gateway)
      case 'usage_fields_present':
        return assertUsageFieldsPresent(direct, gateway)
      case 'choices_non_empty':
        return assertChoicesNonEmpty(direct, gateway)
      case 'content_non_empty':
        return assertContentNonEmpty(direct, gateway)
      case 'stream_has_chunks':
        return assertStreamHasChunks(direct, gateway)
      case 'stream_has_done':
        return assertStreamHasDone(direct, gateway)
      case 'content_chunks_present':
        return assertContentChunksPresent(direct, gateway)
      case 'tool_call_present':
        return assertToolCallPresent(direct, gateway)
      case 'tool_name_match':
        return assertToolNameMatch(direct, gateway, caseSpec)
      case 'tool_args_valid_json':
        return assertToolArgsValidJson(direct, gateway)
      case 'tool_call_id_present':
        return assertToolCallIdPresent(direct, gateway)
      case 'finish_reason_tool_calls':
        return assertFinishReasonToolCalls(direct, gateway)
      case 'no_tool_call':
        return assertNoToolCall(direct, gateway)
      case 'usage_present':
        return assertUsagePresent(direct, gateway)
      case 'usage_prompt_tokens_type':
        return assertUsageTokenType(gateway, 'prompt_tokens')
      case 'usage_completion_tokens_type':
        return assertUsageTokenType(gateway, 'completion_tokens')
      case 'usage_total_tokens_type':
        return assertUsageTokenType(gateway, 'total_tokens')
      case 'usage_shape_match':
        return assertUsageShapeMatch(direct, gateway)
      case 'error_envelope_present':
        return assertErrorEnvelopePresent(direct, gateway)
      case 'error_type_present':
        return assertErrorTypePresent(direct, gateway)
      default:
        return { name: assertionName, passed: false, message: `Unknown assertion: ${assertionName}` }
    }
  } catch (err) {
    return { name: assertionName, passed: false, message: `Assertion threw: ${err.message}` }
  }
}

// ── Individual assertion implementations ────────────────────────────────────

function assertHttpStatusMatch(direct, gateway) {
  const dStatus = direct._meta?.http_status ?? 200
  const gStatus = gateway._meta?.http_status ?? 200
  return {
    name: 'http_status_match',
    passed: dStatus === gStatus,
    message: dStatus !== gStatus ? `Direct: ${dStatus}, Gateway: ${gStatus}` : null,
  }
}

function assertEnvelopeParseable(_direct, gateway) {
  const has = gateway && typeof gateway === 'object' && !gateway._meta?.parse_error
  return {
    name: 'envelope_parseable',
    passed: has,
    message: has ? null : 'Gateway response is not a valid JSON object',
  }
}

function assertFinishReasonPresent(direct, gateway) {
  const gFR = gateway?.choices?.[0]?.finish_reason
  const dFR = direct?.choices?.[0]?.finish_reason
  if (!dFR && !gFR) {
    return { name: 'finish_reason_present', passed: true, message: 'Neither side has finish_reason (both intermediate or error)' }
  }
  return {
    name: 'finish_reason_present',
    passed: gFR != null,
    message: gFR == null ? `Direct finish_reason: ${dFR}, Gateway: missing` : null,
  }
}

function assertUsageFieldsPresent(direct, gateway) {
  const dUsage = direct?.usage
  const gUsage = gateway?.usage
  if (!dUsage) {
    return { name: 'usage_fields_present', passed: true, message: 'Direct has no usage; skipped' }
  }
  if (!gUsage) {
    return { name: 'usage_fields_present', passed: false, message: 'Direct has usage but Gateway does not' }
  }
  const missing = ['prompt_tokens', 'completion_tokens'].filter((k) => !(k in gUsage))
  return {
    name: 'usage_fields_present',
    passed: missing.length === 0,
    message: missing.length > 0 ? `Gateway missing usage fields: ${missing.join(', ')}` : null,
  }
}

function assertChoicesNonEmpty(_direct, gateway) {
  const has = Array.isArray(gateway?.choices) && gateway.choices.length > 0
  return { name: 'choices_non_empty', passed: has, message: has ? null : 'Gateway choices array is empty or missing' }
}

function assertContentNonEmpty(_direct, gateway) {
  const content = gateway?.choices?.[0]?.message?.content
  const has = typeof content === 'string' && content.length > 0
  return { name: 'content_non_empty', passed: has, message: has ? null : 'Gateway choices[0].message.content is empty or missing' }
}

function assertStreamHasChunks(direct, gateway) {
  const gChunks = gateway?._stream_summary?.total_chunks ?? 0
  const dChunks = direct?._stream_summary?.total_chunks ?? 0
  if (dChunks === 0 && gChunks === 0) {
    return { name: 'stream_has_chunks', passed: false, message: 'Neither side received any chunks' }
  }
  return {
    name: 'stream_has_chunks',
    passed: gChunks > 0,
    message: gChunks === 0 ? `Direct: ${dChunks} chunks, Gateway: 0 chunks` : null,
  }
}

function assertStreamHasDone(direct, gateway) {
  const gDone = gateway?._stream_summary?.has_done ?? false
  const dDone = direct?._stream_summary?.has_done ?? false
  if (!dDone && !gDone) {
    return { name: 'stream_has_done', passed: true, message: 'Neither side sent [DONE] (provider-specific)' }
  }
  return {
    name: 'stream_has_done',
    passed: gDone,
    message: gDone ? null : `Direct has [DONE] but Gateway does not`,
  }
}

function assertContentChunksPresent(direct, gateway) {
  const gContent = gateway?._stream_summary?.has_content ?? false
  const dContent = direct?._stream_summary?.has_content ?? false
  if (!dContent) {
    return { name: 'content_chunks_present', passed: true, message: 'Direct has no content chunks (tool-only stream)' }
  }
  return {
    name: 'content_chunks_present',
    passed: gContent,
    message: gContent ? null : 'Direct has content chunks but Gateway does not',
  }
}

function assertToolCallPresent(direct, gateway) {
  const dTc = direct?.choices?.[0]?.message?.tool_calls
  const gTc = gateway?.choices?.[0]?.message?.tool_calls
  if (!Array.isArray(dTc) || dTc.length === 0) {
    return { name: 'tool_call_present', passed: false, message: 'Direct did not produce tool_calls (model behavior)' }
  }
  return {
    name: 'tool_call_present',
    passed: Array.isArray(gTc) && gTc.length > 0,
    message: Array.isArray(gTc) && gTc.length > 0 ? null : 'Direct has tool_calls but Gateway does not',
  }
}

function assertToolNameMatch(direct, gateway, _caseSpec) {
  const dNames = (direct?.choices?.[0]?.message?.tool_calls || []).map((tc) => tc.function?.name).sort()
  const gNames = (gateway?.choices?.[0]?.message?.tool_calls || []).map((tc) => tc.function?.name).sort()
  const match = JSON.stringify(dNames) === JSON.stringify(gNames)
  return {
    name: 'tool_name_match',
    passed: match,
    message: match ? null : `Direct tools: [${dNames}], Gateway tools: [${gNames}]`,
  }
}

function assertToolArgsValidJson(_direct, gateway) {
  const tcs = gateway?.choices?.[0]?.message?.tool_calls || []
  const invalid = []
  for (const tc of tcs) {
    if (!tc.function?.arguments) continue
    try {
      JSON.parse(tc.function.arguments)
    } catch {
      invalid.push(tc.function?.name || 'unknown')
    }
  }
  return {
    name: 'tool_args_valid_json',
    passed: invalid.length === 0,
    message: invalid.length > 0 ? `Invalid JSON arguments in tools: ${invalid.join(', ')}` : null,
  }
}

function assertToolCallIdPresent(_direct, gateway) {
  const tcs = gateway?.choices?.[0]?.message?.tool_calls || []
  const missing = tcs.filter((tc) => !tc.id || typeof tc.id !== 'string' || tc.id.length === 0)
  return {
    name: 'tool_call_id_present',
    passed: missing.length === 0,
    message: missing.length > 0 ? `${missing.length} tool call(s) missing id` : null,
  }
}

function assertFinishReasonToolCalls(direct, gateway) {
  const dFR = direct?.choices?.[0]?.finish_reason
  const gFR = gateway?.choices?.[0]?.finish_reason
  if (dFR !== 'tool_calls' && dFR !== 'tool_use') {
    return { name: 'finish_reason_tool_calls', passed: true, message: `Direct finish_reason is '${dFR}' not tool_calls (model behavior)` }
  }
  const gNorm = gFR === 'tool_calls' || gFR === 'tool_use'
  return {
    name: 'finish_reason_tool_calls',
    passed: gNorm,
    message: gNorm ? null : `Direct finish_reason: ${dFR}, Gateway: ${gFR}`,
  }
}

function assertNoToolCall(_direct, gateway) {
  const gTc = gateway?.choices?.[0]?.message?.tool_calls
  const hasTc = Array.isArray(gTc) && gTc.length > 0
  return {
    name: 'no_tool_call',
    passed: !hasTc,
    message: hasTc ? 'Gateway produced unexpected tool_calls in resolved conversation' : null,
  }
}

function assertUsagePresent(direct, gateway) {
  const dUsage = direct?.usage
  const gUsage = gateway?.usage
  if (!dUsage) {
    return { name: 'usage_present', passed: gUsage == null, message: gUsage ? 'Gateway has usage but Direct does not (unexpected but benign)' : null }
  }
  return {
    name: 'usage_present',
    passed: gUsage != null,
    message: gUsage != null ? null : 'Direct has usage but Gateway does not',
  }
}

function assertUsageTokenType(response, field) {
  const value = response?.usage?.[field]
  const valid = typeof value === 'number' && Number.isInteger(value) && value >= 0
  return {
    name: `usage_${field}_type`,
    passed: valid,
    message: valid ? null : `usage.${field} = ${JSON.stringify(value)} (expected non-negative integer)`,
  }
}

function assertUsageShapeMatch(direct, gateway) {
  const dKeys = Object.keys(direct?.usage || {}).sort()
  const gKeys = Object.keys(gateway?.usage || {}).sort()
  const missing = dKeys.filter((k) => !gKeys.includes(k))
  return {
    name: 'usage_shape_match',
    passed: missing.length === 0,
    message: missing.length > 0 ? `Gateway usage missing keys present in Direct: ${missing.join(', ')}` : null,
  }
}

function assertErrorEnvelopePresent(direct, gateway) {
  const dErr = direct?.error
  const gErr = gateway?.error
  if (!dErr) {
    return { name: 'error_envelope_present', passed: true, message: 'Direct has no error (non-error response)' }
  }
  return {
    name: 'error_envelope_present',
    passed: gErr != null && typeof gErr.message === 'string',
    message: gErr != null ? null : 'Direct has error envelope but Gateway does not',
  }
}

function assertErrorTypePresent(direct, gateway) {
  const dType = direct?.error?.type
  const gType = gateway?.error?.type
  if (!dType) {
    return { name: 'error_type_present', passed: true, message: 'Direct has no error.type' }
  }
  return {
    name: 'error_type_present',
    passed: gType != null,
    message: gType != null ? null : `Direct error.type: ${dType}, Gateway: missing`,
  }
}

// ── Classification engine ───────────────────────────────────────────────────

/**
 * Determine if a response side (direct or gateway) represents a failure.
 */
function isSideFailed(response) {
  if (!response) return true
  if (response._meta?.parse_error) return true
  if (response._meta?.transport_error) return true
  const status = response._meta?.http_status ?? 200
  return status >= 400
}

/**
 * Determine if two failing sides have the same error class.
 */
function isSameErrorClass(direct, gateway) {
  const dStatus = direct?._meta?.http_status ?? 0
  const gStatus = gateway?._meta?.http_status ?? 0
  // Same HTTP status category (4xx vs 5xx)
  if (Math.floor(dStatus / 100) === Math.floor(gStatus / 100)) return true
  // Both transport errors
  if (direct?._meta?.transport_error && gateway?._meta?.transport_error) return true
  return false
}

/**
 * Classify a differential comparison result.
 *
 * @param {object} direct - Direct provider response (with _meta)
 * @param {object} gateway - Gateway response (with _meta)
 * @param {{ name: string, passed: boolean, message: string|null }[]} assertions
 * @param {object} caseSpec - Case definition
 * @returns {{ result: string, failure_class: string|null }}
 */
export function classifyResult(direct, gateway, assertions, caseSpec) {
  const directFailed = isSideFailed(direct)
  const gatewayFailed = isSideFailed(gateway)

  // Network/transport errors on either side → FLAKY
  if (direct?._meta?.transport_error || gateway?._meta?.transport_error) {
    return { result: 'FAIL', failure_class: 'FLAKY' }
  }

  // Both sides failed with same error class → UPSTREAM_LIMITATION
  if (directFailed && gatewayFailed && isSameErrorClass(direct, gateway)) {
    return { result: 'FAIL', failure_class: 'UPSTREAM_LIMITATION' }
  }

  // Direct PASS + Gateway FAIL → GATEWAY_BUG
  if (!directFailed && gatewayFailed) {
    return { result: 'FAIL', failure_class: 'GATEWAY_BUG' }
  }

  // Direct FAIL + Gateway PASS → unusual, but gateway may be doing fallback
  if (directFailed && !gatewayFailed) {
    return { result: 'PASS', failure_class: null }
  }

  // Both passed HTTP — check assertions
  const failedAssertions = assertions.filter((a) => !a.passed)

  // Model behavior: Direct didn't produce expected content (e.g. no tool_calls)
  // and Gateway also didn't — this is model randomness, not gateway fault
  const modelBehaviorAssertions = failedAssertions.filter((a) =>
    a.message?.includes('model behavior') || a.message?.includes('Direct did not produce'),
  )
  if (modelBehaviorAssertions.length > 0 && modelBehaviorAssertions.length === failedAssertions.length) {
    return { result: 'FAIL', failure_class: 'MODEL_BEHAVIOR' }
  }

  if (failedAssertions.length === 0) {
    return { result: 'PASS', failure_class: null }
  }

  // Gateway dropped fields that direct has → GATEWAY_BUG
  return { result: 'FAIL', failure_class: 'GATEWAY_BUG' }
}

// ── Main comparison orchestrator ────────────────────────────────────────────

/**
 * Compare direct and gateway responses for a single test case.
 *
 * @param {object} directRaw - Raw direct response (with _meta: { http_status })
 * @param {object} gatewayRaw - Raw gateway response (with _meta: { http_status })
 * @param {object} caseSpec - Case definition from cases.json
 * @returns {object} Unified test result conforming to test-result.schema.json
 */
export function compareResponses(directRaw, gatewayRaw, caseSpec, durationMs = 0) {
  const isStream = caseSpec.stream

  let directNorm, gatewayNorm
  const normalizerNames = (caseSpec.normalizers || []).filter((n) => n !== 'normalize_stream_events')

  if (isStream) {
    // For streaming: apply per-chunk normalizers, then stream summary
    const { normalized: dChunks } = applyNormalizers(
      directRaw._stream_chunks || [],
      normalizerNames,
      { side: 'direct', caseSpec },
    )
    const { normalized: gChunks } = applyNormalizers(
      gatewayRaw._stream_chunks || [],
      normalizerNames,
      { side: 'gateway', caseSpec },
    )

    // Build stream summaries
    const dSummary = normalize_stream_events(
      Array.isArray(dChunks) ? dChunks : directRaw._stream_chunks || [],
      { side: 'direct' },
    )
    const gSummary = normalize_stream_events(
      Array.isArray(gChunks) ? gChunks : gatewayRaw._stream_chunks || [],
      { side: 'gateway' },
    )

    directNorm = { ...directRaw, _stream_summary: dSummary }
    gatewayNorm = { ...gatewayRaw, _stream_summary: gSummary }
  } else {
    // Non-streaming: normalize the response bodies
    const { normalized: dNorm } = applyNormalizers(directRaw, normalizerNames, { side: 'direct', caseSpec })
    const { normalized: gNorm } = applyNormalizers(gatewayRaw, normalizerNames, { side: 'gateway', caseSpec })
    directNorm = dNorm
    gatewayNorm = gNorm
  }

  // Run all declared assertions
  const caseAssertions = caseSpec.assertions || []
  const assertionResults = caseAssertions.map((a) =>
    runAssertion(a.name, directNorm, gatewayNorm, caseSpec),
  )

  // Classify result
  const { result, failure_class } = classifyResult(directNorm, gatewayNorm, assertionResults, caseSpec)

  // Build evidence (metadata only — no prompt/response/thinking content)
  const failedAssertions = assertionResults.filter((a) => !a.passed)
  const notes = [
    `differential: direct(${directRaw._meta?.http_status ?? '?'}) vs gateway(${gatewayRaw._meta?.http_status ?? '?'})`,
    ...failedAssertions.map((a) => `${a.name}: ${a.message}`),
  ]
    .filter(Boolean)
    .join('. ')

  const PROTOCOL_MAP = {
    openai_chat_completions: 'openai_chat_completions',
    openai_responses: 'openai_responses',
    anthropic_messages: 'anthropic_messages',
  }

  return {
    case_id: `differential.${caseSpec.case_id}`,
    protocol_in: PROTOCOL_MAP[caseSpec.protocol] || caseSpec.protocol,
    protocol_upstream: PROTOCOL_MAP[caseSpec.protocol] || caseSpec.protocol,
    mode: 'native',
    feature: caseSpec.feature,
    provider: caseSpec.provider,
    requested_model: caseSpec.model_default,
    resolved_model: caseSpec.model_default,
    result,
    failure_class,
    http_status: gatewayRaw._meta?.http_status ?? null,
    stream: caseSpec.stream,
    request_id: null,
    retry_count: 0,
    fallback_count: 0,
    duration_ms: durationMs,
    evidence: {
      assertions: assertionResults,
      notes,
    },
    tool_name: 'differential',
    tool_version: '0.1.0',
    timestamp: new Date().toISOString(),
  }
}
