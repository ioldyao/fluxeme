import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useWalletOverview, useWalletTransactions, useRecharge, useRedeemKey, useCreateRechargeKey, useRechargeKeys, useEstimatedDays } from '@/api/wallet';
import { useCurrency } from '@/store/currency';
import { useAuth } from '@/store/auth';
import { PageHeader } from '@/components/PageHeader';
import { Wallet, CreditCard, KeyRound, Receipt, AlertTriangle, Copy, Check, Loader2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';

export default function WalletPage() {
  const { t, i18n } = useTranslation();
  const { currency, rate } = useCurrency();
  const { role } = useAuth();
  const isAdmin = role === 'admin';

  const { data: overview, isLoading: loadingOv } = useWalletOverview();
  const { data: txData, isLoading: loadingTx } = useWalletTransactions(1, 100);
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
  const [threshold, setThreshold] = useState('');

  const fmt = (usd: number) => {
    const v = currency === 'cny' ? usd * rate : usd;
    const s = currency === 'cny' ? '¥' : '$';
    return `${s}${v.toFixed(2)}`;
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

  const lowBalance = overview && estimated?.days != null && estimated.days < 7;

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
          {txData && txData.items.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-xs text-muted-foreground">
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txTime')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txType')}</th>
                    <th className="text-right px-5 py-3 font-medium">{t('wallet.txAmount')}</th>
                    <th className="text-right px-5 py-3 font-medium">{t('wallet.txBefore')}</th>
                    <th className="text-right px-5 py-3 font-medium">{t('wallet.txAfter')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txMethod')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txStatus')}</th>
                    <th className="text-left px-5 py-3 font-medium">{t('wallet.txNote')}</th>
                  </tr>
                </thead>
                <tbody>
                  {txData.items.map((tx) => (
                    <tr key={tx.id} className="border-b last:border-0">
                      <td className="px-5 py-3 text-muted-foreground whitespace-nowrap">
                        {new Date(tx.created_at).toLocaleString(i18n.language === 'zh' ? 'zh-CN' : 'en-US')}
                      </td>
                      <td className="px-5 py-3">
                        <span className={`text-xs font-medium px-2 py-0.5 rounded-full ${
                          tx.tx_type === 'recharge' ? 'bg-green-500/10 text-green-600' : 'bg-destructive/10 text-destructive'
                        }`}>
                          {tx.tx_type === 'recharge' ? t('wallet.type.recharge') : t('wallet.type.deduction')}
                        </span>
                      </td>
                      <td className={`px-5 py-3 text-right font-mono ${tx.amount >= 0 ? 'text-green-600' : 'text-destructive'}`}>
                        {tx.amount >= 0 ? '+' : ''}{fmt(Math.abs(tx.amount))}
                      </td>
                      <td className="px-5 py-3 text-right font-mono text-muted-foreground">{fmt(tx.balance_before)}</td>
                      <td className="px-5 py-3 text-right font-mono text-muted-foreground">{fmt(tx.balance_after)}</td>
                      <td className="px-5 py-3 text-muted-foreground">{tx.method}</td>
                      <td className="px-5 py-3">
                        <span className={`text-xs ${tx.status === 'completed' ? 'text-green-600' : 'text-muted-foreground'}`}>
                          {tx.status}
                        </span>
                      </td>
                      <td className="px-5 py-3 text-muted-foreground max-w-[200px] truncate">{tx.note}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
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
