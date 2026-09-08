import { Paper, Text, ThemeIcon } from '@mantine/core';
import { type ReactNode } from 'react';
import { IconInbox } from './icons';
import styles from './Feedback.module.scss';

interface EmptyStateProps {
  title: string;
  description?: string;
  action?: ReactNode;
  layout?: 'inline' | 'centered';
}

export function EmptyState({ title, description, action, layout = 'inline' }: EmptyStateProps) {
  return <Paper withBorder radius="lg" className={styles.empty} data-layout={layout}>
    <div className={styles.emptyContent}>
      <ThemeIcon size={42} radius="xl" variant="outline" color="var(--muted)" className={styles.emptyIcon} aria-hidden="true"><IconInbox size={20} /></ThemeIcon>
      <div>
        <Text component="div" className={styles.emptyTitle}>{title}</Text>
        {description && <Text component="div" className={styles.emptyDescription}>{description}</Text>}
      </div>
    </div>
    {action && <div className={styles.emptyAction}>{action}</div>}
  </Paper>;
}
