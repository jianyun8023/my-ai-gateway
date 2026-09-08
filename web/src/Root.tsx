import { useEffect } from 'react';
import App from './App';
import { useThemeStore } from './stores/useThemeStore';
import { ConsoleProvider } from './components/ui/ConsoleProvider';

export function Root() {
  const initializeTheme = useThemeStore((state) => state.initializeTheme);
  useEffect(() => initializeTheme(), [initializeTheme]);
  return <ConsoleProvider><App /></ConsoleProvider>;
}
