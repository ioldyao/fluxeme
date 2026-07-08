import { useTranslation } from 'react-i18next';
import { useDashboard } from '@/api/dashboard';
import { useUsage } from '@/api/usage';
import { StatCard } from '@/components/StatCard';
import { EmptyState } from '@/components/EmptyState';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Users, Radio, Braces, Key, Activity,
  CheckCircle2, XCircle,
} from 'lucide-react';

export default function Dashboard() {
  const { t } = useTranslation();
  const { data: stats, isLoading } = useDashboard();
  const { data: usage } = useUsage({ limit: 10 });

  const adminStats = [
    { title: t('dash.users'), value: stats?.users ?? 0, icon: <Users className="h-5 w-5" /> },
    { title: t('dash.channels'), value: stats?.channels ?? 0, icon: <Radio className="h-5 w-5" /> },
    { title: t('dash.models'), value: stats?.models ?? 0, icon: <Braces className="h-5 w-5" /> },
    { title: t('dash.apiKeys'), value: stats?.api_keys ?? 0, icon: <Key className="h-5 w-5" /> },
    { title: t('dash.requests'), value: stats?.total_requests ?? 0, icon: <Activity className="h-5 w-5" /> },
  ];

  return (
    <div className="space-y-6 animate-fade-in">
      <div>
        <h1 className="text-2xl font-semibold">{t('dash.title')}</h1>
        <p className="text-sm text-muted-foreground">{t('dash.subtitle')}</p>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-7 gap-3">
        {adminStats.map((stat) => (
          <StatCard key={stat.title} {...stat} loading={isLoading} />
        ))}
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('dash.recentUsage')}</CardTitle>
        </CardHeader>
        <CardContent>
          {usage && usage.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-2 px-2">{t('table.time')}</th>
                    <th className="text-left py-2 px-2">{t('table.user')}</th>
                    <th className="text-left py-2 px-2">{t('table.model')}</th>
                    <th className="text-right py-2 px-2">{t('table.tokens')}</th>
                    <th className="text-right py-2 px-2">{t('table.latency')}</th>
                    <th className="text-center py-2 px-2">{t('table.status')}</th>
                  </tr>
                </thead>
                <tbody>
                  {usage.map((r) => (
                    <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-2 px-2 text-muted-foreground whitespace-nowrap">
                        {new Date(r.timestamp).toLocaleString()}
                      </td>
                      <td className="py-2 px-2">{r.user_name}</td>
                      <td className="py-2 px-2">{r.model}</td>
                      <td className="py-2 px-2 text-right">{r.total_tokens}</td>
                      <td className="py-2 px-2 text-right">{r.latency_ms}ms</td>
                      <td className="py-2 px-2 text-center">
                        {r.success ? (
                          <CheckCircle2 className="h-4 w-4 text-green-500 inline" />
                        ) : (
                          <XCircle className="h-4 w-4 text-red-500 inline" />
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('dash.noData')} />
          )}
        </CardContent>
      </Card>
    </div>
  );
}
