import { useThemeStore } from '@/stores/useThemeStore';
import type { ChartTheme } from './charts';

// The store changes the root tokens before notifying subscribers. These base
// tokens are resolved colors; canvas cannot use CSS var() expressions directly.
export function useChartTheme(): ChartTheme {
  useThemeStore(state => state.resolvedColorScheme);
  useThemeStore(state => state.style);
  const tokens = getComputedStyle(document.documentElement);
  const color = (name: string) => tokens.getPropertyValue(name).trim();
  return { accent: color('--accent'), secondary: color('--muted'), text: color('--fg'), muted: color('--muted'),
    grid: color('--border'), surface: color('--surface'), font: getComputedStyle(document.body).fontFamily };
}
