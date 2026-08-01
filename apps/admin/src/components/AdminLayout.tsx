import { LayoutShell } from './Layout';
import { AdminSidebar } from './Sidebar';
import { TopBar } from './TopBar';

export function AdminLayout() {
  return <LayoutShell sidebar={<AdminSidebar />} topBar={<TopBar />} />;
}
