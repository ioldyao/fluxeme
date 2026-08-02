import { Navigate, Outlet } from 'react-router-dom';
import { useAuth } from '@shared/store/auth';

/**
 * Gate for admin-only routes. Non-admin users are redirected to '/',
 * which in admin-app resolves to the FlowControlTower entry (the only
 * routes this app serves are admin routes).
 */
export function AdminRoute() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const role = useAuth((s) => s.role);

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  if (role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return <Outlet />;
}
