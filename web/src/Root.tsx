import { useEffect } from 'react';
import App from './App';
import { useThemeStore } from './stores/useThemeStore';

export function Root() {
  const initializeTheme = useThemeStore((state) => state.initializeTheme);
  useEffect(() => initializeTheme(), [initializeTheme]);
  return <App />;
}
