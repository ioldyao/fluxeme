import { LayoutShell } from './Layout';
import { UserSidebar } from './Sidebar';
import { TopBar } from './TopBar';

export function UserLayout() {
  return (
    <LayoutShell
      sidebar={<UserSidebar />}
      topBar={<TopBar adminEntryPath="/flow-control" />}
    />
  );
}
