// @vitest-environment happy-dom
import { act, useState } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SegmentedTabs } from '../SegmentedTabs';
import { TextField, SelectField, TextAreaField } from '../FormField';
import { Button } from '../Button';
import { LanguageSwitcher } from '../LanguageSwitcher';
import { IconButton } from '../IconButton';
import { setTestLanguage } from '@/test/setup';

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

  it('switches language with pressed state and persists the preference', async () => {
    await setTestLanguage('zh');
    act(() => root.render(<LanguageSwitcher />));
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('中文');
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'EN')!.click());
    expect(container.querySelector('[aria-pressed="true"]')?.textContent).toBe('EN');
    expect(localStorage.getItem('my-ai-gateway-language')).toBe('en');
    await act(async () => setTestLanguage('zh'));
  });

  it('associates labels, hints, errors and caller descriptions across all field types', () => {
    act(() => root.render(<><p id="external">Shared guidance</p>
      <TextField label="Name" hint="A name" error="Required" aria-describedby="external" aria-invalid={false} />
      <SelectField label="Protocol" hint="Choose one" error="Unavailable" aria-describedby="external" data={[{ value: 'chat', label: 'Chat' }]} />
      <TextAreaField label="Metadata" hint="JSON" error="Invalid JSON" aria-describedby="external" />
    </>));
    const controls = [...container.querySelectorAll<HTMLInputElement>('input[id], textarea[id]')];
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

  it('renders a themed combobox, ignores disabled options and closes with Escape', async () => {
    const selectKeyDown = vi.fn();
    const onChange = vi.fn();
    act(() => root.render(<SelectField
      label="Protocol"
      value="chat"
      data={[
        { value: 'chat', label: 'Chat' },
        { value: 'responses', label: 'Responses' },
        { value: 'unknown', label: 'Unknown', disabled: true },
      ]}
      onChange={onChange}
      onKeyDown={selectKeyDown}
    />));
    const select = container.querySelector<HTMLInputElement>('[role="combobox"]')!;
    await act(async () => select.click());
    expect(select.getAttribute('aria-expanded')).toBe('true');
    expect(select.getAttribute('data-mantine-stop-propagation')).toBe('true');
    const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')];
    expect(options).toHaveLength(3);
    expect(options[2].hasAttribute('data-combobox-disabled')).toBe(true);
    act(() => options[2].click());
    expect(onChange).not.toHaveBeenCalled();
    expect(select.getAttribute('aria-expanded')).toBe('true');
    act(() => select.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    expect(selectKeyDown).toHaveBeenCalledOnce();
    expect(select.getAttribute('aria-expanded')).toBe('false');
    expect(select.hasAttribute('data-mantine-stop-propagation')).toBe(false);
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

  it('keeps disabled icon help focusable while suppressing activation and row clicks', () => {
    const onClick = vi.fn();
    const onRowClick = vi.fn();
    act(() => root.render(<div onClick={onRowClick}><IconButton label="Unavailable action" disabled onClick={onClick}>×</IconButton></div>));
    const button = container.querySelector<HTMLButtonElement>('button')!;
    expect(button.disabled).toBe(false);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.hasAttribute('data-disabled')).toBe(true);
    act(() => { button.focus(); button.click(); });
    expect(document.activeElement).toBe(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(onRowClick).not.toHaveBeenCalled();
  });
});
