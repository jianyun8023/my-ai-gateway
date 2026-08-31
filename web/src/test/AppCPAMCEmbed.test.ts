import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('gateway standalone shell', () => {
  it('does not mount CPAMC embed/session integration', () => {
    expect(appSource).not.toContain('cpamc');
    expect(appSource).not.toContain('embedSession');
    expect(appSource).not.toContain('loginWithCPAAPIKey');
  });
});
