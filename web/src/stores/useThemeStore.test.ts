// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { STORAGE_KEY_THEME } from '@/utils/constants';
import { useThemeStore } from './useThemeStore';

describe('useThemeStore', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme-style');
    document.documentElement.removeAttribute('data-color-scheme');
    useThemeStore.setState({ mode: 'auto', style: 'utility', resolvedColorScheme: 'light' });
    vi.stubGlobal('matchMedia', () => ({ matches: false, media: '(prefers-color-scheme: dark)',
      addEventListener: () => {}, removeEventListener: () => {} }));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('applies and persists visual style separately from display mode', () => {
    useThemeStore.getState().setStyle('ocean');
    useThemeStore.getState().setMode('dark');

    expect(document.documentElement.getAttribute('data-theme-style')).toBe('ocean');
    expect(document.documentElement.getAttribute('data-color-scheme')).toBe('dark');
    expect(useThemeStore.getState().resolvedColorScheme).toBe('dark');
    const persisted = JSON.parse(localStorage.getItem(STORAGE_KEY_THEME)!);
    expect(persisted.state).toEqual({ mode: 'dark', style: 'ocean' });
  });

  it('tracks system changes only while display mode is auto', () => {
    let systemDark = true;
    let listener = () => {};
    const removeEventListener = vi.fn();
    vi.stubGlobal('matchMedia', () => ({ get matches() { return systemDark; }, media: '(prefers-color-scheme: dark)',
      addEventListener: (_type: string, next: () => void) => { listener = next; }, removeEventListener }));

    useThemeStore.getState().setMode('auto');
    const cleanup = useThemeStore.getState().initializeTheme();
    expect(useThemeStore.getState().resolvedColorScheme).toBe('dark');
    expect(document.documentElement.getAttribute('data-color-scheme')).toBe('dark');

    systemDark = false;
    listener();
    expect(useThemeStore.getState().resolvedColorScheme).toBe('light');

    useThemeStore.getState().setMode('dark');
    listener();
    expect(useThemeStore.getState().resolvedColorScheme).toBe('dark');
    cleanup();
    expect(removeEventListener).toHaveBeenCalledOnce();
  });
});
