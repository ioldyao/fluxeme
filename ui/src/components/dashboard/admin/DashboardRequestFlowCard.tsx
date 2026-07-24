import { useTranslation } from 'react-i18next';
import { ArrowRight } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';

type DashboardRequestFlowCardProps = {
  requests24h: number;
  successRate24h: number;
};

type FlowNodeProps = {
  title: string;
  subtitle: string;
  value: number;
};

function FlowNode({ title, subtitle, value }: FlowNodeProps) {
  return (
    <div className="rounded-lg border bg-muted/30 p-4">
      <div className="text-sm font-medium text-foreground">{title}</div>
      <div className="mt-1 text-xs text-muted-foreground">{subtitle}</div>
      <div className="mt-4 text-2xl font-semibold tracking-tight">{value.toLocaleString()}</div>
    </div>
  );
}

export function DashboardRequestFlowCard({ requests24h, successRate24h }: DashboardRequestFlowCardProps) {
  const { t } = useTranslation();
  const processedRequests = requests24h;
  const successfulResponses = Math.round(requests24h * (successRate24h / 100));

  return (
    <Card className="card-hover">
      <CardHeader>
        <h3 className="text-base font-semibold leading-none">{t('dash.requestFlow')}</h3>
        <CardDescription>{t('dash.requestFlowSub')}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1fr_auto_1fr_auto_1fr] lg:items-center">
          <FlowNode title={t('dash.requestIngress')} subtitle={t('dash.requestIngressSub')} value={requests24h} />
          <div className="hidden justify-center text-muted-foreground lg:flex"><ArrowRight className="size-5" /></div>
          <FlowNode title={t('dash.gatewayProcessing')} subtitle={t('dash.gatewayProcessingSub')} value={processedRequests} />
          <div className="hidden justify-center text-muted-foreground lg:flex"><ArrowRight className="size-5" /></div>
          <FlowNode title={t('dash.modelResponses')} subtitle={t('dash.modelResponsesSub')} value={successfulResponses} />
        </div>
      </CardContent>
    </Card>
  );
}
