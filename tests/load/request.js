// Run with the pinned k6 + xk6-sse binary. No prompt/response is logged or saved.
import http from 'k6/http';
import sse from 'k6/x/sse';
import { Counter, Rate, Trend } from 'k6/metrics';

const requests = new Counter('load_requests');
const failures = Object.fromEntries(['http', 'incomplete', 'invalid_json', 'stream_error', 'resource_exhaustion', 'transport'].map(kind => [kind, new Counter('load_failure_' + kind)]));
function classifyError(error) { return /address|too many open files|resource temporarily unavailable/i.test(String(error)) ? 'resource_exhaustion' : 'transport'; }
const success = new Rate('load_success');
const completed = new Rate('load_stream_completed');
const latency = new Trend('load_latency_ms', true);
const ttft = new Trend('load_ttft_ms', true);
const protocol = __ENV.LOAD_PROTOCOL;
const streaming = __ENV.LOAD_STREAM === 'true';
const url = __ENV.LOAD_URL + (protocol === 'chat' ? '/v1/chat/completions' : '/v1/responses');
const model = __ENV.LOAD_MODEL;
const headers = { 'Content-Type': 'application/json' };
if (__ENV.LOAD_KEY) headers.Authorization = 'Bearer ' + __ENV.LOAD_KEY;
const body = JSON.stringify(protocol === 'chat'
  ? { model, messages: [{ role: 'user', content: 'Reply with OK.' }], max_tokens: 16, stream: streaming }
  : { model, input: 'Reply with OK.', max_output_tokens: 16, stream: streaming });

export const options = {
  scenarios: { load: { executor: 'per-vu-iterations', vus: Number(__ENV.LOAD_CONCURRENCY), iterations: Number(__ENV.LOAD_ITERATIONS), maxDuration: __ENV.LOAD_MAX_DURATION || '60s' } },
  summaryTrendStats: ['avg', 'min', 'max', 'p(50)', 'p(95)', 'p(99)', 'count'],
  thresholds: { load_success: ['rate==1'], ...(streaming ? { load_stream_completed: ['rate==1'] } : {}) },
  systemTags: ['status', 'method'],
  setupTimeout: '120s',
};

function perform(measure) {
  const start = Date.now();
  let ok = false;
  let terminal = false;
  let firstToken = null;
  let failed = false;
  let httpDuration = null;
  let failureKind = 'incomplete';
  try {
    if (streaming) {
      const response = sse.open(url, { method: 'POST', body, headers, timeout: '30s' }, client => {
        client.on('event', event => {
          if (!event.data) return;
          if (event.data === '[DONE]') { terminal = true; client.close(); return; }
          let data;
          try { data = JSON.parse(event.data); } catch (_) { failed = true; failureKind = 'invalid_json'; client.close(); return; }
          if (data.error || data.type === 'error' || data.type === 'response.failed' || data.type === 'response.incomplete') {
            failed = true; failureKind = 'stream_error'; client.close(); return;
          }
          const hasToken = protocol === 'chat'
            ? (data.choices || []).some(c => typeof c.delta?.content === 'string' && c.delta.content.length > 0)
            : data.type === 'response.output_text.delta' && typeof data.delta === 'string' && data.delta.length > 0;
          if (hasToken && firstToken === null) firstToken = Date.now() - start;
          if (data.type === 'response.completed') { terminal = true; client.close(); }
        });
        client.on('error', e => { failed = true; failureKind = classifyError(e.error()); client.close(); });
      });
      if (response && response.status !== 200) failureKind = response.error ? classifyError(response.error) : 'http';
      ok = response && response.status === 200 && terminal && !failed && firstToken !== null;
    } else {
      const response = http.post(url, body, { headers, timeout: '30s' });
      httpDuration = response.timings.duration;
      if (response.status !== 200) failureKind = response.error ? classifyError(response.error) : 'http';
      let data;
      try { data = response.json(); } catch (_) { data = null; }
      ok = response.status === 200 && data && !data.error && (protocol === 'chat' ? Array.isArray(data.choices) && data.choices.length > 0 : data.status === 'completed');
    }
  } catch (error) { ok = false; failureKind = classifyError(error); }
  if (measure) {
    requests.add(1);
    success.add(Boolean(ok));
    if (!ok) failures[failureKind].add(1);
    latency.add(httpDuration ?? Date.now() - start);
    if (streaming) {
      completed.add(Boolean(ok));
      if (ok && firstToken !== null) ttft.add(firstToken);
    }
  }
}

// Prime the target process; warm-up is excluded from custom metrics.
export function setup() { for (let i = 0; i < 5; i++) perform(false); }
export default function () { perform(true); }
export function handleSummary(data) {
  const metrics = {};
  for (const name of ['load_requests', 'load_success', 'load_stream_completed', 'load_latency_ms', 'load_ttft_ms', 'iterations', ...Object.keys(failures).map(k => 'load_failure_' + k)]) {
    if (data.metrics[name]) metrics[name] = data.metrics[name];
  }
  return { [__ENV.LOAD_SUMMARY]: JSON.stringify({ metrics, state: data.state }) };
}
