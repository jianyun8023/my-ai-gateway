// @vitest-environment happy-dom
import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SegmentedTabs } from '../SegmentedTabs';
import { TextField, SelectField, TextAreaField } from '../FormField';
import { Button } from '../Button';

describe('shared console interactions', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it('moves tab focus and selection with arrows, Home and End, keeping one tab stop', () => {
    function Tabs() {
      const [value, setValue] = useState('sources');
      return <><SegmentedTabs id="catalog" value={value} onChange={setValue} label="Catalog" options={[
        { value: 'sources', label: 'Sources', count: 0 }, { value: 'accounts', label: 'Accounts', count: 2 },
      ]} /><div id="catalog-panel" role="tabpanel" aria-labelledby={`catalog-${value}`}>{value}</div></>;
    }
    act(() => root.render(<Tabs />));
    const tabs = [...container.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
    const press = (key: string) => act(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true })));
    tabs[0].focus();
    press('ArrowLeft');
    expect(document.activeElement).toBe(tabs[1]);
    expect(tabs.map(tab => tab.tabIndex)).toEqual([-1, 0]);
    expect(container.querySelector('[role="tabpanel"]')?.textContent).toBe('accounts');
    press('ArrowRight');
    expect(document.activeElement).toBe(tabs[0]);
    press('End');
    expect(document.activeElement).toBe(tabs[1]);
    press('Home');
    expect(document.activeElement).toBe(tabs[0]);
    expect(document.getElementById(tabs[0].getAttribute('aria-controls')!)).not.toBeNull();
  });

  it('exposes filter selection as pressed buttons without tab panel semantics', () => {
    const onChange = vi.fn();
    act(() => root.render(<SegmentedTabs mode="group" value="auto" onChange={onChange} label="Granularity" options={[{ value: 'auto', label: 'Auto' }, { value: 'day', label: 'Day' }]} />));
    expect(container.querySelector('[role="tablist"]')).toBeNull();
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('Auto');
    act(() => container.querySelectorAll<HTMLButtonElement>('button')[1].click());
    expect(onChange).toHaveBeenCalledWith('day');
  });

  it('associates labels, hints, errors and caller descriptions across all field types', () => {
    act(() => root.render(<><p id="external">Shared guidance</p>
      <TextField label="Name" hint="A name" error="Required" aria-describedby="external" aria-invalid={false} />
      <SelectField label="Protocol" hint="Choose one" error="Unavailable" aria-describedby="external"><option>Chat</option></SelectField>
      <TextAreaField label="Metadata" hint="JSON" error="Invalid JSON" aria-describedby="external" />
    </>));
    const controls = [...container.querySelectorAll<HTMLInputElement>('input, select, textarea')];
    expect(new Set(controls.map(control => control.id)).size).toBe(3);
    for (const control of controls) {
      expect(container.querySelector(`label[for="${control.id}"]`)).not.toBeNull();
      expect(control.getAttribute('aria-invalid')).toBe('true');
      const ids = control.getAttribute('aria-describedby')!.split(' ');
      expect(ids).toContain('external');
      expect(ids).toHaveLength(3);
      expect(ids.every(id => document.getElementById(id))).toBe(true);
    }
  });

  it('does not submit forms by default and prevents busy button activation', () => {
    const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());
    const onBusyClick = vi.fn();
    act(() => root.render(<form onSubmit={onSubmit}><Button>Cancel</Button><Button type="submit">Save</Button><Button loading onClick={onBusyClick}>Saving</Button></form>));
    const buttons = [...container.querySelectorAll<HTMLButtonElement>('button')];
    act(() => buttons[0].click());
    expect(onSubmit).not.toHaveBeenCalled();
    act(() => buttons[1].click());
    expect(onSubmit).toHaveBeenCalledTimes(1);
    act(() => buttons[2].click());
    expect(onBusyClick).not.toHaveBeenCalled();
    expect(buttons[2].getAttribute('aria-busy')).toBe('true');
  });
});
