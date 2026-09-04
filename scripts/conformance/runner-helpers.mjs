/**
 * runner-helpers.mjs — Shared utilities for conformance runners.
 *
 * Provides: version loading, conformance-target lifecycle, env validation,
 *           report persistence, and case filtering.
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
export const REPO_ROOT = resolve(__dirname, '../..')
export const VERSIONS_PATH = resolve(REPO_ROOT, 'tests/tooling/versions.json')
export const CONFIG_PATH = resolve(REPO_ROOT, 'tests/conformance/config.json')
export const DEFAULT_REPORT_DIR = resolve(REPO_ROOT, 'target/test-reports/conformance')

// ── Version loading ─────────────────────────────────────────────────────────

export function loadVersions() {
  return JSON.parse(readFileSync(VERSIONS_PATH, 'utf8'))
}

export function loadConfig() {
  return JSON.parse(readFileSync(CONFIG_PATH, 'utf8'))
}

// ── Environment validation ──────────────────────────────────────────────────

export function requireEnv(name) {
  const value = process.env[name]
  if (!value) {
    console.error(`ERROR: Required environment variable ${name} is not set.`)
    process.exit(2)
  }
  return value
}

export function optionalEnv(name, fallback) {
  return process.env[name] || fallback
}

// ── Report directory ────────────────────────────────────────────────────────

export function ensureReportDir(subdir) {
  const dir = subdir
    ? resolve(DEFAULT_REPORT_DIR, subdir)
    : DEFAULT_REPORT_DIR
  mkdirSync(dir, { recursive: true })
  return dir
}

export function writeReport(dir, filename, data) {
  const path = resolve(dir, filename)
  writeFileSync(path, typeof data === 'string' ? data : JSON.stringify(data, null, 2))
  console.error(`  → ${path}`)
  return path
}

// ── Conformance target lifecycle ────────────────────────────────────────────

/**
 * Start the conformance-target cargo example and wait for readiness.
 *
 * Returns { process, baseUrl, model, cleanup }.
 * Call cleanup() to kill the process.
 */
export async function startConformanceTarget(opts = {}) {
  const cargoHome = opts.cargoHome || process.env.CARGO_HOME || '/tmp/my-ai-gateway-cargo'
  const timeout = opts.timeout || 120_000

  return new Promise((resolvePromise, reject) => {
    const env = { ...process.env, CARGO_HOME: cargoHome }
    const child = spawn(
      'cargo',
      ['run', '--example', 'conformance-target', '--features', 'test-support'],
      { cwd: REPO_ROOT, env, stdio: ['ignore', 'pipe', 'pipe'] },
    )

    let baseUrl = null
    let model = null
    let ready = false
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill('SIGTERM')
      reject(new Error(`conformance-target did not become ready within ${timeout}ms`))
    }, timeout)

    child.stdout.on('data', (chunk) => {
      const text = chunk.toString()
      for (const line of text.split('\n')) {
        const trimmed = line.trim()
        if (trimmed.startsWith('CONFORMANCE_GATEWAY_URL=')) {
          baseUrl = trimmed.split('=', 2)[1]
        }
        if (trimmed.startsWith('CONFORMANCE_MODEL=')) {
          model = trimmed.split('=', 2)[1]
        }
        if (trimmed === 'CONFORMANCE_READY=true') {
          ready = true
        }
      }
      if (ready && baseUrl && model) {
        clearTimeout(timer)
        resolvePromise({
          process: child,
          baseUrl,
          model,
          cleanup: () => {
            child.kill('SIGTERM')
            return new Promise((res) => child.on('exit', res))
          },
        })
      }
    })

    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })

    child.on('error', (err) => {
      clearTimeout(timer)
      reject(new Error(`Failed to start conformance-target: ${err.message}`))
    })

    child.on('exit', (code) => {
      if (!ready) {
        clearTimeout(timer)
        reject(
          new Error(
            `conformance-target exited with code ${code} before ready.\nstderr: ${stderr.slice(-2000)}`,
          ),
        )
      }
    })
  })
}

/**
 * Wait until the gateway HTTP endpoint is reachable.
 */
export async function waitForReady(baseUrl, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`${baseUrl}/v1/models`, {
        signal: AbortSignal.timeout(2000),
      })
      if (res.ok) return true
    } catch {
      // retry
    }
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`Gateway not reachable at ${baseUrl} within ${timeoutMs}ms`)
}

// ── Argument parsing ────────────────────────────────────────────────────────

export function parseArguments(argv) {
  const args = {
    target: 'local',
    specs: [],
    filter: null,
    dryRun: false,
  }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--target' && argv[i + 1]) {
      args.target = argv[++i]
    } else if (arg.startsWith('--target=')) {
      args.target = arg.split('=', 2)[1]
    } else if (arg === '--spec' && argv[i + 1]) {
      args.specs.push(argv[++i])
    } else if (arg.startsWith('--spec=')) {
      args.specs.push(arg.split('=', 2)[1])
    } else if (arg === '--filter' && argv[i + 1]) {
      args.filter = argv[++i]
    } else if (arg.startsWith('--filter=')) {
      args.filter = arg.split('=', 2)[1]
    } else if (arg === '--dry-run') {
      args.dryRun = true
    }
  }
  return args
}

// ── Child process runner with output capture ────────────────────────────────

/**
 * Run a command and capture stdout/stderr.
 * Returns { exitCode, stdout, stderr }.
 */
export function runCommand(cmd, args, opts = {}) {
  return new Promise((resolvePromise) => {
    const child = spawn(cmd, args, {
      cwd: opts.cwd || REPO_ROOT,
      env: { ...process.env, ...opts.env },
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: opts.timeout || 120_000,
    })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', (chunk) => { stdout += chunk.toString() })
    child.stderr.on('data', (chunk) => { stderr += chunk.toString() })
    child.on('error', (err) => {
      resolvePromise({ exitCode: 1, stdout, stderr: stderr + '\n' + err.message })
    })
    child.on('exit', (code) => {
      resolvePromise({ exitCode: code ?? 1, stdout, stderr })
    })
  })
}

// ── Metadata-only summary ───────────────────────────────────────────────────

export function printSummary(report) {
  const total = report.results.length
  const pass = report.results.filter((r) => r.result === 'PASS').length
  const fail = report.results.filter((r) => r.result === 'FAIL').length
  const unsupported = report.results.filter((r) => r.result === 'UNSUPPORTED').length
  const skip = report.results.filter((r) => r.result === 'SKIPPED').length
  console.error(`\n${report.tool} v${report.tool_version} — ${total} cases: ${pass} PASS, ${fail} FAIL, ${unsupported} UNSUPPORTED, ${skip} SKIPPED`)
  if (fail > 0) {
    for (const r of report.results.filter((r) => r.result === 'FAIL')) {
      console.error(`  ✗ ${r.case_id}: ${r.evidence?.notes || ''}`)
    }
  }
  return fail
}
