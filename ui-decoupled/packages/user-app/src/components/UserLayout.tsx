import { LayoutShell } from '@shared/components/layout/LayoutShell';
import { TopBar } from '@shared/components/layout/TopBar';
import { UserSidebar } from './UserSidebar';

// Absolute URL of the admin app, e.g. "http://192.168.x.x:5174/".
// Configure via VITE_ADMIN_APP_URL; the "go to admin" button is hidden
// entirely (see TopBar) when this is unset.
const ADMIN_APP_URL = import.meta.env.VITE_ADMIN_APP_URL as string | undefined;

export function UserLayout() {
  return (
    <LayoutShell
      sidebar={<UserSidebar />}
      topBar={<TopBar adminEntryUrl={ADMIN_APP_URL} />}
    />
  );
}
