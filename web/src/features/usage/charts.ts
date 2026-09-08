import { formatCompact, formatCompactWithTitle } from '@/utils/formatCompact';
import type { TooltipItem } from 'chart.js';

const compactTick = (value: string | number): string => formatCompact(Number(value));

const compactTooltipLabel = (context: TooltipItem<'line' | 'bar'>): string => {
  const raw = Number(context.parsed.y ?? context.parsed.x ?? 0);
  const { display, exact } = formatCompactWithTitle(raw);
  const label = context.dataset.label ? `${context.dataset.label}: ` : '';
  return `${label}${display} (${exact})`;
};

export const horizontalTokenBarOptions = {
  indexAxis: 'y' as const,
  responsive: true,
  maintainAspectRatio: false,
  plugins: {
    legend: { display: false },
    tooltip: { callbacks: { label: compactTooltipLabel } },
  },
  scales: {
    x: { ticks: { callback: compactTick } },
  },
};

export const lineChartOptions = {
  responsive: true,
  maintainAspectRatio: false,
  interaction: { mode: 'index' as const, intersect: false },
  plugins: {
    tooltip: { callbacks: { label: compactTooltipLabel } },
  },
  scales: {
    y: {
      ticks: { callback: compactTick },
    },
    secondary: {
      position: 'right' as const,
      grid: { display: false },
      ticks: { callback: compactTick },
    },
  },
};

export const makeChartColors = () => [
  'oklch(58% 0.16 145)',
  'oklch(74% 0.08 195)',
  'oklch(70% 0.16 80)',
  'oklch(78% 0.05 220)',
  'oklch(58% 0.2 25)',
  'oklch(66% 0.08 240)',
  'oklch(68% 0.1 125)',
];
