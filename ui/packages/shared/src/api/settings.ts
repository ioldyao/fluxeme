import { api } from './client';
import { useCurrency } from '../store/currency';
import type { SsoConfig, SsoConfigRequest } from '../types';

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

// ── SSO Configs ─────────────────────────────────────────────────────────

export async function listSsoConfigs(): Promise<SsoConfig[]> {
  return api<SsoConfig[]>('/settings/sso-configs');
}

export async function getSsoConfig(id: string): Promise<SsoConfig> {
  return api<SsoConfig>(`/settings/sso-configs/${encodeURIComponent(id)}`);
}

export async function createSsoConfig(data: SsoConfigRequest): Promise<SsoConfig> {
  return api<SsoConfig>('/settings/sso-configs', {
    method: 'POST',
    body: data,
  });
}

export async function updateSsoConfig(id: string, data: SsoConfigRequest): Promise<SsoConfig> {
  return api<SsoConfig>(`/settings/sso-configs/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: data,
  });
}

export async function deleteSsoConfig(id: string): Promise<void> {
  await api(`/settings/sso-configs/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}
