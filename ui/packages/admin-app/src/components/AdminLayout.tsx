import { LayoutShell } from '@/components/Layout';
import { AdminSidebar } from '@/components/Sidebar';
import { TopBar } from '@/components/TopBar';

export function AdminLayout() {
  return (
    <LayoutShell sidebar={<AdminSidebar />} topBar={<TopBar userEntryPath="/" />} />
  );
}
