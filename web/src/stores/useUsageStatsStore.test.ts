import { beforeEach, describe, expect, it, vi } from 'vitest'

const apiMocks = vi.hoisted(() => {
  class MockApiError extends Error {
    status: number

    constructor(status: number, message = 'api error') {
      super(message)
      this.status = status
    }
  }

  return {
    ApiError: MockApiError,
    fetchUsageOverview: vi.fn(),
    fetchUsageOverviewRealtime: vi.fn(),
  }
})

vi.mock('@/lib/api', () => apiMocks)

describe('useUsageStatsStore', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    const { useUsageStatsStore } = await import('./useUsageStatsStore')
    useUsageStatsStore.getState().clearUsageStats()
  })

  it('keeps overview and realtime loaders independent', async () => {
    const { useUsageStatsStore } = await import('./useUsageStatsStore')
    let resolveOverview: (value: unknown) => void = () => undefined
    apiMocks.fetchUsageOverview.mockReturnValue(new Promise((resolve) => {
      resolveOverview = resolve
    }))
    apiMocks.fetchUsageOverviewRealtime.mockResolvedValue({
      window: '30m',
      bucket_seconds: 60,
      token_velocity: [],
      response_level: [],
      current_usage: { models: [], api_keys: [], auth_files: [], ai_providers: [] },
      request_level: [],
      cache_level: [],
    })

    const overviewLoad = useUsageStatsStore.getState().loadUsageStats({
      force: true,
      range: '24h',
      apiKeyId: '9007199254740993',
    })
    const realtimeLoad = useUsageStatsStore.getState().loadUsageStatsRealtime({
      force: true,
      apiKeyId: '9007199254740993',
      realtimeWindow: '30m',
    })

    await Promise.resolve()

    expect(apiMocks.fetchUsageOverview).toHaveBeenCalledTimes(1)
    expect(apiMocks.fetchUsageOverviewRealtime).toHaveBeenCalledTimes(1)
    expect(apiMocks.fetchUsageOverviewRealtime).toHaveBeenCalledWith({
      signal: expect.any(AbortSignal),
      apiKeyId: '9007199254740993',
      window: '30m',
    })

    resolveOverview({
      usage: { total_requests: 1, success_count: 1, failure_count: 0, total_tokens: 20 },
    })
    await Promise.all([overviewLoad, realtimeLoad])

    const state = useUsageStatsStore.getState()
    expect('realtime' in (state.usage ?? {})).toBe(false)
    expect(state.realtime?.window).toBe('30m')
  })

  it('does not reload overview when only the realtime window changes', async () => {
    const { useUsageStatsStore } = await import('./useUsageStatsStore')
    apiMocks.fetchUsageOverview.mockResolvedValue({
      usage: { total_requests: 1, success_count: 1, failure_count: 0, total_tokens: 20 },
    })
    apiMocks.fetchUsageOverviewRealtime.mockResolvedValue({
      window: '15m',
      bucket_seconds: 60,
      token_velocity: [],
      response_level: [],
      current_usage: { models: [], api_keys: [], auth_files: [], ai_providers: [] },
      request_level: [],
      cache_level: [],
    })

    await useUsageStatsStore.getState().loadUsageStats({
      force: true,
      range: '24h',
      apiKeyId: '9007199254740993',
    })
    await useUsageStatsStore.getState().loadUsageStatsRealtime({
      force: true,
      apiKeyId: '9007199254740993',
      realtimeWindow: '60m',
    })

    expect(apiMocks.fetchUsageOverview).toHaveBeenCalledTimes(1)
    expect(apiMocks.fetchUsageOverviewRealtime).toHaveBeenCalledTimes(1)
    expect(apiMocks.fetchUsageOverviewRealtime).toHaveBeenCalledWith({
      signal: expect.any(AbortSignal),
      apiKeyId: '9007199254740993',
      window: '60m',
    })
  })

  it('does not keep an error from a different realtime query when cached data is fresh', async () => {
    const { useUsageStatsStore } = await import('./useUsageStatsStore')
    apiMocks.fetchUsageOverviewRealtime.mockResolvedValueOnce({
      window: '15m',
      bucket_seconds: 30,
      token_velocity: [],
      response_level: [],
      current_usage: { models: [], api_keys: [], auth_files: [], ai_providers: [] },
      request_level: [],
      cache_level: [],
    })

    await useUsageStatsStore.getState().loadUsageStatsRealtime({
      force: true,
      realtimeWindow: '15m',
    })

    apiMocks.fetchUsageOverviewRealtime.mockRejectedValueOnce(new Error('60m failed'))
    await useUsageStatsStore.getState().loadUsageStatsRealtime({
      force: true,
      realtimeWindow: '60m',
    })

    expect(useUsageStatsStore.getState().realtimeError).toBe('60m failed')

    await useUsageStatsStore.getState().loadUsageStatsRealtime({
      realtimeWindow: '15m',
    })

    const state = useUsageStatsStore.getState()
    expect(state.realtime?.window).toBe('15m')
    expect(state.realtimeError).toBe('')
  })
})
