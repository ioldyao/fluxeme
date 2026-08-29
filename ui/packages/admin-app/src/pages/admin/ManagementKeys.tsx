import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy, KeyRound, Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import {
  createManagementApiKey,
  deleteManagementApiKey,
  listManagementApiKeys,
  setManagementApiKeyEnabled,
} from '@fluxeme/shared/src/api/managementKeys';
import type { ManagementApiKey } from '@fluxeme/shared/src/types';

export default function ManagementKeys() {
  const { t } = useTranslation();
  const [keys, setKeys] = useState<ManagementApiKey[]>([]);
  const [name, setName] = useState('');
  const [expiresAt, setExpiresAt] = useState('');
  const [newKey, setNewKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);

  const loadKeys = async () => {
    try {
      setKeys(await listManagementApiKeys());
    } catch {
      toast.error(t('managementKeys.loadFailed'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadKeys();
  }, []);

  const handleCreate = async () => {
    if (name.trim().length > 100) {
      toast.error(t('managementKeys.nameTooLong'));
      return;
    }
    setCreating(true);
    try {
      const result = await createManagementApiKey({
        name: name.trim() || undefined,
        expires_at: expiresAt ? new Date(expiresAt).toISOString() : undefined,
      });
      setNewKey(result.key);
      setName('');
      setExpiresAt('');
      await loadKeys();
      toast.success(t('managementKeys.created'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('managementKeys.createFailed'));
    } finally {
      setCreating(false);
    }
  };

  const handleCopy = async () => {
    if (!newKey) return;
    await navigator.clipboard.writeText(newKey);
    setCopied(true);
    toast.success(t('managementKeys.copied'));
    window.setTimeout(() => setCopied(false), 2000);
  };

  const handleEnabled = async (key: ManagementApiKey, enabled: boolean) => {
    try {
      await setManagementApiKeyEnabled(key.id, enabled);
      setKeys((current) => current.map((item) => (
        item.id === key.id ? { ...item, enabled } : item
      )));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('managementKeys.updateFailed'));
    }
  };

  const handleDelete = async (key: ManagementApiKey) => {
    if (!window.confirm(t('managementKeys.deleteConfirm', { name: key.name || key.key_prefix }))) {
      return;
    }
    try {
      await deleteManagementApiKey(key.id);
      setKeys((current) => current.filter((item) => item.id !== key.id));
      toast.success(t('managementKeys.deleted'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('managementKeys.deleteFailed'));
    }
  };

  return (
    <div className="max-w-4xl space-y-6 animate-fade-in">
      <PageHeader
        title={t('managementKeys.title')}
        description={t('managementKeys.subtitle')}
      />

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <KeyRound className="size-4 text-brand" />
            {t('managementKeys.createTitle')}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-muted-foreground">{t('managementKeys.securityHint')}</p>
          <div className="grid gap-4 md:grid-cols-[1fr_220px_auto] md:items-end">
            <div className="space-y-1.5">
              <label htmlFor="management-key-name" className="text-sm font-medium">
                {t('managementKeys.nameLabel')}
              </label>
              <Input
                id="management-key-name"
                value={name}
                maxLength={100}
                placeholder={t('managementKeys.namePlaceholder')}
                onChange={(event) => setName(event.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <label htmlFor="management-key-expiry" className="text-sm font-medium">
                {t('managementKeys.expiresLabel')}
              </label>
              <Input
                id="management-key-expiry"
                type="datetime-local"
                value={expiresAt}
                onChange={(event) => setExpiresAt(event.target.value)}
              />
            </div>
            <Button onClick={handleCreate} disabled={creating}>
              <Plus className="mr-1 size-4" />
              {creating ? t('common.loading') : t('managementKeys.createButton')}
            </Button>
          </div>

          {newKey ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4">
              <p className="mb-2 text-sm font-medium text-destructive">
                {t('managementKeys.copyWarning')}
              </p>
              <div className="flex items-center gap-2">
                <code className="min-w-0 flex-1 break-all rounded bg-background px-3 py-2 font-mono text-xs">
                  {newKey}
                </code>
                <Button variant="outline" size="sm" onClick={handleCopy}>
                  {copied ? <Check className="mr-1 size-4" /> : <Copy className="mr-1 size-4" />}
                  {copied ? t('managementKeys.copied') : t('managementKeys.copy')}
                </Button>
              </div>
            </div>
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('managementKeys.listTitle')}</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          {loading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : keys.length === 0 ? (
            <div className="p-8 text-center text-muted-foreground">{t('managementKeys.empty')}</div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-xs text-muted-foreground">
                    <th className="px-5 py-3 font-medium">{t('managementKeys.keyColumn')}</th>
                    <th className="px-5 py-3 font-medium">{t('managementKeys.nameColumn')}</th>
                    <th className="px-5 py-3 font-medium">{t('managementKeys.expiresColumn')}</th>
                    <th className="px-5 py-3 font-medium">{t('managementKeys.statusColumn')}</th>
                    <th className="px-5 py-3 text-right font-medium">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {keys.map((key) => (
                    <tr key={key.id} className="border-b last:border-0">
                      <td className="px-5 py-3 font-mono text-xs">{key.key_prefix}</td>
                      <td className="px-5 py-3">{key.name || '—'}</td>
                      <td className="px-5 py-3 text-xs text-muted-foreground">
                        {key.expires_at ? new Date(key.expires_at).toLocaleString() : t('managementKeys.never')}
                      </td>
                      <td className="px-5 py-3">
                        <Switch
                          checked={key.enabled}
                          aria-label={t('managementKeys.toggleLabel', { name: key.name || key.key_prefix })}
                          onCheckedChange={(enabled) => void handleEnabled(key, enabled)}
                        />
                      </td>
                      <td className="px-5 py-3 text-right">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="text-destructive hover:text-destructive"
                          aria-label={t('managementKeys.delete')}
                          onClick={() => void handleDelete(key)}
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
