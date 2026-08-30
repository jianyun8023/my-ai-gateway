import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./useThemeStore.ts', import.meta.url), 'utf8');

describe('useThemeStore source contract', () => {
  it('defaults new users to auto theme and persists theme selection', () => {
    expect(source).toContain("theme: 'auto'");
    expect(source).toContain('persist(');
    expect(source).toContain('name: STORAGE_KEY_THEME');
    expect(source).toContain('setTheme: (theme) =>');
  });
});
