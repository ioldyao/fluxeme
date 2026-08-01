import { lazy, type ComponentType } from 'react';
import {
  Braces,
  Cog,
  Key,
  LayoutDashboard,
  Receipt,
  Route,
  ScrollText,
  User,
  Wallet,
} from 'lucide-react';

export type RouteGuard = 'public' | 'auth';

export interface RouteConfig {
  path?: string;
  index?: boolean;
  Component: ComponentType;
  guard: RouteGuard;
  label?: string;
  icon?: ComponentType<{ className?: string }>;
  nav?: boolean;
  end?: boolean;
}

export type NavRoute = RouteConfig & Required<Pick<RouteConfig, 'nav'>>;

const isNavRoute = (route: RouteConfig): route is NavRoute => !!route.nav;

export const publicRoutes: RouteConfig[] = [
  { path: '/login', Component: lazy(() => import('@/pages/Login')), guard: 'public' },
  { path: '/register', Component: lazy(() => import('@/pages/Register')), guard: 'public' },
  { path: '/sso/callback', Component: lazy(() => import('@/pages/SsoCallback')), guard: 'public' },
];

export const authRoutes: RouteConfig[] = [
  { index: true, path: '/', Component: lazy(() => import('@/pages/Dashboard')), guard: 'auth', label: 'nav.dashboard', icon: LayoutDashboard, nav: true, end: true },
  { path: '/models/marketplace', Component: lazy(() => import('@/pages/ModelsMarketplace')), guard: 'auth', label: 'nav.modelMarketplace', icon: Braces, nav: true },
  { path: '/models/routes', Component: lazy(() => import('@/pages/MyRules')), guard: 'auth', label: 'nav.myRules', icon: Route, nav: true },
  { path: '/api-keys', Component: lazy(() => import('@/pages/ApiKeys')), guard: 'auth', label: 'nav.apiKeys', icon: Key, nav: true },
  { path: '/usage', Component: lazy(() => import('@/pages/Usage')), guard: 'auth', label: 'nav.usage', icon: ScrollText, nav: true },
  { path: '/wallet', Component: lazy(() => import('@/pages/Wallet')), guard: 'auth', label: 'nav.wallet', icon: Wallet, nav: true },
  { path: '/bills', Component: lazy(() => import('@/pages/Bills')), guard: 'auth', label: 'nav.bills', icon: Receipt, nav: true },
  { path: '/profile', Component: lazy(() => import('@/pages/Profile')), guard: 'auth', label: 'nav.profile', icon: User, nav: true },
  { path: '/settings', Component: lazy(() => import('@/pages/Settings')), guard: 'auth', label: 'nav.settings', icon: Cog, nav: true },
];

export const catchAllRoutes: RouteConfig[] = [
  { path: '*', Component: lazy(() => import('@/pages/NotFound')), guard: 'public' },
];

export const userNavRoutes = authRoutes.filter(isNavRoute);
