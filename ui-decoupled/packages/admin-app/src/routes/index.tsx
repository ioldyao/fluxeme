import { Navigate, Route, Routes } from 'react-router-dom';
import { ProtectedRoute } from '@shared/routes/ProtectedRoute';
import { PermissionRoute } from '@shared/routes/PermissionRoute';
import { AdminLayout } from '@/components/AdminLayout';
import { AdminRoute } from '@/components/AdminRoute';
import { publicRoutes, adminRoutes, catchAllRoutes } from './config';
import type { RouteConfig } from './config';

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
          <Route element={<AdminLayout />}>
            <Route index element={<Navigate to="/flow-control" replace />} />
            {adminRoutes.map(renderRoute)}
          </Route>
        </Route>
      </Route>
      {catchAllRoutes.map(renderRoute)}
    </Routes>
  );
}
