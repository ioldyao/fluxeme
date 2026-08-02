import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useCurrentSession } from '@shared/api/auth';
import { useAuth } from '@shared/store/auth';
import { Cog } from 'lucide-react';

export default function SsoCallback() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const setCurrentSession = useAuth((s) => s.setCurrentSession);
  const clear = useAuth((s) => s.clear);
  const [error, setError] = useState('');
  const currentSession = useCurrentSession(true);
  const isUnauthorized = useMemo(
    () => currentSession.error instanceof Error && currentSession.error.message === 'unauthorized',
    [currentSession.error],
  );

  useEffect(() => {
    if (window.location.hash) {
      window.location.hash = '';
      window.history.replaceState(null, '', window.location.pathname);
    }
  }, []);

  useEffect(() => {
    if (currentSession.isSuccess) {
      setCurrentSession(currentSession.data);
      navigate('/', { replace: true });
    }
  }, [currentSession.data, currentSession.isSuccess, navigate, setCurrentSession]);

  useEffect(() => {
    if (!currentSession.isError) {
      return;
    }

    if (isUnauthorized) {
      clear();
      setError(t('sso.verifyFailed'));
      return;
    }

    setError(t('sso.serviceUnavailable'));
  }, [clear, currentSession.isError, isUnauthorized, t]);

  if (error) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background p-4">
        <div className="text-center space-y-4">
          <Cog className="h-8 w-8 text-destructive mx-auto" />
          <p className="text-destructive">{error}</p>
          <button
            className="text-sm text-primary underline"
            onClick={() => navigate('/login')}
          >
            {t('sso.backToLogin')}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <div className="text-center space-y-4">
        <Cog className="h-8 w-8 text-brand mx-auto animate-spin" />
        <p className="text-muted-foreground">{t('sso.loading')}</p>
      </div>
    </div>
  );
}
