import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(new URL('./App.tsx', import.meta.url), 'utf8');
const usagePageSource = readFileSync(new URL('./pages/GatewayUsagePage.tsx', import.meta.url), 'utf8');
const shellSource = readFileSync(new URL('./components/gateway/GatewayConsoleShell.tsx', import.meta.url), 'utf8');

describe('gateway-native App shell', () => {
  it('composes the gateway Usage and Management spaces without legacy Keeper routes', () => {
    expect(appSource).toContain("import('./pages/GatewayUsagePage')");
    expect(appSource).toContain('<GatewayConsoleShell');
    expect(appSource).toContain('<GatewayUsagePage');
    expect(appSource).toContain('<GatewayManagementPage');
    expect(appSource).not.toContain('LoginPage');
    expect(appSource).not.toContain('KeyRankingPage');
    expect(appSource).not.toContain("from './pages/UsagePage'");
    expect(appSource).not.toContain('getSession');
  });

  it('keeps the Admin key state in the shared shell only', () => {
    expect(shellSource).toContain('sessionStorage');
    expect(shellSource).toContain('GATEWAY_ADMIN_KEY_STORAGE_KEY');
    expect(usagePageSource).not.toContain('sessionStorage');
    expect(usagePageSource).not.toContain('Admin Key');
  });
});
