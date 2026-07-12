import { useTranslation } from 'react-i18next';
import { useBillingSummary, usePeriodSummary, useDeductions } from '@/api/billing';
import { useCurrency } from '@/store/currency';
import { PageHeader } from '@/components/PageHeader';
import { Wallet, Receipt, Activity, TrendingDown, Download } from 'lucide-react';

export default function Bills() {
  const { t } = useTranslation();
  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth() + 1;
  const { data: summary } = useBillingSummary();
  const { data: period } = usePeriodSummary(year, month);
  const { data: deductions } = useDeductions(year, month);
  const { currency, rate } = useCurrency();

  const fmt = (usd: number) => {
    if (usd === 0) return '¥0.00';
    const v = currency === 'cny' ? usd * rate : usd;
    const s = currency === 'cny' ? '¥' : '$';
    return `${s}${v.toFixed(2)}`;
  };

  const cardStyle = 'rounded-xl border p-5 space-y-2';

  return (
    <div>
      <PageHeader title={t('bills.title')} description={t('bills.subtitle')} />

      {/* Summary cards row */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 px-6 mb-8">
        <div className={cardStyle}>
          <div className="flex items-center gap-2 text-muted-foreground">
            <Wallet className="h-4 w-4" />
            <span className="text-xs font-medium uppercase tracking-wider">{t('bills.balance')}</span>
          </div>
          <div className="text-2xl font-bold">{summary ? fmt(summary.balance) : '—'}</div>
          <div className="text-xs text-muted-foreground">{t('bills.remainingQuota')}</div>
        </div>
        <div className={cardStyle}>
          <div className="flex items-center gap-2 text-muted-foreground">
            <Receipt className="h-4 w-4" />
            <span className="text-xs font-medium uppercase tracking-wider">{t('bills.totalUsage')}</span>
          </div>
          <div className="text-2xl font-bold">{summary ? fmt(summary.total_cost) : '—'}</div>
          <div className="text-xs text-muted-foreground">{t('bills.totalConsumed')}</div>
        </div>
        <div className={cardStyle}>
          <div className="flex items-center gap-2 text-muted-foreground">
            <Activity className="h-4 w-4" />
            <span className="text-xs font-medium uppercase tracking-wider">{t('bills.apiRequests')}</span>
          </div>
          <div className="text-2xl font-bold">{summary ? summary.total_requests.toLocaleString() : '—'}</div>
          <div className="text-xs text-muted-foreground">{t('bills.totalRequests')}</div>
        </div>
      </div>

      {/* Period summary */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border">
          <div className="border-b px-5 py-3 flex items-center justify-between">
            <h3 className="font-semibold text-sm">{t('bills.periodTitle', { year, month })}</h3>
            {period && (
              <span className="text-xs text-muted-foreground">
                {fmt(period.total_cost)}
              </span>
            )}
          </div>
          {period ? (
            <div className="p-5 space-y-5">
              {/* Stats */}
              <div className="grid grid-cols-3 gap-4">
                <div>
                  <div className="text-xs text-muted-foreground">{t('bills.totalCost')}</div>
                  <div className="text-xl font-bold">{fmt(period.total_cost)}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">{t('bills.requests')}</div>
                  <div className="text-xl font-bold">{period.total_requests.toLocaleString()}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">{t('bills.totalTokens')}</div>
                  <div className="text-xl font-bold">{period.total_tokens.toLocaleString()}</div>
                </div>
              </div>

              {/* Model breakdown */}
              {period.by_model.length > 0 && (
                <div>
                  <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">{t('bills.byModel')}</h4>
                  <div className="space-y-1.5">
                    {period.by_model.map((m) => (
                      <div key={m.model} className="flex items-center gap-3">
                        <div className="flex-1 min-w-0">
                          <div className="flex justify-between text-sm">
                            <span className="truncate">{m.model}</span>
                            <span className="font-mono text-xs">{fmt(m.cost)}</span>
                          </div>
                          <div className="h-1.5 bg-muted rounded-full mt-1 overflow-hidden">
                            <div className="h-full bg-brand rounded-full" style={{ width: `${m.percentage}%` }} />
                          </div>
                        </div>
                        <span className="text-xs text-muted-foreground w-10 text-right">{m.percentage}%</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Channel breakdown */}
              {period.by_channel.length > 0 && (
                <div>
                  <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">{t('bills.byChannel')}</h4>
                  <div className="space-y-1.5">
                    {period.by_channel.map((c) => (
                      <div key={c.channel} className="flex items-center gap-3">
                        <div className="flex-1 min-w-0">
                          <div className="flex justify-between text-sm">
                            <span className="truncate font-mono text-xs">{c.channel}</span>
                            <span className="font-mono text-xs">{fmt(c.cost)}</span>
                          </div>
                          <div className="h-1.5 bg-muted rounded-full mt-1 overflow-hidden">
                            <div className="h-full bg-brand rounded-full" style={{ width: `${c.percentage}%` }} />
                          </div>
                        </div>
                        <span className="text-xs text-muted-foreground w-10 text-right">{c.percentage}%</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="p-8 text-center text-muted-foreground text-sm">{t('common.loading')}</div>
          )}
        </div>
      </div>

      {/* Deduction records */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border">
          <div className="border-b px-5 py-3 flex items-center gap-2">
            <TrendingDown className="h-4 w-4 text-muted-foreground" />
            <h3 className="font-semibold text-sm">{t('bills.deductions')}</h3>
          </div>
          {deductions && deductions.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-xs text-muted-foreground">
                    <th className="text-left px-5 py-3 font-medium">{t('bills.deductionTime')}</th>
                    <th className="text-right px-5 py-3 font-medium">{t('bills.deductionAmount')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('bills.deductionMethod')}</th>
                  </tr>
                </thead>
                <tbody>
                  {deductions.map((d) => (
                    <tr key={d.time} className="border-b last:border-0">
                      <td className="px-5 py-3 text-muted-foreground">{new Date(d.time).toLocaleDateString()}</td>
                      <td className="px-5 py-3 text-right font-mono text-red-500">{fmt(d.amount)}</td>
                      <td className="px-5 py-3">{d.method}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="p-8 text-center text-muted-foreground text-sm">{t('bills.noDeductions')}</div>
          )}
        </div>
      </div>

      {/* Top-up & Invoice records (placeholder) */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border p-5">
          <h3 className="font-semibold text-sm mb-3">{t('bills.rechargeInvoices')}</h3>
          <div className="p-6 text-center text-muted-foreground text-sm">{t('bills.noData')}</div>
        </div>
      </div>
    </div>
  );
}
