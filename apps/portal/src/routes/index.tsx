import { Suspense } from 'react';
import { Route, Routes } from 'react-router-dom';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { UserLayout } from '@/components/UserLayout';
import { authRoutes, catchAllRoutes, publicRoutes } from './config';
import type { RouteConfig } from './config';

function RenderRoute({ route }: { route: RouteConfig }) {
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
      element={<RenderRoute route={route} />}
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
