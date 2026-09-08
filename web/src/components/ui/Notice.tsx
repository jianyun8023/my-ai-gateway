import { Alert } from '@mantine/core';
import type { PropsWithChildren, ReactNode } from 'react';
import { IconCircleCheck, IconTriangleAlert } from './icons';
import styles from './Feedback.module.scss';

export function Notice({ children, tone = 'danger', action }: PropsWithChildren<{ tone?: 'danger' | 'success' | 'warning'; action?: ReactNode }>) {
  return <Alert className={`${styles.tone} ${styles.notice}`} data-tone={tone}
    role={tone === 'danger' ? 'alert' : 'status'}
    icon={tone === 'success' ? <IconCircleCheck size={18} aria-hidden="true" /> : <IconTriangleAlert size={18} aria-hidden="true" />}
    classNames={{ body: styles.noticeBody, message: styles.noticeMessage, icon: styles.noticeIcon }}
    vars={() => ({ root: { '--alert-bg': 'var(--status-bg)', '--alert-color': 'var(--status-color)',
      '--alert-bd': '1px solid color-mix(in oklch, var(--status-color) 30%, var(--border))' } })}
  >
    <div className={styles.noticeContent}>{children}</div>
    {action && <div className={styles.noticeAction}>{action}</div>}
  </Alert>;
}
