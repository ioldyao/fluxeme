import { api } from './client';
import { useCurrency } from '../store/currency';

/** Public app config — no auth required. Returns global display settings. */
export function fetchAppConfig() {
  return api<{ currency: string; rate: number }>('/app/config', {
    skipAuthErrorHandling: true,
  });
}

/** Admin-only: save global currency settings. */
export function saveCurrencySettings(currency: string) {
  return api<{ currency: string; rate: number }>('/settings/currency', {
    method: 'PUT',
    body: { currency },
  });
}

/** Load global currency settings into the zustand store (no auth needed). */
export async function loadCurrencySettings() {
  try {
    const data = await fetchAppConfig();
    useCurrency.getState().setCurrency(data.currency as 'usd' | 'cny');
    useCurrency.getState().setLoaded(true);
  } catch {
    useCurrency.getState().setLoaded(true);
  }
}
