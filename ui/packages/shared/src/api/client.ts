import { useAuth } from '../store/auth';
import i18n from '../i18n';
import { toast } from 'sonner';

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? '';

type ApiOptions = Omit<RequestInit, 'body'> & {
  body?: unknown;
  skipAuthErrorHandling?: boolean;
};

/** Guards against multiple simultaneous 401 clear-toast sequences. */
let _clearingAuth = false;

export async function api<T>(path: string, opts: ApiOptions = {}): Promise<T> {
  const { body, skipAuthErrorHandling = false, ...fetchOpts } = opts;
  const headers = new Headers(fetchOpts.headers);

  let fetchBody: BodyInit | undefined;
  if (body !== undefined && body !== null) {
    headers.set('Content-Type', 'application/json');
    fetchBody = JSON.stringify(body);
  }

  const response = await fetch(`${API_BASE}/api${path}`, {
    ...fetchOpts,
    credentials: 'include',
    headers,
    body: fetchBody,
  });

  if (response.status === 401) {
    if (!skipAuthErrorHandling && !_clearingAuth) {
      _clearingAuth = true;
      toast.error(i18n.t('login.sessionExpired'));
      useAuth.getState().clear();
      // Allow the React Router guard to handle the redirect declaratively.
      // Reset the latch after a grace period so subsequent 401s can fire
      // if the user returns without a valid session.
      setTimeout(() => { _clearingAuth = false; }, 3000);
    }

    throw new Error('unauthorized');
  }

  if (response.status === 403) {
    const data = await response.json().catch(() => ({}));
    const message =
      typeof data.error === 'string'
        ? data.error
        : data.error?.message || data.message || i18n.t('err.accessDenied');
    toast.error(message);
    throw new Error(message);
  }

  if (!response.ok) {
    const data = await response.json().catch(() => ({}));
    const message =
      typeof data.error === 'string'
        ? data.error
        : data.error?.message || data.message || 'Request failed';
    throw new Error(message);
  }

  return response.json();
}