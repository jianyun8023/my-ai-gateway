import type { PropsWithChildren, ReactNode } from 'react';
import { IconCircleCheck, IconTriangleAlert } from './icons';
import styles from './ConsolePrimitives.module.scss';

export function Notice({ children, tone = 'danger', action }: PropsWithChildren<{ tone?: 'danger' | 'success'; action?: ReactNode }>) {
  return (
    <div className={styles.notice} data-tone={tone} role={tone === 'danger' ? 'alert' : 'status'}>
      {tone === 'danger' ? <IconTriangleAlert size={18} /> : <IconCircleCheck size={18} />}
      <div className={styles.noticeContent}>{children}</div>
      {action && <div className={styles.noticeAction}>{action}</div>}
    </div>
  );
}
