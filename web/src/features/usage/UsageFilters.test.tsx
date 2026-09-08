// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultFilters, resolveFilterWindow } from '@/gateway-usage/filterState';
import { setTestLanguage } from '@/test/setup';
import { FilterBar } from './UsageFilters';
import { useUsageFilters } from './useUsageFilters';

describe('usage time presets', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  function Probe() {
    const state = useUsageFilters();
    return <>
      <FilterBar draft={state.draft} onChange={state.setDraft} onApply={state.apply} onPresetSelect={state.selectPreset} onReset={state.reset} loading={false} />
      <output>{JSON.stringify(state.filters)}</output>
      {state.invalidRange && <p role="alert">Invalid range</p>}
    </>;
  }
  const click = (label: string) => act(() => [...container.querySelectorAll('button')].find(button => button.textContent === label)!.click());
  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('zh');
    localStorage.clear();
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => root.render(<Probe />));
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
  });

  it('defaults to today and applies yesterday and rolling 24 hours immediately', () => {
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('今天');
    click('昨天');
    const yesterday = JSON.parse(container.querySelector('output')!.textContent!);
    expect(yesterday).toMatchObject(resolveFilterWindow({ ...defaultFilters(), relativePreset: 'yesterday' }));
    click('最近 24 小时');
    const rolling = JSON.parse(container.querySelector('output')!.textContent!);
    expect(Date.parse(rolling.to) - Date.parse(rolling.from)).toBe(86_400_000);
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('最近 24 小时');
    click('重置');
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('今天');
  });

  it('keeps filtering usable when local storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('Blocked', 'SecurityError'); });
    click('昨天');
    expect(JSON.parse(container.querySelector('output')!.textContent!).relativePreset).toBe('yesterday');
  });

  it('allows clearing custom dates and reports the invalid range without crashing', () => {
    click('自定义');
    const input = container.querySelector<HTMLInputElement>('input[type="datetime-local"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    click('应用筛选');
    expect(container.querySelector('[role="alert"]')?.textContent).toBe('Invalid range');
    expect(JSON.parse(container.querySelector('output')!.textContent!).relativePreset).toBe('today');
  });
});
