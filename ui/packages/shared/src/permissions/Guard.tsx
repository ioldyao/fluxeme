import type { ReactNode } from 'react';
import { usePermission, type Permission } from './usePermission';

interface GuardProps {
  perm: Permission;
  children: ReactNode;
  fallback?: ReactNode;
}

/** Permission-gated wrapper. Renders children only if the user has the required permission. */
export function Guard({ perm, children, fallback = null }: GuardProps) {
  const allowed = usePermission(perm);
  return allowed ? <>{children}</> : fallback;
}
