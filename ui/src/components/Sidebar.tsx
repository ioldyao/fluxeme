import type { ComponentType, ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { Cog } from 'lucide-react';
import { adminNavRoutes, type NavRoute, userNavRoutes } from '@/routes/config';

type NavGroup = {
  label: string;
  items: string[];
};

const USER_NAV_GROUPS: NavGroup[] = [
  { label: 'nav.group.overview', items: ['nav.dashboard'] },
  { label: 'nav.group.models', items: ['nav.modelMarketplace'] },
  { label: 'nav.group.developer', items: ['nav.apiKeys', 'nav.myRules', 'nav.usage'] },
];

const USER_SECONDARY_ITEMS = ['nav.wallet', 'nav.bills', 'nav.profile', 'nav.settings'];

const ADMIN_NAV_GROUPS: NavGroup[] = [
  { label: 'nav.group.overview', items: ['nav.flowControl'] },
  { label: 'nav.group.models', items: ['nav.models', 'nav.modelPricing'] },
  { label: 'nav.group.channels', items: ['nav.channels'] },
  { label: 'nav.group.security', items: ['nav.rules', 'nav.moderation', 'nav.users', 'nav.rechargeKeys'] },
];

function createNavIndex(routes: NavRoute[]) {
  return Object.fromEntries(
    routes
      .filter((route): route is NavRoute & { label: string } => Boolean(route.label))
      .map((route) => [route.label, route]),
  );
}

function resolveNavItems(byLabel: Record<string, NavRoute>, labels: string[]) {
  return labels.map((label) => byLabel[label]).filter(Boolean);
}

type SidebarFrameProps = {
  badgeLabel?: string;
  children: ReactNode;
  footer?: ReactNode;
};

function SidebarFrame({ badgeLabel, children, footer }: SidebarFrameProps) {
  const { t } = useTranslation();

  return (
    <aside className="fixed left-0 top-0 z-30 flex h-screen w-48 flex-col border-r bg-sidebar">
      <div className="flex h-14 items-center gap-2 border-b px-5">
        <Cog className="h-5 w-5 text-brand" />
        <span className="text-sm font-semibold">{t('nav.subtitle')}</span>
        {badgeLabel ? (
          <span className="rounded-full bg-brand/10 px-2 py-0.5 text-[10px] font-semibold text-brand">
            {badgeLabel}
          </span>
        ) : null}
      </div>
      <nav className="flex-1 space-y-0 overflow-y-auto p-3">{children}</nav>
      {footer ? <div className="mt-auto space-y-0.5 border-t p-3">{footer}</div> : null}
    </aside>
  );
}

type SidebarSectionProps = {
  label: string;
  items: NavRoute[];
};

function SidebarSection({ label, items }: SidebarSectionProps) {
  const { t } = useTranslation();

  if (items.length === 0) {
    return null;
  }

  return (
    <div>
      <div className="px-3 pb-1">
        <span className="select-none text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/35">
          {t(label)}
        </span>
      </div>
      <div className="space-y-0.5">
        {items.map((item) => (
          <SidebarNavItem key={item.path ?? item.label} item={item} />
        ))}
      </div>
    </div>
  );
}

type SidebarNavItemProps = {
  item: NavRoute;
  icon?: ComponentType<{ className?: string }>;
};

function SidebarNavItem({ item, icon: IconOverride }: SidebarNavItemProps) {
  const { t } = useTranslation();
  const Icon = IconOverride ?? item.icon;

  return (
    <NavLink
      to={item.path ?? '/'}
      end={item.end}
      className={({ isActive }) => `nav-link ${isActive ? 'active' : 'text-muted-foreground'}`}
    >
      {Icon ? <Icon className="h-4 w-4" /> : null}
      {item.label ? t(item.label) : null}
    </NavLink>
  );
}

export function UserSidebar() {
  const byLabel = createNavIndex(userNavRoutes);

  return (
    <SidebarFrame
      footer={resolveNavItems(byLabel, USER_SECONDARY_ITEMS).map((item) => (
        <SidebarNavItem key={item.path ?? item.label} item={item} />
      ))}
    >
      {USER_NAV_GROUPS.map((group) => (
        <SidebarSection
          key={group.label}
          label={group.label}
          items={resolveNavItems(byLabel, group.items)}
        />
      ))}
    </SidebarFrame>
  );
}

export function AdminSidebar() {
  const { t } = useTranslation();
  const byLabel = createNavIndex(adminNavRoutes);

  return (
    <SidebarFrame badgeLabel={t('nav.adminEntry')}>
      {ADMIN_NAV_GROUPS.map((group) => (
        <SidebarSection
          key={group.label}
          label={group.label}
          items={resolveNavItems(byLabel, group.items)}
        />
      ))}
    </SidebarFrame>
  );
}
