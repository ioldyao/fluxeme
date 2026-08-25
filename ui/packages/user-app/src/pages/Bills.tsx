import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useBillingSummary, usePeriodSummary, useBillingActivities, useDeductions, useBillingMonths, usePeriodSummaryAll } from '@fluxeme/shared/src/api/billing';
import { useCurrency } from '@fluxeme/shared/src/store/currency';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { Wallet, Activity, ChevronDown, BarChart3 } from 'lucide-react';

function asBillingNumber(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value ?? 0);
  return Number.isFinite(parsed) ? parsed : 0;
}

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
  const { data: deductionsData } = useDeductions(active.year, active.month, dedPage, 15);
  const [activityPage, setActivityPage] = useState(1);
  const ACTIVITY_PAGE_SIZE = 50;
  const { data: activitiesData } = useBillingActivities(active.year, active.month, ACTIVITY_PAGE_SIZE, (activityPage - 1) * ACTIVITY_PAGE_SIZE);
  const activities = activitiesData?.activities ?? [];
  const activityTotalPages = Math.max(1, Math.ceil((activitiesData?.total ?? 0) / ACTIVITY_PAGE_SIZE));
  const deductions = deductionsData?.items;
  const dedTotal = deductionsData?.total ?? 0;
  const dedTotalPages = Math.max(1, Math.ceil(dedTotal / 15));
  const { currency } = useCurrency();
  const [open, setOpen] = useState(false);
  const [compareOpen, setCompareOpen] = useState(false);
  const { data: allMonths } = usePeriodSummaryAll();

  const fmt = (value: unknown) => {
    const safeUsd = asBillingNumber(value);
    const s = currency === 'cny' ? '¥' : '$';
    return safeUsd === 0 ? `${s}0` : `${s}${safeUsd.toFixed(6)}`;
  };

  const cardStyle = 'rounded-xl border p-5 space-y-2';

  return (
    <div>
      <PageHeader title={t('bills.title')} description={t('bills.subtitle')} />

      {/* Activity summary row */}
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
            <Wallet className="h-4 w-4" />
            <span className="text-xs font-medium uppercase tracking-wider">本期钱包实扣</span>
          </div>
          <div className="text-2xl font-bold">{period ? fmt(period.wallet_amount) : '—'}</div>
          <div className="text-xs text-muted-foreground">只统计实际钱包承担金额</div>
        </div>
        <div className={cardStyle}>
          <div className="flex items-center gap-2 text-muted-foreground">
            <Activity className="h-4 w-4" />
            <span className="text-xs font-medium uppercase tracking-wider">本期活动</span>
          </div>
          <div className="text-2xl font-bold">{period ? period.request_count.toLocaleString() : '—'}</div>
          <div className="text-xs text-muted-foreground">成功 {period?.success_count ?? 0} · 失败 {period?.failure_count ?? 0} · 中断 {period?.interrupted_count ?? 0}</div>
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
                {fmt(period.wallet_amount)}
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
                  <div className="text-xs text-muted-foreground">本期钱包实扣</div>
                  <div className="text-xl font-bold">{fmt(period.wallet_amount)}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">{t('bills.requests')}</div>
                  <div className="text-xl font-bold">{period.request_count.toLocaleString()}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">计费 Token</div>
                  <div className="text-xl font-bold">{period.total_tokens.toLocaleString()}</div>
                </div>
              </div>

              {/* Model breakdown */}
              {period.wallet_amount > 0 && (
                <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                  本期钱包实扣：<span className="font-mono font-semibold text-foreground">{fmt(period.wallet_amount)}</span>
                </div>
              )}
            </div>
          ) : (
            <div className="p-8 text-center text-muted-foreground text-sm">{t('common.loading')}</div>
          )}
        </div>
      </div>

      {/* Activity records */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border">
          <div className="border-b px-5 py-3 flex items-center gap-2">
            <Activity className="h-4 w-4 text-muted-foreground" />
            <h3 className="font-semibold text-sm">本期账单活动记录</h3>
            <span className="text-xs text-muted-foreground ml-auto">免费、资源包和钱包活动均会记录</span>
          </div>
          {activities.length > 0 ? (
            <>
              <div className="overflow-x-auto"><table className="w-full text-sm"><thead><tr className="border-b text-xs text-muted-foreground"><th className="text-left px-5 py-3">时间</th><th className="text-left px-5 py-3">模型</th><th className="text-left px-5 py-3">状态</th><th className="text-right px-5 py-3">Token</th><th className="text-right px-5 py-3">资源包 units</th><th className="text-right px-5 py-3">钱包扣款</th><th className="text-left px-5 py-3">结算来源</th></tr></thead><tbody>{activities.map((item) => <tr key={item.request_id} className="border-b last:border-0"><td className="px-5 py-3 text-muted-foreground">{new Date(item.timestamp).toLocaleString()}</td><td className="px-5 py-3">{item.model}</td><td className="px-5 py-3"><span className="rounded-full bg-muted px-2 py-1 text-xs">{item.activity_status}</span></td><td className="px-5 py-3 text-right font-mono">{item.total_tokens.toLocaleString()}</td><td className="px-5 py-3 text-right font-mono">{item.package_units.toLocaleString()}</td><td className="px-5 py-3 text-right font-mono" title={asBillingNumber(item.wallet_amount).toFixed(8)}>{fmt(item.wallet_amount)}</td><td className="px-5 py-3">{item.charge_source}</td></tr>)}</tbody></table></div>
              <div className="flex items-center justify-between border-t px-5 py-3 text-xs text-muted-foreground"><span>第 {activityPage} / {activityTotalPages} 页 · 共 {activitiesData?.total ?? 0} 条活动</span><div className="flex gap-2"><button className="rounded border px-3 py-1 disabled:opacity-40" disabled={activityPage <= 1} onClick={() => setActivityPage((page) => page - 1)}>上一页</button><button className="rounded border px-3 py-1 disabled:opacity-40" disabled={activityPage >= activityTotalPages} onClick={() => setActivityPage((page) => page + 1)}>下一页</button></div></div>
            </>
          ) : deductions && deductions.length > 0 ? (
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
                        <td className="px-5 py-3 text-right font-mono text-destructive">{fmt(d.amount)}</td>
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
                    <span className="font-mono">{fmt(m.wallet_amount ?? 0)}</span>
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
