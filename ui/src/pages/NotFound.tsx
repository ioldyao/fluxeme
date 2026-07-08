import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { FileX } from 'lucide-react';

export default function NotFound() {
  const navigate = useNavigate();
  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm text-center">
        <CardHeader>
          <div className="flex justify-center mb-2">
            <FileX className="h-12 w-12 text-muted-foreground" />
          </div>
          <CardTitle>404</CardTitle>
          <CardDescription>页面不存在</CardDescription>
        </CardHeader>
        <CardContent>
          <Button onClick={() => navigate('/')}>返回首页</Button>
        </CardContent>
      </Card>
    </div>
  );
}
