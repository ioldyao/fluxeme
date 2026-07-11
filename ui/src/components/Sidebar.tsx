import { NavLink } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { Cog, Settings, User } from 'lucide-react';
import { navRoutes } from '@/routes/config';

const NAV_GROUPS: { label: string; items: string[] }[] = [
  { label: '', items: ['nav.dashboard'] },
  { label: 'nav.group.models', items: ['nav.modelMarketplace', 'nav.myModels'] },
  { label: 'nav.group.management', items: ['nav.users', 'nav.channels', 'nav.models', 'nav.rules', 'nav.modelPricing'] },
  { label: 'nav.group.developer', items: ['nav.apiKeys', 'nav.usage'] },
];

export function Sidebar() {
  const { t } = useTranslation();
  const role = useAuth((s) => s.role);

  const visible = navRoutes.filter((item) => item.guard !== 'admin' || role === 'admin');
  const settings = visible.filter((item) => item.label === 'nav.settings');
  const byLabel = Object.fromEntries(visible.map((item) => [item.label, item]));

  return (
    <aside className="w-60 h-screen fixed left-0 top-0 border-r bg-sidebar flex flex-col z-30">
      <div className="flex items-center gap-2 px-5 h-14 border-b">
        <Cog className="h-5 w-5 text-brand" />
        <span className="font-semibold text-sm">{t('nav.subtitle')}</span>
      </div>
      <nav className="flex-1 overflow-y-auto p-3 space-y-4">
        {NAV_GROUPS.map((group) => {
          const items = group.items.map((lbl) => byLabel[lbl]).filter(Boolean);
          if (items.length === 0) return null;
          return (
            <div key={group.label || '__standalone'}>
              {group.label && (
                <div className="px-3 pb-1">
                  <span className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/35 select-none">
                    {t(group.label)}
                  </span>
                </div>
              )}
              <div className="space-y-0.5">
                {items.map((item) => (
                  <NavLink
                    key={item.path ?? item.label}
                    to={item.path ?? '/'}
                    end={item.end}
                    className={({ isActive }) =>
                      `flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
                        isActive
                          ? 'bg-accent text-accent-foreground font-medium'
                          : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
                      }`
                    }
                  >
                    {item.icon && <item.icon className="h-4 w-4" />}
                    {item.label && t(item.label)}
                  </NavLink>
                ))}
              </div>
            </div>
          );
        })}
      </nav>
      <div className="p-3 border-t mt-auto space-y-0.5">
        <NavLink
          to="/profile"
          className={({ isActive }) =>
            `flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
              isActive
                ? 'bg-accent text-accent-foreground font-medium'
                : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
            }`
          }
        >
          <User className="h-4 w-4" />
          {t('nav.profile')}
        </NavLink>
        {settings.map((item) => (
          <NavLink
            key={item.path ?? item.label}
            to={item.path ?? '/'}
            end={item.end}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
                isActive
                  ? 'bg-accent text-accent-foreground font-medium'
                  : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
              }`
            }
          >
            <Settings className="h-4 w-4" />
            {item.label && t(item.label)}
          </NavLink>
        ))}
      </div>
    </aside>
  );
}
