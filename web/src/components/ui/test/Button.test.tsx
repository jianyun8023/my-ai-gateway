import { resolve } from 'node:path';
import React, { type ComponentType } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compile } from 'sass';
import { describe, expect, it } from 'vitest';
import { Button } from '../Button';

const componentsCSS = compile(resolve(process.cwd(), 'src/styles/components.scss')).css;

type ActionButtonProbeProps = React.ComponentProps<typeof Button> & {
  appearance?: 'action';
};

const ActionButtonProbe = Button as ComponentType<ActionButtonProbeProps>;

describe('Button', () => {
  it('exposes the shared action appearance without changing its semantic variant', () => {
    const primary = renderToStaticMarkup(<ActionButtonProbe appearance="action">Save</ActionButtonProbe>);
    const danger = renderToStaticMarkup(
      <ActionButtonProbe appearance="action" variant="danger">Delete</ActionButtonProbe>,
    );

    expect(primary).toContain('class="btn btn-primary btn-action"');
    expect(danger).toContain('class="btn btn-danger btn-action"');
  });

  it('keeps action buttons on the established compact pill contract', () => {
    expect(componentsCSS).toMatch(
      /\.btn-action \{[^}]*min-height: 32px;[^}]*border-radius: 999px;[^}]*padding: 7px 12px;[^}]*font-size: 12px;/,
    );
    expect(componentsCSS).toMatch(
      /\.btn-action\.btn-secondary, \.btn-action\.btn-ghost \{\s*box-shadow: 0 8px 20px rgba\(0, 0, 0, 0\.08\);\s*\}/,
    );
    expect(componentsCSS).toMatch(
      /\.btn-action\.btn-danger \{\s*box-shadow: none;\s*\}/,
    );
    expect(componentsCSS).toMatch(/\.btn\.btn-secondary \{[^}]*background-color: var\(--bg-tertiary\);/);
    expect(componentsCSS).toMatch(/\.btn\.btn-danger \{[^}]*background-color: var\(--danger-color\);/);
  });
});
