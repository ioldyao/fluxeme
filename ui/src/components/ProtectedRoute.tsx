import { useEffect } from 'react';
import { Navigate, Outlet } from 'react-router-dom';
import { useAuth } from '@/store/auth';
import { useMyPermissions } from '@/api/auth';

export function ProtectedRoute() {
  const token = useAuth((s) => s.token);
  const setPermissions = useAuth((s) => s.setPermissions);

  // Sync permissions from backend on mount so old tokens (issued before RBAC)
  // or server-side permission changes take effect without re-login.
  const { data } = useMyPermissions();

  useEffect(() => {
    if (data?.permissions) {
      setPermissions(data.permissions);
    }
  }, [data, setPermissions]);

  if (!token) return <Navigate to="/login" replace />;
  return <Outlet />;
}
