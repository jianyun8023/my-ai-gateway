import { Tabs } from '@mantine/core';
import { Button } from './Button';
import styles from './ConsolePrimitives.module.scss';

type SegmentedProps<T extends string> = {
  value: T;
  options: ReadonlyArray<{ value: T; label: string; count?: number }>;
  onChange: (value: T) => void;
  label: string;
} & ({ mode: 'group'; id?: string } | { mode?: 'tabs'; id: string });

// Tabs switch content panels; group mode selects a filter without implying a panel.
export function SegmentedTabs<T extends string>({ value, options, onChange, label, mode = 'tabs', id }: SegmentedProps<T>) {
  if (mode === 'group') {
    return <div className={styles.segmented} role="group" aria-label={label}>
      {options.map((option) => <Button key={option.value}
        variant={value === option.value ? 'secondary' : 'ghost'}
        aria-pressed={value === option.value} onClick={() => onChange(option.value)}
      >{option.label}{option.count !== undefined && <small>{option.count}</small>}</Button>)}
    </div>;
  }

  return <Tabs value={value} onChange={(next) => { if (next !== null) onChange(next as T); }}
    variant="pills" className={styles.tabs}
    classNames={{ list: styles.segmented, tab: styles.segmentedTab }}
  >
    <Tabs.List aria-label={label}>
      {options.map((option) => <Tabs.Tab key={option.value} value={option.value}
        id={`${id}-${option.value}`} aria-controls={`${id}-panel`}
        rightSection={option.count !== undefined ? <small>{option.count}</small> : undefined}
      >{option.label}</Tabs.Tab>)}
    </Tabs.List>
  </Tabs>;
}
