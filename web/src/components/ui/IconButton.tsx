import { ActionIcon, Tooltip } from '@mantine/core';
import type { PropsWithChildren, ButtonHTMLAttributes } from 'react';
import styles from './Controls.module.scss';

export function IconButton({ label, className = '', children, ...props
}: PropsWithChildren<ButtonHTMLAttributes<HTMLButtonElement> & { label: string; loading?: boolean }>) {
  return <Tooltip label={label} events={{ hover: true, focus: true, touch: false }}>
    <ActionIcon type="button" data-ui="icon-button" variant="subtle"
      vars={() => ({ root: { '--ai-size': 'var(--console-icon-size)', '--ai-bg': 'transparent',
        '--ai-color': 'var(--muted)', '--ai-hover': 'var(--bg-hover)', '--ai-hover-color': 'var(--fg)' } })}
      className={[styles.iconButton, className].join(' ').trim()} aria-label={label} {...props}>
      {children}
    </ActionIcon>
  </Tooltip>;
}
