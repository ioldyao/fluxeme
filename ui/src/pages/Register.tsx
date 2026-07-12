import { useState } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Cog, ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { useSetupRegister, useSetupStatus } from '@/api/auth';
import { useAuth } from '@/store/auth';

export default function Register() {
  const navigate = useNavigate();
  const token = useAuth((s) => s.token);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const register = useSetupRegister();
  const { data: status, isLoading: statusLoading } = useSetupStatus();

  if (token) return <Navigate to="/" replace />;

  if (statusLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background p-4">
        <p className="text-muted-foreground">Loading...</p>
      </div>
    );
  }

  // If setup is not required (admin exists), redirect to login
  if (status && !status.setup_required) {
    return <Navigate to="/login" replace />;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!username || !password) {
      toast.error('Please fill in all fields');
      return;
    }
    if (password.length < 8) {
      toast.error('Password must be at least 8 characters');
      return;
    }

    register.mutate(
      { username, password },
      {
        onSuccess: () => {
          toast.success('Registration successful! Please log in.');
          navigate('/login');
        },
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
          <CardTitle>初始管理员注册</CardTitle>
          <CardDescription>创建你的管理员账号以开始使用</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">用户名</Label>
              <Input
                id="username"
                placeholder="输入管理员用户名"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoFocus
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">密码</Label>
              <Input
                id="password"
                type="password"
                placeholder="至少8位，包含大小写字母和数字"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            <Button type="submit" className="w-full" disabled={register.isPending}>
              {register.isPending ? '注册中...' : '注册'}
            </Button>
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
