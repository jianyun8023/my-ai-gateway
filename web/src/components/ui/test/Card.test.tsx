// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Card } from '../Card';
import { Button } from '../Button';

describe('Card composition', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it('preserves headings, metadata, actions and caller attributes around table content', () => {
    const onAction = vi.fn();
    act(() => root.render(<Card title="Models" subtitle="Confirmed catalog" titleMeta={<span>3 items</span>}
      extra={<Button onClick={onAction}>Refresh</Button>} variant="flush" id="catalog-card" aria-label="Catalog"
    ><table><tbody><tr><td>model-a</td></tr></tbody></table></Card>));
    expect(container.querySelector('#catalog-card')?.getAttribute('aria-label')).toBe('Catalog');
    expect(container.querySelector('h3')?.textContent).toBe('Models');
    expect(container.querySelector('p')?.textContent).toBe('Confirmed catalog');
    expect(container.textContent).toContain('3 items');
    expect(container.querySelector('td')?.textContent).toBe('model-a');
    act(() => container.querySelector('button')!.click());
    expect(onAction).toHaveBeenCalledOnce();
  });

  it('supports body-only cards without adding an empty heading', () => {
    act(() => root.render(<Card>Body</Card>));
    expect(container.querySelector('h3')).toBeNull();
    expect(container.querySelector('[data-ui="card"]')?.textContent).toBe('Body');
  });
});
