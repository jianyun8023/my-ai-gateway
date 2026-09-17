import { createTheme, type CSSVariablesResolver } from '@mantine/core';
import controls from './Controls.module.scss';
import feedback from './Feedback.module.scss';
import table from './Table.module.scss';
import navigation from './Navigation.module.scss';
import notifications from './Notifications.module.scss';
import accordion from './Accordion.module.scss';

// Colors and font families resolve from gateway-brand.scss on :root, including Portals.
export const consoleTheme = createTheme({
  primaryColor: 'green',
  fontFamily: 'var(--font-body)',
  fontFamilyMonospace: 'var(--font-mono)',
  headings: { fontFamily: 'var(--font-display)' },
  defaultRadius: 'md',
  fontSizes: { xs: 'var(--fs-small)', sm: 'var(--fs-meta)', md: 'var(--fs-body)', lg: 'var(--fs-h3)', xl: 'var(--fs-h2)' },
  respectReducedMotion: true,
  spacing: { xs: 'var(--gap-xs)', sm: 'var(--gap-sm)', md: 'var(--gap-md)', lg: 'var(--gap-lg)', xl: 'var(--gap-xl)' },
  radius: { xs: 'var(--radius-xs)', sm: 'var(--radius-sm)', md: 'var(--radius)', lg: 'var(--radius-lg)', xl: 'var(--radius-xl)' },
  breakpoints: { xs: '23.75em', sm: '37.5em', md: '57.5em', lg: '75em', xl: '90em' },
  components: {
    Accordion: {
      defaultProps: { variant: 'contained', radius: 'md' },
      classNames: { item: accordion.item, control: accordion.control, label: accordion.label, panel: accordion.panel },
    },
    Notification: { classNames: { root: notifications.notification, description: notifications.description, closeButton: notifications.closeButton } },
    NavLink: { classNames: { root: navigation.link, label: navigation.label, section: navigation.section } },
    Progress: { styles: { root: { background: 'var(--fg-soft)' } } },
    Table: {
      defaultProps: { horizontalSpacing: 13, verticalSpacing: 10 },
      classNames: { table: table.table, thead: table.thead, th: table.th, td: table.td, tr: table.tr },
    },
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
    Combobox: {
      defaultProps: { withinPortal: true, zIndex: 1200, middlewares: { flip: true, shift: true } },
      classNames: { dropdown: controls.selectDropdown, option: controls.selectOption },
    },
    Select: {
      defaultProps: {
        comboboxProps: {
          withinPortal: true,
          zIndex: 1200,
          middlewares: { flip: true, shift: true },
          transitionProps: { duration: 120 },
        },
      },
      classNames: { dropdown: controls.selectDropdown, option: controls.selectOption },
    },
    Switch: { defaultProps: { color: 'var(--accent)' }, classNames: { input: controls.switchInput } },
    Checkbox: { defaultProps: { color: 'var(--accent)', iconColor: 'var(--primary-contrast)' } },
    Tooltip: { defaultProps: { withinPortal: true, zIndex: 1300, multiline: true, maw: 320 } },
  },
});

export const overlayDefaults = {
  zIndex: 1000,
  transitionProps: { duration: 180, timingFunction: 'cubic-bezier(0.23, 1, 0.32, 1)' },
  overlayProps: { backgroundOpacity: 0.4, color: '#000' },
} as const;

export const drawerTransition = {
  duration: 180,
  timingFunction: 'cubic-bezier(0.32, 0.72, 0, 1)',
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
