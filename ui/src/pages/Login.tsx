import { useState, useEffect } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useLogin } from '@/api/auth';
import { useAuth } from '@/store/auth';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Cog, LogIn } from 'lucide-react';
import { toast } from 'sonner';
import { api } from '@/api/client';

export default function Login() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const token = useAuth((s) => s.token);
  const login = useLogin();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [ssoEnabled, setSsoEnabled] = useState(false);
  const [ssoProviderName, setSsoProviderName] = useState('');
  const [ssoLoading, setSsoLoading] = useState(true);

  useEffect(() => {
    api<{ setup_required: boolean }>('/setup/status')
      .then((res) => {
        if (res.setup_required) {
          navigate('/register', { replace: true });
          return;
        }
      })
      .catch(() => {});
    api<{ enabled: boolean; provider_name: string }>('/sso/status')
      .then((res) => {
        setSsoEnabled(res.enabled);
        setSsoProviderName(res.provider_name);
      })
      .catch(() => {
        // SSO endpoint not available
      })
      .finally(() => setSsoLoading(false));
  }, [navigate]);

  if (token) return <Navigate to="/" replace />;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!username || !password) {
      toast.error(t('login.error'));
      return;
    }
    login.mutate(
      { username, password },
      {
        onSuccess: () => navigate('/'),
        onError: (err) => toast.error(err.message),
      },
    );
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="text-center">
          <div className="flex justify-center mb-2">
            <Cog className="h-8 w-8 text-brand" />
          </div>
          <CardTitle>{t('login.title')}</CardTitle>
          <CardDescription>{t('login.subtitle')}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">{t('login.username')}</Label>
              <Input
                id="username"
                placeholder={t('login.usernamePlaceholder')}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoFocus
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">{t('login.password')}</Label>
              <Input
                id="password"
                type="password"
                placeholder={t('login.passwordPlaceholder')}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            <Button type="submit" className="w-full" disabled={login.isPending}>
              {login.isPending ? t('login.auth') : t('login.enter')}
            </Button>
          </form>

          {ssoEnabled && !ssoLoading && (
            <>
              <div className="relative my-6">
                <div className="absolute inset-0 flex items-center">
                  <span className="w-full border-t" />
                </div>
                <div className="relative flex justify-center text-xs uppercase">
                  <span className="bg-card px-2 text-muted-foreground">Or</span>
                </div>
              </div>

              <Button
                variant="outline"
                className="w-full"
                onClick={() => {
                  window.location.href = '/api/sso/login';
                }}
              >
                <LogIn className="h-4 w-4 mr-2" />
                {ssoProviderName ? `Sign in with ${ssoProviderName}` : 'SSO Login'}
              </Button>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
