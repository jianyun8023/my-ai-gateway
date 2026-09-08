import { useCallback, useState } from 'react';

// Keep nonsensitive form/detail data until Mantine reports the exit complete.
// Secrets deliberately use ordinary state so closing clears them immediately.
export function useOverlayState<T>() {
  const [value, updateValue] = useState<T>();
  const [opened, setOpened] = useState(false);
  const setValue = useCallback((next: T | undefined) => {
    if (next !== undefined) updateValue(next);
    setOpened(next !== undefined);
  }, []);
  const afterExit = useCallback(() => {
    if (!opened) updateValue(undefined);
  }, [opened]);
  return { value, setValue, opened, afterExit };
}
