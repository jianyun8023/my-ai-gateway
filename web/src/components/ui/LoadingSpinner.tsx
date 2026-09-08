import { Loader } from '@mantine/core';

// The enclosing LoadingState supplies the live announcement.
export function LoadingSpinner({ size = 20, className = '' }: { size?: number; className?: string }) {
  return <Loader size={size} className={className} aria-hidden="true" />;
}
