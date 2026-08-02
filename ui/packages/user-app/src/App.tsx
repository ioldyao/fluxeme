import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@fluxeme/shared';
import { useSessionQuery, useCurrentSession } from '@fluxeme/shared/src/api/auth';
import { loadCurrencySettings } from '@fluxeme/shared/src/api/settings';
import { AppRoutes } from './routes';
import { useAuth } from '@fluxeme/shared/src/store/auth';

function SessionBootstrapper() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const setCurrentSession = useAuth((s) => s.setCurrentSession);
  const setPermissions = useAuth((s) => s.setPermissions);
  const clear = useAuth((s) => s.clear);

  // Phase 1: Session probe (always 200, anonymous-safe)
  const session = useSessionQuery();
  const isAuthed = session.data?.authenticated ?? false;

  // Phase 2: When authenticated, fetch full session detail + permissions
  const currentSession = useCurrentSession(isAuthed && !isSessionResolved);
  const permsQuery = useQuery({
    queryKey: ['auth', 'permissions'],
    queryFn: () => api<string[]>('/me/permissions'),
    enabled: isAuthed && isSessionResolved,
    staleTime: 120_000,
  });

  // Session detail resolved → set current session
  useEffect(() => {
    if (currentSession.isSuccess) {
      setCurrentSession(currentSession.data);
    }
  }, [currentSession.data, currentSession.isSuccess, setCurrentSession]);

  // Session probe says not authenticated → resolve as anonymous
  useEffect(() => {
    if (session.isSuccess && !session.data.authenticated && !isSessionResolved) {
      clear();
    }
  }, [session.isSuccess, session.data?.authenticated, isSessionResolved, clear]);

  // Permissions loaded
  useEffect(() => {
    if (permsQuery.data) setPermissions(permsQuery.data);
  }, [permsQuery.data, setPermissions]);

  // Load global currency settings (public API)
  useEffect(() => {
    void loadCurrencySettings();
  }, []);

  // Loading state
  if (session.isPending) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background p-4">
        <p className="text-muted-foreground">Loading...</p>
      </div>
    );
  }

  // Non-401 server error
  if (session.isError) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 p-4">
        <div className="max-w-sm space-y-3 text-center">
          <p className="text-sm font-medium text-foreground">Unable to connect</p>
          <p className="text-sm text-muted-foreground">
            The authentication service may be temporarily unavailable.
          </p>
          <button
            autoFocus
            className="text-sm text-primary underline"
            onClick={() => void session.refetch()}
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  return null;
}

export default function App() {
  return (
    <>
      <SessionBootstrapper />
      <AppRoutes />
    </>
  );
}
