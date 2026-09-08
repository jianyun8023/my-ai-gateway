// @vitest-environment happy-dom
import { act, useState } from 'react';
import { createRoot } from '@/test/render';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import { Modal } from '../Modal';
import { useThemeStore } from '@/stores/useThemeStore';

const escape = () => (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

describe('console overlay composition', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(async () => {
    await setTestLanguage('zh');
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
  });

  it('labels dialog and drawer content and preserves external form submission', async () => {
    const submit = vi.fn((event: React.FormEvent) => event.preventDefault());
    await act(async () => root.render(<Modal open title="编辑来源" variant="drawer" onClose={() => {}}
      footer={<button type="submit" form="source-form">保存</button>}>
      <form id="source-form" onSubmit={submit}><label>名称<input defaultValue="source-a" /></label></form>
    </Modal>));
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(document.getElementById(dialog.getAttribute('aria-labelledby')!)?.textContent).toBe('编辑来源');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    act(() => dialog.querySelector<HTMLButtonElement>('button[form="source-form"]')!.click());
    expect(submit).toHaveBeenCalledOnce();
  });

  it('closes only the last opened overlay across dialog/drawer types', async () => {
    const parentClose = vi.fn();
    const childClose = vi.fn();
    function Flow() {
      const [child, setChild] = useState(false);
      return <Modal open title="详情" variant="drawer" onClose={parentClose}>
        <button onClick={() => setChild(true)}>确认</button>
        <Modal open={child} title="危险操作" onClose={() => { childClose(); setChild(false); }}>说明</Modal>
      </Modal>;
    }
    await act(async () => root.render(<Flow />));
    act(() => [...container.querySelectorAll('button')].find(b => b.textContent === '确认')!.click());
    const dialogs = [...document.querySelectorAll<HTMLElement>('[role="dialog"]')];
    dialogs.at(-1)!.querySelector<HTMLElement>('button')!.focus();
    act(escape);
    expect(childClose).toHaveBeenCalledOnce();
    expect(parentClose).not.toHaveBeenCalled();
    dialogs[0].querySelector<HTMLElement>('button')!.focus();
    act(escape);
    expect(parentClose).toHaveBeenCalledOnce();
  });

  it('keeps busy confirmation open for Escape, close button and backdrop', async () => {
    const onClose = vi.fn();
    await act(async () => root.render(<Modal open title="提交中" closeDisabled onClose={onClose}>保存中</Modal>));
    const close = container.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!;
    expect(close.disabled).toBe(true);
    act(() => { close.click(); escape(); container.querySelector<HTMLElement>('.mantine-Modal-overlay')?.click(); });
    expect(onClose).not.toHaveBeenCalled();
  });

  it('unregisters a conditionally unmounted drawer before opening another dialog', async () => {
    const close = vi.fn();
    await act(async () => root.render(<Modal key="detail" open variant="drawer" title="来源详情" onClose={() => {}}>详情</Modal>));
    await act(async () => root.render(<Modal key="editor" open title="编辑来源" onClose={close}>表单</Modal>));
    container.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!.focus();
    act(escape);
    expect(close).toHaveBeenCalledOnce();
  });

  it('applies the existing theme state to Mantine while an overlay is open', async () => {
    await act(async () => root.render(<Modal open title="主题" onClose={() => {}}>内容</Modal>));
    act(() => useThemeStore.getState().setTheme('dark'));
    expect(document.documentElement.getAttribute('data-mantine-color-scheme')).toBe('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    act(() => useThemeStore.getState().setTheme('white'));
    expect(document.documentElement.getAttribute('data-mantine-color-scheme')).toBe('light');
  });

  it('returns focus to the trigger after a conditionally mounted detail drawer closes', async () => {
    function Detail({ onExit }: { onExit: () => void }) {
      const [open, setOpen] = useState(true);
      return <Modal open={open} variant="drawer" title="详情" onClose={() => setOpen(false)} onExitTransitionEnd={onExit}>内容</Modal>;
    }
    function Page() {
      const [selected, setSelected] = useState(false);
      return <><button onClick={() => setSelected(true)}>查看</button>{selected && <Detail onExit={() => setSelected(false)} />}</>;
    }
    await act(async () => root.render(<Page />));
    const trigger = container.querySelector<HTMLButtonElement>('button')!;
    await act(async () => { trigger.focus(); trigger.click(); });
    const close = container.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!;
    act(() => { close.focus(); close.click(); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
    expect(document.activeElement).toBe(trigger);
  });
});
