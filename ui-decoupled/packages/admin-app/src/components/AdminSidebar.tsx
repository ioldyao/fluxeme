import { useTranslation } from 'react-i18next';
import {
  SidebarFrame,
  SidebarSection,
  createNavIndex,
  resolveNavItems,
  type NavGroup,
} from '@shared/components/layout/SidebarPrimitives';
import { adminNavRoutes } from '@/routes/config';

const ADMIN_NAV_GROUPS: NavGroup[] = [
  { label: 'nav.group.overview', items: ['nav.flowControl'] },
  { label: 'nav.group.models', items: ['nav.models', 'nav.modelPricing'] },
  { label: 'nav.group.channels', items: ['nav.channels'] },
  { label: 'nav.group.security', items: ['nav.rules', 'nav.moderation', 'nav.users', 'nav.rechargeKeys', 'nav.announcements', 'nav.adminSettings'] },
];

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
