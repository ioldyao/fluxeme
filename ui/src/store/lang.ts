import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';

interface LangState {
  lang: 'zh' | 'en';
  setLang: (lang: 'zh' | 'en') => void;
}

export const useLang = create<LangState>()(
  persist(
    (set) => ({
      lang: 'zh',
      setLang: (lang) => {
        i18n.changeLanguage(lang);
        set({ lang });
      },
    }),
    { name: 'lang' },
  ),
);
