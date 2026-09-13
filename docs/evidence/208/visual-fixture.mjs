// Synthetic data for layout review; protocol modes are not real Provider claims.
import { createServer } from 'node:http';

const date = '2026-09-12T11:42:52Z';
const protocols = ['openai_chat_completions', 'openai_responses', 'anthropic_messages'];
const sources = ['a', 'b'].map((id, index) => ({
  id: `demo-${id}`, display_name: index === 0 ? '演示来源 A' : '演示来源 B · 长名称布局验证',
  provider_preset_id: 'synthetic', provider_preset_version: 1, provider_preset_snapshot: {},
  base_url: 'https://provider.example', endpoints: {}, auth_config: {},
  protocol_capabilities: Object.fromEntries(protocols.map((protocol, i) => [protocol, {
    mode: (index === 0 ? ['native', 'adapter', 'unsupported'] : ['native', 'native', 'unknown'])[i],
  }])), enabled: true, created_at: date, updated_at: date,
}));
const accounts = sources.map((source, index) => ({
  id: `account-${index}`, source_id: source.id, display_name: index === 0 ? '主账号' : '演示来源 B 主账号',
  credential_configured: true, enabled: true, weight: 100, health_status: 'healthy',
  created_at: date, updated_at: date,
}));
const models = source => Array.from({ length: 4 }, (_, i) => ({
  id: i + 1, source_id: source.id, upstream_model_id: `model-${i}`, display_name: `Model ${i}`,
  confirmation_status: source.id === 'demo-b' && i < 2 ? 'pending' : 'confirmed',
  availability_status: 'available', metadata: {}, field_sources: {},
  created_at: date, updated_at: date,
}));
const latest = source => ({
  data: { id: 1, source_id: source.id, status: 'succeeded', started_at: date, completed_at: date,
    diff: { added: [], changed: [], missing: [] }, discovered_model_count: 4 },
  diff: { added: [], changed: [], missing: [] }, last_discovered_at: date,
});

createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  if (req.method !== 'GET') {
    res.writeHead(405);
    res.end(JSON.stringify({ error: { code: 'read_only_fixture', message: 'Read-only visual fixture' } }));
    return;
  }
  const path = new URL(req.url, 'http://localhost').pathname;
  let body;
  if (path === '/admin/sources') body = { data: sources };
  else if (path === '/admin/accounts') body = { data: accounts };
  else {
    const source = sources.find(item => path.startsWith(`/admin/sources/${item.id}/`));
    if (source && path.endsWith('/discoveries/latest')) body = latest(source);
    else if (source && path.endsWith('/models')) body = { data: models(source) };
    else body = { data: [] };
  }
  res.end(JSON.stringify(body));
}).listen(8798, '127.0.0.1', () => console.log('Read-only visual fixture: http://127.0.0.1:8798'));
