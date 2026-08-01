import { useAuth } from '@shared/store/auth';

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
  | 'admin:moderation'
  | 'admin:policies'
  | 'admin:announcements';

/** Known permission strings for use in route configs. */
export const PERMISSIONS = {
  DASHBOARD: 'admin:dashboard' as Permission,
  USERS: 'admin:users' as Permission,
  CHANNELS: 'admin:channels' as Permission,
  MODELS: 'admin:models' as Permission,
  MODEL_PRICING: 'admin:model-pricing' as Permission,
  RULES: 'admin:rules' as Permission,
  MODERATION: 'admin:moderation' as Permission,
  USAGE: 'admin:usage' as Permission,
  BILLS: 'admin:bills' as Permission,
  RECHARGE_KEYS: 'admin:recharge-keys' as Permission,
  HEALTH: 'admin:health' as Permission,
  SETTINGS: 'admin:settings' as Permission,
  GATEWAY: 'admin:gateway' as Permission,
  POLICIES: 'admin:policies' as Permission,
  ANNOUNCEMENTS: 'admin:announcements' as Permission,
} as const;

/** Check if the current user has a specific permission.
 *
 * Permissions are fetched from the backend Casbin layer via
 * GET /api/me/permissions and cached in the auth store.
 */
export function usePermission(perm: Permission): boolean {
  const permissions = useAuth((s) => s.permissions);
  const role = useAuth((s) => s.role);

  // Fallback: if permissions haven't loaded yet, use role-based check
  if (permissions.length === 0) {
    return role === 'admin';
  }

  return permissions.includes(perm);
}
