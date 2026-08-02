import { useAuth } from '@shared/store/auth';
import i18n from '@shared/i18n';
import { toast } from 'sonner';

type ApiOptions = Omit<RequestInit, 'body'> & {
  body?: unknown;
  skipAuthErrorHandling?: boolean;
};

// Empty string keeps the old same-origin behavior (relative '/api/...'),
// which still works when a dev-server proxy or a reverse proxy in front
// of both apps forwards '/api' to the backend. Set VITE_API_BASE_URL to
// point directly at the backend when the frontend is served from a
// different host/port than the API (e.g. IP:PORT direct access without
// a gateway in front).
const API_BASE_URL = import.meta.env.VITE_API_BASE_URL ?? '';

/**
 * Builds a ws:// or wss:// URL for a backend path (e.g. '/api/health/ws').
 * Derives host/protocol from VITE_API_BASE_URL when set; otherwise falls
 * back to the current page's origin, matching the old same-origin
 * behavior. Needed because a plain `window.location.host` would point
 * at the frontend's own host once user-app/admin-app are served from a
 * different port/domain than the backend.
 */
export function getWsUrl(path: string): string {
  if (API_BASE_URL) {
    const httpUrl = new URL(path, API_BASE_URL);
    httpUrl.protocol = httpUrl.protocol === 'https:' ? 'wss:' : 'ws:';
    return httpUrl.toString();
  }
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${window.location.host}${path}`;
}

export async function api<T>(path: string, opts: ApiOptions = {}): Promise<T> {
  const { body, skipAuthErrorHandling = false, ...fetchOpts } = opts;
  const headers = new Headers(fetchOpts.headers);

  let fetchBody: BodyInit | undefined;
  if (body !== undefined && body !== null) {
    headers.set('Content-Type', 'application/json');
    fetchBody = JSON.stringify(body);
  }

  const response = await fetch(`${API_BASE_URL}/api${path}`, {
    // Required once the frontend and backend are on different
    // origins (different port or host): without this, the browser
    // won't send/receive the session cookie cross-origin.
    credentials: 'include',
    ...fetchOpts,
    headers,
    body: fetchBody,
  });

  if (response.status === 401) {
    if (!skipAuthErrorHandling) {
      toast.error(i18n.t('login.sessionExpired'));
      useAuth.getState().clear();
      setTimeout(() => {
        window.location.href = '/login';
      }, 1500);
    }

    throw new Error('unauthorized');
  }

  if (response.status === 403) {
    toast.error(i18n.t('err.accessDenied'));
    throw new Error('forbidden');
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
