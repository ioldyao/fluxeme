import { useTranslation } from 'react-i18next';
import { useLogout } from '@fluxeme/shared/src/api/auth';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { Button } from '@fluxeme/shared/src/components/ui/button';

export function AdminAccessDeniedPage() {
  const { t } = useTranslation();
  const logout = useLogout();
  const clear = useAuth((s) => s.clear);
  const isLoading = logout.isPending;

  const handleSwitch = () => {
    logout.mutate(undefined, {
      onSuccess: () => {
        clear();
        window.location.replace('/admin/login');
      },
      onError: () => {
        clear();
        window.location.replace('/admin/login');
      },
    });
  };

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-background p-4">
      <div className="max-w-md text-center space-y-4">
        <div className="text-6xl">🔒</div>
        <h1 className="text-xl font-bold">{t('adminAccessDenied.title')}</h1>
        <p className="text-sm text-muted-foreground">
          {t('adminAccessDenied.description')}
        </p>
        <div className="pt-4 flex flex-col sm:flex-row gap-3 justify-center">
          <a
            href="/"
            className="inline-flex items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          >
            {t('adminAccessDenied.goToUserCenter')}
          </a>
          <Button
            variant="outline"
            onClick={handleSwitch}
            disabled={isLoading}
          >
            {t('adminAccessDenied.switchAccount')}
          </Button>
        </div>
      </div>
    </div>
  );
}
