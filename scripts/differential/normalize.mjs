/**
 * normalize.mjs — Explicit normalizers for Direct vs Gateway differential testing.
 *
 * Each normalizer targets a specific class of dynamic/non-deterministic fields.
 * Every stripped or transformed field is documented with the reason for exclusion.
 *
 * Design principles:
 *  - Normalizers ONLY touch fields that are inherently non-deterministic between
 *    direct and gateway responses (IDs, timestamps, exact token counts).
 *  - Structural fields (choices, tool_calls, finish_reason, usage shape) are
 *    PRESERVED so the comparator can assert on them.
 *  - Adding a new normalizer requires documenting which field it affects and why.
 */

// ── strip_dynamic_ids ───────────────────────────────────────────────────────
//
// Affected fields (non-streaming):
//   - response.id: Provider-generated unique ID; Gateway may wrap or reassign.
//   - response.system_fingerprint: Provider internal build identifier.
//   - choices[].message.tool_calls[].id: Provider-generated call ID; Gateway
//     may proxy or regenerate. Presence is asserted separately.
//
// Affected fields (streaming):
//   - chunk.id: Same as above, per chunk.
//   - chunk.system_fingerprint: Same as above, per chunk.
//
// Reason: These are opaque identifiers that differ between direct and gateway
// by design. Comparing them would produce false positives.

export function strip_dynamic_ids(response, _ctx) {
  if (!response || typeof response !== 'object') return response
  const r = { ...response }

  // Top-level response ID and system fingerprint
  if ('id' in r) r.id = '<stripped:dynamic_id>'
  if ('system_fingerprint' in r) r.system_fingerprint = '<stripped:system_fingerprint>'

  // Tool call IDs in choices — preserve structure, strip opaque ID value
  if (Array.isArray(r.choices)) {
    r.choices = r.choices.map((choice) => {
      const c = { ...choice }
      if (c.message && Array.isArray(c.message.tool_calls)) {
        c.message = {
          ...c.message,
          tool_calls: c.message.tool_calls.map((tc) => ({
            ...tc,
            id: tc.id ? '<stripped:tool_call_id>' : tc.id,
          })),
        }
      }
      if (c.delta && Array.isArray(c.delta.tool_calls)) {
        c.delta = {
          ...c.delta,
          tool_calls: c.delta.tool_calls.map((tc) => ({
            ...tc,
            id: tc.id ? '<stripped:tool_call_id>' : tc.id,
          })),
        }
      }
      return c
    })
  }

  return r
}

// ── strip_timestamps ────────────────────────────────────────────────────────
//
// Affected fields:
//   - response.created: Unix timestamp of response creation. Direct and gateway
//     responses are created at slightly different times.
//
// Reason: Time-of-creation differs by definition between two independent
// requests (direct first, gateway second).

export function strip_timestamps(response, _ctx) {
  if (!response || typeof response !== 'object') return response
  const r = { ...response }
  if ('created' in r) r.created = '<stripped:timestamp>'
  return r
}

// ── normalize_finish_reason ─────────────────────────────────────────────────
//
// Affected fields:
//   - choices[].finish_reason: Canonicalized to lowercase. Some providers use
//     different casing or alternative names for the same semantic reason.
//
// Canonical mapping:
//   "stop" / "end_turn" / "end" → "stop"
//   "length" / "max_tokens" → "length"
//   "tool_calls" / "tool_use" → "tool_calls"
//   "content_filter" → "content_filter"
//   null → null (streaming intermediate chunks)
//
// Reason: Provider-specific naming for semantically identical stop conditions.

const FINISH_REASON_MAP = {
  stop: 'stop',
  end_turn: 'stop',
  end: 'stop',
  length: 'length',
  max_tokens: 'length',
  tool_calls: 'tool_calls',
  tool_use: 'tool_calls',
  content_filter: 'content_filter',
}

export function normalize_finish_reason(response, _ctx) {
  if (!response || typeof response !== 'object') return response
  const r = { ...response }

  if (Array.isArray(r.choices)) {
    r.choices = r.choices.map((choice) => {
      const c = { ...choice }
      if (c.finish_reason !== undefined && c.finish_reason !== null) {
        const key = String(c.finish_reason).toLowerCase()
        c.finish_reason = FINISH_REASON_MAP[key] || c.finish_reason
      }
      return c
    })
  }

  return r
}

// ── normalize_usage_shape ───────────────────────────────────────────────────
//
// Affected fields:
//   - usage.prompt_tokens: Exact value replaced with type marker.
//   - usage.completion_tokens: Exact value replaced with type marker.
//   - usage.total_tokens: Exact value replaced with type marker.
//   - usage.prompt_cache_hit_tokens (provider extension): Preserved as key if present.
//   - usage.prompt_cache_miss_tokens (provider extension): Preserved as key if present.
//   - usage.completion_tokens_details (OpenAI extension): Structure preserved, values replaced.
//
// What is NOT affected:
//   - usage key presence — if direct has usage, gateway must too.
//   - usage key set — gateway must be a superset of direct's keys.
//
// Reason: Token counts may differ slightly due to gateway overhead or
// tokenizer differences; shape and presence matter, exact counts do not.

export function normalize_usage_shape(response, _ctx) {
  if (!response || typeof response !== 'object') return response
  if (!response.usage || typeof response.usage !== 'object') return response

  const r = { ...response }
  const usage = { ...r.usage }

  for (const [key, value] of Object.entries(usage)) {
    if (typeof value === 'number') {
      usage[key] = `<normalized:${typeof value}>`
    } else if (typeof value === 'object' && value !== null) {
      // Preserve nested structure (e.g. completion_tokens_details) but normalize values
      const nested = { ...value }
      for (const [nk, nv] of Object.entries(nested)) {
        if (typeof nv === 'number') {
          nested[nk] = `<normalized:${typeof nv}>`
        }
      }
      usage[key] = nested
    }
  }

  r.usage = usage
  return r
}

// ── normalize_tool_call ─────────────────────────────────────────────────────
//
// Affected fields:
//   - choices[].message.tool_calls[].function.arguments: Parsed and re-serialized
//     to canonical JSON (sorted keys, no whitespace variation). The actual argument
//     VALUES are model-dependent and may differ between runs; this normalizer only
//     ensures JSON validity and canonical form.
//
// What is NOT affected:
//   - Tool call presence (compared separately)
//   - Tool function name (compared separately)
//   - Tool call count
//   - Tool call index
//
// Reason: Models may produce JSON with different formatting or key ordering.

export function normalize_tool_call(response, _ctx) {
  if (!response || typeof response !== 'object') return response
  const r = { ...response }

  if (Array.isArray(r.choices)) {
    r.choices = r.choices.map((choice) => {
      const c = { ...choice }
      if (c.message && Array.isArray(c.message.tool_calls)) {
        c.message = {
          ...c.message,
          tool_calls: c.message.tool_calls.map((tc) => {
            if (!tc.function || typeof tc.function.arguments !== 'string') return tc
            try {
              const parsed = JSON.parse(tc.function.arguments)
              return {
                ...tc,
                function: {
                  ...tc.function,
                  arguments: JSON.stringify(parsed, Object.keys(parsed).sort()),
                },
              }
            } catch {
              return tc
            }
          }),
        }
      }
      return c
    })
  }

  return r
}

// ── normalize_stream_events ─────────────────────────────────────────────────
//
// This normalizer operates on the parsed stream result (array of chunks),
// not on a single response. It extracts a structural summary for comparison.
//
// Extracted structure:
//   - total_chunks: Number of data chunks (excluding [DONE])
//   - has_done: Whether [DONE] sentinel was received
//   - has_content: Whether any chunk contained delta.content
//   - final_finish_reason: finish_reason from the last non-null chunk
//   - has_usage: Whether any chunk contained usage data
//   - tool_call_names: Sorted list of tool function names called
//   - role: Role from the first chunk (should be "assistant")
//
// What is NOT extracted:
//   - Exact text content (model-dependent)
//   - Exact chunk count (may vary by network buffering)
//   - Per-chunk IDs (stripped by strip_dynamic_ids)
//   - Per-chunk timestamps (stripped by strip_timestamps)
//
// Reason: SSE chunk boundaries and exact delta splitting are transport-level
// details that may differ between direct and gateway without semantic impact.

export function normalize_stream_events(chunks, _ctx) {
  if (!Array.isArray(chunks)) return chunks

  let totalChunks = 0
  let hasDone = false
  let hasContent = false
  let hasUsage = false
  let finalFinishReason = null
  let role = null
  const toolCallNames = new Set()

  for (const chunk of chunks) {
    if (chunk === '[DONE]') {
      hasDone = true
      continue
    }
    totalChunks++

    if (typeof chunk !== 'object' || chunk === null) continue

    if (Array.isArray(chunk.choices)) {
      for (const choice of chunk.choices) {
        if (choice.delta?.content) hasContent = true
        if (choice.delta?.role && !role) role = choice.delta.role
        if (choice.finish_reason) {
          const key = String(choice.finish_reason).toLowerCase()
          finalFinishReason = FINISH_REASON_MAP[key] || choice.finish_reason
        }
        if (Array.isArray(choice.delta?.tool_calls)) {
          for (const tc of choice.delta.tool_calls) {
            if (tc.function?.name) toolCallNames.add(tc.function.name)
          }
        }
      }
    }

    if (chunk.usage && typeof chunk.usage === 'object') hasUsage = true
  }

  return {
    total_chunks: totalChunks,
    has_done: hasDone,
    has_content: hasContent,
    has_usage: hasUsage,
    final_finish_reason: finalFinishReason,
    tool_call_names: [...toolCallNames].sort(),
    role,
  }
}

// ── Normalizer registry ─────────────────────────────────────────────────────

const NORMALIZERS = {
  strip_dynamic_ids,
  strip_timestamps,
  normalize_finish_reason,
  normalize_usage_shape,
  normalize_tool_call,
  normalize_stream_events,
}

/**
 * Apply a list of normalizers to a response object.
 *
 * For streaming responses, normalizers are applied per-chunk EXCEPT
 * normalize_stream_events which operates on the full chunk array.
 *
 * @param {object|object[]} response - Parsed response or array of stream chunks
 * @param {string[]} normalizerNames - Names of normalizers to apply
 * @param {object} ctx - Context (case metadata)
 * @returns {{ normalized: object, applied: string[] }}
 */
export function applyNormalizers(response, normalizerNames, ctx = {}) {
  const applied = []
  let result = response

  for (const name of normalizerNames) {
    const fn = NORMALIZERS[name]
    if (!fn) {
      throw new Error(`Unknown normalizer: '${name}'. Available: ${Object.keys(NORMALIZERS).join(', ')}`)
    }

    if (name === 'normalize_stream_events') {
      // Stream normalizer operates on the full chunk array
      if (Array.isArray(result)) {
        result = fn(result, ctx)
        applied.push(name)
      }
    } else if (Array.isArray(result)) {
      // Apply per-chunk normalizer to each chunk in the stream
      result = result.map((chunk) => {
        if (chunk === '[DONE]' || typeof chunk !== 'object') return chunk
        return fn(chunk, ctx)
      })
      applied.push(name)
    } else {
      result = fn(result, ctx)
      applied.push(name)
    }
  }

  return { normalized: result, applied }
}

/**
 * List all available normalizer names with descriptions.
 */
export function listNormalizers() {
  return [
    { name: 'strip_dynamic_ids', fields: ['id', 'system_fingerprint', 'tool_calls[].id'], reason: 'Provider-generated opaque IDs differ by design' },
    { name: 'strip_timestamps', fields: ['created'], reason: 'Two requests made at different times' },
    { name: 'normalize_finish_reason', fields: ['choices[].finish_reason'], reason: 'Provider-specific naming for same semantic stop condition' },
    { name: 'normalize_usage_shape', fields: ['usage.*'], reason: 'Token counts may differ; shape and presence matter' },
    { name: 'normalize_tool_call', fields: ['choices[].message.tool_calls[].function.arguments'], reason: 'JSON formatting/key order differences' },
    { name: 'normalize_stream_events', fields: ['(stream chunks)'], reason: 'SSE chunk boundaries are transport-level' },
  ]
}
