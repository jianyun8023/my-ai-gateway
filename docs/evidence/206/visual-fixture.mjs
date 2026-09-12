import { createServer } from 'node:http';
const date = '2026-09-12T14:00:00Z';
const protocols = ['openai_chat_completions', 'openai_responses', 'anthropic_messages'];
const sources = ['a', 'b'].map(id => ({
  id: `demo-${id}`, display_name: `演示来源 ${id.toUpperCase()}`, provider_preset_id: 'synthetic', provider_preset_version: 1,
  provider_preset_snapshot: {}, base_url: 'https://provider.example', endpoints: { openai_chat_completions: '/v1/chat/completions', openai_responses: '/v1/responses', anthropic_messages: '/v1/messages' },
  auth_config: {}, protocol_capabilities: Object.fromEntries(protocols.map(p => [p, { mode: 'native' }])), enabled: true, created_at: date, updated_at: date,
}));
const accounts = sources.map((source, i) => ({
  id: `account-${i}`, source_id: source.id, display_name: i === 0 ? '主账号' : '备用账号', credential_configured: true, enabled: true,
  weight: 100, health_status: 'healthy', created_at: date, updated_at: date,
}));
const names = ['deepseek-flash', 'k3', 'k3-256k', 'kimi-for-coding', 'kimi-for-coding-highspeed', 'protocol-specific-fallback', 'pending-model'];
const logicalModels = names.map((name, i) => ({
  id: `logical-${i}`, public_name: name, display_name: i === 5 ? '协议差异与备用线路（模拟）' : name,
  status: i === 6 ? 'pending' : 'confirmed', metadata: {}, field_sources: {}, enabled: true, request_timeout_ms: null, max_retries: null,
  created_at: date, updated_at: date,
}));
const bindings = [], routes = [], rows = [];
for (const [index, model] of logicalModels.entries()) {
  for (const protocol of protocols) {
    const route = { id: `${model.id}-${protocol}`, logical_model_id: model.id, public_name: model.public_name, protocols: [protocol], strategy: 'ordered_fallback', allow_lossy_conversion: index === 5, enabled: true, created_at: date, updated_at: date };
    routes.push(route);
    const candidates = index === 5 ? (protocol === protocols[1] ? [1, 0] : [0, 1]) : [0];
    for (const [rank, ai] of candidates.entries()) {
      const source = sources[ai], account = accounts[ai];
      const binding = { id: bindings.length + 1, logical_model_id: model.id, source_id: source.id, account_id: account.id, upstream_model_id: model.public_name, protocol,
        status: model.status, enabled: true, priority: 100 - rank, created_at: date, updated_at: date };
      bindings.push(binding);
      if (index === 6) continue;
      const adapter = index === 5 && protocol === protocols[1] && ai === 0;
      rows.push({ route_id: route.id, source: { source_id: source.id, display_name: source.display_name }, account: { account_id: account.id, display_name: account.display_name, enabled: true },
        model: model.public_name, model_display_name: model.display_name, upstream_model_id: model.public_name,
        protocols: protocols.map(p => ({ protocol_in: p, status: p === protocol ? 'routable' : 'unroutable', binding_id: p === protocol ? binding.id : null,
          selection: p === protocol ? rank === 0 ? 'primary' : 'fallback' : null, selection_rank: p === protocol ? rank : null,
          protocol_upstream: p === protocol ? adapter ? protocols[2] : protocol : null,
          endpoint: p === protocol ? `https://provider.example${source.endpoints[adapter ? protocols[2] : protocol]}` : null,
          mode: p === protocol ? adapter ? 'adapter' : 'native' : null, adapter: adapter && p === protocol ? 'synthetic-responses-to-messages' : null,
          conversion_chain: p === protocol ? [{ protocol_from: protocol, protocol_to: adapter ? protocols[2] : protocol, mode: adapter ? 'adapter' : 'native', adapter: adapter ? 'synthetic-responses-to-messages' : null }] : [],
          effective_capabilities: { streaming: 'native', tools: 'native', tool_streaming: 'unsupported', thinking: adapter ? 'translated' : 'native', web_search: 'unsupported', file_search: 'unsupported', vision: 'unsupported', usage: 'native' },
          degraded: adapter && p === protocol, degraded_features: adapter && p === protocol ? ['thinking'] : [], allow_lossy_conversion: p === protocol ? adapter : null,
          error: p === protocol ? null : { code: 'runtime_binding_not_available', message: 'This route does not handle the protocol.', route_id: route.id },
        })) });
    }
  }
}
const capabilities = { version: 'v1', fact_source: 'runtime_snapshot', snapshot_revision: 107, snapshot_generated_at: date, data: rows };
const resources = { '/admin/sources': sources, '/admin/accounts': accounts, '/admin/logical-models': logicalModels, '/admin/model-bindings': bindings, '/admin/routes': routes };
createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  if (req.method !== 'GET') { res.writeHead(405); res.end(JSON.stringify({ error: { code: 'read_only_fixture', message: 'Visual fixture is read only' } })); return; }
  const path = new URL(req.url, 'http://localhost').pathname;
  if (path === '/admin/capabilities') res.end(JSON.stringify(capabilities));
  else if (resources[path]) res.end(JSON.stringify({ data: resources[path] }));
  else res.end(JSON.stringify({ data: [] }));
}).listen(8796, '127.0.0.1', () => console.log('Synthetic read-only API on 127.0.0.1:8796'));
