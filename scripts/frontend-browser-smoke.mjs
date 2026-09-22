import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { tmpdir } from 'node:os';

const projectRoot = new URL('..', import.meta.url).pathname;
const distRoot = join(projectRoot, 'web', 'dist');
const chromeCandidates = [
  process.env.CHROME_BIN,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
].filter(Boolean);

const source = (id, name) => ({
  id,
  display_name: name,
  provider_preset_id: 'synthetic',
  provider_preset_version: 1,
  provider_preset_snapshot: {
    schema_version: 1,
    default_base_url: 'https://provider.example',
    protocols: {
      openai_chat_completions: { endpoint: '/v1/chat/completions', mode: 'native' },
      openai_responses: { endpoint: '/v1/responses', mode: 'native' },
      anthropic_messages: { endpoint: '/v1/messages', mode: 'unsupported' },
    },
    discovery: { support: 'unsupported', reason: 'Synthetic browser fixture' },
  },
  base_url: 'https://provider.example',
  endpoints: {
    openai_chat_completions: '/v1/chat/completions',
    openai_responses: '/v1/responses',
    anthropic_messages: '/v1/messages',
  },
  auth_config: { credential_header: { header: 'authorization', prefix: 'Bearer' } },
  protocol_capabilities: {
    openai_chat_completions: { mode: 'native' },
    openai_responses: { mode: 'native' },
    anthropic_messages: { mode: 'unsupported' },
  },
  enabled: true,
  created_at: '2026-09-16T00:00:00Z',
  updated_at: '2026-09-16T00:00:00Z',
});

const sources = [source('source-a', 'Source A'), source('source-b', 'Source B')];
const accounts = sources.map((item) => ({
  id: `account-${item.id.at(-1)}`,
  source_id: item.id,
  display_name: `Account ${item.id.at(-1).toUpperCase()}`,
  credential_configured: true,
  enabled: true,
  weight: 100,
  health_status: 'unknown',
  cooldown_until: null,
  created_at: item.created_at,
  updated_at: item.updated_at,
}));

const upstreamQuotas = [{
  account: {
    account_id: 'account-a', account_display_name: 'Account A', source_id: 'source-a',
    source_display_name: 'Source A', provider_id: 'kimi_code', enabled: true,
  },
  status: 'exhausted',
  resources: [
    { type: 'window', key: '5h', label: '5 hours', unit: 'percent', remaining: 100, used: 0, limit: 100, reset_at: '2026-09-23T00:00:00Z' },
    { type: 'window', key: '7d', label: '7 days', unit: 'percent', remaining: 0, used: 100, limit: 100, reset_at: '2026-09-25T00:00:00Z' },
  ],
  fetched_at: '2026-09-22T00:00:00Z', attempted_at: '2026-09-22T00:00:00Z', latency_ms: 120,
  stale: false, refresh_error: null,
}];

const healthResponse = {
  fact_source: 'postgresql', stale_after_secs: 600,
  data: [{
    account_id: 'account-a', source_id: 'source-a', display_name: 'Account A', enabled: true,
    health_status: 'stale', health_updated_at: '2026-09-21T23:00:00Z', stale: true, cooldown_until: null,
    health: {
      available: true, source_enabled: true, consecutive_failures: 0, cooldown_remaining_ms: 0,
      status: 'stale', source: 'passive', stale: true, updated_at: '2026-09-21T23:00:00Z',
      cooldown_until: null, last_error: 'Synthetic health record is stale', last_success_at: null,
      last_probe_at: null, last_probe_status: null, last_probe_error: null,
    },
  }],
};

const eventResponse = {
  version: 'v1',
  timezone: 'UTC',
  fact_source: 'postgresql_unified_read_model',
  range: { from: null, since: null, to: null, boundary: '[from,to)' },
  data: [{
    event_id: 'usage:req-browser',
    occurred_at: '2026-09-16T00:00:00Z',
    category: 'request',
    event_type: 'request.failed',
    level: 'error',
    subject_type: 'request',
    subject_id: 'req-browser',
    correlation_id: 'req-browser',
    message: 'Synthetic browser event',
    details: {
      error_summary: 'Synthetic upstream rate limit', status_code: 429,
      logical_model: 'reasoning-model-with-a-long-display-name',
      source_id: 'source-a', account_id: 'account-a',
    },
    source: 'usage_events',
  }],
  page: { limit: 100, has_more: false, next_cursor: null },
};

const usageEvent = {
  request_id: 'req-browser', created_at: '2026-09-16T00:00:00Z',
  logical_model: 'reasoning-model-with-a-long-display-name', upstream_model_id: 'upstream-model',
  provider_id: 'synthetic', source_id: 'source-a', account_id: 'account-a',
  protocol_in: 'openai_responses', protocol_upstream: 'openai_responses',
  status_code: 429, success: false, retry_count: 0, usage_source: 'missing',
  error_summary: 'Synthetic upstream rate limit', streamed: true,
};
const virtualKeys = [{
  id: 1, name: 'codex-long-key-name', key_prefix: 'gw_d5ecc0f8', key_recoverable: true,
  allowed_models: ['reasoning-model-with-a-long-display-name', 'kimi-for-coding-highspeed', 'k3'],
  enabled: true, created_at: '2026-09-07T08:23:09Z', last_used_at: '2026-09-22T08:39:12Z',
}];
let snapshotRevision = 107;

const json = (response, statusCode, body) => {
  response.writeHead(statusCode, { 'content-type': 'application/json; charset=utf-8' });
  response.end(JSON.stringify(body));
};

const mime = (path) => ({
  '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.svg': 'image/svg+xml',
}[extname(path)] ?? 'application/octet-stream');

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://localhost');
  if (url.pathname === '/admin/keys') return json(response, 200, { data: virtualKeys });
  if (url.pathname === '/admin/upstream-quotas') return json(response, 200, { data: upstreamQuotas });
  if (url.pathname === '/admin/health') return json(response, 200, healthResponse);
  if (url.pathname === '/admin/capabilities') return json(response, 200, {
    version: 'v1', fact_source: 'runtime_snapshot', snapshot_revision: snapshotRevision,
    snapshot_generated_at: `2026-09-22T00:00:${String(snapshotRevision - 100).padStart(2, '0')}Z`, data: [],
  });
  if (url.pathname === '/admin/config/reload') {
    snapshotRevision += 1;
    return json(response, 200, { status: 'reloaded', snapshot_revision: snapshotRevision, snapshot_generated_at: '2026-09-22T00:00:08Z' });
  }
  if (url.pathname === '/admin/usage/events/req-browser') return json(response, 200, {
    version: 'v1', data: usageEvent,
    attempts: [{ attempt_no: 0, account_id: 'account-a', source_id: 'source-a', status_code: 429, success: false }],
  });
  if (url.pathname === '/admin/usage/events') return json(response, 200, { version: 'v1', data: [], page: { has_more: false, next_cursor: null } });
  if (url.pathname === '/admin/events') {
    if (url.searchParams.get('subject_type') === 'source' || url.searchParams.get('correlation_id')) {
      return json(response, 200, url.searchParams.get('subject_type') === 'source' ? { ...eventResponse, data: [] } : eventResponse);
    }
    return json(response, 503, { error: { code: 'events_query_failed', message: 'Synthetic initial failure' } });
  }
  if (url.pathname === '/admin/sources') return json(response, 200, { data: sources });
  if (url.pathname === '/admin/accounts') return json(response, 200, { data: accounts });
  if (url.pathname === '/admin/provider-presets') return json(response, 200, { data: [{ id: 'synthetic', version: 1, display_name: 'Synthetic', definition: sources[0].provider_preset_snapshot, created_at: sources[0].created_at }] });
  if (/^\/admin\/sources\/[^/]+\/discoveries\/latest$/.test(url.pathname)) return json(response, 404, { error: { code: 'discovery_not_found', message: 'No discovery run' } });
  if (/^\/admin\/sources\/[^/]+\/models$/.test(url.pathname)) return json(response, 200, { data: [] });
  if (/^\/admin\/sources\/[^/]+\/preset-diff$/.test(url.pathname)) return json(response, 200, { data: { source_id: url.pathname.split('/')[3], provider_preset_id: 'synthetic', source_version: 1, latest_version: 1, changes: [] } });
  if (url.pathname.startsWith('/admin/')) return json(response, 200, { data: [] });

  let filePath = join(distRoot, url.pathname === '/' ? 'index.html' : url.pathname);
  try {
    if (!(await stat(filePath)).isFile()) filePath = join(distRoot, 'index.html');
  } catch {
    filePath = join(distRoot, 'index.html');
  }
  try {
    const body = await readFile(filePath);
    response.writeHead(200, { 'content-type': `${mime(filePath)}; charset=utf-8` });
    response.end(body);
  } catch (error) {
    console.error('Failed to serve frontend smoke asset:', error);
    response.writeHead(500);
    response.end('Internal Server Error');
  }
});

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const listen = () => new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server.address().port)));
const withTimeout = (promise, milliseconds, label) => new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error(`Timed out waiting for ${label}`)), milliseconds);
  Promise.resolve(promise).then(
    (value) => { clearTimeout(timeout); resolve(value); },
    (error) => { clearTimeout(timeout); reject(error); },
  );
});

const connectCdp = async (webSocketUrl) => {
  const socket = new WebSocket(webSocketUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true });
    socket.addEventListener('error', reject, { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (!message.id) return;
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    if (message.error) entry.reject(new Error(message.error.message));
    else entry.resolve(message.result);
  });
  return {
    close: () => socket.close(),
    send(method, params = {}) {
      const id = nextId++;
      return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          pending.delete(id);
          reject(new Error(`Timed out waiting for CDP ${method}`));
        }, 10_000);
        pending.set(id, {
          resolve: (value) => { clearTimeout(timeout); resolve(value); },
          reject: (error) => { clearTimeout(timeout); reject(error); },
        });
        socket.send(JSON.stringify({ id, method, params }));
      });
    },
  };
};

const run = async () => {
  await stat(join(distRoot, 'index.html'));
  let chromePath;
  for (const candidate of chromeCandidates) {
    try {
      if ((await stat(candidate)).isFile()) { chromePath = candidate; break; }
    } catch { /* try next candidate */ }
  }
  if (!chromePath) throw new Error('Chrome/Chromium not found; set CHROME_BIN');

  const port = await listen();
  const profile = await mkdtemp(join(tmpdir(), 'my-ai-gateway-browser-smoke-'));
  const chrome = spawn(chromePath, [
    '--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank',
  ], { stdio: 'ignore' });
  let cdp;
  try {
    const devtoolsFile = join(profile, 'DevToolsActivePort');
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try { await stat(devtoolsFile); break; } catch { await delay(50); }
      if (chrome.exitCode !== null) throw new Error(`Chrome exited before CDP startup (${chrome.exitCode})`);
    }
    const [debugPort] = (await readFile(devtoolsFile, 'utf8')).trim().split('\n');
    const page = await fetch(`http://127.0.0.1:${debugPort}/json/new?${encodeURIComponent(`http://127.0.0.1:${port}/#runtime-events`)}`, { method: 'PUT' }).then((response) => response.json());
    cdp = await connectCdp(page.webSocketDebuggerUrl);
    await cdp.send('Runtime.enable');
    await cdp.send('Page.enable');
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
    await cdp.send('Page.reload');

    const evaluate = async (expression) => {
      const result = await cdp.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.text);
      return result.result.value;
    };
    const waitFor = async (expression, label) => {
      for (let attempt = 0; attempt < 100; attempt += 1) {
        if (await evaluate(expression)) return;
        await delay(50);
      }
      throw new Error(`Timed out waiting for ${label}`);
    };
    const setInput = (label, value) => evaluate(`(() => {
      const label = [...document.querySelectorAll('label')].find((item) => item.textContent.trim() === ${JSON.stringify(label)});
      const input = label && document.getElementById(label.htmlFor);
      if (!input) return false;
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(input, ${JSON.stringify(value)});
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    })()`);
    const clickButton = (text) => evaluate(`(() => {
      const button = [...document.querySelectorAll('button')].find((item) => item.textContent.trim() === ${JSON.stringify(text)});
      if (!button) return false;
      button.focus();
      button.click();
      return true;
    })()`);
    const screenshot = async (name) => {
      const directory = process.env.FRONTEND_SMOKE_SCREENSHOT_DIR;
      if (!directory) return;
      await mkdir(directory, { recursive: true });
      const result = await cdp.send('Page.captureScreenshot', { format: 'png' });
      await writeFile(join(directory, `${name}.png`), Buffer.from(result.data, 'base64'));
    };
    const assertLocalTableLayout = async (pageId, statusText) => {
      const result = await evaluate(`(() => {
        const page = document.querySelector('[data-od-id="${pageId}"]');
        const pill = [...page.querySelectorAll('[data-ui="status-pill"]')].find(el => el.textContent === ${JSON.stringify(statusText)});
        return {
          overflow: document.documentElement.scrollWidth > innerWidth + 1,
          statusHeight: pill?.getBoundingClientRect().height,
          overflowingCells: [...page.querySelectorAll('tbody td')].filter(el => el.scrollWidth > el.clientWidth + 1).map(el => ({ column: el.cellIndex, text: el.textContent, width: el.clientWidth, content: el.scrollWidth })),
        };
      })()`);
      if (result.overflow) throw new Error(`${pageId} overflows the viewport`);
      if (!result.statusHeight || result.statusHeight > 32) throw new Error(`${pageId} short status wraps: ${JSON.stringify(result)}`);
      if (result.overflowingCells.length) throw new Error(`${pageId} cell content overlaps: ${JSON.stringify(result.overflowingCells)}`);
    };

    await waitFor(`document.body.textContent.includes('events_query_failed')`, 'initial runtime-event failure');
    if (!(await evaluate(`Boolean(document.querySelector('section[aria-label="运行事件筛选"]'))`))) throw new Error('Runtime event filters disappeared after failure');
    if (!(await setInput('关联 / 操作 ID', 'req-browser'))) throw new Error('Correlation field not found');
    if (!(await clickButton('应用'))) throw new Error('Apply button not found');
    await waitFor(`document.body.textContent.includes('Synthetic upstream rate limit') && !document.body.textContent.includes('events_query_failed')`, 'runtime-event recovery');
    await assertLocalTableLayout('page-runtime-events', '错误');
    if (await evaluate(`document.querySelector('[data-od-id="page-runtime-events"]').textContent.includes('postgresql_unified_read_model')`)) throw new Error('Internal read model leaked into the event list');
    await screenshot('runtime-events-desktop');

    await evaluate(`document.querySelector('[data-od-id="page-runtime-events"] tbody button').click()`);
    await waitFor(`Boolean(document.querySelector('[role="dialog"]'))`, 'runtime event drawer');
    if (!(await clickButton('查看请求详情'))) throw new Error('Request detail navigation missing');
    await waitFor(`location.hash === '#events?request_id=req-browser' && Boolean(document.querySelector('[data-od-id="event-drawer"]'))`, 'historical request detail');
    if (!(await evaluate(`document.querySelector('[data-od-id="event-drawer"]').textContent.includes('Synthetic upstream rate limit')`))) throw new Error('Deep-linked request details lost server data');
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape' });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape' });
    await waitFor(`!document.querySelector('[role="dialog"]') && location.hash === '#events'`, 'request detail close');

    await evaluate(`location.hash = '#settings'`);
    await waitFor(`document.body.textContent.includes('codex-long-key-name')`, 'settings page');
    if (!(await clickButton('重新加载运行时'))) throw new Error('Runtime reload missing');
    await waitFor(`document.querySelector('[data-od-id="page-settings"]').textContent.includes('108')`, 'runtime reload result');
    snapshotRevision = 109;
    if (!(await clickButton('刷新'))) throw new Error('Settings refresh missing');
    await waitFor(`document.querySelector('[data-od-id="page-settings"]').textContent.includes('109')`, 'fresh snapshot after reload');
    await evaluate(`document.querySelector('[role="status"] button')?.click()`);
    await waitFor(`!document.querySelector('[role="status"] button')`, 'notification exit');
    await assertLocalTableLayout('page-settings', '已启用');
    const actionRows = await evaluate(`(() => {
      const row = document.querySelector('[data-od-id="page-settings"] tbody tr');
      return [...row.querySelectorAll('button')].map(el => el.getBoundingClientRect().top);
    })()`);
    if (new Set(actionRows).size !== 1) throw new Error('Key actions wrapped onto multiple rows');
    await screenshot('settings-desktop');

    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
    await assertLocalTableLayout('page-settings', '已启用');
    await evaluate(`document.querySelector('[data-od-id="page-settings"] table').scrollIntoView({ block: 'center' })`);
    await screenshot('settings-mobile');
    await evaluate(`location.hash = '#runtime-events?correlation_id=req-browser'`);
    await waitFor(`Boolean(document.querySelector('[data-od-id="page-runtime-events"] tbody'))`, 'mobile event list');
    await assertLocalTableLayout('page-runtime-events', '错误');
    await evaluate(`document.querySelector('[data-od-id="page-runtime-events"] table').scrollIntoView({ block: 'center' })`);
    await screenshot('runtime-events-mobile');
    await evaluate(`localStorage.setItem('my-ai-gateway-language', 'en'); localStorage.setItem('my-ai-gateway-theme', JSON.stringify({ state: { mode: 'dark', style: 'nebula' }, version: 0 }))`);
    await cdp.send('Page.reload');
    await waitFor(`document.documentElement.dataset.colorScheme === 'dark' && document.body.textContent.includes('Request failed')`, 'dark English runtime events');
    await assertLocalTableLayout('page-runtime-events', 'Error');
    await evaluate(`document.querySelector('[data-od-id="page-runtime-events"] table').scrollIntoView({ block: 'center' })`);
    await screenshot('runtime-events-dark-en-mobile');
    await evaluate(`document.querySelector('[aria-label="Runtime events"]').scrollLeft = 500`);
    const eventActionVisible = await evaluate(`(() => { const rect = document.querySelector('[data-od-id="page-runtime-events"] tbody button').getBoundingClientRect(); return rect.left >= 0 && rect.right <= innerWidth; })()`);
    if (!eventActionVisible) throw new Error('Mobile event action disappeared during horizontal scrolling');
    await screenshot('runtime-events-dark-en-mobile-context');
    await evaluate(`location.hash = '#settings'`);
    await waitFor(`document.body.textContent.includes('codex-long-key-name')`, 'dark English settings');
    await assertLocalTableLayout('page-settings', 'Active');
    await evaluate(`document.querySelector('[data-od-id="page-settings"] table').scrollIntoView({ block: 'center' })`);
    await screenshot('settings-dark-en-mobile');
    await evaluate(`document.querySelector('[aria-label="Virtual keys table"]').scrollLeft = 10000`);
    await screenshot('settings-dark-en-mobile-actions');
    await evaluate(`localStorage.setItem('my-ai-gateway-language', 'zh')`);
    await cdp.send('Page.reload');
    await waitFor(`document.body.textContent.includes('系统设置')`, 'restore Chinese');
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });

    await evaluate(`location.hash = '#upstream-quotas'`);
    await waitFor(`Boolean(document.querySelector('[data-od-id="page-upstream-quotas"] tbody'))`, 'upstream quota list');
    if (!(await evaluate(`document.querySelector('[data-od-id="page-upstream-quotas"]').textContent.includes('该快照中的 7 天窗口 已无剩余额度')`))) throw new Error('Quota window exhaustion evidence missing');
    if (!(await evaluate(`document.querySelector('[data-od-id="page-upstream-quotas"]').textContent.includes('健康状态待复核')`))) throw new Error('Stale routing health was not separated from quota');
    await assertLocalTableLayout('page-upstream-quotas', '已耗尽');
    await screenshot('upstream-quotas-desktop');
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
    await assertLocalTableLayout('page-upstream-quotas', '已耗尽');
    await evaluate(`document.querySelector('[data-od-id="page-upstream-quotas"] table').scrollIntoView({ block: 'center' })`);
    await screenshot('upstream-quotas-mobile-table');
    await evaluate(`document.querySelector('[data-od-id="page-upstream-quotas"] [role="region"]').scrollLeft = 10000`);
    await screenshot('upstream-quotas-mobile-actions');
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });

    await evaluate(`location.hash = '#sources/source-a/edit'`);
    await waitFor(`document.querySelector('#source-displayName')?.value === 'Source A'`, 'source A edit page');
    await evaluate(`(() => { const input = document.querySelector('#source-displayName'); const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; setter.call(input, 'Leaked draft'); input.dispatchEvent(new Event('input', { bubbles: true })); })()`);
    await evaluate(`location.hash = '#sources/source-b/edit'`);
    await waitFor(`document.querySelector('#source-displayName')?.value === 'Source B'`, 'isolated source B edit page');
    if (await evaluate(`document.body.textContent.includes('Leaked draft')`)) throw new Error('Source A draft leaked into source B');
    if (!(await evaluate(`document.activeElement === document.querySelector('h1')`))) throw new Error('Route change did not focus the page title');

    if (!(await clickButton('网关连接'))) throw new Error('Connection dialog trigger not found');
    await waitFor(`Boolean(document.querySelector('[role="dialog"]'))`, 'portal dialog');
    if (await evaluate(`document.querySelector('.app-frame').contains(document.querySelector('[role="dialog"]'))`)) throw new Error('Dialog did not render through a Portal');
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape' });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape' });
    await waitFor(`!document.querySelector('[role="dialog"]')`, 'Escape close');
    if (!(await evaluate(`document.activeElement?.textContent.trim() === '网关连接'`))) throw new Error('Focus did not return after Escape');

    console.log('browser smoke passed: event recovery/context/detail navigation, quota and health state separation, snapshot refresh, desktop/mobile table layout, source isolation, route focus, Portal/Escape');
  } finally {
    cdp?.close();
    if (chrome.exitCode === null) {
      const chromeExit = new Promise((resolve) => chrome.once('exit', resolve));
      chrome.kill('SIGTERM');
      try {
        await withTimeout(chromeExit, 5_000, 'Chrome shutdown');
      } catch {
        chrome.kill('SIGKILL');
        await withTimeout(chromeExit, 5_000, 'forced Chrome shutdown').catch(() => {});
      }
    }
    server.close();
    await rm(profile, { recursive: true, force: true });
  }
};

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
