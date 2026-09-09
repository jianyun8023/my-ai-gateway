// @vitest-environment happy-dom
import { act, useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot } from '@/test/render';
import { setTestLanguage } from '@/test/setup';
import { RemoteFilterField } from './RemoteFilterField';

type Result = { data: string[]; has_more: boolean };
type Load = (search: string, signal: AbortSignal) => Promise<Result>;

function Probe({ load, contextKey = '' }: { load: Load; contextKey?: string }) {
  const [value, setValue] = useState('');
  return <RemoteFilterField label="模型" value={value} onChange={setValue} loadOptions={load} contextKey={contextKey} />;
}

describe('remote filter candidates', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const input = () => container.querySelector('input')!;
  const type = (value: string) => act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input(), value);
    input().dispatchEvent(new Event('input', { bubbles: true }));
  });
  const debounce = () => act(async () => { await vi.advanceTimersByTimeAsync(250); });

  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('zh');
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
  });

  it('loads on demand, exposes truncation, selects by keyboard and clears the value', async () => {
    const load = vi.fn<Load>().mockResolvedValue({ data: ['historical-model', 'other-model'], has_more: true });
    act(() => root.render(<Probe load={load} />));
    expect(load).not.toHaveBeenCalled();
    act(() => input().click());
    expect(input().getAttribute('aria-busy')).toBe('true');
    await debounce();
    expect(load).toHaveBeenCalledWith('', expect.any(AbortSignal));
    expect(container.textContent).toContain('仅显示部分候选');
    act(() => {
      input().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', code: 'ArrowDown', bubbles: true }));
    });
    act(() => input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true })));
    expect(input().value).toBe('historical-model');
    expect(input().getAttribute('aria-expanded')).toBe('false');
    act(() => container.querySelector<HTMLButtonElement>('button[aria-label="清空模型"]')!.click());
    expect(input().value).toBe('');
  });

  it('debounces typing and drops responses after search, context and loader changes', async () => {
    const requests: { signal: AbortSignal; resolve: (result: Result) => void }[] = [];
    const load = vi.fn<Load>((_search, signal) => new Promise((resolve) => requests.push({ signal, resolve })));
    act(() => root.render(<Probe load={load} />));
    type('a');
    type('ab');
    await debounce();
    expect(load).toHaveBeenCalledTimes(1);
    expect(load.mock.calls[0][0]).toBe('ab');
    type('abc');
    expect(requests[0].signal.aborted).toBe(true);
    await debounce();
    await act(async () => requests[1].resolve({ data: ['abc-new'], has_more: false }));
    await act(async () => requests[0].resolve({ data: ['ab-stale'], has_more: false }));
    expect(container.textContent).toContain('abc-new');
    expect(container.textContent).not.toContain('ab-stale');
    act(() => root.render(<Probe load={load} contextKey="new-identity" />));
    expect(container.textContent).not.toContain('abc-new');
    await debounce();
    const replacement = vi.fn<Load>().mockResolvedValue({ data: [], has_more: false });
    act(() => root.render(<Probe load={replacement} contextKey="new-identity" />));
    expect(requests[2].signal.aborted).toBe(true);
    await act(async () => requests[2].resolve({ data: ['wrong-range'], has_more: false }));
    await debounce();
    expect(input().value).toBe('abc');
    expect(container.textContent).toContain('没有匹配候选');
    expect(container.textContent).not.toContain('wrong-range');
  });

  it('keeps manual input on failure, retries and cancels pending requests on unmount', async () => {
    const load = vi.fn<Load>().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ data: ['saved-value'], has_more: false });
    act(() => root.render(<Probe load={load} />));
    type('saved');
    await debounce();
    expect(input().value).toBe('saved');
    expect(container.textContent).toContain('候选加载失败');
    act(() => [...container.querySelectorAll('button')].find((button) => button.textContent === '重试')!.click());
    await debounce();
    expect(container.textContent).toContain('saved-value');
    expect(container.textContent).not.toContain('候选加载失败');
    let pendingSignal: AbortSignal | undefined;
    load.mockImplementation((_search, signal) => {
      pendingSignal = signal;
      return new Promise(() => {});
    });
    type('manual-value');
    await debounce();
    expect(input().value).toBe('manual-value');
    act(() => root.render(null));
    expect(pendingSignal?.aborted).toBe(true);
  });
});
