import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@fluxeme/shared';
import { useCurrentSession } from '@fluxeme/shared/src/api/auth';
import { loadCurrencySettings } from '@fluxeme/shared/src/api/settings';
import { AppRoutes } from './routes';
import { useAuth } from '@fluxeme/shared/src/store/auth';

function SessionBootstrapper() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const isAuthenticated = useAuth((s) => s.isAuthenticated);
  const userId = useAuth((s) => s.userId);
  const setCurrentSession = useAuth((s) => s.setCurrentSession);
  const setPermissions = useAuth((s) => s.setPermissions);
  const clear = useAuth((s) => s.clear);

  const pathname = window.location.pathname;
  const isSsoCallbackRoute = pathname === '/sso/callback';
  const isPublicAuthRoute = pathname === '/login' || pathname === '/register';
  // There is a persisted session hint in localStorage
  const hasSessionHint = isAuthenticated || Boolean(userId);
  // Only verify session on the server when we have a reason to believe
  // there IS a session — otherwise let the route guard redirect declaratively.
  const shouldCheckSession =
    !isSessionResolved && !isSsoCallbackRoute && hasSessionHint;

  const currentSession = useCurrentSession(shouldCheckSession);
  const isUnauthorized =
    currentSession.error instanceof Error &&
    currentSession.error.message === 'unauthorized';

  // Session resolved by server response
  useEffect(() => {
    if (currentSession.isSuccess) {
      setCurrentSession(currentSession.data);
    }
  }, [currentSession.data, currentSession.isSuccess, setCurrentSession]);

  // No local session hint and not on a public page → no point checking with
  // the server. Mark resolved immediately so the route guard can redirect.
  useEffect(() => {
    if (!isSessionResolved && !shouldCheckSession && !isPublicAuthRoute) {
      // user visits a protected page with no local session → fast path to login
      clear();
    }
  }, [isSessionResolved, shouldCheckSession, isPublicAuthRoute, clear]);

  // Clear auth on server 401
  useEffect(() => {
    if (!currentSession.isError) return;
    if (isUnauthorized) clear();
  }, [clear, currentSession.isError, isUnauthorized]);

  // Load global currency settings once authenticated
  const isAuthed = useAuth((s) => s.isAuthenticated);
  useEffect(() => {
    if (isAuthed) {
      void loadCurrencySettings();
    }
  }, [isAuthed]);

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

  // Non-401 server error on a non-public page → show retry UI
  if (currentSession.isError && !isUnauthorized && !isPublicAuthRoute) {
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