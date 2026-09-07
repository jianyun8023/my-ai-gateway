import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync, spawn } from 'node:child_process';
import os from 'node:os';
import { setTimeout as delay } from 'node:timers/promises';
import { startConformanceTarget } from '../conformance/runner-helpers.mjs';
import { summarize, compare, markdown } from './compare.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const config = JSON.parse(readFileSync(resolve(root, 'tests/load/scenarios.json')));
const versions = JSON.parse(readFileSync(resolve(root, 'tests/tooling/versions.json')));
export function parseArgs(argv) {
  const result = { target: 'mock', concurrency: config.concurrency, iterations: config.iterations_per_vu, cases: config.cases.map(v => v.id), list: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--list') { result.list = true; continue; }
    if (!['--target', '--concurrency', '--iterations', '--case'].includes(arg) || !argv[i + 1]) throw new Error(`Unknown/incomplete option ${arg}`);
    const value = argv[++i];
    if (arg === '--target') result.target = value;
    if (arg === '--concurrency') result.concurrency = value.split(',').map(Number);
    if (arg === '--iterations') result.iterations = Number(value);
    if (arg === '--case') result.cases = value.split(',');
  }
  if (!['mock', 'live'].includes(result.target)) throw new Error('target must be mock or live');
  if (![...result.concurrency, result.iterations].every(v => Number.isSafeInteger(v) && v > 0)) throw new Error('concurrency and iterations must be positive integers');
  if (new Set(result.concurrency).size !== result.concurrency.length) throw new Error('Duplicate concurrency');
  if (!result.cases.length || result.cases.some(id => !config.cases.some(v => v.id === id))) throw new Error('Unknown load case');
  return result;
}

function requireUrl(value, label) {
  if (!value) throw new Error(`${label} is required`);
  const parsed = new URL(value);
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash) throw new Error(`${label} must be an HTTP(S) base URL without credentials/query/fragment`);
  return value.replace(/\/$/, '');
}

export function liveTarget(env) {
  if (env.LOAD_TESTS !== '1') throw new Error('Live load requires explicit LOAD_TESTS=1');
  for (const key of ['LOAD_DIRECT_KEY', 'LOAD_GATEWAY_KEY', 'LOAD_DIRECT_MODEL', 'LOAD_GATEWAY_MODEL']) if (!env[key]) throw new Error(`${key} is required`);
  return {
    directUrl: requireUrl(env.LOAD_DIRECT_URL, 'LOAD_DIRECT_URL'),
    baseUrl: requireUrl(env.LOAD_GATEWAY_URL, 'LOAD_GATEWAY_URL'),
    directModel: env.LOAD_DIRECT_MODEL, model: env.LOAD_GATEWAY_MODEL,
    directKey: env.LOAD_DIRECT_KEY, gatewayKey: env.LOAD_GATEWAY_KEY,
    cleanup: async () => {},
  };
}

function k6(binary, env) {
  return new Promise((res, rej) => {
    // Keys are passed only via child environment, never command arguments or reports.
    const child = spawn(binary, ['run', '--quiet', resolve(root, 'tests/load/request.js')], { cwd: root, env: { ...env, K6_AUTO_EXTENSION_RESOLUTION: 'false', K6_NO_USAGE_REPORT: 'true' }, stdio: ['ignore', 'ignore', 'pipe'] });
    const timer = setTimeout(() => child.kill('SIGKILL'), 180_000);
    let lastError = '';
    child.stderr.on('data', data => { lastError = (lastError + data).slice(-1000); });
    child.on('error', error => { clearTimeout(timer); rej(error); });
    child.on('close', code => { clearTimeout(timer); res({ code, diagnostic: lastError }); });
  });
}

export async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  if (args.list) { console.log(JSON.stringify(args, null, 2)); return; }
  let target;
  if (args.target === 'live') target = liveTarget(process.env); // fail before spawning anything
  const binary = process.env.K6_BIN || resolve(root, 'target/tools/k6');
  const version = execFileSync(binary, ['version'], { encoding: 'utf8' });
  if (!new RegExp(`(?:k6|k6-sse) v${versions.k6.replaceAll('.', '\\.')} `).test(version) || !version.includes(`github.com/phymbert/xk6-sse v${versions.xk6_sse}`)) throw new Error(`Use pinned k6 ${versions.k6} + xk6-sse ${versions.xk6_sse}; see tests/load/README.md`);
  const started = new Date();
  const out = resolve(root, 'target/test-reports/load', started.toISOString().replaceAll(':', '-'));
  mkdirSync(out, { recursive: true });
  const report = {
    started_at: started.toISOString(), target: args.target, profile: args.target === 'mock' ? 'release/test-support/no-database' : 'external', versions,
    environment: { os: os.platform(), arch: os.arch(), release: os.release(), cpu: os.cpus()[0]?.model, logical_cpus: os.cpus().length, memory_bytes: os.totalmem(), node: process.version, gateway_commit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(), working_tree_dirty: !!execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim(), network: args.target === 'mock' ? 'loopback, Mock and Gateway share a process' : 'external; record deployment hardware separately', iterations_per_vu: args.iterations, concurrency: args.concurrency },
    limitations: ['本地 Mock 使用 release/test-support，不包含 PostgreSQL Usage 持久化、真实 Provider 或生产网络开销。', '每个 case/并发组合使用独立 Mock/Gateway 进程和端口，两路径共享同一进程；各路径先执行 5 次未计入指标的预热；Direct/Gateway 按档位交替执行先后顺序。', 'TTFT/SSE 总时长使用客户端毫秒时钟；JSON 总时长采用 k6 HTTP duration（不含 DNS/连接建立）；吞吐分母为 k6 testRunDurationMs。', '本地 SSE 单路径达到 4,000 请求时，执行前等待 31 秒回收前序连接；此间隔不计入指标。', '短样本的 p95/p99 仅用于探索；负差值表示测量噪声/调度差异，不截为零。', '未达到请求数或完成率门限判 FAIL，工具失败不会记为 SKIPPED/PASS；失败后的更高并发标为 SKIPPED/PRIOR_FAILURE。'], rows: [],
  };
  let failed = false;
  try {
    for (const [levelIndex, concurrency] of args.concurrency.entries()) {
      for (const scenario of config.cases.filter(v => args.cases.includes(v.id))) {
        const row = { case_id: scenario.id, concurrency };
        if (failed) { report.rows.push({ ...row, result: 'SKIPPED', reason: 'PRIOR_FAILURE: earlier measurement failed; inspect its result first' }); continue; }
        if (args.target === 'mock') {
          const local = await startConformanceTarget({ release: true, timeout: 300_000 });
          target = { ...local, directUrl: local.mockUrl, directModel: local.model, directKey: '', gatewayKey: '' };
        }
        const order = levelIndex % 2 ? ['gateway', 'direct'] : ['direct', 'gateway'];
        for (const path of order) {
          // xk6-sse creates one transport per request. Large local runs must
          // let prior TCP TIME_WAIT sockets expire before using ephemeral ports.
          if (args.target === 'mock' && scenario.stream && concurrency * args.iterations >= 4000) {
            console.log('Local SSE connection cooldown: 31 seconds (excluded from measurements)');
            await delay(31_000);
          }
          const summaryFile = resolve(out, `${scenario.id}-${concurrency}-${path}.json`);
          console.log(`${scenario.id} concurrency=${concurrency} path=${path}`);
          const result = await k6(binary, {
            ...process.env, LOAD_URL: path === 'direct' ? target.directUrl : target.baseUrl,
            LOAD_MODEL: path === 'direct' ? target.directModel : target.model,
            LOAD_KEY: path === 'direct' ? target.directKey : target.gatewayKey,
            LOAD_PROTOCOL: scenario.protocol, LOAD_STREAM: String(scenario.stream), LOAD_CONCURRENCY: String(concurrency), LOAD_ITERATIONS: String(args.iterations), LOAD_SUMMARY: summaryFile,
          });
          try {
            row[path] = summarize(JSON.parse(readFileSync(summaryFile)), concurrency * args.iterations, scenario.stream);
            if (result.code !== 0) row[path].result = 'FAIL';
          } catch (_) {
            row[path] = { result: 'FAIL', reason: 'TOOL_ERROR: no valid summary', exit_code: result.code };
          }
          // Do not persist tool stderr: it may contain upstream URLs or service error details.
        }
        Object.assign(row, compare(row.direct, row.gateway));
        if (row.result !== 'PASS') failed = true;
        report.rows.push(row);
        if (args.target === 'mock') { await target.cleanup(); target = null; }
      }
    }
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    if (target) await target.cleanup();
    report.finished_at = new Date().toISOString();
    report.result = failed || report.rows.length === 0 ? 'FAIL' : 'PASS';
    writeFileSync(resolve(out, 'summary.json'), JSON.stringify(report, null, 2));
    writeFileSync(resolve(out, 'summary.md'), markdown(report));
    console.log(`Report: ${resolve(out, 'summary.md')}`);
  }
  if (failed) process.exitCode = 1;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main().catch(error => { console.error(error.message); process.exitCode = 2; });
