export function summarize(summary, expectedCount, streaming) {
  const metrics = summary.metrics || {};
  const count = metrics.load_requests?.values?.count;
  const rate = metrics.load_success?.values?.rate;
  if (!Number.isInteger(count) || count <= 0 || !Number.isFinite(rate)) throw new Error('Missing/non-finite load measurements (zero cases cannot pass)');
  const trend = name => {
    const values = metrics[name]?.values;
    if (!values) return null;
    const result = {};
    for (const p of [50, 95, 99]) {
      const value = values[`p(${p})`];
      if (!Number.isFinite(value)) throw new Error(`Missing p${p} for ${name}`);
      result[`p${p}`] = value;
    }
    return result;
  };
  const streamRate = streaming ? metrics.load_stream_completed?.values?.rate : null;
  if (streaming && !Number.isFinite(streamRate)) throw new Error('Missing stream completion measurements');
  const tokenLatency = streaming ? trend('load_ttft_ms') : null;
  const duration = summary.state?.testRunDurationMs;
  if (!Number.isFinite(duration) || duration <= 0) throw new Error('Missing run duration');
  return {
    result: count === expectedCount && rate === 1 && (!streaming || streamRate === 1 && tokenLatency) ? 'PASS' : 'FAIL',
    request_count: count, expected_request_count: expectedCount, success_rate: rate,
    total_latency_ms: trend('load_latency_ms') ?? (() => { throw new Error('Missing latency measurements'); })(), ttft_ms: tokenLatency,
    stream_completion_rate: streamRate,
    failure_counts: Object.fromEntries(Object.entries(metrics).filter(([name]) => name.startsWith('load_failure_')).map(([name, metric]) => [name.slice(13), metric.values.count])),
    requests_per_second: count / (duration / 1000),
  };
}

export function compare(direct, gateway) {
  const delta = key => direct[key] && gateway[key]
    ? Object.fromEntries([50, 95, 99].map(p => [`p${p}`, gateway[key][`p${p}`] - direct[key][`p${p}`]])) : null;
  return {
    result: direct.result === 'PASS' && gateway.result === 'PASS' ? 'PASS' : 'FAIL',
    gateway_added_ttft_ms: delta('ttft_ms'),
    gateway_added_total_latency_ms: delta('total_latency_ms'),
  };
}

export function markdown(report) {
  const fmt = value => !Number.isFinite(value) ? '—' : value.toFixed(2);
  const lines = ['# Gateway 本地性能基线', '', `时间：${report.started_at}；目标：${report.target}；profile：${report.profile}`, '',
    'TTFT 为首个非空文本 delta；非流式 TTFT 不适用。差值为 Gateway 分位数减 Direct 分位数，保留负值，不代表逐请求配对差值。', '',
    '| Case | 并发 | 结果 | Direct/Gateway 请求数 | Gateway 成功率 | Δ latency p50/p95/p99 ms | Δ TTFT p50/p95/p99 ms |',
    '| --- | ---: | --- | --- | ---: | --- | --- |'];
  const triple = value => value ? [50, 95, 99].map(p => fmt(value[`p${p}`])).join(' / ') : '—';
  for (const row of report.rows) lines.push(`| ${row.case_id} | ${row.concurrency} | ${row.result} | ${row.direct?.request_count ?? '—'} / ${row.gateway?.request_count ?? '—'} | ${row.gateway ? fmt(100 * row.gateway.success_rate) + '%' : '—'} | ${triple(row.gateway_added_total_latency_ms)} | ${triple(row.gateway_added_ttft_ms)} |`);
  lines.push('', '## 环境与边界', '', '```json', JSON.stringify(report.environment, null, 2), '```', '', ...report.limitations.map(v => `- ${v}`), '');
  return lines.join('\n');
}
