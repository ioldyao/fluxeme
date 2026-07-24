import { useTranslation } from 'react-i18next';
import { Activity, Layers3, ShieldCheck, Wallet } from 'lucide-react';

type DashboardGatewayHealthStripProps = {
  availability: number;
  modelCount: number;
  apiKeyCount: number;
  channelCount: number;
  requests24h: number;
  totalTokens24h: number;
  avgLatencyMs24h: number;
  cost24hLabel: string;
};

function statusTone(availability: number) {
  if (availability >= 99) {
    return {
      title: 'gateway.healthy',
      dot: 'bg-emerald-500 shadow-[0_0_0_6px_rgba(20,150,106,0.12)]',
    };
  }
  if (availability >= 95) {
    return {
      title: 'gateway.degraded',
      dot: 'bg-amber-500 shadow-[0_0_0_6px_rgba(217,145,19,0.14)]',
    };
  }
  return {
    title: 'gateway.unstable',
    dot: 'bg-red-500 shadow-[0_0_0_6px_rgba(216,75,75,0.14)]',
  };
}

export function DashboardGatewayHealthStrip({
  availability,
  modelCount,
  apiKeyCount,
  channelCount,
  requests24h,
  totalTokens24h,
  avgLatencyMs24h,
  cost24hLabel,
}: DashboardGatewayHealthStripProps) {
  const { t } = useTranslation();
  const tone = statusTone(availability);
  const latencyLabel = avgLatencyMs24h >= 1000
    ? `${(avgLatencyMs24h / 1000).toFixed(2)}s`
    : `${avgLatencyMs24h.toFixed(0)}ms`;

  const metrics = [
    {
      title: t('dash.requests'),
      value: requests24h.toLocaleString(),
      hint: t('dash.last24Hours'),
      icon: <Activity className="size-4" />,
    },
    {
      title: t('usage.totalTokens'),
      value: totalTokens24h.toLocaleString(),
      hint: t('dash.last24Hours'),
      icon: <Layers3 className="size-4" />,
    },
    {
      title: t('dash.avgLatency'),
      value: latencyLabel,
      hint: t('dash.performanceSub'),
      icon: <ShieldCheck className="size-4" />,
    },
    {
      title: t('dash.cost24h'),
      value: cost24hLabel,
      hint: t('dash.last24Hours'),
      icon: <Wallet className="size-4" />,
    },
  ];

  return (
    <section className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">{t('dash.adminSectionTitle')}</h2>
        <p className="mt-1 text-sm text-muted-foreground">{t('dash.adminSectionSub')}</p>
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.4fr_repeat(4,minmax(0,1fr))]">
        <div className="rounded-xl border bg-card p-5 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <span className={`size-3 rounded-full ${tone.dot}`} aria-hidden="true" />
              <div>
                <div className="font-semibold text-foreground">{t(tone.title)}</div>
                <div className="mt-1 text-sm text-muted-foreground">
                  {t('dash.gatewayHealthMeta', { modelCount, channelCount, apiKeyCount })}
                </div>
              </div>
            </div>
            <div className="text-right">
              <div className="text-2xl font-semibold tracking-tight">{availability.toFixed(2)}%</div>
              <div className="text-xs text-muted-foreground">{t('dash.availability')}</div>
            </div>
          </div>
        </div>

        {metrics.map((metric) => (
          <div key={metric.title} className="rounded-xl border bg-card p-4 shadow-sm">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="rounded-md bg-brand/10 p-1 text-brand">{metric.icon}</span>
              {metric.title}
            </div>
            <div className="mt-3 text-2xl font-semibold tracking-tight">{metric.value}</div>
            <div className="mt-1 text-xs text-muted-foreground">{metric.hint}</div>
          </div>
        ))}
      </div>
    </section>
  );
}
