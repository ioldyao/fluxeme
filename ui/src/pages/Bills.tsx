import { useState, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useBillingSummary, usePeriodSummary, useDeductions, useBillingMonths, usePeriodSummaryAll } from '@/api/billing';
import { useCurrency } from '@/store/currency';
import { usePermission, Guard } from '@/permissions';
import { PageHeader } from '@/components/PageHeader';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Wallet, Receipt, Activity, TrendingDown, ChevronDown, BarChart3 } from 'lucide-react';

export default function Bills() {
  const { t, i18n } = useTranslation();
  const { data: rawMonths } = useBillingMonths();
  const months = useMemo(() => (rawMonths ?? []).map((m) => {
    const [y, mo] = m.split('-').map(Number);
    return { label: `${y}年${mo}月`, year: y, month: mo };
  }), [rawMonths]);
  const [sel, setSel] = useState(0);
  const safeSel = sel < months.length ? sel : 0;
  const active = months[safeSel] ?? { year: 0, month: 0 };
  const { data: summary } = useBillingSummary();
  const { data: period } = usePeriodSummary(active.year, active.month);
  const [dedPage, setDedPage] = useState(1);
  const DED_PAGE_SIZE = 15;
  useEffect(() => { setDedPage(1); }, [sel]);
  const { data: deductionsData } = useDeductions(active.year, active.month, dedPage, DED_PAGE_SIZE);
  const deductions = deductionsData?.items;
  const dedTotal = deductionsData?.total ?? 0;
  const dedTotalPages = Math.max(1, Math.ceil(dedTotal / DED_PAGE_SIZE));
  const { currency, rate } = useCurrency();
  const [open, setOpen] = useState(false);
  const [compareOpen, setCompareOpen] = useState(false);
  const { data: allMonths } = usePeriodSummaryAll(usePermission('admin:period-summary-all'));

  const fmt = (usd: number) => {
    if (usd === 0) return '¥0.000000';
    const v = currency === 'cny' ? usd * rate : usd;
    const s = currency === 'cny' ? '¥' : '$';
    return `${s}${v.toFixed(6)}`;
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
          <div className="border-b px-5 py-3 flex items-center justify-between relative">
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">{t('bills.periodLabel')}</span>
              <button
                onClick={() => setOpen(!open)}
                className="flex items-center gap-1 text-sm font-semibold hover:text-foreground transition-colors"
              >
                {active.label}
                <ChevronDown className={`h-3.5 w-3.5 transition-transform ${open ? 'rotate-180' : ''}`} />
              </button>
              <button
                onClick={() => setCompareOpen(true)}
                className="ml-1 p-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
                title={t('bills.compareTooltip')}
              >
                <BarChart3 className="h-4 w-4" />
              </button>
            </div>
            {period && (
              <span className="text-xs text-muted-foreground">
                {fmt(period.total_cost)}
              </span>
            )}
            {open && (
              <>
                <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
                <div className="absolute top-full left-0 mt-1 z-20 w-44 rounded-lg border bg-popover p-1 shadow-md">
                  {months.map((m, i) => (
                    <button
                      key={i}
                      onClick={() => { setSel(i); setOpen(false); }}
                      className={`w-full text-left px-3 py-1.5 text-sm rounded-md transition-colors ${
                        i === safeSel ? 'bg-accent font-medium' : 'hover:bg-accent/50'
                      }`}
                    >
                      {m.label}
                    </button>
                  ))}
                </div>
              </>
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

              {/* Channel breakdown — admin only */}
              {period.by_channel.length > 0 && (
                <Guard perm="admin:billing-channels" fallback={null}>
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
                </Guard>
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
            <span className="text-xs text-muted-foreground ml-auto">{t('wallet.txTotal', { total: dedTotal })}</span>
          </div>
          {deductions && deductions.length > 0 ? (
            <div>
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
              {dedTotalPages > 1 && (
                <div className="flex items-center justify-center gap-2 px-5 py-3 border-t">
                  <button
                    onClick={() => setDedPage(p => Math.max(1, p - 1))}
                    disabled={dedPage <= 1}
                    className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                  >
                    {t('wallet.prevPage')}
                  </button>
                  <span className="text-xs text-muted-foreground">
                    {dedPage} / {dedTotalPages}
                  </span>
                  <button
                    onClick={() => setDedPage(p => Math.min(dedTotalPages, p + 1))}
                    disabled={dedPage >= dedTotalPages}
                    className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                  >
                    {t('wallet.nextPage')}
                  </button>
                </div>
              )}
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

      {/* Period comparison dialog */}
      <Dialog open={compareOpen} onOpenChange={setCompareOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t('bills.compareTitle')}</DialogTitle>
          </DialogHeader>
          <div className="space-y-1">
            {allMonths?.map((m) => {
              const label = i18n.language === 'zh' ? `${m.month.replace('-', '年')}月` : m.month;
              return (
                <div key={m.month} className="flex items-center justify-between px-3 py-2.5 rounded-lg hover:bg-accent/50 transition-colors">
                  <span className="font-medium text-sm">{label}</span>
                  <div className="flex items-center gap-4 text-sm">
                    <span className="font-mono">{fmt(m.total_cost)}</span>
                    <span className="text-muted-foreground">{m.total_requests.toLocaleString()} 次</span>
                  </div>
                </div>
              );
            })}
            {allMonths?.length === 0 && (
              <div className="p-8 text-center text-muted-foreground text-sm">{t('bills.noData')}</div>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
