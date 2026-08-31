import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(new URL('./App.tsx', import.meta.url), 'utf8');

describe('gateway-native App shell', () => {
  it('mounts only the gateway usage product surface', () => {
    expect(appSource).toContain("import { GatewayUsagePage } from './pages/GatewayUsagePage';");
    expect(appSource).toContain('<GatewayUsagePage />');
    expect(appSource).not.toContain('LoginPage');
    expect(appSource).not.toContain('KeyRankingPage');
    expect(appSource).not.toContain("from './pages/UsagePage'");
    expect(appSource).not.toContain('getSession');
  });
});
