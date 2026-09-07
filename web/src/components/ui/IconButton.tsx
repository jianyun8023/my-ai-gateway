import type { PropsWithChildren, ButtonHTMLAttributes } from 'react';
import styles from './ConsolePrimitives.module.scss';

export function IconButton({
  label,
  className = '',
  children,
  ...props
}: PropsWithChildren<ButtonHTMLAttributes<HTMLButtonElement> & { label: string }>) {
  return (
    <button
      type="button"
      data-ui="icon-button"
      className={`${styles.iconButton} ${className}`.trim()}
      aria-label={label}
      title={label}
      {...props}
    >
      {children}
    </button>
  );
}
