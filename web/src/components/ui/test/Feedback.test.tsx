// @vitest-environment happy-dom
import { act, useState } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Notice } from '../Notice';
import { LoadingState } from '../LoadingState';
import { EmptyState } from '../EmptyState';
import { StatusPill } from '../StatusPill';
import { Button } from '../Button';

describe('console feedback', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it('keeps retry and dismissal available with error and success live-region semantics', () => {
    function Feedback() {
      const [retried, setRetried] = useState(false);
      const [dismissed, setDismissed] = useState(false);
      if (dismissed) return null;
      return retried ? <Notice tone="success" action={<Button onClick={() => setDismissed(true)}>Dismiss</Button>}>Updated</Notice>
        : <Notice action={<Button onClick={() => setRetried(true)}>Retry</Button>}><strong>Failed to load</strong><code>upstream_timeout</code></Notice>;
    }
    act(() => root.render(<Feedback />));
    const alert = container.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain('upstream_timeout');
    expect(document.getElementById(alert.getAttribute('aria-describedby')!)?.textContent).toContain('Failed to load');
    act(() => container.querySelector('button')!.click());
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(container.querySelector('[role="status"]')?.textContent).toContain('Updated');
    act(() => container.querySelector('button')!.click());
    expect(container.querySelector('[role="status"]')).toBeNull();
  });

  it('announces loading once and replaces it with an actionable empty state', () => {
    const onCreate = vi.fn();
    act(() => root.render(<LoadingState label="Loading attempts" layout="inline" />));
    expect(container.querySelectorAll('[role="status"]')).toHaveLength(1);
    expect(container.querySelector('[role="status"]')?.textContent).toBe('Loading attempts');
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull();
    act(() => root.render(<EmptyState title="No models" description="Add a source to discover models" layout="centered"
      action={<Button onClick={onCreate}>Add source</Button>} />));
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
    expect(container.textContent).toContain('Add a source to discover models');
    act(() => container.querySelector('button')!.click());
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it('preserves warning details and unknown state labels without turning badges into live regions', () => {
    act(() => root.render(<><Notice tone="warning"><strong>Discovery unsupported</strong><code>NoAuthenticatedCatalogEndpoint</code></Notice>
      <StatusPill>unknown</StatusPill><StatusPill tone="warning">ModelExtensionIdentifierWithOriginalCase</StatusPill></>));
    expect(container.querySelector('[role="status"]')?.textContent).toContain('NoAuthenticatedCatalogEndpoint');
    expect(container.querySelector('[role="alert"]')).toBeNull();
    const badges = [...container.querySelectorAll('[data-ui="status-pill"]')];
    expect(badges.map(badge => badge.textContent)).toEqual(['unknown', 'ModelExtensionIdentifierWithOriginalCase']);
    expect(badges.every(badge => !badge.hasAttribute('role'))).toBe(true);
  });
});
