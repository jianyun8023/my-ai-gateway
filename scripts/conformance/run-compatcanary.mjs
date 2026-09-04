#!/usr/bin/env node
/**
 * run-compatcanary.mjs — Run CompatCanary probes against local Gateway.
 *
 * Usage:
 *   node scripts/conformance/run-compatcanary.mjs [--target local|external]
 *       [--profile chat|modern]
 *
 * Default: starts local conformance-target, runs both profiles.
 * Versions read from tests/tooling/versions.json (single source of truth).
 * Results: target/test-reports/conformance/compatcanary-*.json
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
import { normalizeCompatcanary } from './normalize-report.mjs'

const ALL_PROFILES = ['chat', 'modern']

function parseLocalArgs(argv) {
  const base = parseArguments(argv)
  const profiles = []
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--profile' && argv[i + 1]) {
      profiles.push(argv[++i])
    } else if (argv[i].startsWith('--profile=')) {
      profiles.push(argv[i].split('=', 2)[1])
    }
  }
  return { ...base, profiles: profiles.length > 0 ? profiles : ALL_PROFILES }
}

async function main() {
  const args = parseLocalArgs(process.argv.slice(2))
  const versions = loadVersions()
  const config = loadConfig()
  const version = versions.compatcanary
  const reportDir = ensureReportDir('compatcanary')

  console.error(`=== CompatCanary v${version} probe scan ===`)
  console.error(`  Profiles: ${args.profiles.join(', ')}`)
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

  // CompatCanary follows the OpenAI base-URL convention: it appends paths like
  // `/chat/completions` directly, so the URL must include the `/v1` prefix.
  const ccBaseUrl = baseUrl.endsWith('/v1')
    ? baseUrl
    : `${baseUrl.replace(/\/+$/, '')}/v1`

  let totalFail = 0

  try {
    for (const profile of args.profiles) {
      console.error(`\n--- compatcanary --profile ${profile} ---`)

      const npxArgs = [
        `compatcanary@${version}`,
        '--base-url', ccBaseUrl,
        '--api-key', apiKey,
        '--model', model,
        '--profile', profile,
        '--format', 'json',
      ]

      if (args.dryRun) {
        console.error(`  [dry-run] npx ${npxArgs.join(' ')}`)
        continue
      }

      const result = await runCommand('npx', npxArgs, {
        env: { NODE_NO_WARNINGS: '1' },
        timeout: 120_000,
      })

      // Preserve raw report regardless of exit code
      const rawFilename = `compatcanary-${profile}-raw.json`
      let rawReport = null

      try {
        rawReport = JSON.parse(result.stdout)
      } catch {
        rawReport = {
          probes: [],
          _raw_stdout: result.stdout.slice(0, 5000),
          _parse_error: 'Could not parse CompatCanary JSON output',
        }
      }

      writeReport(reportDir, rawFilename, rawReport)

      if (result.stderr) {
        writeReport(reportDir, `compatcanary-${profile}-stderr.txt`, result.stderr)
      }

      // Normalize to unified schema
      const normalized = normalizeCompatcanary(rawReport)
      writeReport(reportDir, `compatcanary-${profile}-normalized.json`, normalized)

      const failCount = printSummary(normalized)
      totalFail += failCount

      if (result.exitCode !== 0) {
        console.error(`  compatcanary exited with code ${result.exitCode}`)
        if (failCount === 0) totalFail += 1
      }
    }

    // Also generate markdown report if both profiles ran
    if (args.profiles.length > 1 && !args.dryRun) {
      for (const profile of args.profiles) {
        const mdArgs = [
          `compatcanary@${version}`,
          '--base-url', ccBaseUrl,
          '--api-key', apiKey,
          '--model', model,
          '--profile', profile,
          '--format', 'markdown',
        ]
        const mdResult = await runCommand('npx', mdArgs, {
          env: { NODE_NO_WARNINGS: '1' },
          timeout: 120_000,
        })
        if (mdResult.stdout) {
          writeReport(reportDir, `compatcanary-${profile}.md`, mdResult.stdout)
        }
      }
    }
  } finally {
    if (cleanup) {
      console.error('\n  Stopping conformance-target…')
      await cleanup()
    }
  }

  console.error(`\n=== CompatCanary scan complete (${totalFail} failures) ===`)
  process.exit(totalFail > 0 ? 1 : 0)
}

main().catch((err) => {
  console.error(`FATAL: ${err.message}`)
  process.exit(2)
})
