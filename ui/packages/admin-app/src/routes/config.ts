import { lazy, type ComponentType } from 'react';
import {
  Bell,
  Cog,
  Cpu,
  DollarSign,
  KeyRound,
  Package,
  Radio,
  Receipt,
  Route,
  WalletCards,
  ScrollText,
  Shield,
  Users,
} from 'lucide-react';
import type { Permission } from '@fluxeme/shared/src/permissions/usePermission';

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
  /** Fine-grained Casbin permission required (admin routes only). */
  perm?: Permission;
}

export type NavRoute = RouteConfig & Required<Pick<RouteConfig, 'nav'>>;

const isNavRoute = (route: RouteConfig): route is NavRoute => !!route.nav;

export const publicRoutes: RouteConfig[] = [
  { path: '/login', Component: lazy(() => import('@/pages/Login')), guard: 'public' },
  { path: '/register', Component: lazy(() => import('@/pages/Register')), guard: 'public' },
  { path: '/sso/callback', Component: lazy(() => import('@/pages/SsoCallback')), guard: 'public' },
];

export const adminRoutes: RouteConfig[] = [
  { index: true, Component: lazy(() => import('@/pages/FlowControlTower')), guard: 'admin', label: 'nav.flowControl', icon: Radio, nav: true, perm: 'admin:dashboard' },
  { path: '/flow-control', Component: lazy(() => import('@/pages/FlowControlTower')), guard: 'admin', label: 'nav.flowControl', icon: Radio, nav: true, perm: 'admin:dashboard' },
  { path: '/users', Component: lazy(() => import('@/pages/Users')), guard: 'admin', label: 'nav.users', icon: Users, nav: true, perm: 'admin:users' },
  { path: '/teams', Component: lazy(() => import('@/pages/Teams')), guard: 'admin', label: 'nav.teams', icon: Users, nav: true, perm: 'admin:teams' },
  { path: '/channels', Component: lazy(() => import('@/pages/Channels')), guard: 'admin', label: 'nav.channels', icon: Radio, nav: true, perm: 'admin:channels' },
  { path: '/models', Component: lazy(() => import('@/pages/Models')), guard: 'admin', label: 'nav.models', icon: Cpu, nav: true, end: true, perm: 'admin:models' },
  { path: '/moderation', Component: lazy(() => import('@/pages/Moderation')), guard: 'admin', label: 'nav.moderation', icon: Shield, nav: true, perm: 'admin:moderation' },
  { path: '/rules', Component: lazy(() => import('@/pages/Rules')), guard: 'admin', label: 'nav.rules', icon: Route, nav: true, perm: 'admin:rules' },
  { path: '/pricing', Component: lazy(() => import('@/pages/ModelPricing')), guard: 'admin', label: 'nav.modelPricing', icon: DollarSign, nav: true, perm: 'admin:model-pricing' },
  { path: '/billing', Component: lazy(() => import('@/pages/Billing')), guard: 'admin', label: 'nav.bills', icon: Receipt, nav: true, perm: 'admin:bills' },
  { path: '/billing-groups', Component: lazy(() => import('@/pages/BillingGroups')), guard: 'admin', label: 'nav.billingGroups', icon: WalletCards, nav: true, perm: 'admin:billing-groups' },
  { path: '/usage-log', Component: lazy(() => import('@/pages/UsageLog')), guard: 'admin', label: 'nav.usage', icon: ScrollText, nav: true, perm: 'admin:usage' },
  { path: '/recharge-keys', Component: lazy(() => import('@/pages/admin/RechargeKeys')), guard: 'admin', label: 'nav.rechargeKeys', icon: KeyRound, nav: true, perm: 'admin:recharge-keys' },
  { path: '/announcements', Component: lazy(() => import('@/pages/admin/Announcements')), guard: 'admin', label: 'nav.announcements', icon: Bell, nav: true, perm: 'admin:announcements' },
  { path: '/skills', Component: lazy(() => import('@/pages/SkillHubAdmin')), guard: 'admin', label: 'nav.skillHubAdmin', icon: Package, nav: true, perm: 'admin:skillhub' },
  { path: '/token-packages', Component: lazy(() => import('@/pages/TokenPackages')), guard: 'admin', label: 'nav.tokenPackages', icon: Package, nav: true, perm: 'admin:bills' },
  { path: '/gateway-settings', Component: lazy(() => import('@/pages/admin/AdminSettings')), guard: 'admin', label: 'nav.adminSettings', icon: Cog, nav: true, perm: 'admin:settings' },
  { path: '/management-keys', Component: lazy(() => import('@/pages/admin/ManagementKeys')), guard: 'admin', label: 'nav.managementKeys', icon: KeyRound, nav: true, perm: 'admin:management-keys' },
];

export const catchAllRoutes: RouteConfig[] = [
  { path: '*', Component: lazy(() => import('@/pages/NotFound')), guard: 'public' },
];

export const adminNavRoutes = adminRoutes.filter(isNavRoute);
