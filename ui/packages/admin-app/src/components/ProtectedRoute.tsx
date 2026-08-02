import { Navigate, Outlet } from 'react-router-dom';
import { useAuth } from '@fluxeme/shared/src/store/auth';

export function ProtectedRoute() {
  const isAuthenticated = useAuth((s) => s.isAuthenticated);
  const isSessionResolved = useAuth((s) => s.isSessionResolved);

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  return <Outlet />;
}
