import { Navigate, Outlet, useLocation } from 'react-router-dom';
import { useAuth } from '@fluxeme/shared/src/store/auth';

export function ProtectedRoute() {
  const { isAuthenticated, isSessionResolved } = useAuth();
  const location = useLocation();

  if (!isSessionResolved) {
    return <div className="p-8 text-center text-muted-foreground">Loading...</div>;
  }

  if (!isAuthenticated) {
    // Remember where the user was trying to go, so we can send them back
    // after a successful login.
    return <Navigate to="/login" state={{ from: location.pathname }} replace />;
  }

  return <Outlet />;
}
