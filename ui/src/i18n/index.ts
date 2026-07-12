import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import zh from './zh';
import en from './en';

function getInitialLang(): string {
  if (typeof window === 'undefined') return 'zh';
  try {
    const stored = localStorage.getItem('lang');
    if (stored) {
      const parsed = JSON.parse(stored);
      if (parsed?.state?.lang) return parsed.state.lang;
    }
  } catch {}
  return navigator.language.startsWith('zh') ? 'zh' : 'en';
}

const lang = getInitialLang();

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: lang,
  fallbackLng: 'zh',
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
