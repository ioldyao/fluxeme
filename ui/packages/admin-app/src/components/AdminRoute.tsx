import { Outlet } from 'react-router-dom';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { AdminAccessDeniedPage } from './AdminAccessDeniedPage';

export function AdminRoute() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const permissions = useAuth((s) => s.permissions);

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  const canAccessAdmin = permissions.includes('admin:dashboard');

  if (!canAccessAdmin) {
    return <AdminAccessDeniedPage />;
  }

  return <Outlet />;
}
