import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseArgs, liveTarget } from './run.mjs';
import { summarize, compare, markdown } from './compare.mjs';

const fixture = () => ({ metrics: {
  load_requests: { values: { count: 20 } }, load_success: { values: { rate: 1 } },
  load_latency_ms: { values: { 'p(50)': 10, 'p(95)': 20, 'p(99)': 30 } },
  load_ttft_ms: { values: { 'p(50)': 2, 'p(95)': 3, 'p(99)': 4 } },
  load_stream_completed: { values: { rate: 1 } },
}, state: { testRunDurationMs: 2000 } });

test('load defaults cover six concurrency levels and four protocol cases', () => {
  assert.deepEqual(parseArgs([]).concurrency, [1,5,10,25,50,100]);
  assert.equal(parseArgs([]).cases.length, 4);
  assert.equal(parseArgs([]).target, 'mock');
  assert.deepEqual(parseArgs(['--concurrency','1,5','--iterations','2','--case','responses.sse']).cases, ['responses.sse']);
});
for (const argv of [['--concurrency','0'],['--concurrency','1,1'],['--iterations','NaN'],['--case','no-such-case'],['--target','production'],['--concurrency'],['--oops']]) {
  test(`invalid load arguments fail: ${argv}`, () => assert.throws(() => parseArgs(argv)));
}
test('live load requires opt-in, two identities, and explicit URLs/models', () => {
  assert.throws(() => liveTarget({}), /LOAD_TESTS/);
  assert.throws(() => liveTarget({LOAD_TESTS:'1'}), /LOAD_DIRECT_KEY/);
  const env = { LOAD_TESTS:'1', LOAD_DIRECT_KEY:'direct-secret', LOAD_GATEWAY_KEY:'gateway-secret', LOAD_DIRECT_MODEL:'upstream', LOAD_GATEWAY_MODEL:'logical', LOAD_DIRECT_URL:'https://provider.example', LOAD_GATEWAY_URL:'http://localhost:8787' };
  assert.equal(liveTarget(env).directModel, 'upstream');
  assert.throws(() => liveTarget({...env, LOAD_DIRECT_URL:'https://secret@provider.example'}), /credentials/);
  assert.throws(() => liveTarget({...env, LOAD_DIRECT_URL:'https://provider.example?key=secret'}), /query/);
});
test('summary reports counts/rates, TTFT and percentile latency', () => {
  const s = summarize(fixture(),20,true);
  assert.equal(s.result,'PASS'); assert.equal(s.requests_per_second,10);
  assert.equal(s.ttft_ms.p95,3); assert.equal(s.total_latency_ms.p99,30);
  assert.equal(summarize(fixture(),20,false).ttft_ms,null);
});
test('missing, empty and nonfinite measurements never pass', () => {
  assert.throws(() => summarize({},1,true));
  let f=fixture(); f.metrics.load_requests.values.count=0; assert.throws(()=>summarize(f,1,true));
  f=fixture(); delete f.metrics.load_latency_ms; assert.throws(()=>summarize(f,20,true));
  f=fixture(); f.metrics.load_ttft_ms.values['p(95)']=NaN; assert.throws(()=>summarize(f,20,true));
  f=fixture(); delete f.metrics.load_stream_completed; assert.throws(()=>summarize(f,20,true));
});
test('partial completion, failed requests and missing TTFT fail', () => {
  assert.equal(summarize(fixture(),21,true).result,'FAIL');
  let f=fixture(); f.metrics.load_success.values.rate=0.9; assert.equal(summarize(f,20,true).result,'FAIL');
  f=fixture(); f.metrics.load_stream_completed.values.rate=0.9; assert.equal(summarize(f,20,true).result,'FAIL');
  f=fixture(); delete f.metrics.load_ttft_ms; assert.equal(summarize(f,20,true).result,'FAIL');
});
test('summary retains classified failure counts without service error details', () => {
  const f = fixture();
  f.metrics.load_failure_resource_exhaustion = { values: { count: 2 } };
  f.metrics.load_success.values.rate = 0.9;
  const result = summarize(f, 20, true);
  assert.equal(result.result, 'FAIL');
  assert.deepEqual(result.failure_counts, { resource_exhaustion: 2 });
});
test('comparison preserves negative deltas and failures', () => {
  const direct=summarize(fixture(),20,true); const gateway=structuredClone(direct);
  gateway.ttft_ms.p50=1;
  assert.equal(compare(direct,gateway).gateway_added_ttft_ms.p50,-1);
  gateway.result='FAIL'; assert.equal(compare(direct,gateway).result,'FAIL');
  assert.equal(compare({result:'FAIL'},{result:'FAIL'}).gateway_added_ttft_ms,null);
});
test('markdown describes limitations and failed tool rows without NaN', () => {
  const md=markdown({started_at:'today',target:'mock',profile:'release',environment:{},limitations:['no DB'], rows:[{case_id:'chat.json',concurrency:1,result:'FAIL',direct:{result:'FAIL'},gateway:{result:'FAIL'}}]});
  assert.ok(md.includes('no DB')); assert.ok(!md.includes('NaN')); assert.ok(md.includes('FAIL'));
});
