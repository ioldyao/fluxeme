import { LayoutShell } from '@shared/components/layout/LayoutShell';
import { TopBar } from '@shared/components/layout/TopBar';
import { AdminSidebar } from './AdminSidebar';

// Absolute URL of the user app, e.g. "http://192.168.x.x:5173/".
// Configure via VITE_USER_APP_URL; the "go to user" button is hidden
// entirely (see TopBar) when this is unset.
const USER_APP_URL = import.meta.env.VITE_USER_APP_URL as string | undefined;

export function AdminLayout() {
  return (
    <LayoutShell
      sidebar={<AdminSidebar />}
      topBar={<TopBar userEntryUrl={USER_APP_URL} />}
    />
  );
}
