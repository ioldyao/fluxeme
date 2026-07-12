import { useState, useMemo, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useWalletOverview, useWalletTransactions, useRecharge, useRedeemKey, useCreateRechargeKey, useRechargeKeys, useRevokeKey, useEstimatedDays } from '@/api/wallet';
import { useCurrency } from '@/store/currency';
import { useAuth } from '@/store/auth';
import { PageHeader } from '@/components/PageHeader';
import { Wallet, CreditCard, KeyRound, Receipt, AlertTriangle, Copy, Check, Loader2 } from 'lucide-react';
import { toast } from 'sonner';

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}

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
  const totalPages = Math.max(1, Math.ceil((txData?.total_dates ?? 0) / PAGE_SIZE));

  const toLocalDate = (utcStr: string) => {
    const d = new Date(utcStr);
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
  };

  // Group transactions by day with aggregates
  const dayGroups = useMemo(() => {
    if (!txData?.items) return [];
    const groups: Record<string, typeof txData.items> = {};
    for (const tx of txData.items) {
      const day = toLocalDate(tx.created_at);
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

  // ── Key filter state ──
  const [keySearch, setKeySearch] = useState('');
  const [keyStatus, setKeyStatus] = useState('');
  const [keyUserSearch, setKeyUserSearch] = useState('');
  const [createKeyExpiry, setCreateKeyExpiry] = useState('');
  const debouncedKeySearch = useDebounce(keySearch, 300);
  const debouncedKeyUser = useDebounce(keyUserSearch, 300);
  const revokeKey = useRevokeKey();

  const toggleDay = (day: string) => {
    setExpandedDays(prev => {
      const next = new Set(prev);
      if (next.has(day)) next.delete(day); else next.add(day);
      return next;
    });
  };
  const { data: estimated } = useEstimatedDays();
  const [keyPage, setKeyPage] = useState(1);
  const KEY_PAGE_SIZE = 20;
  const { data: keysData } = useRechargeKeys(keyPage, KEY_PAGE_SIZE, {
    search: debouncedKeySearch || undefined,
    status: keyStatus || undefined,
    used_by: debouncedKeyUser || undefined,
  });
  const keys = keysData?.items;
  const keyTotal = keysData?.total ?? 0;
  const keyTotalPages = Math.max(1, Math.ceil(keyTotal / KEY_PAGE_SIZE));
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
    toast.info(t('wallet.devInProgress'));
  };

  const handleRedeem = () => {
    if (!keyInput.trim()) return;
    redeem.mutate(keyInput.trim(), {
      onSuccess: (res) => {
        toast.success(t('wallet.redeemSuccess', { amount: fmt(res.amount) }));
        setKeyInput('');
      },
      onError: (err: Error) => {
        toast.error(err.message);
      },
    });
  };

  const handleCreateKey = () => {
    const amt = Number(createKeyAmt);
    if (!amt || amt <= 0) return;
    const expires_at = createKeyExpiry ? new Date(createKeyExpiry).toISOString() : undefined;
    createKey.mutate({ amount: amt, expires_at }, {
      onSuccess: (res) => {
        setNewKey(res.key);
        setCreateKeyAmt('');
        setCreateKeyExpiry('');
        toast.success(t('wallet.createKeySuccess'));
      },
      onError: (err: Error) => {
        toast.error(err.message);
      },
    });
  };

  const handleRevokeKey = (key: string) => {
    if (!window.confirm(t('wallet.revokeConfirm', { key: key.substring(0, 8) + '...' }))) return;
    revokeKey.mutate(key, {
      onSuccess: () => toast.success(t('wallet.revokeSuccess')),
      onError: (err: Error) => toast.error(err.message),
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
              <div className="mt-2">
                <label className="text-xs text-muted-foreground">{t('wallet.createKeyExpiresLabel')}</label>
                <input
                  type="datetime-local"
                  value={createKeyExpiry}
                  onChange={(e) => setCreateKeyExpiry(e.target.value)}
                  className="mt-1 h-9 w-full rounded-md border bg-background px-3 text-sm"
                />
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
                {t('wallet.txTotal', { total: txData.total_dates })}
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
                          <span
                            onClick={(e) => { e.stopPropagation(); navigate(`/usage?date=${day.date}`); }}
                            className="text-destructive hover:underline cursor-pointer"
                          >
                            {t('wallet.groupDeduction', { count: day.deductionCount, amount: fmt(day.deductionTotal) })}
                          </span>
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
      {isAdmin && (
        <div className="px-6 mb-8">
          <div className="rounded-xl border">
            <div className="border-b px-5 py-3 flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-muted-foreground" />
              <h3 className="font-semibold text-sm">{t('wallet.createKey')}</h3>
              <span className="text-xs text-muted-foreground ml-auto">{t('wallet.txTotal', { total: keyTotal })}</span>
            </div>

            {/* ── Filter bar ── */}
            <div className="border-b px-5 py-2.5 flex items-center gap-3 flex-wrap">
              <input
                type="text"
                placeholder={t('wallet.filterByKey')}
                value={keySearch}
                onChange={(e) => { setKeySearch(e.target.value); setKeyPage(1); }}
                className="h-7 w-40 rounded-md border bg-background px-2 text-xs"
              />
              <input
                type="text"
                placeholder={t('wallet.filterByUser')}
                value={keyUserSearch}
                onChange={(e) => { setKeyUserSearch(e.target.value); setKeyPage(1); }}
                className="h-7 w-40 rounded-md border bg-background px-2 text-xs"
              />
              <select
                value={keyStatus}
                onChange={(e) => { setKeyStatus(e.target.value); setKeyPage(1); }}
                className="h-7 rounded-md border bg-background px-2 text-xs"
              >
                <option value="">{t('wallet.filterAllTypes')}</option>
                <option value="active">{t('wallet.statusActive')}</option>
                <option value="used">{t('wallet.statusUsed')}</option>
                <option value="expired">{t('wallet.statusExpired')}</option>
                <option value="revoked">{t('wallet.statusRevoked')}</option>
              </select>
            </div>

            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-xs text-muted-foreground">
                    <th className="text-left px-5 py-3 font-medium">Key</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.keyStatus')}</th>
                    <th className="text-right px-5 py-3 font-medium">{t('wallet.txAmount')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.usedBy')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.usedAt')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.keyExpires')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.createdBy')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.createdAt')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txAction')}</th>
                  </tr>
                </thead>
                <tbody>
                  {keys && keys.length > 0 ? keys.map((k) => {
                    const now = new Date();
                    const isUsed = !!k.used_by;
                    const isExpired = !isUsed && !!k.expires_at && new Date(k.expires_at) < now;
                    const isRevoked = k.revoked;
                    const statusClass = isUsed ? 'bg-gray-500/10 text-gray-500' : isExpired ? 'bg-yellow-500/10 text-yellow-600' : isRevoked ? 'bg-destructive/10 text-destructive' : 'bg-green-500/10 text-green-600';
                    const statusLabel = isUsed ? t('wallet.statusUsed') : isExpired ? t('wallet.statusExpired') : isRevoked ? t('wallet.statusRevoked') : t('wallet.statusActive');
                    const isActive = !isUsed && !isExpired && !isRevoked;
                    return (
                      <tr key={k.key} className="border-b last:border-0">
                        <td className="px-5 py-3 font-mono text-xs">{k.key.substring(0, 8)}...</td>
                        <td className="px-5 py-3">
                          <span className={`text-xs font-medium px-2 py-0.5 rounded-full ${statusClass}`}>
                            {statusLabel}
                          </span>
                        </td>
                        <td className="px-5 py-3 text-right font-mono">{fmt(k.amount)}</td>
                        <td className="px-5 py-3">{k.used_by || '—'}</td>
                        <td className="px-5 py-3 text-muted-foreground text-xs">
                          {k.used_at ? new Date(k.used_at).toLocaleString() : '—'}
                        </td>
                        <td className="px-5 py-3 text-muted-foreground text-xs">
                          {k.expires_at ? new Date(k.expires_at).toLocaleDateString() : t('wallet.keyNeverExpires')}
                        </td>
                        <td className="px-5 py-3">{k.created_by}</td>
                        <td className="px-5 py-3 text-muted-foreground text-xs">
                          {new Date(k.created_at).toLocaleDateString()}
                        </td>
                        <td className="px-5 py-3">
                          {isActive && (
                            <button
                              onClick={() => handleRevokeKey(k.key)}
                              disabled={revokeKey.isPending}
                              className="text-xs px-2 py-1 rounded-md border border-destructive/30 text-destructive hover:bg-destructive/10 disabled:opacity-50"
                            >
                              {t('wallet.revokeKey')}
                            </button>
                          )}
                        </td>
                      </tr>
                    );
                  }) : (
                    <tr>
                      <td colSpan={9} className="px-5 py-8 text-center text-muted-foreground text-sm">
                        {t('wallet.empty')}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            {keyTotalPages > 1 && (
              <div className="flex items-center justify-center gap-2 px-5 py-3 border-t">
                <button
                  onClick={() => setKeyPage(p => Math.max(1, p - 1))}
                  disabled={keyPage <= 1}
                  className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                >
                  {t('wallet.prevPage')}
                </button>
                <span className="text-xs text-muted-foreground">
                  {keyPage} / {keyTotalPages}
                </span>
                <button
                  onClick={() => setKeyPage(p => Math.min(keyTotalPages, p + 1))}
                  disabled={keyPage >= keyTotalPages}
                  className="px-3 py-1 text-xs rounded-md border hover:bg-accent disabled:opacity-30"
                >
                  {t('wallet.nextPage')}
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
