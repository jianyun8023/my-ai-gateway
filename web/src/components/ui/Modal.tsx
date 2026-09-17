import { useContext, useEffect, useEffectEvent, useId, type PropsWithChildren, type ReactNode } from 'react';
import { Drawer, Modal as MantineModal, ModalStackContext } from '@mantine/core';
import { useFocusReturn, useMediaQuery, useMounted } from '@mantine/hooks';
import { useTranslation } from 'react-i18next';
import { drawerTransition, overlayDefaults } from './theme';
import { IconX } from './icons';
import styles from './Overlay.module.scss';

interface ModalProps {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  onExitTransitionEnd?: () => void;
  footer?: ReactNode;
  width?: number | string;
  className?: string;
  closeDisabled?: boolean;
  variant?: 'dialog' | 'drawer';
  position?: 'left' | 'right';
}

export function Modal({ open, title, onClose, onExitTransitionEnd, footer, width = 520, className,
  closeDisabled = false, variant = 'dialog', position = 'right', children }: PropsWithChildren<ModalProps>) {
  const { t } = useTranslation('console');
  const stackId = useId();
  const stack = useContext(ModalStackContext);
  const mobile = useMediaQuery('(max-width: 600px)');
  const mounted = useMounted();
  const opened = mounted && open;
  // Keep Mantine's focus-return lifecycle independent of stack trapFocus changes.
  // The initial closed render also supports conditionally mounted detail drawers.
  useFocusReturn({ opened });
  // Detail components can unmount on navigation or switch directly to an editor.
  // Mantine handles opened changes; this also removes conditionally unmounted entries.
  const unregister = useEffectEvent(() => stack?.removeModal(stackId));
  useEffect(() => () => unregister(), []);
  const props = {
    ...overlayDefaults,
    transitionProps: variant === 'drawer' ? drawerTransition : overlayDefaults.transitionProps,
    opened,
    returnFocus: false,
    inert: !opened,
    stackId,
    title,
    onClose: () => { if (!closeDisabled) onClose(); },
    onExitTransitionEnd,
    closeButtonProps: { 'aria-label': t('common.close'), disabled: closeDisabled, size: mobile ? 44 : 32, icon: <IconX size={20} /> },
    closeOnEscape: !closeDisabled,
    closeOnClickOutside: !closeDisabled,
    size: mobile && variant === 'drawer' ? '100%' : width,
    classNames: { content: `${styles.content} ${className ?? ''}`, body: styles.body, header: styles.header, title: styles.title },
    children: <><div className={styles.scrollArea}>{children}</div>{footer && <div className={styles.footer}>{footer}</div>}</>,
  };
  return variant === 'drawer' ? <Drawer {...props} position={position} /> : <MantineModal {...props} centered />;
}
