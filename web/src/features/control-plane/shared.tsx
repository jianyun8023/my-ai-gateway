import type { AdminErrorShape, GatewayProtocol } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import {
  IconRefreshCw,
} from '@/components/ui/icons';
import { Modal } from '@/components/ui/Modal';
import { Notice } from '@/components/ui/Notice';
import { StatusPill } from '@/components/ui/StatusPill';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import {
  type PropsWithChildren,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';

export function ProtocolPill({ protocol }: { protocol: GatewayProtocol }) {
  return <StatusPill>{PROTOCOL_LABELS[protocol]}</StatusPill>;
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  const { t } = useTranslation('console');
  return (
    <label className={styles.toggle} aria-label={label}>
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span aria-hidden="true"><i /></span>
      <em>{t(checked ? 'common.enabled' : 'common.disabled')}</em>
    </label>
  );
}

export function PageActions({ children }: PropsWithChildren) {
  return <div className={styles.pageActions}>{children}</div>;
}

export function FilterBar({ children }: PropsWithChildren) {
  return <div className={styles.filterBar}>{children}</div>;
}

export function ErrorState({ error, onRetry }: { error: AdminErrorShape; onRetry?: () => void }) {
  const { t } = useTranslation('console');
  const unauthorized = error.status === 401 || error.code === 'unauthorized';
  return (
    <Notice action={onRetry && <Button size="sm" variant="secondary" onClick={onRetry}><IconRefreshCw size={14} />{t('common.retry')}</Button>}>
      <strong>{unauthorized ? t('errors.admin_key_invalid') : t('errors.admin_api_failed')}</strong>
      {error.code && <code>{error.code}</code>}
    </Notice>
  );
}

export function SuccessNotice({ message, onDismiss }: { message?: string; onDismiss?: () => void }) {
  const { t } = useTranslation('console');
  if (!message) return null;
  return <Notice tone="success" action={onDismiss && <Button variant="ghost" size="sm" onClick={onDismiss} aria-label={t('common.close_notice_aria')}>{t('common.close')}</Button>}>{message}</Notice>;
}

export { CheckboxField } from '@/components/ui/CheckboxField';

export function FormGrid({ children }: PropsWithChildren) {
  return <div className={styles.formGrid}>{children}</div>;
}

export function FormError({ message }: { message?: string }) {
  if (!message) return null;
  return <div className={styles.formError} role="alert">{message}</div>;
}

export function DetailList({ children }: PropsWithChildren) {
  return <dl className={styles.detailList}>{children}</dl>;
}

export function DetailItem({ label, children }: PropsWithChildren<{ label: string }>) {
  return <div><dt>{label}</dt><dd>{children}</dd></div>;
}

export function DrawerSection({ title, children }: PropsWithChildren<{ title: string }>) {
  return <section className={styles.drawerSection}><h3>{title}</h3>{children}</section>;
}

export function EmptyTable({ title, description }: { title: string; description?: string }) {
  return <EmptyState title={title} description={description} layout="centered" />;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  busy,
  danger = false,
  onCancel,
  onConfirm,
  onExitTransitionEnd,
}: {
  open: boolean;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onExitTransitionEnd?: () => void;
}) {
  const { t } = useTranslation('console');
  return (
    <Modal
      open={open}
      onExitTransitionEnd={onExitTransitionEnd}
      title={title}
      onClose={onCancel}
      closeDisabled={busy}
      width={480}
      footer={(
        <>
          <Button variant="secondary" onClick={onCancel} disabled={busy}>{t('common.cancel')}</Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm} loading={busy}>{confirmLabel}</Button>
        </>
      )}
    >
      <div className={styles.confirmBody}>{description}</div>
    </Modal>
  );
}
