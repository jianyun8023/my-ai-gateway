import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(new URL('./GatewayUsagePage.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('../features/usage/Usage.module.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const appSource = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8');
const shellSource = readFileSync(new URL('../components/gateway/GatewayConsoleShell.tsx', import.meta.url), 'utf8');
const shellStyles = readFileSync(new URL('../components/gateway/GatewayConsoleShell.module.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const brandStyles = readFileSync(new URL('../styles/gateway-brand.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('GatewayUsagePage prototype adaptation', () => {
  it('uses the sidebar shell with flat navigation matching the prototype', () => {
    expect(shellSource).toContain('className={styles.sidebar}');
    expect(shellSource).toContain('navItemsById');
    // 导航标签已迁移到 console i18n(shell.nav.*),按当前语言渲染全部六个扁平页面。
    for (const pageId of ['overview', 'analysis', 'events', 'sources', 'models', 'settings']) {
      expect(appSource).toContain(`t('shell.nav.${pageId}')`);
    }
    expect(pageSource).not.toMatch(/Round-Robin/);
  });

  it('implements the reviewed tablet, phone, and small-phone breakpoints', () => {
    expect(shellStyles).toContain('@media (max-width: 920px)');
    expect(shellStyles).toContain('@media (max-width: 600px)');
    expect(shellStyles).toContain('@media (max-width: 380px)');
    expect(shellSource).toContain('aria-controls="gateway-navigation"');
    expect(styles).toMatch(/\.statsGrid\s*\{\s*min-width: 0;/);
    expect(styles).toMatch(/\.chartGrid\s*\{\s*min-width: 0;/);
  });

  it('exposes the OKLCh brand tokens through a shared application stylesheet', () => {
    for (const token of ['--bg:', '--surface:', '--fg:', '--muted:', '--border:', '--accent:']) {
      expect(brandStyles).toContain(token);
    }
    expect(brandStyles).toContain('oklch(');
  });

  it('does not import or embed the archived static prototype', () => {
    expect(pageSource).not.toContain('ai-gateway-prototype.html');
    expect(pageSource).not.toContain('iframe');
    expect(shellSource).not.toContain('ai-gateway-prototype.html');
    expect(appSource).not.toContain('iframe');
  });
});
