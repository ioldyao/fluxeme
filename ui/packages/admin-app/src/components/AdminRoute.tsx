import { Outlet } from 'react-router-dom';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { AdminAccessDeniedPage } from './AdminAccessDeniedPage';

export function AdminRoute() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const role = useAuth((s) => s.role);

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  if (role !== 'admin') {
    return <AdminAccessDeniedPage />;
  }

  return <Outlet />;
}
