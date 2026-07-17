import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Plus, Pencil, Trash2, RefreshCw, Power } from 'lucide-react';
import { useFilterRules, useCreateFilterRule, useUpdateFilterRule, useDeleteFilterRule } from '@/api/moderation';
import { useChannels } from '@/api/channels';
import { api } from '@/api/client';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Switch } from '@/components/ui/switch';
import { Badge } from '@/components/ui/badge';
import type { ContentFilterRule } from '@/types';

const SCOPE_LABELS: Record<string, string> = {
  request: 'moderation.scopeRequest',
  response: 'moderation.scopeResponse',
  both: 'moderation.scopeBoth',
};

const ACTION_LABELS: Record<string, string> = {
  block: 'moderation.actionBlock',
  mask: 'moderation.actionMask',
};

const TYPE_LABELS: Record<string, string> = {
  regex: 'Regex',
  keyword: 'Keyword',
};

export default function ModerationPage() {
  const { t } = useTranslation();
  const { data: rules, isLoading, isError, refetch } = useFilterRules();
  const { data: channels } = useChannels();
  const createRule = useCreateFilterRule();
  const deleteRule = useDeleteFilterRule();
  const [editRule, setEditRule] = useState<ContentFilterRule | null>(null);
  const updateRule = useUpdateFilterRule(editRule?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [enabled, setEnabled] = useState(true);
  const [enabledLoading, setEnabledLoading] = useState(true);

  useEffect(() => {
    api<{ enabled: boolean }>('/moderation/enabled')
      .then((r) => setEnabled(r.enabled))
      .catch(() => {})
      .finally(() => setEnabledLoading(false));
  }, []);

  const toggleEnabled = async (checked: boolean) => {
    setEnabled(checked);
    try {
      await api('/moderation/enabled', { method: 'PUT', body: { enabled: checked } });
    } catch {
      setEnabled(!checked);
      toast.error('Failed to update moderation status');
    }
  };
  const [deleteTarget, setDeleteTarget] = useState<ContentFilterRule | null>(null);

  const [form, setForm] = useState<Partial<ContentFilterRule>>({
    name: '',
    pattern_type: 'keyword',
    pattern: '',
    action: 'block',
    scope: 'both',
    channel_id: null,
    replacement: '[REDACTED]',
    enabled: true,
    priority: 1,
  });

  const resetForm = () => {
    setForm({
      name: '',
      pattern_type: 'keyword',
      pattern: '',
      action: 'block',
      scope: 'both',
      channel_id: null,
      replacement: '[REDACTED]',
      enabled: true,
      priority: 1,
    });
  };

  const openAdd = () => {
    resetForm();
    setShowAdd(true);
    setEditRule(null);
  };

  const openEdit = (rule: ContentFilterRule) => {
    setForm({ ...rule });
    setEditRule(rule);
    setShowAdd(true);
  };

  const handleSubmit = async () => {
    if (!form.name || !form.pattern) {
      toast.error(t('moderation.requiredFields'));
      return;
    }
    if (editRule) {
      updateRule.mutate(form, {
        onSuccess: () => {
          toast.success(t('toast.updated'));
          setShowAdd(false);
          setEditRule(null);
          refetch();
        },
        onError: (err) => toast.error(err.message),
      });
    } else {
      createRule.mutate(form, {
        onSuccess: () => {
          toast.success(t('toast.created'));
          setShowAdd(false);
          refetch();
        },
        onError: (err) => toast.error(err.message),
      });
    }
  };

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteRule.mutate(deleteTarget.id, {
      onSuccess: () => {
        toast.success(t('toast.deleted'));
        setDeleteTarget(null);
        refetch();
      },
      onError: (err) => toast.error(err.message),
    });
  };

  const channelName = (chId: string | null | undefined) => {
    if (!chId) return t('moderation.global');
    const ch = channels?.find((c) => c.id === chId);
    return ch ? `${ch.name || ch.id} (${ch.provider})` : chId;
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('moderation.title')}
        description={t('moderation.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={openAdd}>
              <Plus className="size-4 mr-1" />{t('moderation.addRule')}
            </Button>
          </>
        }
      />

      {/* Global Toggle */}
      <Card>
        <CardContent className="p-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Power className="size-5 text-muted-foreground" />
            <div>
              <p className="text-sm font-medium">{t('moderation.enableLabel')}</p>
              <p className="text-xs text-muted-foreground">{t('moderation.enableHint')}</p>
            </div>
          </div>
          <Switch
            checked={enabled}
            onCheckedChange={toggleEnabled}
            disabled={enabledLoading}
          />
        </CardContent>
      </Card>

      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : isError ? (
            <div className="flex items-center justify-center p-8">
              <div className="text-center">
                <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
                <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
              </div>
            </div>
          ) : rules && rules.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('moderation.type')}</th>
                    <th className="text-left py-3 px-4">{t('moderation.scope')}</th>
                    <th className="text-left py-3 px-4">{t('moderation.action')}</th>
                    <th className="text-left py-3 px-4">{t('moderation.channel')}</th>
                    <th className="text-center py-3 px-4">{t('table.priority')}</th>
                    <th className="text-center py-3 px-4">{t('table.statusLabel')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {rules.map((rule) => (
                    <tr key={rule.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4">
                        <div className="font-medium">{rule.name}</div>
                        <div className="text-xs text-muted-foreground font-mono truncate max-w-[200px]">
                          {rule.pattern}
                        </div>
                      </td>
                      <td className="py-3 px-4">
                        <Badge variant="outline">{TYPE_LABELS[rule.pattern_type] || rule.pattern_type}</Badge>
                      </td>
                      <td className="py-3 px-4">{t(SCOPE_LABELS[rule.scope] || rule.scope)}</td>
                      <td className="py-3 px-4">
                        <Badge variant={rule.action === 'block' ? 'destructive' : 'secondary'}>
                          {t(ACTION_LABELS[rule.action] || rule.action)}
                        </Badge>
                      </td>
                      <td className="py-3 px-4 text-xs text-muted-foreground">
                        {channelName(rule.channel_id)}
                      </td>
                      <td className="py-3 px-4 text-center">{rule.priority}</td>
                      <td className="py-3 px-4 text-center">
                        <Switch checked={rule.enabled} onCheckedChange={() => {}} disabled />
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => openEdit(rule)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(rule)}>
                          <Trash2 className="size-3.5 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('moderation.noRules')} />
          )}
        </CardContent>
      </Card>

      {/* Add/Edit Dialog */}
      {showAdd && (
        <div className="fixed inset-0 z-50 flex items-start justify-center pt-[10vh]">
          <div className="fixed inset-0 bg-black/50" onClick={() => setShowAdd(false)} />
          <div className="relative bg-background rounded-lg shadow-lg w-full max-w-lg mx-4 p-6 space-y-4 max-h-[80vh] overflow-y-auto">
            <h2 className="text-lg font-semibold">
              {editRule ? t('moderation.editRule') : t('moderation.addRule')}
            </h2>

            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t('moderation.ruleName')}</label>
              <input
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                value={form.name || ''}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t('moderation.type')}</label>
                <select
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                  value={form.pattern_type}
                  onChange={(e) => setForm({ ...form, pattern_type: e.target.value as 'regex' | 'keyword' })}
                >
                  <option value="keyword">Keyword</option>
                  <option value="regex">Regex</option>
                </select>
              </div>

              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t('moderation.action')}</label>
                <select
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                  value={form.action}
                  onChange={(e) => setForm({ ...form, action: e.target.value as 'block' | 'mask' })}
                >
                  <option value="block">{t('moderation.actionBlock')}</option>
                  <option value="mask">{t('moderation.actionMask')}</option>
                </select>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t('moderation.scope')}</label>
                <select
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                  value={form.scope}
                  onChange={(e) => setForm({ ...form, scope: e.target.value as 'request' | 'response' | 'both' })}
                >
                  <option value="request">{t('moderation.scopeRequest')}</option>
                  <option value="response">{t('moderation.scopeResponse')}</option>
                  <option value="both">{t('moderation.scopeBoth')}</option>
                </select>
              </div>

              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t('moderation.priority')}</label>
                <input
                  type="number"
                  min="1"
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                  value={form.priority ?? 1}
                  onChange={(e) => setForm({ ...form, priority: parseInt(e.target.value) || 1 })}
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t('moderation.pattern')}</label>
              <textarea
                className="flex min-h-[60px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm"
                value={form.pattern || ''}
                onChange={(e) => setForm({ ...form, pattern: e.target.value })}
                placeholder={form.pattern_type === 'regex' ? '[1-9]\\d{17}[\\dXx]' : 'badword1, badword2'}
              />
              <p className="text-[11px] text-muted-foreground">
                {form.pattern_type === 'regex'
                  ? t('moderation.regexHint')
                  : t('moderation.keywordHint')}
              </p>
            </div>

            {form.action === 'mask' && (
              <div className="space-y-1.5">
                <label className="text-sm font-medium">{t('moderation.replacement')}</label>
                <input
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                  value={form.replacement || '[REDACTED]'}
                  onChange={(e) => setForm({ ...form, replacement: e.target.value })}
                />
              </div>
            )}

            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t('moderation.channel')}</label>
              <select
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
                value={form.channel_id || ''}
                onChange={(e) => setForm({ ...form, channel_id: e.target.value || null })}
              >
                <option value="">{t('moderation.global')}</option>
                {channels?.map((ch) => (
                  <option key={ch.id} value={ch.id}>
                    {ch.name || ch.id} ({ch.provider})
                  </option>
                ))}
              </select>
            </div>

            <div className="flex items-center gap-2">
              <label className="text-sm font-medium">{t('common.enabled')}</label>
              <Switch
                checked={form.enabled ?? true}
                onCheckedChange={(v) => setForm({ ...form, enabled: v })}
              />
            </div>

            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" onClick={() => setShowAdd(false)}>
                {t('common.cancel')}
              </Button>
              <Button onClick={handleSubmit} disabled={createRule.isPending || updateRule.isPending}>
                {editRule ? t('common.save') : t('common.create')}
              </Button>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteRule')} "${deleteTarget?.name}"?`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
