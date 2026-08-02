import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@shared/api/client';
import { useCurrentSession } from '@shared/api/auth';
import { useAuth } from '@shared/store/auth';

/**
 * Resolves the current session on mount, fetches fine-grained Casbin
 * permissions once authenticated, and clears stale auth state on 401.
 * Shared verbatim between user-app and admin-app; both need identical
 * session-bootstrapping behavior even though their routes/pages differ.
 */
export function SessionBootstrapper() {
  const isSessionResolved = useAuth((s) => s.isSessionResolved);
  const isAuthenticated = useAuth((s) => s.isAuthenticated);
  const userId = useAuth((s) => s.userId);
  const setCurrentSession = useAuth((s) => s.setCurrentSession);
  const clear = useAuth((s) => s.clear);
  const pathname = window.location.pathname;
  const isSsoCallbackRoute = pathname === '/sso/callback';
  const isPublicAuthRoute = pathname === '/login' || pathname === '/register';
  const hasSessionHint = isAuthenticated || Boolean(userId);
  const currentSession = useCurrentSession(
    !isSessionResolved && !isSsoCallbackRoute && (!isPublicAuthRoute || hasSessionHint),
  );
  const isUnauthorized = currentSession.error instanceof Error && currentSession.error.message === 'unauthorized';

  useEffect(() => {
    if (currentSession.isSuccess) {
      setCurrentSession(currentSession.data);
    }
  }, [currentSession.data, currentSession.isSuccess, setCurrentSession]);

  // Fetch granular permissions from backend once authenticated
  const isAuthed = useAuth((s) => s.isAuthenticated);
  const setPermissions = useAuth((s) => s.setPermissions);
  const { data: permsData } = useQuery({
    queryKey: ['auth', 'permissions'],
    queryFn: () => api<string[]>('/me/permissions'),
    enabled: isAuthed,
    staleTime: 120_000,
  });
  useEffect(() => {
    if (permsData) {
      setPermissions(permsData);
    }
  }, [permsData, setPermissions]);

  useEffect(() => {
    if (!currentSession.isError) {
      return;
    }

    if (isUnauthorized) {
      clear();
      return;
    }
  }, [clear, currentSession.isError, isUnauthorized]);

  if (currentSession.isError && !isUnauthorized && !isPublicAuthRoute) {
    return (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 p-4"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="session-bootstrap-title"
        aria-describedby="session-bootstrap-description"
      >
        <div className="max-w-sm space-y-3 text-center">
          <p id="session-bootstrap-title" className="text-sm font-medium text-foreground">
            Unable to verify the current session.
          </p>
          <p id="session-bootstrap-description" className="text-sm text-muted-foreground">
            The authentication service may be temporarily unavailable.
          </p>
          <button
            autoFocus
            className="text-sm text-primary underline"
            onClick={() => {
              void currentSession.refetch();
            }}
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  return null;
}
