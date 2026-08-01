import { Suspense } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import { AdminLayout } from '@/components/AdminLayout';
import { AdminRoute } from '@/components/AdminRoute';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { useAuth } from '@/store/auth';
import { adminRoutes, catchAllRoutes, publicRoutes } from './config';
import type { RouteConfig } from './config';

function PermissionRoute({ route }: { route: RouteConfig }) {
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

function renderRoute(route: RouteConfig) {
  return (
    <Route
      key={route.path ?? 'index'}
      {...(route.index ? { index: true } : { path: route.path })}
      element={<PermissionRoute route={route} />}
    />
  );
}

export function AppRoutes() {
  return (
    <Routes>
      {publicRoutes.map(renderRoute)}
      <Route element={<ProtectedRoute />}>
        <Route element={<AdminRoute />}>
          <Route element={<AdminLayout />}>{adminRoutes.map(renderRoute)}</Route>
        </Route>
      </Route>
      {catchAllRoutes.map(renderRoute)}
    </Routes>
  );
}
