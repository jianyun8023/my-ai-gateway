import { Badge } from '@mantine/core';
import type { PropsWithChildren } from 'react';
import styles from './Feedback.module.scss';

export type StatusTone = 'success' | 'warning' | 'danger' | 'accent' | 'muted';

export function StatusPill({ children, tone = 'muted', className = '' }: PropsWithChildren<{ tone?: StatusTone; className?: string }>) {
  return <Badge component="span" className={`${styles.tone} ${styles.pill} ${className}`} classNames={{ label: styles.pillLabel }}
    data-ui="status-pill" data-tone={tone}
    vars={() => ({ root: { '--badge-bg': 'var(--status-bg)', '--badge-color': 'var(--status-color)', '--badge-bd': '0' } })}
  >{children}</Badge>;
}
