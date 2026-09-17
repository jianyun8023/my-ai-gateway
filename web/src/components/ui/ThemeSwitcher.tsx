import { Text } from '@mantine/core';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  THEME_MODES,
  THEME_STYLES,
  useThemeStore,
  type ThemeMode,
  type ThemeStyle,
} from '@/stores/useThemeStore';
import { IconSunAsterisk } from './icons';
import { IconButton } from './IconButton';
import { Popover } from './overlays';
import styles from './ThemeSwitcher.module.scss';

export function ThemeSwitcher() {
  const [opened, setOpened] = useState(false);
  const { t } = useTranslation('console');
  const mode = useThemeStore((state) => state.mode);
  const style = useThemeStore((state) => state.style);
  const setMode = useThemeStore((state) => state.setMode);
  const setStyle = useThemeStore((state) => state.setStyle);
  const styleLabel = t(`shell.appearance.styles.${style}.label`);
  const modeLabel = t(`shell.appearance.modes.${mode}`);

  return <Popover opened={opened} onChange={setOpened} position="bottom-end"
    width="min(360px, calc(100vw - 24px))" trapFocus>
    <Popover.Target>
      <IconButton label={t('shell.appearance.trigger', { style: styleLabel, mode: modeLabel })}
        aria-haspopup="dialog" aria-expanded={opened} onClick={() => setOpened((value) => !value)}>
        <IconSunAsterisk size={18} />
      </IconButton>
    </Popover.Target>
    <Popover.Dropdown role="dialog" aria-label={t('shell.appearance.title')} inert={!opened} className={styles.panel}>
      <div className={styles.heading}>
        <Text component="strong">{t('shell.appearance.title')}</Text>
        <Text component="span">{t('shell.appearance.description')}</Text>
      </div>

      <fieldset className={styles.group}>
        <legend>{t('shell.appearance.style_label')}</legend>
        <div className={styles.styleGrid} role="radiogroup" aria-label={t('shell.appearance.style_label')}>
          {THEME_STYLES.map((option: ThemeStyle) => <button key={option} type="button" role="radio"
            aria-checked={style === option} data-theme-option={option}
            onClick={() => setStyle(option)} className={styles.styleOption}>
            <span className={styles.preview} aria-hidden="true"><i /><i /><i /></span>
            <span className={styles.optionCopy}>
              <strong>{t(`shell.appearance.styles.${option}.label`)}</strong>
              <small>{t(`shell.appearance.styles.${option}.description`)}</small>
            </span>
          </button>)}
        </div>
      </fieldset>

      <fieldset className={styles.group}>
        <legend>{t('shell.appearance.mode_label')}</legend>
        <div className={styles.modeGrid} role="radiogroup" aria-label={t('shell.appearance.mode_label')}>
          {THEME_MODES.map((option: ThemeMode) => <button key={option} type="button" role="radio"
            aria-checked={mode === option} data-mode-option={option}
            onClick={() => setMode(option)} className={styles.modeOption}>
            {t(`shell.appearance.modes.${option}`)}
          </button>)}
        </div>
      </fieldset>
    </Popover.Dropdown>
  </Popover>;
}
