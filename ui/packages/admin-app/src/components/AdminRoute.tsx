import { Outlet } from 'react-router-dom';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { AdminAccessDeniedPage } from './AdminAccessDeniedPage';

export function AdminRoute() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const permissions = useAuth((s) => s.permissions);
  const role = useAuth((s) => s.role);

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  // Permissions load asynchronously after the session resolves. Until they
  // arrive the array is empty, so fall back to the role (matching PermissionRoute
  // and usePermission) instead of denying access on an empty list.
  const canAccessAdmin = permissions.length > 0
    ? permissions.includes('admin:dashboard')
    : role === 'admin';

  if (!canAccessAdmin) {
    return <AdminAccessDeniedPage />;
  }

  return <Outlet />;
}
