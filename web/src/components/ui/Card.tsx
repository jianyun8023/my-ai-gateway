import { Paper, Text, Title } from '@mantine/core';
import { type PropsWithChildren, type ReactNode, type HTMLAttributes } from 'react';
import styles from './Card.module.scss';

interface CardProps extends Omit<HTMLAttributes<HTMLDivElement>, 'title'> {
  title?: ReactNode;
  subtitle?: ReactNode;
  titleMeta?: ReactNode;
  extra?: ReactNode;
  variant?: 'default' | 'flush';
  className?: string;
}

export function Card({ title, subtitle, titleMeta, extra, variant = 'default', children, className = '', ...props }: PropsWithChildren<CardProps>) {
  const hasHeading = title || subtitle || titleMeta;
  return <Paper {...props} withBorder radius="var(--keeper-card-radius)" className={`${styles.card} ${className}`} data-ui="card" data-variant={variant}>
    {(hasHeading || extra) && <div className={styles.header}>
      {hasHeading && <div className={styles.heading}>
        {(title || titleMeta) && <div className={styles.titleTrack}>
          {title && <Title order={3} className={styles.title}>{title}</Title>}
          {titleMeta && <div className={styles.titleMeta}>{titleMeta}</div>}
        </div>}
        {subtitle && <Text component="p" className={styles.subtitle}>{subtitle}</Text>}
      </div>}
      {extra && <div className={styles.actions}>{extra}</div>}
    </div>}
    {children}
  </Paper>;
}
