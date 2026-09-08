import { Paper } from '@mantine/core';
import type { PropsWithChildren } from 'react';
import styles from './FilterPanel.module.scss';

export function FilterPanel({ label, className, children }: PropsWithChildren<{ label: string; className?: string }>) {
  return <Paper component="section" withBorder radius={12} p={12} aria-label={label}
    className={[styles.panel, className].filter(Boolean).join(' ')}>{children}</Paper>;
}
