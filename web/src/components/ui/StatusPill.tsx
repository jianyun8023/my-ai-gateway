import type { PropsWithChildren } from 'react';
import styles from './ConsolePrimitives.module.scss';

export type StatusTone = 'success' | 'warning' | 'danger' | 'accent' | 'muted';

export function StatusPill({
  children,
  tone = 'muted',
  className = '',
}: PropsWithChildren<{ tone?: StatusTone; className?: string }>) {
  return <span className={`${styles.pill} ${className}`} data-ui="status-pill" data-tone={tone}>{children}</span>;
}
