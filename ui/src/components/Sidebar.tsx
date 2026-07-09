import { NavLink } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import {
  LayoutDashboard,
  Users,
  Radio,
  Braces,
  Key,
  Route,
  ScrollText,
  Cog,
} from 'lucide-react';

const navItems: { to: string; label: string; icon: React.ComponentType<{ className?: string }>; adminOnly: boolean; end?: boolean }[] = [
  { to: '/', label: 'nav.dashboard', icon: LayoutDashboard, adminOnly: false, end: true },
  { to: '/users', label: 'nav.users', icon: Users, adminOnly: true },
  { to: '/channels', label: 'nav.channels', icon: Radio, adminOnly: true },
  { to: '/models', label: 'nav.models', icon: Braces, adminOnly: true, end: true },
  { to: '/models/marketplace', label: 'nav.modelMarketplace', icon: Braces, adminOnly: false },
  { to: '/models/mine', label: 'nav.myModels', icon: Braces, adminOnly: false },
  { to: '/api-keys', label: 'nav.apiKeys', icon: Key, adminOnly: false },
  { to: '/rules', label: 'nav.rules', icon: Route, adminOnly: true },
  { to: '/usage', label: 'nav.usage', icon: ScrollText, adminOnly: false },
  { to: '/settings', label: 'nav.settings', icon: Cog, adminOnly: false },
];

export function Sidebar() {
  const { t } = useTranslation();
  const role = useAuth((s) => s.role);

  return (
    <aside className="w-60 h-screen fixed left-0 top-0 border-r bg-sidebar flex flex-col z-30">
      <div className="flex items-center gap-2 px-5 h-14 border-b">
        <Cog className="h-5 w-5 text-brand" />
        <span className="font-semibold text-sm">{t('nav.subtitle')}</span>
      </div>
      <nav className="flex-1 p-3 space-y-1">
        {navItems
          .filter((item) => !item.adminOnly || role === 'admin')
          .map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) =>
                `flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
                  isActive
                    ? 'bg-accent text-accent-foreground font-medium'
                    : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
                }`
              }
            >
              <item.icon className="h-4 w-4" />
              {t(item.label)}
            </NavLink>
          ))}
      </nav>
    </aside>
  );
}
