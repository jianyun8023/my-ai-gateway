import type { PropsWithChildren } from 'react';
import styles from './DetailList.module.scss';

export function DetailList({ children, layout = 'rows' }: PropsWithChildren<{ layout?: 'rows' | 'grid' }>) {
  return <dl className={`${styles.list} ${styles[layout]}`}>{children}</dl>;
}

export function DetailItem({ label, children }: PropsWithChildren<{ label: string }>) {
  return <div><dt>{label}</dt><dd>{children}</dd></div>;
}
