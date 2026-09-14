// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import { UsageBadge } from './UsageBadge';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

describe('UsageBadge', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it('shows the short label and explains the parsed SSE semantics on hover', async () => {
    await act(async () => root.render(<UsageBadge source="parsed" />));
    expect(container.querySelector('[data-ui="status-pill"]')?.textContent).toBe('上游流式');
    // floating-ui registers native mouseenter/mousemove on the reference element.
    act(() => { const target = container.querySelector('span')!; target.dispatchEvent(new MouseEvent('mouseenter')); target.dispatchEvent(new MouseEvent('mousemove')); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 60)); });
    const tooltip = document.querySelector('[role="tooltip"]');
    expect(tooltip?.textContent).toContain('上游返回（流式响应）');
    expect(tooltip?.textContent).toContain('上游 SSE 流式响应');
    expect(tooltip?.textContent).toContain('不是本地估算');
  });

  it('keeps estimated distinct from upstream-reported sources', async () => {
    await act(async () => root.render(<UsageBadge source="estimated" />));
    expect(container.querySelector('[data-ui="status-pill"]')?.textContent).toBe('估算');
    act(() => { const target = container.querySelector('span')!; target.dispatchEvent(new MouseEvent('mouseenter')); target.dispatchEvent(new MouseEvent('mousemove')); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 60)); });
    const tooltip = document.querySelector('[role="tooltip"]');
    expect(tooltip?.textContent).toContain('本地估算');
    expect(tooltip?.textContent).toContain('上游未报告');
  });
});
