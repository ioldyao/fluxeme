import { lazy, type ComponentType } from 'react';
import {
  LayoutDashboard,
  Users,
  Radio,
  Braces,
  Key,
  Route,
  ScrollText,
  Cog,
  DollarSign,
} from 'lucide-react';

export type RouteGuard = 'public' | 'auth' | 'admin';

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

export const publicRoutes: RouteConfig[] = [
  { path: '/login', Component: lazy(() => import('@/pages/Login')), guard: 'public' },
  { path: '/register', Component: lazy(() => import('@/pages/Register')), guard: 'public' },
  { path: '/sso/callback', Component: lazy(() => import('@/pages/SsoCallback')), guard: 'public' },
];

export const authRoutes: RouteConfig[] = [
  { index: true, path: '/', Component: lazy(() => import('@/pages/Dashboard')), guard: 'auth', label: 'nav.dashboard', icon: LayoutDashboard, nav: true, end: true },
  { path: '/models/marketplace', Component: lazy(() => import('@/pages/ModelsMarketplace')), guard: 'auth', label: 'nav.modelMarketplace', icon: Braces, nav: true },
  { path: '/models/mine', Component: lazy(() => import('@/pages/MyModels')), guard: 'auth', label: 'nav.myModels', icon: Braces, nav: true },
  { path: '/api-keys', Component: lazy(() => import('@/pages/ApiKeys')), guard: 'auth', label: 'nav.apiKeys', icon: Key, nav: true },
  { path: '/usage', Component: lazy(() => import('@/pages/Usage')), guard: 'auth', label: 'nav.usage', icon: ScrollText, nav: true },
  { path: '/profile', Component: lazy(() => import('@/pages/Profile')), guard: 'auth' },
  { path: '/settings', Component: lazy(() => import('@/pages/Settings')), guard: 'auth', label: 'nav.settings', icon: Cog, nav: true },
];

export const adminRoutes: RouteConfig[] = [
  { path: '/users', Component: lazy(() => import('@/pages/Users')), guard: 'admin', label: 'nav.users', icon: Users, nav: true },
  { path: '/channels', Component: lazy(() => import('@/pages/Channels')), guard: 'admin', label: 'nav.channels', icon: Radio, nav: true },
  { path: '/models', Component: lazy(() => import('@/pages/Models')), guard: 'admin', label: 'nav.models', icon: Braces, nav: true, end: true },
  { path: '/rules', Component: lazy(() => import('@/pages/Rules')), guard: 'admin', label: 'nav.rules', icon: Route, nav: true },
  { path: '/pricing', Component: lazy(() => import('@/pages/ModelPricing')), guard: 'admin', label: 'nav.modelPricing', icon: DollarSign, nav: true },
];

export const catchAllRoutes: RouteConfig[] = [
  { path: '*', Component: lazy(() => import('@/pages/NotFound')), guard: 'public' },
];

export const navRoutes: (RouteConfig & Required<Pick<RouteConfig, 'nav'>>)[] = [
  ...authRoutes.filter((r): r is RouteConfig & Required<Pick<RouteConfig, 'nav'>> => !!r.nav),
  ...adminRoutes.filter((r): r is RouteConfig & Required<Pick<RouteConfig, 'nav'>> => !!r.nav),
];
