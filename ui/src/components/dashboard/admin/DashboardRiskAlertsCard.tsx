import { useTranslation } from 'react-i18next';
import { AlertTriangle, Info } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';

type RiskAlert = {
  id: string;
  title: string;
  description: string;
  severity: 'warn' | 'info';
};

type DashboardRiskAlertsCardProps = {
  alerts: RiskAlert[];
};

export function DashboardRiskAlertsCard({ alerts }: DashboardRiskAlertsCardProps) {
  const { t } = useTranslation();

  return (
    <Card className="card-hover">
      <CardHeader>
        <h3 className="text-base font-semibold leading-none">{t('dash.riskAlerts')}</h3>
        <CardDescription>{t('dash.riskAlertsSub')}</CardDescription>
      </CardHeader>
      <CardContent>
        {alerts.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noAlerts')}</p>
        ) : (
          <div className="space-y-3">
            {alerts.map((alert) => {
              const isWarn = alert.severity === 'warn';
              const Icon = isWarn ? AlertTriangle : Info;
              return (
                <div key={alert.id} className="flex gap-3 rounded-lg border bg-muted/20 p-4">
                  <div className={`mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md ${isWarn ? 'bg-amber-500/15 text-amber-700' : 'bg-brand/10 text-brand'}`}>
                    <Icon className="size-4" />
                  </div>
                  <div>
                    <div className="text-sm font-medium text-foreground">{alert.title}</div>
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">{alert.description}</p>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
