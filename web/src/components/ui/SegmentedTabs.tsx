import { useRef } from 'react';
import styles from './ConsolePrimitives.module.scss';

type SegmentedProps<T extends string> = {
  value: T;
  options: ReadonlyArray<{ value: T; label: string; count?: number }>;
  onChange: (value: T) => void;
  label: string;
} & ({ mode: 'group'; id?: string } | { mode?: 'tabs'; id: string });

// Tabs switch content panels; group mode selects a filter without implying a panel.
export function SegmentedTabs<T extends string>({ value, options, onChange, label, mode = 'tabs', id }: SegmentedProps<T>) {
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  return (
    <div className={styles.segmented} role={mode === 'tabs' ? 'tablist' : 'group'} aria-label={label}>
      {options.map((option, index) => (
        <button
          key={option.value}
          ref={(element) => { buttons.current[index] = element; }}
          id={mode === 'tabs' ? `${id}-${option.value}` : undefined}
          type="button"
          role={mode === 'tabs' ? 'tab' : undefined}
          aria-selected={mode === 'tabs' ? value === option.value : undefined}
          aria-controls={mode === 'tabs' ? `${id}-panel` : undefined}
          aria-pressed={mode === 'group' ? value === option.value : undefined}
          tabIndex={mode === 'tabs' && value !== option.value ? -1 : 0}
          data-active={value === option.value}
          onClick={() => onChange(option.value)}
          onKeyDown={(event) => {
            if (mode !== 'tabs') return;
            let next: number;
            switch (event.key) {
              case 'ArrowRight': next = (index + 1) % options.length; break;
              case 'ArrowLeft': next = (index - 1 + options.length) % options.length; break;
              case 'Home': next = 0; break;
              case 'End': next = options.length - 1; break;
              default: return;
            }
            event.preventDefault();
            buttons.current[next]?.focus();
            onChange(options[next].value);
          }}
        >
          <span>{option.label}</span>
          {option.count !== undefined && <small>{option.count}</small>}
        </button>
      ))}
    </div>
  );
}
