import { Button as MantineButton } from '@mantine/core';
import { type ButtonHTMLAttributes, type PropsWithChildren } from 'react';
import styles from './Controls.module.scss';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: 'md' | 'sm';
  appearance?: 'default' | 'action';
  fullWidth?: boolean;
  loading?: boolean;
}

const colors = {
  primary: ['var(--accent)', 'var(--primary-hover)', 'var(--primary-contrast)', 'var(--accent)'],
  secondary: ['var(--bg-tertiary)', 'var(--bg-hover)', 'var(--fg)', 'var(--border)'],
  ghost: ['transparent', 'var(--bg-hover)', 'var(--muted)', 'transparent'],
  danger: ['var(--danger)', 'color-mix(in srgb, var(--danger) 92%, #000)', '#fff', 'var(--danger)'],
};

export function Button({ children, variant = 'primary', size = 'md', appearance = 'default',
  fullWidth = false, loading = false, className = '', disabled, type = 'button', ...rest
}: PropsWithChildren<ButtonProps>) {
  const [background, hover, color, border] = colors[variant];
  return <MantineButton {...rest} type={type} data-ui="button" data-appearance={appearance}
    className={[styles.button, className].join(' ').trim()} classNames={{ label: styles.buttonLabel }}
    size={size} fullWidth={fullWidth} loading={loading} disabled={disabled || loading}
    aria-busy={loading || rest['aria-busy']}
    vars={() => ({ root: { '--button-bg': background, '--button-hover': hover,
      '--button-color': color, '--button-hover-color': color, '--button-bd': '1px solid ' + border } })}
  >{children}</MantineButton>;
}
