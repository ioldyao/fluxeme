import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type CurrencyCode = 'cny' | 'usd';

interface CurrencyState {
  currency: CurrencyCode;
  rate: number;
  setCurrency: (c: CurrencyCode) => void;
  setRate: (r: number) => void;
}

export const CURRENCY_SYMBOL: Record<CurrencyCode, string> = {
  cny: '¥',
  usd: '$',
};

export const CURRENCY_CODE: Record<CurrencyCode, string> = {
  cny: 'CNY',
  usd: 'USD',
};

export const useCurrency = create<CurrencyState>()(
  persist(
    (set) => ({
      currency: 'usd',
      rate: 7.2,
      setCurrency: (currency) => set({ currency }),
      setRate: (rate) => set({ rate }),
    }),
    { name: 'currency' },
  ),
);
