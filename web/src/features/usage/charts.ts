import { formatCompact, formatCompactWithTitle } from '@/utils/formatCompact';
import type { ChartOptions, TooltipItem } from 'chart.js';

export interface ChartTheme { accent: string; secondary: string; text: string; muted: string; grid: string; surface: string; font: string }

function tooltipLabel(context: TooltipItem<'line' | 'bar'>): string {
  const { display, exact } = formatCompactWithTitle(Number(context.parsed.y));
  return `${context.dataset.label ?? ''}: ${display} (${exact})`;
}

// Both charts use time on x and a numeric measure on y. Distributions use DOM
// progress rows, avoiding category-index tooltips and preserving keyboard access.
function commonOptions(theme: ChartTheme) {
  return {
    responsive: true, maintainAspectRatio: false, animation: false as const,
    interaction: { mode: 'index' as const, intersect: false },
    plugins: {
      legend: { position: 'top' as const, align: 'end' as const, labels: { color: theme.text, boxWidth: 10, boxHeight: 10, font: { family: theme.font, size: 11 } } },
      tooltip: { backgroundColor: theme.surface, titleColor: theme.text, bodyColor: theme.text, borderColor: theme.grid, borderWidth: 1,
        titleFont: { family: theme.font }, bodyFont: { family: theme.font }, callbacks: { label: tooltipLabel } },
    },
  };
}

const valueAxis = (theme: ChartTheme, title: string) => ({
  beginAtZero: true, border: { display: false }, grid: { color: theme.grid },
  ticks: { color: theme.muted, maxTicksLimit: 5, font: { family: theme.font, size: 11 }, callback: (value: string | number) => formatCompact(Number(value)) },
  title: { display: true, text: title, color: theme.muted, font: { family: theme.font, size: 11 } },
});
const timeAxis = (theme: ChartTheme) => ({
  border: { display: false }, grid: { display: false },
  ticks: { color: theme.muted, maxTicksLimit: 6, maxRotation: 0, font: { family: theme.font, size: 11 } },
});

export function stackedTrendOptions(theme: ChartTheme): ChartOptions<'bar'> {
  return { ...commonOptions(theme), scales: { x: { ...timeAxis(theme), stacked: true }, y: { ...valueAxis(theme, 'Token'), stacked: true } } };
}
export function metricTrendOptions(theme: ChartTheme, primary: string, secondary: string): ChartOptions<'line'> {
  return { ...commonOptions(theme), scales: { x: timeAxis(theme), y: valueAxis(theme, primary),
    secondary: { ...valueAxis(theme, secondary), position: 'right', grid: { display: false } } } };
}
