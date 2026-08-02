import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRules, useCreateRule, useUpdateRule, useDeleteRule } from '@fluxeme/shared/src/api/rules';
import { RuleForm } from '@/forms/RuleForm';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { Pencil, Trash2, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { RoutingRule } from '@fluxeme/shared/src/types';

export default function Rules() {
  const { t } = useTranslation();
  const { data: rules, isLoading, isError, refetch } = useRules();
  const createRule = useCreateRule();
  const deleteRule = useDeleteRule();
  const [editRule, setEditRule] = useState<RoutingRule | null>(null);
  const updateRule = useUpdateRule(editRule?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<RoutingRule | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteRule.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('rule.title')}
        description={t('rule.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('rule.add')}
            </Button>
          </>
        }
      />
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
                    <th className="text-left py-3 px-4">{t('table.userId')}</th>
                    <th className="text-left py-3 px-4">{t('form.sourceModel')}</th>
                    <th className="text-left py-3 px-4">{t('form.targetModel')}</th>
                    <th className="text-left py-3 px-4">{t('table.channel')}</th>
                    <th className="text-left py-3 px-4">{t('form.upstreamModel')}</th>
                    <th className="text-left py-3 px-4">{t('form.priority')}</th>
                    <th className="text-left py-3 px-4">{t('form.enabled')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {rules.map((rule) => (
                    <tr key={rule.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{rule.name}</td>
                      <td className="py-3 px-4">{rule.user_id}</td>
                      <td className="py-3 px-4 text-xs font-mono text-muted-foreground">{rule.source_model}</td>
                      <td className="py-3 px-4 text-xs font-mono text-muted-foreground">{rule.target_model || '-'}</td>
                      <td className="py-3 px-4 text-xs">{rule.channel_id || '-'}</td>
                      <td className="py-3 px-4 text-xs">{rule.upstream_model || '-'}</td>
                      <td className="py-3 px-4">{rule.priority}</td>
                      <td className="py-3 px-4">
                        {rule.enabled ? <Badge variant="default" className="text-xs">ON</Badge> : <Badge variant="secondary" className="text-xs">OFF</Badge>}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditRule(rule)}>
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
            <EmptyState message={t('empty.noRules')} />
          )}
        </CardContent>
      </Card>
      {(showAdd || editRule) && (
        <RuleForm
          rule={editRule}
          open={true}
          onOpenChange={(open) => { if (!open) { setShowAdd(false); setEditRule(null); }}}
          onSubmit={(data) => {
            if (editRule) {
              updateRule.mutate(data, {
                onSuccess: () => { toast.success(t('toast.updated')); setEditRule(null); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            } else {
              createRule.mutate(data, {
                onSuccess: () => { toast.success(t('toast.created')); setShowAdd(false); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            }
          }}
          isPending={createRule.isPending || updateRule.isPending}
        />
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteRule')}${deleteTarget?.name}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
