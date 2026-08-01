import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Copy, Check, Loader2, KeyRound, Search } from 'lucide-react';
import { useRechargeKeys, useCreateRechargeKey, useRevokeKey } from '@/api/wallet';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@fluxeme/ui/button';

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}

export default function RechargeKeys() {
  const { t } = useTranslation();

  // ── Create key ──
  const [createKeyAmt, setCreateKeyAmt] = useState('');
  const [createKeyExpiry, setCreateKeyExpiry] = useState('');
  const [newKey, setNewKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const createKey = useCreateRechargeKey();

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

  const copyKey = async (key: string) => {
    await navigator.clipboard.writeText(key);
    setCopied(true);
    toast.success(t('wallet.keyCopied'));
    setTimeout(() => setCopied(false), 2000);
  };

  // ── List keys ──
  const [keySearch, setKeySearch] = useState('');
  const [keyStatus, setKeyStatus] = useState('');
  const [keyUserSearch, setKeyUserSearch] = useState('');
  const [keyPage, setKeyPage] = useState(1);
  const KEY_PAGE_SIZE = 20;
  const debouncedKeySearch = useDebounce(keySearch, 300);
  const debouncedKeyUser = useDebounce(keyUserSearch, 300);

  const { data: keysData } = useRechargeKeys(keyPage, KEY_PAGE_SIZE, {
    search: debouncedKeySearch || undefined,
    status: keyStatus || undefined,
    used_by: debouncedKeyUser || undefined,
  });
  const keys = keysData?.items;
  const keyTotal = keysData?.total ?? 0;
  const keyTotalPages = Math.max(1, Math.ceil(keyTotal / KEY_PAGE_SIZE));

  // ── Revoke key ──
  const revokeKey = useRevokeKey();
  const handleRevokeKey = (key: string) => {
    if (!window.confirm(t('wallet.revokeConfirm', { key: key.substring(0, 8) + '...' }))) return;
    revokeKey.mutate(key, {
      onSuccess: () => toast.success(t('wallet.revokeSuccess')),
      onError: (err: Error) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title={t('wallet.createKey')}
        description={t('wallet.createKeySub')}
      />

      {/* ── Create Key ── */}
      <div className="rounded-xl border">
        <div className="border-b px-5 py-3 flex items-center gap-2">
          <KeyRound className="h-4 w-4 text-muted-foreground" />
          <h3 className="font-semibold text-sm">{t('wallet.createKey')}</h3>
        </div>
        <div className="p-5 space-y-4">
          <div className="flex items-end gap-3">
            <div className="flex-1">
              <label className="text-xs text-muted-foreground mb-1 block">{t('wallet.txAmount')}</label>
              <input
                type="number"
                min="1"
                placeholder="0.00"
                value={createKeyAmt}
                onChange={(e) => setCreateKeyAmt(e.target.value)}
                className="h-9 w-full rounded-md border bg-background px-3 text-sm"
              />
            </div>
            <div className="flex-1">
              <label className="text-xs text-muted-foreground mb-1 block">{t('wallet.createKeyExpiresLabel')}</label>
              <input
                type="datetime-local"
                value={createKeyExpiry}
                onChange={(e) => setCreateKeyExpiry(e.target.value)}
                className="h-9 w-full rounded-md border bg-background px-3 text-sm"
              />
            </div>
            <Button
              variant="default"
              size="sm"
              onClick={handleCreateKey}
              disabled={createKey.isPending || !createKeyAmt}
              className="shrink-0"
            >
              {createKey.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-1" /> : null}
              {t('wallet.createKeyBtn')}
            </Button>
          </div>

          {newKey && (
            <div className="flex items-center gap-2 p-3 rounded-md bg-muted">
              <code className="flex-1 text-xs font-mono break-all">{newKey}</code>
              <button onClick={() => copyKey(newKey)} className="p-1 hover:text-foreground text-muted-foreground">
                {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* ── Key List ── */}
      <div className="rounded-xl border">
        <div className="border-b px-5 py-3 flex items-center gap-2">
          <h3 className="font-semibold text-sm">{t('wallet.txTotal', { total: keyTotal })}</h3>
        </div>

        {/* Filter bar */}
        <div className="border-b px-5 py-2.5 flex items-center gap-3 flex-wrap">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
            <input
              type="text"
              placeholder={t('wallet.filterByKey')}
              value={keySearch}
              onChange={(e) => { setKeySearch(e.target.value); setKeyPage(1); }}
              className="h-7 w-40 rounded-md border bg-background pl-8 pr-2 text-xs"
            />
          </div>
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
            <input
              type="text"
              placeholder={t('wallet.filterByUser')}
              value={keyUserSearch}
              onChange={(e) => { setKeyUserSearch(e.target.value); setKeyPage(1); }}
              className="h-7 w-40 rounded-md border bg-background pl-8 pr-2 text-xs"
            />
          </div>
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

        {/* Table */}
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
                const statusClass = isUsed
                  ? 'bg-gray-500/10 text-gray-500'
                  : isExpired
                    ? 'bg-yellow-500/10 text-yellow-600'
                    : isRevoked
                      ? 'bg-destructive/10 text-destructive'
                      : 'bg-green-500/10 text-green-600';
                const statusLabel = isUsed
                  ? t('wallet.statusUsed')
                  : isExpired
                    ? t('wallet.statusExpired')
                    : isRevoked
                      ? t('wallet.statusRevoked')
                      : t('wallet.statusActive');
                const isActive = !isUsed && !isExpired && !isRevoked;
                return (
                  <tr key={k.key} className="border-b last:border-0">
                    <td className="px-5 py-3 font-mono text-xs">{k.key.substring(0, 8)}...</td>
                    <td className="px-5 py-3">
                      <span className={`text-xs font-medium px-2 py-0.5 rounded-full ${statusClass}`}>
                        {statusLabel}
                      </span>
                    </td>
                    <td className="px-5 py-3 text-right font-mono text-sm">{k.amount.toFixed(6)}</td>
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
                        <Button
                          variant="outline"
                          size="xs"
                          onClick={() => handleRevokeKey(k.key)}
                          disabled={revokeKey.isPending}
                          className="border-destructive/30 text-destructive hover:bg-destructive/10"
                        >
                          {t('wallet.revokeKey')}
                        </Button>
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

        {/* Pagination */}
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
  );
}
