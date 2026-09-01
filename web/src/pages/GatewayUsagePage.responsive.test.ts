import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(new URL('./GatewayUsagePage.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./GatewayUsagePage.module.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const appSource = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8');
const shellSource = readFileSync(new URL('../components/gateway/GatewayConsoleShell.tsx', import.meta.url), 'utf8');
const shellStyles = readFileSync(new URL('../components/gateway/GatewayConsoleShell.module.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const brandStyles = readFileSync(new URL('../styles/gateway-brand.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('GatewayUsagePage prototype adaptation', () => {
  it('uses the sidebar shell with flat navigation matching the prototype', () => {
    expect(shellSource).toContain('className={styles.sidebar}');
    expect(shellSource).toContain('navItemsById');
    expect(appSource).toContain("'总览'");
    expect(appSource).toContain("'用量分析'");
    expect(appSource).toContain("'来源管理'");
    expect(appSource).toContain("'模型与路由'");
    expect(appSource).toContain("'系统设置'");
    expect(pageSource).not.toMatch(/Round-Robin/);
  });

  it('implements the reviewed tablet, phone, and small-phone breakpoints', () => {
    expect(shellStyles).toContain('@media (max-width: 920px)');
    expect(shellStyles).toContain('@media (max-width: 600px)');
    expect(shellStyles).toContain('@media (max-width: 380px)');
    expect(shellStyles).toContain('@media (prefers-reduced-motion: reduce)');
    expect(shellSource).toContain('aria-label="打开导航"');
    expect(shellSource).toContain("e.key === 'Escape'");
    expect(shellSource).toContain("document.body.style.overflow = 'hidden'");
    expect(styles).toMatch(/\.eventTable \{ overflow-x: auto; \}/);
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
