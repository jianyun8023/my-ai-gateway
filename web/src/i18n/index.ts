import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

const LANGUAGE_STORAGE_KEY = 'my-ai-gateway-language';
const DEFAULT_LANGUAGE = 'en';

const SUPPORTED_LANGUAGES = ['en', 'zh'] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const isSupportedLanguage = (
  language: string | null
): language is SupportedLanguage => SUPPORTED_LANGUAGES.includes(language as SupportedLanguage);

const detectBrowserLanguage = (): SupportedLanguage => {
  if (typeof navigator === 'undefined') return DEFAULT_LANGUAGE;

  const candidates = navigator.languages.length > 0 ? navigator.languages : [navigator.language];
  for (const candidate of candidates) {
    const lower = candidate.toLowerCase();
    if (lower.startsWith('zh')) return 'zh';
    if (lower.startsWith('en')) return 'en';
  }

  return DEFAULT_LANGUAGE;
};

const getInitialLanguage = (): SupportedLanguage => {
  if (typeof window === 'undefined') return DEFAULT_LANGUAGE;

  const saved = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (isSupportedLanguage(saved)) return saved;

  return detectBrowserLanguage();
};

if (!i18n.isInitialized) {
  void i18n.use(initReactI18next).init({
    resources: { en: {}, zh: {} },
    lng: getInitialLanguage(),
    fallbackLng: DEFAULT_LANGUAGE,
    interpolation: { escapeValue: false },
  });
}

export const persistLanguage = (language: SupportedLanguage) => {
  if (typeof window === 'undefined' || !isSupportedLanguage(language)) return;
  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
};

export default i18n;
