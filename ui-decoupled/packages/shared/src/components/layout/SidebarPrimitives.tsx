import type { ComponentType, ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { Cog } from 'lucide-react';
import type { ComponentType as CT } from 'react';

/**
 * Minimal shape a NavRoute must satisfy to be rendered by the sidebar.
 * Each app defines its own concrete NavRoute type (derived from its own
 * routes/config.ts) and it must be structurally compatible with this.
 */
export type SidebarNavRoute = {
  path?: string;
  label?: string;
  icon?: CT<{ className?: string }>;
  end?: boolean;
};

export type NavGroup = {
  label: string;
  items: string[];
};

export function createNavIndex<T extends SidebarNavRoute>(routes: T[]): Record<string, T> {
  return Object.fromEntries(
    routes
      .filter((route): route is T & { label: string } => Boolean(route.label))
      .map((route) => [route.label, route]),
  );
}

export function resolveNavItems<T extends SidebarNavRoute>(
  byLabel: Record<string, T>,
  labels: string[],
): T[] {
  return labels.map((label) => byLabel[label]).filter(Boolean);
}

type SidebarFrameProps = {
  badgeLabel?: string;
  children: ReactNode;
  footer?: ReactNode;
};

export function SidebarFrame({ badgeLabel, children, footer }: SidebarFrameProps) {
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

type SidebarSectionProps<T extends SidebarNavRoute> = {
  label: string;
  items: T[];
};

export function SidebarSection<T extends SidebarNavRoute>({ label, items }: SidebarSectionProps<T>) {
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

type SidebarNavItemProps<T extends SidebarNavRoute> = {
  item: T;
  icon?: ComponentType<{ className?: string }>;
};

export function SidebarNavItem<T extends SidebarNavRoute>({ item, icon: IconOverride }: SidebarNavItemProps<T>) {
  const { t } = useTranslation();
  const Icon = IconOverride ?? item.icon;

  return (
    <NavLink
      to={item.path ?? '/'}
      end={item.end}
      className={({ isActive }: { isActive: boolean }) => `nav-link ${isActive ? 'active' : 'text-muted-foreground'}`}
    >
      {Icon ? <Icon className="h-4 w-4" /> : null}
      {item.label ? t(item.label) : null}
    </NavLink>
  );
}
