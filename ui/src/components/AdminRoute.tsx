import { Navigate, Outlet } from 'react-router-dom';
import { useAuth } from '@/store/auth';

export function AdminRoute() {
  const role = useAuth((s) => s.role);
  if (role !== 'admin') return <Navigate to="/" replace />;
  return <Outlet />;
}
