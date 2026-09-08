import type { ReactNode } from 'react';
import { Button } from './Button';

interface FormActionsProps {
  form: string;
  cancelLabel: string;
  submitLabel: string;
  submitIcon?: ReactNode;
  busy?: boolean;
  onCancel: () => void;
}

export function FormActions({ form, cancelLabel, submitLabel, submitIcon, busy, onCancel }: FormActionsProps) {
  return <>
    <Button variant="secondary" disabled={busy} onClick={onCancel}>{cancelLabel}</Button>
    <Button type="submit" form={form} loading={busy}>{submitIcon}{submitLabel}</Button>
  </>;
}
