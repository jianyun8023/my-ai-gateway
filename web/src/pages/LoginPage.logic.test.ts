import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { getLoginErrorForMode } from './LoginPage';

const source = readFileSync(new URL('./LoginPage.tsx', import.meta.url), 'utf8');
const stylesSource = readFileSync(new URL('./LoginPage.module.scss', import.meta.url), 'utf8');

describe('LoginPage mode-specific errors', () => {
  it('shows only the active login mode error', () => {
    expect(getLoginErrorForMode('admin', { adminError: 'bad password', apiKeyError: 'bad api key' })).toBe('bad password');
    expect(getLoginErrorForMode('api_key', { adminError: 'bad password', apiKeyError: 'bad api key' })).toBe('bad api key');
  });

  it('does not leak API Key failures onto the admin tab or admin failures onto the API Key tab', () => {
    expect(getLoginErrorForMode('admin', { adminError: '', apiKeyError: 'bad api key' })).toBe('');
    expect(getLoginErrorForMode('api_key', { adminError: 'bad password', apiKeyError: '' })).toBe('');
  });

  it('keeps the login hero concise and exposes theme switching', () => {
    expect(source).toContain('styles.themeSwitcher');
    expect(source).toContain('useThemeStore');
    expect(source).not.toContain('capabilityGrid');
    expect(source).not.toContain('capability_persistence');
  });

  it('fills the app main area instead of adding a second viewport height', () => {
    expect(stylesSource).toMatch(/\.pageShell\s*\{[\s\S]*?flex:\s*1\s+1\s+auto;/);
    expect(stylesSource).toMatch(/\.pageShell\s*\{[\s\S]*?min-height:\s*0;/);
    expect(stylesSource).not.toMatch(/\.pageShell\s*\{[\s\S]*?min-height:\s*100v?h;/);
  });
});
