/** Gateway console appearance state. */

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { STORAGE_KEY_THEME } from '@/utils/constants';

export const THEME_MODES = ['auto', 'light', 'dark'] as const;
export const THEME_STYLES = ['utility', 'ocean', 'nebula', 'sandstone'] as const;

export type ThemeMode = (typeof THEME_MODES)[number];
export type ThemeStyle = (typeof THEME_STYLES)[number];
export type ResolvedColorScheme = Exclude<ThemeMode, 'auto'>;

interface ThemeState {
  mode: ThemeMode;
  style: ThemeStyle;
  resolvedColorScheme: ResolvedColorScheme;
  setMode: (mode: ThemeMode) => void;
  setStyle: (style: ThemeStyle) => void;
  initializeTheme: () => () => void;
}

const getSystemColorScheme = (): ResolvedColorScheme => {
  if (window.matchMedia?.('(prefers-color-scheme: dark)').matches) return 'dark';
  return 'light';
};

const resolveColorScheme = (mode: ThemeMode): ResolvedColorScheme => (
  mode === 'auto' ? getSystemColorScheme() : mode
);

const applyAppearance = (style: ThemeStyle, colorScheme: ResolvedColorScheme) => {
  document.documentElement.setAttribute('data-theme-style', style);
  document.documentElement.setAttribute('data-color-scheme', colorScheme);
};

export const useThemeStore = create<ThemeState>()(
  persist(
    (set, get) => ({
      mode: 'auto',
      style: 'utility',
      resolvedColorScheme: 'light',

      setMode: (mode) => {
        const resolvedColorScheme = resolveColorScheme(mode);
        applyAppearance(get().style, resolvedColorScheme);
        set({ mode, resolvedColorScheme });
      },

      setStyle: (style) => {
        applyAppearance(style, get().resolvedColorScheme);
        set({ style });
      },

      initializeTheme: () => {
        const { mode, style } = get();
        const resolvedColorScheme = resolveColorScheme(mode);
        applyAppearance(style, resolvedColorScheme);
        set({ resolvedColorScheme });

        if (!window.matchMedia) return () => {};
        const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
        const listener = () => {
          const state = get();
          if (state.mode !== 'auto') return;
          const nextColorScheme = getSystemColorScheme();
          applyAppearance(state.style, nextColorScheme);
          set({ resolvedColorScheme: nextColorScheme });
        };

        mediaQuery.addEventListener('change', listener);
        return () => mediaQuery.removeEventListener('change', listener);
      },
    }),
    {
      name: STORAGE_KEY_THEME,
      partialize: ({ mode, style }) => ({ mode, style }),
    },
  ),
);
