import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(new URL('./GatewayUsagePage.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./GatewayUsagePage.module.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const brandStyles = readFileSync(new URL('../styles/gateway-brand.scss', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const gatewayIcon = readFileSync(new URL('../assets/gateway-icon.svg', import.meta.url), 'utf8');

describe('GatewayUsagePage prototype adaptation', () => {
  it('uses the sidebar shell while keeping the first-release navigation boundary', () => {
    expect(pageSource).toContain('className={styles.sidebar}');
    expect(pageSource).toContain('GATEWAY_USAGE_TABS.map');
    expect(pageSource).not.toMatch(/来源管理|模型与路由|系统设置|Round-Robin/);
  });

  it('implements the reviewed tablet, phone, and small-phone breakpoints', () => {
    expect(styles).toContain('@media (max-width: 920px)');
    expect(styles).toContain('@media (max-width: 600px)');
    expect(styles).toContain('@media (max-width: 380px)');
    expect(pageSource).toContain('aria-label="打开导航"');
    expect(pageSource).toContain("event.key === 'Escape'");
    expect(pageSource).toContain("document.body.style.overflow = 'hidden'");
    expect(styles).toMatch(/\.eventTable \{ overflow-x: auto; \}/);
  });

  it('exposes the OKLCh brand tokens through a shared application stylesheet', () => {
    for (const token of ['--bg:', '--surface:', '--fg:', '--muted:', '--border:', '--accent:']) {
      expect(brandStyles).toContain(token);
    }
    expect(brandStyles).toContain('oklch(');
    expect(gatewayIcon).toContain('oklch(58% 0.16 145)');
    expect(gatewayIcon).not.toContain('linearGradient');
  });

  it('does not import or embed the archived static prototype', () => {
    expect(pageSource).not.toContain('ai-gateway-prototype.html');
    expect(pageSource).not.toContain('iframe');
  });
});
