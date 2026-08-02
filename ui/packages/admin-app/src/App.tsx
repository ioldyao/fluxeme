import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@fluxeme/shared';
import { useCurrentSession } from '@fluxeme/shared/src/api/auth';
import { loadCurrencySettings } from '@fluxeme/shared/src/api/settings';
import { AppRoutes } from './routes';
import { useAuth } from '@fluxeme/shared/src/store/auth';

function SessionBootstrapper() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const setCurrentSession = useAuth((s) => s.setCurrentSession);
  const setPermissions = useAuth((s) => s.setPermissions);
  const clear = useAuth((s) => s.clear);
  const isAuthed = useAuth((s) => s.isAuthenticated);

  // Always check session on mount — no localStorage guesswork.
  // /api/me with skipAuthErrorHandling returns 401 for anonymous users
  // without triggering toast/clear/redirect.
  const currentSession = useCurrentSession(!isSessionResolved);
  const isUnauthorized =
    currentSession.error instanceof Error &&
    currentSession.error.message === 'unauthorized';

  // Session resolved by server
  useEffect(() => {
    if (currentSession.isSuccess) {
      setCurrentSession(currentSession.data);
    }
  }, [currentSession.data, currentSession.isSuccess, setCurrentSession]);

  // Server returned 401 — mark resolved as anonymous
  useEffect(() => {
    if (isUnauthorized && !isSessionResolved) {
      clear();
    }
  }, [isUnauthorized, isSessionResolved, clear]);

  // Load global currency settings (public API, works for all)
  useEffect(() => {
    void loadCurrencySettings();
  }, []);

  // Load permissions once authenticated
  const { data: permsData } = useQuery({
    queryKey: ['auth', 'permissions'],
    queryFn: () => api<string[]>('/me/permissions'),
    enabled: isAuthed,
    staleTime: 120_000,
  });
  useEffect(() => {
    if (permsData) setPermissions(permsData);
  }, [permsData, setPermissions]);

  // Non-401 server error on mount → show retry UI
  if (currentSession.isError && !isUnauthorized) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 p-4">
        <div className="max-w-sm space-y-3 text-center">
          <p className="text-sm font-medium text-foreground">
            Unable to verify the current session.
          </p>
          <p className="text-sm text-muted-foreground">
            The authentication service may be temporarily unavailable.
          </p>
          <button
            autoFocus
            className="text-sm text-primary underline"
            onClick={() => void currentSession.refetch()}
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
