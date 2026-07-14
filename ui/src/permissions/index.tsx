import { useAuth } from '@/store/auth';

export type Permission =
  | 'admin:exchange-rates'
  | 'admin:wallet-keys'
  | 'admin:period-summary-all'
  | 'admin:dashboard'
  | 'admin:usage-filters'
  | 'admin:billing-channels';

const ROLE_PERMISSIONS: Record<string, Permission[]> = {
  admin: [
    'admin:exchange-rates',
    'admin:wallet-keys',
    'admin:period-summary-all',
    'admin:dashboard',
    'admin:usage-filters',
    'admin:billing-channels',
  ],
  user: [],
};

export function usePermission(perm: Permission): boolean {
  const role = useAuth((s) => s.role);
  return ROLE_PERMISSIONS[role ?? '']?.includes(perm) ?? false;
}

export function Guard({
  perm,
  children,
  fallback = null,
}: {
  perm: Permission;
  children: React.ReactNode;
  fallback?: React.ReactNode;
}) {
  const allowed = usePermission(perm);
  return allowed ? <>{children}</> : <>{fallback}</>;
}
