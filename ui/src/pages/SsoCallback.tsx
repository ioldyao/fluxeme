import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '@/store/auth';
import { Cog } from 'lucide-react';

export default function SsoCallback() {
  const navigate = useNavigate();
  const setSession = useAuth((s) => s.setSession);
  const [error, setError] = useState('');

  useEffect(() => {
    const hash = window.location.hash.slice(1);
    const params = new URLSearchParams(hash);
    const token = params.get('token');

    if (!token) {
      setError('SSO 登录失败：未收到认证令牌');
      return;
    }

    try {
      const payload = JSON.parse(atob(token.split('.')[1]));
      setSession({
        token,
        role: payload.role || 'user',
        user_id: payload.sub || '',
        user_name: payload.name || '',
      });
      navigate('/', { replace: true });
    } catch {
      setError('SSO 登录失败：无效的认证令牌');
    }
  }, [navigate, setSession]);

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
            返回登录
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <div className="text-center space-y-4">
        <Cog className="h-8 w-8 text-brand mx-auto animate-spin" />
        <p className="text-muted-foreground">SSO 登录中...</p>
      </div>
    </div>
  );
}
