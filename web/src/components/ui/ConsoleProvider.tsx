import { useContext, type PropsWithChildren } from 'react';
import { DrawerStackContext, MantineProvider, Modal, ModalStackContext } from '@mantine/core';
import { useThemeStore } from '@/stores/useThemeStore';
import { consoleCssVariables, consoleTheme } from './theme';

// Mantine exposes separate stacks with the same contract. Sharing its stack
// makes a dialog above a drawer (or mobile navigation) the only focus/Esc owner.
function DrawerStackBridge({ children }: PropsWithChildren) {
  const stack = useContext(ModalStackContext);
  return <DrawerStackContext value={stack}>{children}</DrawerStackContext>;
}

export function ConsoleProvider({ children, env }: PropsWithChildren<{ env?: 'test' }>) {
  const colorScheme = useThemeStore((state) => state.resolvedTheme);
  return (
    <MantineProvider theme={consoleTheme} cssVariablesResolver={consoleCssVariables} forceColorScheme={colorScheme} env={env}>
      <Modal.Stack><DrawerStackBridge>{children}</DrawerStackBridge></Modal.Stack>
    </MantineProvider>
  );
}
