import { useTranslation } from 'react-i18next';
import {
  SidebarFrame,
  SidebarSection,
  SidebarNavItem,
  createNavIndex,
  resolveNavItems,
  type NavGroup,
} from '@shared/components/layout/SidebarPrimitives';
import { userNavRoutes } from '@/routes/config';

const USER_NAV_GROUPS: NavGroup[] = [
  { label: 'nav.group.overview', items: ['nav.dashboard'] },
  { label: 'nav.group.models', items: ['nav.modelMarketplace'] },
  { label: 'nav.group.developer', items: ['nav.apiKeys', 'nav.myRules', 'nav.usage'] },
];

const USER_SECONDARY_ITEMS = ['nav.wallet', 'nav.bills', 'nav.profile', 'nav.settings'];

export function UserSidebar() {
  useTranslation(); // ensure re-render on language change, matches original behavior
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
