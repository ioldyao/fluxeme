import { Suspense } from 'react';
import { Route, Routes } from 'react-router-dom';
import { AdminLayout } from '@/components/AdminLayout';
import { AdminRoute } from '@/components/AdminRoute';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { UserLayout } from '@/components/UserLayout';
import { adminRoutes, authRoutes, catchAllRoutes, publicRoutes } from './config';
import type { RouteConfig } from './config';

function renderRoute(route: RouteConfig) {
  const element = (
    <Suspense fallback={<div className="p-8 text-center text-muted-foreground">Loading...</div>}>
      <route.Component />
    </Suspense>
  );

  return (
    <Route
      key={route.path ?? 'index'}
      {...(route.index ? { index: true } : { path: route.path })}
      element={element}
    />
  );
}

export function AppRoutes() {
  return (
    <Routes>
      {publicRoutes.map(renderRoute)}
      <Route element={<ProtectedRoute />}>
        <Route element={<UserLayout />}>{authRoutes.map(renderRoute)}</Route>
        <Route element={<AdminRoute />}>
          <Route element={<AdminLayout />}>{adminRoutes.map(renderRoute)}</Route>
        </Route>
      </Route>
      {catchAllRoutes.map(renderRoute)}
    </Routes>
  );
}
