import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { fetchUsdToCnyRate } from '@/api/exchangeRates';

export type CurrencyCode = 'cny' | 'usd';

interface CurrencyState {
  currency: CurrencyCode;
  rate: number;
  setCurrency: (c: CurrencyCode) => void;
  setRate: (r: number) => void;
  fetchRate: () => Promise<void>;
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
      fetchRate: async () => {
        try {
          const rate = await fetchUsdToCnyRate();
          set({ rate });
        } catch {
          // Rate already has fallback default, keep current value
        }
      },
    }),
    { name: 'currency' },
  ),
);
