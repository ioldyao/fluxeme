import { useState, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useWalletOverview, useWalletTransactions, useRecharge, useRedeemKey, useCreateRechargeKey, useRechargeKeys, useEstimatedDays } from '@/api/wallet';
import { useCurrency } from '@/store/currency';
import { useAuth } from '@/store/auth';
import { PageHeader } from '@/components/PageHeader';
import { Wallet, CreditCard, KeyRound, Receipt, AlertTriangle, Copy, Check, Loader2 } from 'lucide-react';
import { toast } from 'sonner';

export default function WalletPage() {
  const navigate = useNavigate();
  const { t, i18n } = useTranslation();
  const { currency, rate } = useCurrency();
  const { role } = useAuth();
  const isAdmin = role === 'admin';

  const { data: overview } = useWalletOverview();

  // ── Transaction filter state ──
  const [dateRange, setDateRange] = useState('today'); // 'today' | '7d' | '30d' | 'all'
  const [txType, setTxType] = useState(''); // '' | 'recharge' | 'deduction'
  const [txPage, setTxPage] = useState(1);
  const [expandedDays, setExpandedDays] = useState<Set<string>>(new Set());
  const PAGE_SIZE = 50;

  const dateParams = useMemo(() => {
    const now = new Date();
    const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    switch (dateRange) {
      case 'today': return { since: todayStart.toISOString(), until: undefined as string | undefined };
      case '7d': return { since: new Date(todayStart.getTime() - 7 * 86400000).toISOString(), until: undefined };
      case '30d': return { since: new Date(todayStart.getTime() - 30 * 86400000).toISOString(), until: undefined };
      default: return {};
    }
  }, [dateRange]);

  const { data: txData, isLoading: loadingTx } = useWalletTransactions(
    txPage, PAGE_SIZE,
    {
      since: dateParams.since,
      until: dateParams.until,
      tx_type: txType || undefined,
    },
  );
  const totalPages = Math.max(1, Math.ceil((txData?.total ?? 0) / PAGE_SIZE));

  // Group transactions by day with aggregates
  const dayGroups = useMemo(() => {
    if (!txData?.items) return [];
    const groups: Record<string, typeof txData.items> = {};
    for (const tx of txData.items) {
      const day = tx.created_at.split('T')[0];
      if (!groups[day]) groups[day] = [];
      groups[day].push(tx);
    }
    return Object.entries(groups)
      .sort(([a], [b]) => b.localeCompare(a))
      .map(([date, items]) => {
        const rechargeItems = items.filter(i => i.tx_type === 'recharge');
        const deductionItems = items.filter(i => i.tx_type !== 'recharge');
        return {
          date,
          items,
          rechargeTotal: rechargeItems.reduce((s, i) => s + Math.abs(i.amount), 0),
          deductionTotal: deductionItems.reduce((s, i) => s + Math.abs(i.amount), 0),
          rechargeCount: rechargeItems.length,
          deductionCount: deductionItems.length,
        };
      });
  }, [txData?.items]);

  const toggleDay = (day: string) => {
    setExpandedDays(prev => {
      const next = new Set(prev);
      if (next.has(day)) next.delete(day); else next.add(day);
      return next;
    });
  };
  const { data: estimated } = useEstimatedDays();
  const { data: keys } = useRechargeKeys();
  const recharge = useRecharge();
  const redeem = useRedeemKey();
  const createKey = useCreateRechargeKey();

  const [rechargeAmt, setRechargeAmt] = useState('');
  const [keyInput, setKeyInput] = useState('');
  const [createKeyAmt, setCreateKeyAmt] = useState('');
  const [newKey, setNewKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const fmt = (usd: number) => {
    const v = currency === 'cny' ? usd * rate : usd;
    const s = currency === 'cny' ? '¥' : '$';
    return `${s}${v.toFixed(6)}`;
  };

  const cardStyle = 'rounded-xl border p-5 space-y-2';

  const handleRecharge = () => {
    const amt = parseFloat(rechargeAmt);
    if (isNaN(amt) || amt <= 0) return;
    recharge.mutate(amt, {
      onSuccess: (res) => {
        toast.success(t('wallet.rechargeSuccess', { amount: fmt(res.amount) }));
        setRechargeAmt('');
      },
    });
  };

  const handleRedeem = () => {
    if (!keyInput.trim()) return;
    redeem.mutate(keyInput.trim(), {
      onSuccess: (res) => {
        toast.success(t('wallet.redeemSuccess', { amount: fmt(res.amount) }));
        setKeyInput('');
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : t('toast.failed')),
    });
  };

  const handleCreateKey = () => {
    const amt = parseFloat(createKeyAmt);
    if (isNaN(amt) || amt <= 0) return;
    createKey.mutate(amt, {
      onSuccess: (res) => {
        setNewKey(res.key);
        setCreateKeyAmt('');
      },
    });
  };

  const copyKey = async (key: string) => {
    await navigator.clipboard.writeText(key);
    setCopied(true);
    toast.success(t('wallet.keyCopied'));
    setTimeout(() => setCopied(false), 2000);
  };

  const lowBalance = overview && (overview.balance <= 0 || (estimated?.days != null && estimated.days < 10));

  return (
    <div>
      <PageHeader title={t('wallet.title')} description={t('wallet.subtitle')} />

      {/* ── 1. Balance Overview ── */}
      <div className="px-6 mb-8">
        <h3 className="text-sm font-semibold mb-3 flex items-center gap-2">
          <Wallet className="h-4 w-4" />
          {t('wallet.balanceOverview')}
        </h3>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <div className={cardStyle}>
            <div className="text-xs text-muted-foreground uppercase tracking-wider">{t('wallet.currentBalance')}</div>
            <div className="text-2xl font-bold">{overview ? fmt(overview.balance) : '—'}</div>
            {estimated?.days != null && (
              <div className="flex items-center gap-1 text-xs">
                {lowBalance ? (
                  <span className="text-destructive flex items-center gap-1">
                    <AlertTriangle className="h-3 w-3" />
                    {t('wallet.estimatedDays')}: {estimated.days.toFixed(1)}d
                  </span>
                ) : (
                  <span className="text-muted-foreground">{t('wallet.estimatedDays')}: {estimated.days.toFixed(1)}d</span>
                )}
              </div>
            )}
          </div>
          <div className={cardStyle}>
            <div className="text-xs text-muted-foreground uppercase tracking-wider">{t('wallet.frozen')}</div>
            <div className="text-xl font-bold">{overview ? fmt(overview.frozen) : '—'}</div>
          </div>
          <div className={cardStyle}>
            <div className="text-xs text-muted-foreground uppercase tracking-wider">{t('wallet.totalConsumed')}</div>
            <div className="text-xl font-bold">{overview ? fmt(overview.total_consumed) : '—'}</div>
          </div>
          <div className={cardStyle}>
            <div className="text-xs text-muted-foreground uppercase tracking-wider">{t('wallet.totalRecharged')}</div>
            <div className="text-xl font-bold">{overview ? fmt(overview.total_recharged) : '—'}</div>
          </div>
        </div>
      </div>

      {/* ── 2. Recharge ── */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6 px-6 mb-8">
        {/* Manual recharge */}
        <div className="rounded-xl border p-5">
          <h3 className="text-sm font-semibold mb-1 flex items-center gap-2">
            <CreditCard className="h-4 w-4" />
            {t('wallet.recharge')}
          </h3>
          <p className="text-xs text-muted-foreground mb-4">{t('wallet.rechargeSub')}</p>
          <div className="flex items-center gap-2">
            <input
              type="number"
              min="1"
              placeholder="0.00"
              value={rechargeAmt}
              onChange={(e) => setRechargeAmt(e.target.value)}
              className="flex-1 h-9 rounded-md border bg-background px-3 text-sm"
            />
            <button
              onClick={handleRecharge}
              disabled={recharge.isPending || !rechargeAmt}
              className="h-9 px-4 rounded-md bg-brand text-white text-sm font-medium hover:opacity-90 disabled:opacity-50 flex items-center gap-1"
            >
              {recharge.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
              {recharge.isPending ? t('wallet.recharging') : t('wallet.rechargeBtn')}
            </button>
          </div>
        </div>

        {/* Key recharge */}
        <div className="rounded-xl border p-5">
          <h3 className="text-sm font-semibold mb-1 flex items-center gap-2">
            <KeyRound className="h-4 w-4" />
            {t('wallet.keyRecharge')}
          </h3>
          <p className="text-xs text-muted-foreground mb-4">{t('wallet.keyRechargeSub')}</p>
          <div className="flex items-center gap-2">
            <input
              type="text"
              placeholder={t('wallet.keyInput')}
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              className="flex-1 h-9 rounded-md border bg-background px-3 text-sm font-mono"
            />
            <button
              onClick={handleRedeem}
              disabled={redeem.isPending || !keyInput.trim()}
              className="h-9 px-4 rounded-md bg-brand text-white text-sm font-medium hover:opacity-90 disabled:opacity-50 flex items-center gap-1"
            >
              {redeem.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
              {t('wallet.redeemBtn')}
            </button>
          </div>

          {/* Admin: create key */}
          {isAdmin && (
            <div className="mt-4 pt-4 border-t">
              <h4 className="text-xs font-semibold mb-1">{t('wallet.createKey')}</h4>
              <p className="text-xs text-muted-foreground mb-3">{t('wallet.createKeySub')}</p>
              <div className="flex items-center gap-2">
                <input
                  type="number"
                  min="1"
                  placeholder="0.00"
                  value={createKeyAmt}
                  onChange={(e) => setCreateKeyAmt(e.target.value)}
                  className="flex-1 h-9 rounded-md border bg-background px-3 text-sm"
                />
                <button
                  onClick={handleCreateKey}
                  disabled={createKey.isPending || !createKeyAmt}
                  className="h-9 px-4 rounded-md border text-sm font-medium hover:bg-accent disabled:opacity-50 flex items-center gap-1"
                >
                  {createKey.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
                  {t('wallet.createKeyBtn')}
                </button>
              </div>
              {newKey && (
                <div className="mt-3 flex items-center gap-2 p-2 rounded-md bg-muted">
                  <code className="flex-1 text-xs font-mono break-all">{newKey}</code>
                  <button onClick={() => copyKey(newKey)} className="p-1 hover:text-foreground text-muted-foreground">
                    {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* ── 3. Balance Alert ── */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border p-5">
          <h3 className="text-sm font-semibold mb-1 flex items-center gap-2">
            <AlertTriangle className="h-4 w-4" />
            {t('wallet.alert')}
          </h3>
          <p className="text-xs text-muted-foreground mb-4">{t('wallet.alertSub')}</p>
          {overview && (
            <div className="flex items-center gap-3">
              <div className={`px-3 py-1.5 rounded-md text-xs font-medium ${
                lowBalance ? 'bg-destructive/10 text-destructive' : 'bg-green-500/10 text-green-600'
              }`}>
                {lowBalance ? t('wallet.alertBelowThreshold') : t('wallet.alertOk')}
              </div>
              <span className="text-xs text-muted-foreground">
                {t('wallet.currentBalance')}: <strong>{fmt(overview.balance)}</strong>
              </span>
            </div>
          )}
        </div>
      </div>

      {/* ── 4. Consumption Flow ── */}
      <div className="px-6 mb-8">
        <div className="rounded-xl border">
          <div className="border-b px-5 py-3 flex items-center gap-2">
            <Receipt className="h-4 w-4 text-muted-foreground" />
            <h3 className="font-semibold text-sm">{t('wallet.consumption')}</h3>
            <span className="text-xs text-muted-foreground">{t('wallet.consumptionSub')}</span>
          </div>

          {/* ── Filter bar ── */}
          <div className="border-b px-5 py-2.5 flex items-center gap-3 flex-wrap">
            {/* Date range tabs */}
            <div className="flex items-center gap-1 text-xs">
              {(['today', '7d', '30d', 'all'] as const).map((key) => (
                <button
                  key={key}
                  onClick={() => { setDateRange(key); setTxPage(1); }}
                  className={`px-2.5 py-1 rounded-md font-medium transition-colors ${
                    dateRange === key
                      ? 'bg-brand text-white'
                      : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                  }`}
                >
                  {key === 'today' ? t('wallet.filterToday') : key === '7d' ? t('wallet.filter7d') : key === '30d' ? t('wallet.filter30d') : t('wallet.filterAll')}
                </button>
              ))}
            </div>
            <span className="text-muted-foreground/40">|</span>
            {/* Type filter */}
            <select
              value={txType}
              onChange={(e) => { setTxType(e.target.value); setTxPage(1); }}
              className="h-7 rounded-md border bg-background px-2 text-xs"
            >
              <option value="">{t('wallet.filterAllTypes')}</option>
              <option value="recharge">{t('wallet.type.recharge')}</option>
              <option value="deduction">{t('wallet.type.deduction')}</option>
            </select>
            {txData && (
              <span className="text-xs text-muted-foreground ml-auto">
                {t('wallet.txTotal', { total: txData.total })}
              </span>
            )}
          </div>

          {/* ── Daily grouped transactions ── */}
          {dayGroups.length > 0 ? (
            <div>
              {dayGroups.map((day) => {
                const isExpanded = expandedDays.has(day.date);
                return (
                  <div key={day.date} className="border-b last:border-0">
                    {/* Day header — click to expand/collapse (only if has recharge rows) */}
                    <button
                      onClick={() => day.rechargeCount > 0 && toggleDay(day.date)}
                      className="w-full flex items-center gap-3 px-5 py-3 hover:bg-muted/30 transition-colors text-left"
                    >
                      {day.rechargeCount > 0 ? (
                        <span className={`text-xs transition-transform ${isExpanded ? 'rotate-90' : ''}`}>▶</span>
                      ) : (
                        <span className="text-xs text-transparent">▶</span>
                      )}
                      <span className="font-semibold text-sm">
                        {new Date(day.date).toLocaleDateString(i18n.language === 'zh' ? 'zh-CN' : 'en-US', { month: 'short', day: 'numeric', year: 'numeric' })}
                      </span>
                      <div className="flex items-center gap-3 text-xs ml-4">
                        {day.deductionCount > 0 && (
                          <button
                            onClick={(e) => { e.stopPropagation(); navigate(`/usage?date=${day.date}`); }}
                            className="text-destructive hover:underline cursor-pointer"
                          >
                            {t('wallet.groupDeduction', { count: day.deductionCount, amount: fmt(day.deductionTotal) })}
                          </button>
                        )}
                        {day.rechargeCount > 0 && (
                          <span className="text-green-600">
                            {t('wallet.groupRecharge', { count: day.rechargeCount, amount: fmt(day.rechargeTotal) })}
                          </span>
                        )}
                      </div>
                    </button>

                    {/* Expanded transaction rows */}
                    {isExpanded && (
                      <div className="overflow-x-auto border-t">
                        <table className="w-full text-sm">
                          <thead>
                            <tr className="border-b text-xs text-muted-foreground bg-muted/20">
                              <th className="text-left px-5 py-2 font-medium">{t('wallet.txTime')}</th>
                              <th className="text-left px-5 py-2 font-medium">{t('wallet.txType')}</th>
                              <th className="text-right px-5 py-2 font-medium">{t('wallet.txAmount')}</th>
                              <th className="text-right px-5 py-2 font-medium">{t('wallet.txBefore')}</th>
                              <th className="text-right px-5 py-2 font-medium">{t('wallet.txAfter')}</th>
                              <th className="text-left px-5 py-2 font-medium">{t('wallet.txMethod')}</th>
                              <th className="text-left px-5 py-2 font-medium">{t('wallet.txStatus')}</th>
                              <th className="text-left px-5 py-2 font-medium">{t('wallet.txNote')}</th>
                            </tr>
                          </thead>
                          <tbody>
                            {day.items.filter(tx => tx.tx_type === 'recharge').map((tx) => (
                              <tr key={tx.id} className="border-b last:border-0">
                                <td className="px-5 py-2.5 text-muted-foreground whitespace-nowrap text-xs">
                                  {new Date(tx.created_at).toLocaleString(i18n.language === 'zh' ? 'zh-CN' : 'en-US')}
                                </td>
                                <td className="px-5 py-2.5">
                                  <span className={`text-xs font-medium px-2 py-0.5 rounded-full ${
                                    tx.tx_type === 'recharge' ? 'bg-green-500/10 text-green-600' : 'bg-destructive/10 text-destructive'
                                  }`}>
                                    {tx.tx_type === 'recharge' ? t('wallet.type.recharge') : t('wallet.type.deduction')}
                                  </span>
                                </td>
                                <td className={`px-5 py-2.5 text-right font-mono text-xs ${tx.amount >= 0 ? 'text-green-600' : 'text-destructive'}`}>
                                  {tx.amount >= 0 ? '+' : ''}{fmt(Math.abs(tx.amount))}
                                </td>
                                <td className="px-5 py-2.5 text-right font-mono text-xs text-muted-foreground">{fmt(tx.balance_before)}</td>
                                <td className="px-5 py-2.5 text-right font-mono text-xs text-muted-foreground">{fmt(tx.balance_after)}</td>
                                <td className="px-5 py-2.5 text-muted-foreground text-xs">{tx.method}</td>
                                <td className="px-5 py-2.5 text-xs">
                                  <span className={`text-xs ${tx.status === 'completed' ? 'text-green-600' : 'text-muted-foreground'}`}>
                                    {tx.status}
                                  </span>
                                </td>
                                <td className="px-5 py-2.5 text-muted-foreground text-xs max-w-[180px] truncate">{tx.note}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </div>
                );
              })}

              {/* ── Pagination ── */}
              {totalPages > 1 && (
                <div className="flex items-center justify-center gap-2 px-5 py-3 border-t">
                  <button
                    onClick={() => setTxPage(p => Math.max(1, p - 1))}
                    disabled={txPage <= 1}
                    className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                  >
                    {t('wallet.prevPage')}
                  </button>
                  <span className="text-xs text-muted-foreground">
                    {txPage} / {totalPages}
                  </span>
                  <button
                    onClick={() => setTxPage(p => Math.min(totalPages, p + 1))}
                    disabled={txPage >= totalPages}
                    className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                  >
                    {t('wallet.nextPage')}
                  </button>
                </div>
              )}
            </div>
          ) : (
            <div className="p-8 text-center text-muted-foreground text-sm">
              {loadingTx ? t('common.loading') : t('wallet.noTransactions')}
            </div>
          )}
        </div>
      </div>

      {/* Admin: recharge key management */}
      {isAdmin && keys && keys.length > 0 && (
        <div className="px-6 mb-8">
          <div className="rounded-xl border">
            <div className="border-b px-5 py-3 flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-muted-foreground" />
              <h3 className="font-semibold text-sm">{t('wallet.createKey')}</h3>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-xs text-muted-foreground">
                    <th className="text-left px-5 py-3 font-medium">Key</th>
                    <th className="text-right px-5 py-3 font-medium">{t('wallet.txAmount')}</th>
                    <th className="text-left px-5 py-3 font-medium">Used By</th>
                    <th className="text-left px-5 py-3 font-medium">Used At</th>
                    <th className="text-left px-5 py-3 font-medium">Created By</th>
                    <th className="text-left px-5 py-3 font-medium">Created At</th>
                  </tr>
                </thead>
                <tbody>
                  {keys.map((k) => (
                    <tr key={k.key} className="border-b last:border-0">
                      <td className="px-5 py-3 font-mono text-xs">{k.key.substring(0, 8)}...</td>
                      <td className="px-5 py-3 text-right font-mono">{fmt(k.amount)}</td>
                      <td className="px-5 py-3">{k.used_by || '—'}</td>
                      <td className="px-5 py-3 text-muted-foreground">
                        {k.used_at ? new Date(k.used_at).toLocaleDateString() : '—'}
                      </td>
                      <td className="px-5 py-3">{k.created_by}</td>
                      <td className="px-5 py-3 text-muted-foreground">
                        {new Date(k.created_at).toLocaleDateString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
