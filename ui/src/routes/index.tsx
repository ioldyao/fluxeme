import { Suspense } from 'react';
import { Routes, Route } from 'react-router-dom';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { AdminRoute } from '@/components/AdminRoute';
import { Layout } from '@/components/Layout';
import { publicRoutes, authRoutes, adminRoutes, catchAllRoutes } from './config';
import type { RouteConfig } from './config';

function renderRoute(r: RouteConfig) {
  const element = (
    <Suspense fallback={<div className="p-8 text-center text-muted-foreground">Loading...</div>}>
      <r.Component />
    </Suspense>
  );
  return (
    <Route key={r.path ?? 'index'} {...(r.index ? { index: true } : { path: r.path })} element={element} />
  );
}

export function AppRoutes() {
  return (
    <Routes>
      {publicRoutes.map(renderRoute)}
      <Route element={<ProtectedRoute />}>
        <Route element={<Layout />}>
          {authRoutes.map(renderRoute)}
          <Route element={<AdminRoute />}>
            {adminRoutes.map(renderRoute)}
          </Route>
        </Route>
      </Route>
      {catchAllRoutes.map(renderRoute)}
    </Routes>
  );
}
