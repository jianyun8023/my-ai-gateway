import {
  useId,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type PropsWithChildren,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import type { AdminErrorShape, GatewayProtocol } from '@/admin-api';
import { Button } from '@/components/ui/Button';
import { LoadingSpinner } from '@/components/ui/LoadingSpinner';
import { Modal } from '@/components/ui/Modal';
import {
  IconCircleCheck,
  IconRefreshCw,
  IconTriangleAlert,
} from '@/components/ui/icons';
import styles from './ControlPlane.module.scss';

export const PROTOCOL_LABELS: Record<GatewayProtocol, string> = {
  openai_chat_completions: 'Chat Completions',
  openai_responses: 'Responses',
  anthropic_messages: 'Messages',
};

export const formatDateTime = (value?: string | null): string => {
  if (!value) return '—';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  }).format(parsed);
};

export const formatJsonValue = (value: unknown): string => {
  if (value === undefined || value === null) return '—';
  if (typeof value === 'string') return value;
  return JSON.stringify(value);
};

export type StatusTone = 'success' | 'warning' | 'danger' | 'accent' | 'muted';

export function StatusPill({
  children,
  tone = 'muted',
  className = '',
}: PropsWithChildren<{ tone?: StatusTone; className?: string }>) {
  return <span className={`${styles.pill} ${className}`} data-tone={tone}>{children}</span>;
}

export function ProtocolPill({ protocol }: { protocol: GatewayProtocol }) {
  return <StatusPill>{PROTOCOL_LABELS[protocol]}</StatusPill>;
}

export function IconButton({
  label,
  className = '',
  children,
  ...props
}: PropsWithChildren<ButtonHTMLAttributes<HTMLButtonElement> & { label: string }>) {
  return (
    <button
      type="button"
      className={`${styles.iconButton} ${className}`.trim()}
      aria-label={label}
      title={label}
      {...props}
    >
      {children}
    </button>
  );
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
  return (
    <label className={styles.toggle} aria-label={label}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span aria-hidden="true"><i /></span>
      <em>{checked ? 'Enabled' : 'Disabled'}</em>
    </label>
  );
}

export function SegmentedTabs<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: ReadonlyArray<{ value: T; label: string; count?: number }>;
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className={styles.segmented} role="tablist" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="tab"
          aria-selected={value === option.value}
          data-active={value === option.value}
          onClick={() => onChange(option.value)}
        >
          <span>{option.label}</span>
          {option.count !== undefined && <small>{option.count}</small>}
        </button>
      ))}
    </div>
  );
}

export function PageActions({ children }: PropsWithChildren) {
  return <div className={styles.pageActions}>{children}</div>;
}

export function FilterBar({ children }: PropsWithChildren) {
  return <div className={styles.filterBar}>{children}</div>;
}

export function TableScroll({ children, label }: PropsWithChildren<{ label: string }>) {
  return <div className={styles.tableScroll} role="region" aria-label={label} tabIndex={0}>{children}</div>;
}

export function LoadingState({ label = '正在加载…' }: { label?: string }) {
  return (
    <div className={styles.loadingState} role="status" aria-live="polite" aria-busy="true">
      <LoadingSpinner size={22} />
      <span>{label}</span>
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: AdminErrorShape; onRetry?: () => void }) {
  const unauthorized = error.status === 401 || error.code === 'unauthorized';
  return (
    <div className={styles.errorState} role="alert" data-unauthorized={unauthorized}>
      <IconTriangleAlert size={18} />
      <div>
        <strong>{unauthorized ? 'Admin Key 未通过验证' : 'Admin API 请求失败'}</strong>
        <span>{error.message}</span>
        {error.code && <code>{error.code}</code>}
      </div>
      {onRetry && (
        <Button size="sm" variant="secondary" onClick={onRetry}>
          <IconRefreshCw size={14} />重试
        </Button>
      )}
    </div>
  );
}

export function SuccessNotice({ message, onDismiss }: { message?: string; onDismiss?: () => void }) {
  if (!message) return null;
  return (
    <div className={styles.successNotice} role="status" aria-live="polite">
      <IconCircleCheck size={17} />
      <span>{message}</span>
      {onDismiss && <button type="button" onClick={onDismiss} aria-label="关闭成功提示">关闭</button>}
    </div>
  );
}

interface FieldBaseProps {
  label: string;
  hint?: string;
  error?: string;
  className?: string;
}

export function TextField({
  label,
  hint,
  error,
  className = '',
  id,
  ...props
}: FieldBaseProps & InputHTMLAttributes<HTMLInputElement>) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  return (
    <div className={`${styles.field} ${className}`.trim()}>
      <label htmlFor={fieldId}>{label}</label>
      <input id={fieldId} aria-invalid={Boolean(error)} {...props} />
      {hint && <small>{hint}</small>}
      {error && <span role="alert">{error}</span>}
    </div>
  );
}

export function SelectField({
  label,
  hint,
  error,
  className = '',
  id,
  children,
  ...props
}: PropsWithChildren<FieldBaseProps & SelectHTMLAttributes<HTMLSelectElement>>) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  return (
    <div className={`${styles.field} ${className}`.trim()}>
      <label htmlFor={fieldId}>{label}</label>
      <select id={fieldId} aria-invalid={Boolean(error)} {...props}>{children}</select>
      {hint && <small>{hint}</small>}
      {error && <span role="alert">{error}</span>}
    </div>
  );
}

export function TextAreaField({
  label,
  hint,
  error,
  className = '',
  id,
  ...props
}: FieldBaseProps & TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  return (
    <div className={`${styles.field} ${className}`.trim()}>
      <label htmlFor={fieldId}>{label}</label>
      <textarea id={fieldId} aria-invalid={Boolean(error)} {...props} />
      {hint && <small>{hint}</small>}
      {error && <span role="alert">{error}</span>}
    </div>
  );
}

export function CheckboxField({
  checked,
  onChange,
  label,
  hint,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <label className={styles.checkboxField}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span><strong>{label}</strong>{hint && <small>{hint}</small>}</span>
    </label>
  );
}

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
  return <div className={styles.emptyTable}><strong>{title}</strong>{description && <span>{description}</span>}</div>;
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
}: {
  open: boolean;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal
      open={open}
      title={title}
      onClose={onCancel}
      closeDisabled={busy}
      width={480}
      footer={(
        <>
          <Button variant="secondary" onClick={onCancel} disabled={busy}>取消</Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm} loading={busy}>{confirmLabel}</Button>
        </>
      )}
    >
      <div className={styles.confirmBody}>{description}</div>
    </Modal>
  );
}

export function RefreshingBar({ visible }: { visible: boolean }) {
  return <div className={styles.refreshingBar} data-visible={visible} aria-hidden={!visible} />;
}
