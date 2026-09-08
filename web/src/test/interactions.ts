import { act } from 'react';

export async function selectComboboxValue(input: HTMLInputElement, value: string) {
  await act(async () => input.click());
  const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')]
    .find((item) => item.getAttribute('value') === value);
  if (!option) throw new Error(`Combobox option not found: ${value}`);
  await act(async () => option.click());
}
