import { NavLink } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { Cog, Settings } from 'lucide-react';
import { navRoutes } from '@/routes/config';

export function Sidebar() {
  const { t } = useTranslation();
  const role = useAuth((s) => s.role);

  const visible = navRoutes.filter((item) => item.guard !== 'admin' || role === 'admin');
  const settings = visible.filter((item) => item.label === 'nav.settings');
  const mainNav = visible.filter((item) => item.label !== 'nav.settings');

  return (
    <aside className="w-60 h-screen fixed left-0 top-0 border-r bg-sidebar flex flex-col z-30">
      <div className="flex items-center gap-2 px-5 h-14 border-b">
        <Cog className="h-5 w-5 text-brand" />
        <span className="font-semibold text-sm">{t('nav.subtitle')}</span>
      </div>
      <nav className="flex-1 p-3 space-y-1">
        {mainNav.map((item) => (
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
      </nav>
      <div className="p-3 border-t mt-auto">
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
