import { Combobox, CloseButton, Loader, ScrollArea, TextInput, useCombobox } from '@mantine/core';
import { useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from './Button';

interface Options {
  data: string[];
  has_more: boolean;
}

interface RemoteFilterFieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  loadOptions: (search: string, signal: AbortSignal) => Promise<Options>;
  contextKey?: string;
  className?: string;
  inputMode?: 'numeric';
}

interface OptionState {
  load: RemoteFilterFieldProps['loadOptions'];
  key: string;
  status: 'loading' | 'ready' | 'error';
  result?: Options;
}

// A generic async field: callers own the endpoint and query scope; Mantine owns
// keyboard navigation, focus, portal positioning and option selection.
export function RemoteFilterField({ label, value, onChange, loadOptions, contextKey = '', className, inputMode }: RemoteFilterFieldProps) {
  const { t } = useTranslation('console');
  const id = useId();
  const [state, setState] = useState<OptionState>();
  const [revision, setRevision] = useState(0);
  const combobox = useCombobox({
    onDropdownOpen: () => setRevision((previous) => previous + 1),
    onDropdownClose: () => combobox.resetSelectedOption(),
  });
  const opened = combobox.dropdownOpened;
  const search = value.trim();
  const key = JSON.stringify([contextKey, search, revision]);
  const current = state?.key === key && state.load === loadOptions ? state : undefined;
  const loading = opened && (!current || current.status === 'loading');
  const failed = current?.status === 'error';

  useEffect(() => {
    if (!opened) return;
    const controller = new AbortController();
    const timeout = setTimeout(() => {
      setState({ load: loadOptions, key, status: 'loading' });
      void loadOptions(search, controller.signal).then((result) => {
        if (!controller.signal.aborted) setState({ load: loadOptions, key, status: 'ready', result });
      }).catch(() => {
        if (!controller.signal.aborted) setState({ load: loadOptions, key, status: 'error' });
      });
    }, 250);
    return () => {
      clearTimeout(timeout);
      controller.abort();
    };
  }, [key, loadOptions, opened, search]);

  const result = current?.result;
  const statusText = loading ? t('common.filter_options_loading')
    : failed ? t('common.filter_options_failed')
      : result?.has_more ? t('common.filter_options_more')
        : result?.data.length === 0 ? t('common.filter_options_empty') : '';

  return (
    <Combobox store={combobox} onOptionSubmit={(selected) => {
      onChange(selected);
      combobox.closeDropdown();
    }}>
      <Combobox.Target withExpandedAttribute>
        <TextInput
          id={id}
          className={className}
          label={label}
          placeholder={t('common.all')}
          value={value}
          inputMode={inputMode}
          autoComplete="off"
          aria-busy={loading}
          description={statusText ? <span role="status">{statusText}{failed && <Button size="sm" variant="ghost" onClick={() => {
            setRevision((previous) => previous + 1);
            combobox.openDropdown();
          }}>{t('common.retry')}</Button>}</span> : undefined}
          onFocus={() => combobox.openDropdown()}
          onClick={() => combobox.openDropdown()}
          onBlur={() => combobox.closeDropdown()}
          onChange={(event) => {
            onChange(event.currentTarget.value);
            combobox.resetSelectedOption();
            combobox.openDropdown();
          }}
          rightSection={loading ? <Loader size={16} /> : value ? <CloseButton
            aria-label={t('common.clear_filter', { label })}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onChange('')}
          /> : <Combobox.Chevron />}
          rightSectionPointerEvents={loading || !value ? 'none' : undefined}
        />
      </Combobox.Target>
      <Combobox.Dropdown>
        <ScrollArea.Autosize mah={240} type="auto">
          <Combobox.Options>
            {result?.data.map((option) => <Combobox.Option value={option} key={option}>{option}</Combobox.Option>)}
            {!result?.data.length && <Combobox.Empty>{statusText}</Combobox.Empty>}
          </Combobox.Options>
        </ScrollArea.Autosize>
      </Combobox.Dropdown>
    </Combobox>
  );
}
