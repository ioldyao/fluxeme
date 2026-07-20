import { useAuth } from '@/store/auth';
import i18n from '@/i18n';
import { toast } from 'sonner';

type ApiOptions = Omit<RequestInit, 'body'> & { body?: unknown };

export async function api<T>(path: string, opts: ApiOptions = {}): Promise<T> {
  const token = useAuth.getState().token;
  const headers = new Headers(opts.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }

  let fetchBody: BodyInit | undefined;
  const { body, ...fetchOpts } = opts;
  if (body !== undefined && body !== null) {
    headers.set('Content-Type', 'application/json');
    fetchBody = JSON.stringify(body);
  }

  const r = await fetch(`/admin/api${path}`, { ...fetchOpts, headers, body: fetchBody });

  if (r.status === 401) {
    toast.error(i18n.t('login.sessionExpired'));
    useAuth.getState().clear();
    setTimeout(() => {
      window.location.href = '/login';
    }, 1500);
    throw new Error('unauthorized');
  }

  if (r.status === 403) {
    toast.error(i18n.t('err.accessDenied'));
    throw new Error('forbidden');
  }

  if (!r.ok) {
    const d = await r.json().catch(() => ({}));
    const msg = typeof d.error === 'string' ? d.error : d.error?.message || d.message || 'Request failed';
    throw new Error(msg);
  }

  return r.json();
}
