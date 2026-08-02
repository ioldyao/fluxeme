import { Suspense, type ComponentType } from 'react';
import { Navigate } from 'react-router-dom';
import { useAuth } from '@shared/store/auth';
import type { Permission } from '@shared/permissions/usePermission';

export type PermissionRouteConfig = {
  Component: ComponentType;
  /** Fine-grained Casbin permission required to render this route. */
  perm?: Permission;
};

/** Wraps a route element and checks fine-grained Casbin permission. */
export function PermissionRoute({ route }: { route: PermissionRouteConfig }) {
  const permissions = useAuth((s) => s.permissions);
  const role = useAuth((s) => s.role);

  if (route.perm) {
    const hasPerm = permissions.length > 0
      ? permissions.includes(route.perm)
      : role === 'admin';
    if (!hasPerm) {
      return <Navigate to="/" replace />;
    }
  }

  return (
    <Suspense fallback={<div className="p-8 text-center text-muted-foreground">Loading...</div>}>
      <route.Component />
    </Suspense>
  );
}
