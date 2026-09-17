import { useContext, useEffect, type PropsWithChildren } from 'react';
import { DrawerStackContext, MantineProvider, Modal, ModalStackContext } from '@mantine/core';
import { Notifications, notifications } from '@mantine/notifications';
import { useThemeStore } from '@/stores/useThemeStore';
import { consoleCssVariables, consoleTheme } from './theme';

// Mantine exposes separate stacks with the same contract. Sharing its stack
// makes a dialog above a drawer (or mobile navigation) the only focus/Esc owner.
function DrawerStackBridge({ children }: PropsWithChildren) {
  const stack = useContext(ModalStackContext);
  return <DrawerStackContext value={stack}>{children}</DrawerStackContext>;
}

export function ConsoleProvider({ children, env }: PropsWithChildren<{ env?: 'test' }>) {
  const colorScheme = useThemeStore((state) => state.resolvedColorScheme);
  useEffect(() => () => notifications.clean(), []);
  return (
    <MantineProvider theme={consoleTheme} cssVariablesResolver={consoleCssVariables} forceColorScheme={colorScheme} env={env}>
      <Modal.Stack><DrawerStackBridge>{children}</DrawerStackBridge></Modal.Stack>
      <Notifications position="top-right" limit={1} autoClose={5000} containerWidth={400}
        zIndex={1400} transitionDuration={env === 'test' ? 0 : 180}
        allowDragDismiss={false} allowScrollDismiss={false} />
    </MantineProvider>
  );
}
