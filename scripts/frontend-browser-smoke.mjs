import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
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
    details: { error_code: 'synthetic_failure' },
    source: 'usage_events',
  }],
  page: { limit: 100, has_more: false, next_cursor: null },
};

const json = (response, statusCode, body) => {
  response.writeHead(statusCode, { 'content-type': 'application/json; charset=utf-8' });
  response.end(JSON.stringify(body));
};

const mime = (path) => ({
  '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.svg': 'image/svg+xml',
}[extname(path)] ?? 'application/octet-stream');

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://localhost');
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
    response.writeHead(500);
    response.end(String(error));
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

    await waitFor(`document.body.textContent.includes('events_query_failed')`, 'initial runtime-event failure');
    if (!(await evaluate(`Boolean(document.querySelector('section[aria-label="运行事件筛选"]'))`))) throw new Error('Runtime event filters disappeared after failure');
    if (!(await setInput('关联 / 操作 ID', 'req-browser'))) throw new Error('Correlation field not found');
    if (!(await clickButton('应用'))) throw new Error('Apply button not found');
    await waitFor(`document.body.textContent.includes('request.failed') && !document.body.textContent.includes('events_query_failed')`, 'runtime-event recovery');

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

    console.log('browser smoke passed: error recovery, source isolation, route focus, Portal/Escape');
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
