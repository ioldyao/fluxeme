import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Cog, ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';

export default function Register() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const r = await fetch('/admin/api/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      if (r.status === 404) {
        toast.error('注册功能尚未开放');
        return;
      }
      if (r.ok) {
        toast.success('注册成功，请登录');
        navigate('/login');
      } else {
        const d = await r.json().catch(() => ({}));
        toast.error(d.error || '注册失败');
      }
    } catch {
      toast.error('注册功能尚未开放');
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="text-center">
          <div className="flex justify-center mb-2">
            <Cog className="h-8 w-8 text-brand" />
          </div>
          <CardTitle>注册账号</CardTitle>
          <CardDescription>创建你的 AI 网关账户</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">{t('login.username')}</Label>
              <Input id="username" value={username} onChange={(e) => setUsername(e.target.value)} autoFocus />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">{t('login.password')}</Label>
              <Input id="password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </div>
            <Button type="submit" className="w-full">注册</Button>
          </form>
          <div className="mt-4 text-center">
            <Button variant="link" size="sm" onClick={() => navigate('/login')}>
              <ArrowLeft className="h-4 w-4 mr-1" />
              返回登录
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
