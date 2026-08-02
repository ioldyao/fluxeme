import { Route, Routes } from 'react-router-dom';
import { ProtectedRoute } from '@shared/routes/ProtectedRoute';
import { PermissionRoute } from '@shared/routes/PermissionRoute';
import { UserLayout } from '@/components/UserLayout';
import { publicRoutes, authRoutes, catchAllRoutes } from './config';
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
        <Route element={<UserLayout />}>{authRoutes.map(renderRoute)}</Route>
      </Route>
      {catchAllRoutes.map(renderRoute)}
    </Routes>
  );
}
