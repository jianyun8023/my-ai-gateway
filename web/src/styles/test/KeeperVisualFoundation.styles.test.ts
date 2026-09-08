import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const brand = readFileSync(new URL('../gateway-brand.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const reset = readFileSync(new URL('../reset.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('Keeper visual foundation', () => {
  it('defines the global card, typography, and control scale', () => {
    expect(brand).toContain('--keeper-card-radius: var(--radius-lg);');
    expect(brand).toContain('--keeper-card-padding: 20px;');
    expect(brand).toContain('--keeper-card-title-size: var(--fs-h3);');
    expect(brand).toContain('--keeper-card-subtitle-size: var(--fs-meta);');
    expect(brand).toContain('--keeper-card-subtitle-weight: 400;');
    expect(brand).toContain('--keeper-body-font-size: 12px;');
    expect(brand).toContain('--keeper-control-font-size: 12px;');
    expect(brand).toContain('--keeper-control-height-sm: 32px;');
    expect(brand).toContain('--keeper-control-height-md: 36px;');
    expect(reset).toMatch(/body\s*\{[\s\S]*?font-size:\s*var\(--keeper-body-font-size\);/);
  });

});
