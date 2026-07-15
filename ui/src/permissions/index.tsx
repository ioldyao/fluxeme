import { useAuth } from '@/store/auth';

/** Frontend-facing permission codes matching backend `perms` constants. */
export const PERMS = {
  // Dashboard
  DASHBOARD_VIEW: 'dashboard:view',
  // Usage logs
  USAGE_READ: 'usage:read',
  USAGE_EXPORT: 'usage:export',
  // Billing
  BILLING_VIEW: 'billing:view',
  BILLING_EXPORT: 'billing:export',
  // Wallet (admin)
  WALLET_READ: 'wallet:read',
  WALLET_RECHARGE: 'wallet:recharge',
  WALLET_MANAGE: 'wallet:manage',
  // API Keys (admin)
  APIKEY_READ: 'apikey:read',
  // Settings
  SETTINGS_READ: 'settings:read',
  SETTINGS_UPDATE: 'settings:update',
  SETTINGS_GATEWAY: 'settings:gateway',
  // Exchange rates
  EXCHANGE_READ: 'exchange:read',
  EXCHANGE_UPDATE: 'exchange:update',
  // Roles & Permissions
  ROLE_READ: 'role:read',
  ROLE_UPDATE: 'role:update',
  PERMISSION_READ: 'permission:read',
  // Users
  USER_CREATE: 'user:create',
  USER_READ: 'user:read',
  USER_UPDATE: 'user:update',
  USER_DELETE: 'user:delete',
} as const;

export type Permission = (typeof PERMS)[keyof typeof PERMS];

/**
 * Check whether the current user has a given permission.
 * Returns `true` if the permission is present in the user's permissions list.
 */
export function usePermission(perm: Permission): boolean {
  const permissions = useAuth((s) => s.permissions);
  return permissions.includes(perm);
}

interface GuardProps {
  perm: Permission;
  children: React.ReactNode;
  fallback?: React.ReactNode;
}

/** Renders children only when the user has the specified permission. */
export function Guard({ perm, children, fallback = null }: GuardProps) {
  const allowed = usePermission(perm);
  return allowed ? <>{children}</> : <>{fallback}</>;
}
