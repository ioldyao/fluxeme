import type { ReactNode } from 'react';
import { useAuth } from '@/store/auth';

export type Permission =
  | 'admin:dashboard'
  | 'admin:users'
  | 'admin:channels'
  | 'admin:models'
  | 'admin:model-pricing'
  | 'admin:rules'
  | 'admin:usage'
  | 'admin:bills'
  | 'admin:recharge-keys'
  | 'admin:health'
  | 'admin:settings'
  | 'admin:gateway'
  | 'admin:moderation';

const ROLE_PERMISSIONS: Record<string, Permission[]> = {
  admin: [
    'admin:dashboard',
    'admin:users',
    'admin:channels',
    'admin:models',
    'admin:model-pricing',
    'admin:rules',
    'admin:usage',
    'admin:bills',
    'admin:recharge-keys',
    'admin:health',
    'admin:settings',
    'admin:gateway',
    'admin:moderation',
  ],
  user: [],
};

/** Check if the current user has a specific permission. */
export function usePermission(perm: Permission): boolean {
  const role = useAuth((s) => s.role);
  return ROLE_PERMISSIONS[role ?? '']?.includes(perm) ?? false;
}

/** Permission-gated wrapper. Renders children only if the user has the required permission. */
export function Guard({
  perm,
  children,
  fallback = null,
}: {
  perm: Permission;
  children: ReactNode;
  fallback?: ReactNode;
}) {
  const allowed = usePermission(perm);
  return allowed ? <>{children}</> : fallback;
}
