import { api } from './client';
import { useCurrency } from '../store/currency';

export function fetchCurrencySettings() {
  return api<{ currency: string; rate: number }>('/settings/currency');
}

export function saveCurrencySettings(currency: string, rate: number) {
  return api<{ currency: string; rate: number }>('/settings/currency', {
    method: 'PUT',
    body: { currency, rate },
  });
}

/** Load global currency settings into the zustand store. */
export async function loadCurrencySettings() {
  try {
    const data = await fetchCurrencySettings();
    useCurrency.getState().setCurrency(data.currency as 'usd' | 'cny');
    useCurrency.getState().setRate(data.rate);
    useCurrency.getState().setLoaded(true);
  } catch {
    // Fall back to defaults already in the store
    useCurrency.getState().setLoaded(true);
  }
}
