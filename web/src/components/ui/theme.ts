import { createTheme, type CSSVariablesResolver } from '@mantine/core';
import controls from './Controls.module.scss';
import feedback from './Feedback.module.scss';

// Colors and font families resolve from gateway-brand.scss on :root, including Portals.
export const consoleTheme = createTheme({
  primaryColor: 'green',
  fontFamily: 'var(--font-body)',
  fontFamilyMonospace: 'var(--font-mono)',
  headings: { fontFamily: 'var(--font-display)' },
  defaultRadius: 'md',
  fontSizes: { xs: 'var(--fs-small)', sm: 'var(--fs-meta)', md: 'var(--fs-body)', lg: 'var(--fs-h3)', xl: 'var(--fs-h2)' },
  respectReducedMotion: true,
  spacing: { xs: '4px', sm: '8px', md: '16px', lg: '24px', xl: '32px' },
  radius: { xs: '4px', sm: '6px', md: '8px', lg: '12px', xl: '16px' },
  breakpoints: { xs: '23.75em', sm: '37.5em', md: '57.5em', lg: '75em', xl: '90em' },
  components: {
    Loader: { defaultProps: { color: 'var(--accent)', type: 'oval' }, classNames: { root: feedback.loader } },
    Input: {
      defaultProps: { size: 'sm' },
      classNames: { input: controls.input },
    },
    InputWrapper: {
      defaultProps: { inputWrapperOrder: ['label', 'input', 'description', 'error'] },
      classNames: { root: controls.field, label: controls.label, description: controls.description, error: controls.error },
    },
    Modal: { defaultProps: { radius: 'lg', padding: 'md', shadow: 'lg' } },
    Drawer: { defaultProps: { padding: 'md', shadow: 'lg' } },
    Popover: {
      defaultProps: { withinPortal: true, shadow: 'md', radius: 'md', zIndex: 1200, returnFocus: true },
      styles: { dropdown: { background: 'var(--surface)', borderColor: 'var(--border)', color: 'var(--fg)' } },
    },
    Switch: { defaultProps: { color: 'var(--accent)' } },
    Checkbox: { defaultProps: { color: 'var(--accent)', iconColor: 'var(--primary-contrast)' } },
    Tooltip: { defaultProps: { withinPortal: true, zIndex: 1300, multiline: true, maw: 320 } },
    Select: { defaultProps: { comboboxProps: { withinPortal: true, zIndex: 1200 } } },
  },
});

export const overlayDefaults = {
  zIndex: 1000,
  transitionProps: { duration: 180 },
  overlayProps: { backgroundOpacity: 0.4, color: '#000' },
} as const;

const semanticVariables = {
  '--mantine-color-body': 'var(--surface)',
  '--mantine-color-text': 'var(--fg)',
  '--mantine-color-dimmed': 'var(--muted)',
  '--mantine-color-default': 'var(--surface)',
  '--mantine-color-default-border': 'var(--border)',
  '--mantine-color-default-hover': 'var(--fg-soft)',
  '--mantine-primary-color-filled': 'var(--accent)',
  '--mantine-primary-color-filled-hover': 'var(--primary-hover)',
  '--mantine-color-error': 'var(--danger)',
};

export const consoleCssVariables: CSSVariablesResolver = () => ({
  variables: {}, light: semanticVariables, dark: semanticVariables,
});
