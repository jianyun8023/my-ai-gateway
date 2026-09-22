import { Checkbox } from '@mantine/core';
import styles from './Controls.module.scss';

export function CheckboxField({ checked, onChange, label, hint, disabled = false }: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  hint?: string;
  disabled?: boolean;
}) {
  return <Checkbox checked={checked} onChange={(event) => onChange(event.currentTarget.checked)}
    label={label} description={hint} disabled={disabled} className={styles.checkbox} classNames={{ label: styles.checkboxLabel }} />;
}
