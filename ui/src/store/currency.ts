import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type CurrencyCode = 'cny' | 'usd';

interface CurrencyState {
  currency: CurrencyCode;
  setCurrency: (c: CurrencyCode) => void;
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
      setCurrency: (currency) => set({ currency }),
    }),
    { name: 'currency' },
  ),
);

// ── Pricing currency preferences (global/per-model + per-model overrides) ──
type CurrencyMode = 'global' | 'per-model';

interface PricingCurrencyState {
  mode: CurrencyMode;
  modelCurrency: Record<string, CurrencyCode>;
  setMode: (mode: CurrencyMode) => void;
  setModelCurrency: (id: string, currency: CurrencyCode) => void;
  effectiveCurrency: (globalCurrency: CurrencyCode, modelId: string | null) => CurrencyCode;
}

export const usePricingCurrency = create<PricingCurrencyState>()(
  persist(
    (set, get) => ({
      mode: 'global' as CurrencyMode,
      modelCurrency: {},
      setMode: (mode) => set({ mode }),
      setModelCurrency: (id, currency) =>
        set((state) => ({ modelCurrency: { ...state.modelCurrency, [id]: currency } })),
      effectiveCurrency: (globalCurrency, modelId) => {
        const { mode, modelCurrency } = get();
        if (mode === 'global') return globalCurrency;
        return modelId ? (modelCurrency[modelId] ?? 'usd') : 'usd';
      },
    }),
    { name: 'pricing-currency' },
  ),
);
