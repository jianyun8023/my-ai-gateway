import { describe, expect, it } from 'vitest'
import config from '../vite.config.js'

function resolveConfigWithEnv({ proxyTarget, host } = {}) {
  const previousProxyTarget = process.env.VITE_API_PROXY_TARGET
  const previousHost = process.env.VITE_DEV_HOST
  if (proxyTarget === undefined) {
    delete process.env.VITE_API_PROXY_TARGET
  } else {
    process.env.VITE_API_PROXY_TARGET = proxyTarget
  }
  if (host === undefined) {
    delete process.env.VITE_DEV_HOST
  } else {
    process.env.VITE_DEV_HOST = host
  }

  try {
    return typeof config === 'function'
      ? config({ command: 'serve', mode: 'development', isSsrBuild: false, isPreview: false })
      : config
  } finally {
    if (previousProxyTarget === undefined) {
      delete process.env.VITE_API_PROXY_TARGET
    } else {
      process.env.VITE_API_PROXY_TARGET = previousProxyTarget
    }
    if (previousHost === undefined) {
      delete process.env.VITE_DEV_HOST
    } else {
      process.env.VITE_DEV_HOST = previousHost
    }
  }
}

function resolveBuildConfig() {
  return typeof config === 'function'
    ? config({ command: 'build', mode: 'production', isSsrBuild: false, isPreview: false })
    : config
}

describe('vite dev server proxy', () => {
  it('proxies every gateway Admin API request to the local backend by default', () => {
    const resolved = resolveConfigWithEnv()

    expect(resolved.server?.host).toBe('127.0.0.1')
    expect(resolved.server?.proxy?.['/admin']?.target).toBe('http://127.0.0.1:8787')
    expect(resolved.server?.proxy?.['/admin']?.changeOrigin).toBe(true)
    expect(resolved.server?.proxy?.['/api']).toBeUndefined()
  })

  it('allows overriding the backend proxy target', () => {
    const resolved = resolveConfigWithEnv({ proxyTarget: 'http://127.0.0.1:9090' })

    expect(resolved.server?.proxy?.['/admin']?.target).toBe('http://127.0.0.1:9090')
  })

  it('allows exposing the development server on the LAN', () => {
    const resolved = resolveConfigWithEnv({ host: '0.0.0.0' })

    expect(resolved.server?.host).toBe('0.0.0.0')
  })

  it('does not add the dev proxy to production build config', () => {
    const resolved = resolveBuildConfig()

    expect(resolved.server?.proxy).toBeUndefined()
  })
})
