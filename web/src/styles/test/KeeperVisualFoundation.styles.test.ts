import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const themes = readFileSync(new URL('../themes.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const reset = readFileSync(new URL('../reset.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('Keeper visual foundation', () => {
  it('defines the global card, typography, and control scale', () => {
    expect(themes).toContain('--keeper-card-radius: 24px;');
    expect(themes).toContain('--keeper-card-padding: 20px;');
    expect(themes).toContain('--keeper-card-title-size: 18px;');
    expect(themes).toContain('--keeper-card-subtitle-size: 12px;');
    expect(themes).toContain('--keeper-card-subtitle-weight: 400;');
    expect(themes).toContain('--keeper-body-font-size: 12px;');
    expect(themes).toContain('--keeper-control-font-size: 12px;');
    expect(themes).toContain('--keeper-control-height-sm: 32px;');
    expect(themes).toContain('--keeper-control-height-md: 36px;');
    expect(themes).toContain('--keeper-toolbar-control-height: 42px;');
    expect(reset).toMatch(/body\s*\{[\s\S]*?font-size:\s*var\(--keeper-body-font-size\);/);
  });

});
