import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const previewUrl = process.env.THEME_PREVIEW_URL ?? 'http://127.0.0.1:5188/#sources';
const chromeCandidates = [
  process.env.CHROME_BIN,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
].filter(Boolean);
const styles = ['utility', 'ocean', 'nebula', 'sandstone'];
const modes = ['light', 'dark'];
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

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
    if (!message.id || !pending.has(message.id)) return;
    const entry = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) entry.reject(new Error(message.error.message));
    else entry.resolve(message.result);
  });
  return {
    close: () => socket.close(),
    send(method, params = {}) {
      const id = nextId++;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        socket.send(JSON.stringify({ id, method, params }));
      });
    },
  };
};

let chromePath;
for (const candidate of chromeCandidates) {
  try {
    if ((await stat(candidate)).isFile()) { chromePath = candidate; break; }
  } catch { /* try next candidate */ }
}
if (!chromePath) throw new Error('Chrome/Chromium not found; set CHROME_BIN');

const profile = await mkdtemp(join(tmpdir(), 'my-ai-gateway-theme-preview-'));
const chrome = spawn(chromePath, [
  '--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
  `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank',
], { stdio: 'ignore' });
let cdp;

try {
  const devtoolsFile = join(profile, 'DevToolsActivePort');
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { await stat(devtoolsFile); break; } catch { await delay(50); }
  }
  const [debugPort] = (await readFile(devtoolsFile, 'utf8')).trim().split('\n');
  const page = await fetch(`http://127.0.0.1:${debugPort}/json/new?${encodeURIComponent(previewUrl)}`, { method: 'PUT' }).then((response) => response.json());
  cdp = await connectCdp(page.webSocketDebuggerUrl);
  await cdp.send('Runtime.enable');
  await cdp.send('Page.enable');
  await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });

  const evaluate = async (expression) => {
    const result = await cdp.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.text);
    return result.result.value;
  };
  const waitFor = async (expression, label) => {
    for (let attempt = 0; attempt < 120; attempt += 1) {
      if (await evaluate(expression)) return;
      await delay(50);
    }
    throw new Error(`Timed out waiting for ${label}`);
  };
  const selectAppearance = async (style, mode) => {
    await evaluate(`localStorage.setItem('my-ai-gateway-theme', ${JSON.stringify(JSON.stringify({ state: { style, mode }, version: 0 }))})`);
    await cdp.send('Page.reload');
    await waitFor(`document.documentElement.dataset.themeStyle === ${JSON.stringify(style)}
      && document.documentElement.dataset.colorScheme === ${JSON.stringify(mode)}
      && document.body.textContent.includes('全部来源')`, `${style} ${mode}`);
    await delay(120);
  };
  const capture = async (filename) => {
    const { data } = await cdp.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    await writeFile(new URL(filename, import.meta.url), Buffer.from(data, 'base64'));
  };

  for (const style of styles) {
    for (const mode of modes) {
      await selectAppearance(style, mode);
      await capture(`${style}-${mode}.png`);
    }
  }

  await selectAppearance('utility', 'light');
  await evaluate(`document.querySelector('button[aria-haspopup="dialog"][aria-label^="外观设置"]')?.click()`);
  await waitFor(`Boolean(document.querySelector('button[data-theme-option="sandstone"]'))`, 'appearance picker');
  await capture('appearance-picker.png');
  console.log('Captured 9 theme preview screenshots in docs/evidence/231');
} finally {
  cdp?.close();
  if (chrome.exitCode === null) chrome.kill('SIGTERM');
  await rm(profile, { recursive: true, force: true });
}
