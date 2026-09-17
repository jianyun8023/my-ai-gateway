import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { shell as enShell } from '@/i18n/console/en/shell';
import { shell as zhShell } from '@/i18n/console/zh/shell';
import { STORAGE_KEY_THEME } from '@/utils/constants';
import { THEME_MODES, THEME_STYLES, type ThemeStyle } from '@/stores/useThemeStore';

const brandStyles = readFileSync(new URL('./gateway-brand.scss', import.meta.url), 'utf8');
const switcherStyles = readFileSync(new URL('../components/ui/ThemeSwitcher.module.scss', import.meta.url), 'utf8');
const indexHtml = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
const mainSource = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8');

const paletteTokens = [
  '--bg', '--surface', '--fg', '--muted', '--border',
  '--accent', '--success', '--warn', '--danger',
] as const;
const radiusTokens = ['--radius-xs', '--radius-sm', '--radius', '--radius-lg', '--radius-xl'] as const;

const themeBlock = (style: ThemeStyle, mode: 'light' | 'dark') => {
  const selectorPattern = mode === 'dark'
    ? `:root\\[data-theme-style='${style}'\\]\\[data-color-scheme='dark'\\]`
    : style === 'utility'
      ? ":root,\\s*:root\\[data-theme-style='utility'\\]"
      : `:root\\[data-theme-style='${style}'\\]`;
  const match = brandStyles.match(new RegExp(`${selectorPattern}\\s*\\{([^}]*)\\}`));
  expect(match, `${style} ${mode} token block`).not.toBeNull();
  return match![1];
};

const inlineRegistry = (source: string, name: string) => {
  const match = source.match(new RegExp(`const ${name} = \\[([^\\]]*)\\]`));
  expect(match, `${name} pre-paint registry`).not.toBeNull();
  return Array.from(match![1].matchAll(/'([^']+)'/g), (entry) => entry[1]);
};

describe('theme contract', () => {
  it.each(THEME_STYLES)('%s defines complete light and dark semantic palettes', (style) => {
    const light = themeBlock(style, 'light');
    const dark = themeBlock(style, 'dark');

    for (const token of paletteTokens) {
      expect(light, `${style} light ${token}`).toContain(`${token}:`);
      expect(dark, `${style} dark ${token}`).toContain(`${token}:`);
    }
    for (const token of radiusTokens) {
      expect(light, `${style} ${token}`).toContain(`${token}:`);
    }
  });

  it.each(THEME_STYLES)('%s has localized picker copy and a preview', (style) => {
    expect(zhShell.appearance.styles[style].label).not.toBe('');
    expect(zhShell.appearance.styles[style].description).not.toBe('');
    expect(enShell.appearance.styles[style].label).not.toBe('');
    expect(enShell.appearance.styles[style].description).not.toBe('');

    if (style === 'utility') {
      expect(switcherStyles).toContain('.preview {');
    } else {
      expect(switcherStyles).toContain(`[data-theme-option='${style}'] .preview`);
    }
  });

  it.each(THEME_MODES)('%s has localized display mode copy', (mode) => {
    expect(zhShell.appearance.modes[mode]).not.toBe('');
    expect(enShell.appearance.modes[mode]).not.toBe('');
  });

  it('keeps the pre-paint bootstrap aligned with the theme registry', () => {
    const head = indexHtml.slice(0, indexHtml.indexOf('</head>'));
    expect(head).toContain(STORAGE_KEY_THEME);
    expect(inlineRegistry(head, 'styles')).toEqual([...THEME_STYLES]);
    expect(inlineRegistry(head, 'modes')).toEqual([...THEME_MODES]);
    expect(head).toContain('document.documentElement.dataset.themeStyle = style');
    expect(head).toContain('document.documentElement.dataset.colorScheme = colorScheme');
  });

  it('synchronizes the store before mounting React', () => {
    const applyIndex = mainSource.indexOf('applyThemeBeforeRender();');
    const renderIndex = mainSource.indexOf("createRoot(document.getElementById('root')!)");
    expect(applyIndex).toBeGreaterThan(-1);
    expect(renderIndex).toBeGreaterThan(applyIndex);
  });
});
