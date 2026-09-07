import type { PropsWithChildren } from 'react';
import styles from './ConsolePrimitives.module.scss';

export function TableScroll({ children, label }: PropsWithChildren<{ label: string }>) {
  return <div className={styles.tableScroll} role="region" aria-label={label} tabIndex={0}>{children}</div>;
}
