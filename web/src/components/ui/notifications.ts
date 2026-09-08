import { notifications } from '@mantine/notifications';
import i18n from '@/i18n';

let operationId: string | undefined;

// A new operation replaces transient feedback; actionable errors stay in Notice.
export function clearOperationNotification() {
  if (operationId) notifications.hide(operationId);
  operationId = undefined;
}

export function notifySuccess(message: string) {
  clearOperationNotification();
  operationId = notifications.show({
    message,
    role: 'status',
    color: 'var(--success)',
    closeButtonProps: { 'aria-label': i18n.t('console:common.close_notice_aria') },
  });
}
