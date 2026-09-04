#!/usr/bin/env node
/**
 * run-llmprobe.mjs — Run llmprobe conformance scan against local Gateway.
 *
 * Usage:
 *   node scripts/conformance/run-llmprobe.mjs [--target local|external]
 *       [--spec chat-completions|responses|anthropic-messages] [--filter <pattern>]
 *
 * Default: starts local conformance-target, scans all three specs.
 * Versions read from tests/tooling/versions.json (single source of truth).
 * Results: target/test-reports/conformance/llmprobe-*.json
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
  REPO_ROOT,
} from './runner-helpers.mjs'
import { normalizeLlmprobe } from './normalize-report.mjs'

const ALL_SPECS = ['chat-completions', 'responses', 'anthropic-messages']

async function main() {
  const args = parseArguments(process.argv.slice(2))
  const versions = loadVersions()
  const config = loadConfig()
  const version = versions.llmprobe
  const specs = args.specs.length > 0 ? args.specs : ALL_SPECS
  const reportDir = ensureReportDir('llmprobe')

  console.error(`=== llmprobe v${version} conformance scan ===`)
  console.error(`  Specs: ${specs.join(', ')}`)
  console.error(`  Target: ${args.target}`)

  // ── Resolve target ──────────────────────────────────────────────────
  let baseUrl, model, cleanup

  if (args.target === 'local') {
    const targetConfig = config.targets.local
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

  let totalFail = 0

  try {
    for (const spec of specs) {
      console.error(`\n--- llmprobe --spec ${spec} ---`)

      const npxArgs = [
        `llmprobe@${version}`,
        '--base-url', baseUrl,
        '--api-key', apiKey,
        '--model', model,
        '--spec', spec,
        '--format', 'json',
      ]
      if (args.filter) {
        npxArgs.push('--filter', args.filter)
      }

      if (args.dryRun) {
        console.error(`  [dry-run] npx ${npxArgs.join(' ')}`)
        continue
      }

      const result = await runCommand('npx', npxArgs, {
        env: { NODE_NO_WARNINGS: '1' },
        timeout: 180_000,
      })

      // Preserve raw report regardless of exit code
      const rawFilename = `llmprobe-${spec}-raw.json`
      let rawReport = null

      try {
        rawReport = JSON.parse(result.stdout)
      } catch {
        rawReport = {
          tests: [],
          _raw_stdout: result.stdout.slice(0, 5000),
          _parse_error: 'Could not parse llmprobe JSON output',
        }
      }

      writeReport(reportDir, rawFilename, rawReport)

      if (result.stderr) {
        const stderrFile = `llmprobe-${spec}-stderr.txt`
        writeReport(reportDir, stderrFile, result.stderr)
      }

      // Normalize to unified schema
      const normalized = normalizeLlmprobe(rawReport, spec)
      writeReport(reportDir, `llmprobe-${spec}-normalized.json`, normalized)

      const failCount = printSummary(normalized)
      totalFail += failCount

      if (result.exitCode !== 0) {
        console.error(`  llmprobe exited with code ${result.exitCode}`)
        if (failCount === 0) totalFail += 1
      }
    }
  } finally {
    if (cleanup) {
      console.error('\n  Stopping conformance-target…')
      await cleanup()
    }
  }

  console.error(`\n=== llmprobe scan complete (${totalFail} failures) ===`)
  process.exit(totalFail > 0 ? 1 : 0)
}

main().catch((err) => {
  console.error(`FATAL: ${err.message}`)
  process.exit(2)
})
