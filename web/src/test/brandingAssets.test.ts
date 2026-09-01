import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const mainSource = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8');
const indexHtml = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
const attribution = readFileSync(new URL('../../THIRD_PARTY_NOTICES.md', import.meta.url), 'utf8');

describe('gateway branding and attribution', () => {
  it('uses the gateway SVG as the browser favicon', () => {
    expect(mainSource).toContain("import faviconUrl from './assets/gateway-icon.svg'");
    expect(mainSource).toContain("faviconEl.type = 'image/svg+xml'");
  });

  it('uses the gateway browser tab title', () => {
    expect(indexHtml).toContain('<title>My AI Gateway · 控制台</title>');
  });

  it('keeps the CPA Usage Keeper source and MIT attribution', () => {
    expect(attribution).toContain('https://github.com/Willxup/cpa-usage-keeper');
    expect(attribution).toContain('MIT License');
    expect(attribution).toContain('Copyright (c) 2026 Will');
  });
});
