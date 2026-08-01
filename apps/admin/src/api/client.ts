import { useAuth } from '@/store/auth';
import i18n from '@fluxeme/shared/i18n';
import { toast } from 'sonner';

type ApiOptions = Omit<RequestInit, 'body'> & {
  body?: unknown;
  skipAuthErrorHandling?: boolean;
};

export async function api<T>(path: string, opts: ApiOptions = {}): Promise<T> {
  const { body, skipAuthErrorHandling = false, ...fetchOpts } = opts;
  const headers = new Headers(fetchOpts.headers);

  let fetchBody: BodyInit | undefined;
  if (body !== undefined && body !== null) {
    headers.set('Content-Type', 'application/json');
    fetchBody = JSON.stringify(body);
  }

  const response = await fetch(`/api${path}`, {
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
