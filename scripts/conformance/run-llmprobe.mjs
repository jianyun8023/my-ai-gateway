#!/usr/bin/env node
/**
 * run-llmprobe.mjs — Run llmprobe conformance scan against local Gateway.
 *
 * Usage:
 *   node scripts/conformance/run-llmprobe.mjs [--target local|external]
 *       [--spec chat-completions|responses|anthropic-messages] [--filter <pattern>]
 *
 * llmprobe probes every surface of an OpenAI-compatible endpoint in one run;
 * it has no per-spec or per-case CLI selection.  `--spec` / `--filter`
 * therefore filter the *normalized* results after the scan, they are not
 * passed to llmprobe.
 *
 * Default: starts local conformance-target, scans all surfaces once.
 * Versions read from tests/tooling/versions.json (single source of truth).
 * Results: target/test-reports/conformance/llmprobe-*.json
 *
 * Failure policy: a scan that produces zero usable cases is a tool failure
 * (exit 2), never a silent pass.
 */

import {
  loadVersions,
  loadConfig,
  startConformanceTarget,
  waitForReady,
  ensureReportDir,
  writeReport,
  parseArguments,
  runCommand,
  printSummary,
  optionalEnv,
  redactSensitiveArgs,
} from './runner-helpers.mjs'
import {
  normalizeLlmprobe,
  LLMPROPE_SPEC_SURFACES,
} from './normalize-report.mjs'

/**
 * Build the llmprobe CLI invocation.  Exported for unit tests.
 *
 * Real CLI (llmprobe 0.6.x): `llmprobe <base-url> -k <key> -m <model>
 * --quick --no-bench --json --no-save --no-color`.  `--quick` keeps the
 * scan to surface probe + core conformance; capability/agentic/eval phases
 * stay not-run (recorded in normalized report metadata).
 */
export function buildLlmprobeArgs({ version, baseUrl, apiKey, model }) {
  return [
    `llmprobe@${version}`,
    baseUrl,
    '--api-key', apiKey,
    '--model', model,
    '--quick',
    '--no-bench',
    '--json',
    '--no-save',
    '--no-color',
  ]
}

/** Filter normalized results by spec selection and case_id substring. */
export function filterNormalizedResults(report, specs, filter) {
  let results = report.results
  if (specs.length > 0) {
    const surfaces = new Set()
    for (const spec of specs) {
      for (const surface of LLMPROPE_SPEC_SURFACES[spec] || []) {
        surfaces.add(surface)
      }
    }
    results = results.filter((r) => surfaces.has(r.llmprobe_surface))
  }
  if (filter) {
    results = results.filter((r) => r.case_id.includes(filter))
  }
  return { ...report, results }
}

async function main() {
  const args = parseArguments(process.argv.slice(2))
  const versions = loadVersions()
  const config = loadConfig()
  const version = versions.llmprobe
  const reportDir = ensureReportDir('llmprobe')

  console.error(`=== llmprobe v${version} conformance scan ===`)
  console.error(`  Specs filter: ${args.specs.length > 0 ? args.specs.join(', ') : '(all surfaces)'}`)
  console.error(`  Target: ${args.target}`)

  // ── Resolve target ──────────────────────────────────────────────────
  let baseUrl, model, cleanup

  if (args.target === 'local') {
    console.error('  Starting conformance-target…')
    const target = await startConformanceTarget()
    baseUrl = target.baseUrl
    model = target.model
    cleanup = target.cleanup
    await waitForReady(baseUrl)
    console.error(`  Gateway ready at ${baseUrl}`)
  } else {
    baseUrl = optionalEnv(
      config.targets.external.base_url_env,
      'http://127.0.0.1:8787',
    )
    model = optionalEnv(config.targets.external.model_env, 'conformance-test-model')
    cleanup = null
  }

  const apiKey = optionalEnv(
    config.targets[args.target]?.api_key_env || 'CONFORMANCE_API_KEY',
    config.targets.local.api_key_default,
  )

  try {
    const npxArgs = buildLlmprobeArgs({ version, baseUrl, apiKey, model })

    if (args.dryRun) {
      console.error(`  [dry-run] npx ${redactSensitiveArgs(npxArgs).join(' ')}`)
      return
    }

    const result = await runCommand('npx', npxArgs, {
      env: { NODE_NO_WARNINGS: '1' },
      timeout: 300_000,
    })

    if (result.stderr) {
      writeReport(reportDir, 'llmprobe-stderr.txt', result.stderr)
    }

    // Preserve raw stdout regardless of exit code, then require valid JSON.
    let rawReport = null
    try {
      rawReport = JSON.parse(result.stdout)
    } catch {
      writeReport(reportDir, 'llmprobe-raw-stdout.txt', result.stdout.slice(0, 20000))
      console.error('FATAL: llmprobe did not produce JSON output on stdout.')
      console.error(`  exit code: ${result.exitCode}; raw stdout saved for inspection.`)
      process.exit(2)
    }
    writeReport(reportDir, 'llmprobe-raw.json', rawReport)

    const normalized = normalizeLlmprobe(rawReport)
    const filtered = filterNormalizedResults(normalized, args.specs, args.filter)
    writeReport(reportDir, 'llmprobe-normalized.json', filtered)

    // A scan with zero usable cases means the tool/runner integration is
    // broken (wrong CLI, unreachable target, …) — fail loudly.
    if (filtered.results.length === 0) {
      console.error('FATAL: llmprobe produced zero usable conformance cases.')
      console.error('  This is a tool failure, not a passing scan.')
      process.exit(2)
    }

    const failCount = printSummary(filtered)
    if (result.exitCode !== 0) {
      // llmprobe exits non-zero on any MUST failure, including ones we
      // normalize away as declared known gaps.  The normalized FAIL count is
      // the authority; the raw exit code is diagnostic only.
      console.error(`  (llmprobe raw exit code: ${result.exitCode}; normalized results are authoritative)`)
    }
    console.error(`\n=== llmprobe scan complete (${failCount} case failures) ===`)
    process.exit(failCount > 0 ? 1 : 0)
  } finally {
    if (cleanup) {
      console.error('\n  Stopping conformance-target…')
      await cleanup()
    }
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((err) => {
    console.error(`FATAL: ${err.message}`)
    process.exit(2)
  })
}
