import { Button } from './Button';
import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import i18n, { isSupportedLanguage, persistLanguage, type SupportedLanguage } from '@/i18n';
import styles from './LanguageSwitcher.module.scss';

// 目前仅支持 en / zh 简体；aria/title 文案由 console 命名空间提供。
const LANGUAGE_OPTIONS: ReadonlyArray<{ value: SupportedLanguage; label: string }> = [
  { value: 'en', label: 'EN' },
  { value: 'zh', label: '中文' },
];

export function LanguageSwitcher({ className = '' }: { className?: string }) {
  const { t } = useTranslation('console');
  const currentLanguage = isSupportedLanguage(i18n.language) ? i18n.language : 'en';

  const handleLanguageChange = useCallback(async (language: SupportedLanguage) => {
    if (currentLanguage === language) return;
    await i18n.changeLanguage(language);
    persistLanguage(language);
  }, [currentLanguage]);

  const switcherClassName = `${styles.languageSwitcher} ${className}`.trim();
  const switchAria = t('common.language_switch');

  return (
    <div className={switcherClassName} role="group" aria-label={switchAria}>
      {LANGUAGE_OPTIONS.map((option) => (
        <Button variant={currentLanguage === option.value ? 'secondary' : 'ghost'} size="sm"
          key={option.value}
          type="button"
          className={styles.languagePill}
          onClick={() => void handleLanguageChange(option.value)}
          aria-label={`${switchAria} ${option.label}`}
          aria-pressed={currentLanguage === option.value}
          title={switchAria}
        >
          {option.label}
        </Button>
      ))}
    </div>
  );
}
