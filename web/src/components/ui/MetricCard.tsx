import { Paper, Text } from '@mantine/core';
import { useId, type ReactNode } from 'react';
import styles from './MetricCard.module.scss';

export function MetricCard({ label, value, exact, hint, tone, compact = false }: {
  label: string; value: string; exact?: string; hint?: ReactNode; tone?: 'success' | 'warning'; compact?: boolean;
}) {
  const labelId = useId();
  return <Paper component="section" aria-labelledby={labelId} withBorder className={styles.card} data-ui="metric-card" data-tone={tone} data-compact={compact}>
    <Text id={labelId} className={styles.label}>{label}</Text>
    <Text component="strong" className={styles.value} title={exact} aria-label={exact}>{value}</Text>
    {hint && <Text component="div" className={styles.hint}>{hint}</Text>}
  </Paper>;
}
