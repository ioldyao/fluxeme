import type { ReactNode } from 'react';
import { Outlet } from 'react-router-dom';

type LayoutShellProps = {
  sidebar: ReactNode;
  topBar: ReactNode;
};

export function LayoutShell({ sidebar, topBar }: LayoutShellProps) {
  return (
    <div className="flex min-h-screen bg-background">
      {sidebar}
      <div className="ml-48 flex min-w-0 flex-1 flex-col">
        {topBar}
        <main className="flex-1 animate-fade-in p-4 lg:p-6">
          <div className="w-full min-w-0">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
