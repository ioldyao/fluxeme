import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRules, useDeleteRule } from '@/api/rules';
import { RuleForm } from '@/forms/RuleForm';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Pencil, Trash2, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { RoutingRule } from '@/types';

export default function Rules() {
  const { t } = useTranslation();
  const { data: rules, isLoading, refetch } = useRules();
  const deleteRule = useDeleteRule();
  const [editRule, setEditRule] = useState<RoutingRule | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<RoutingRule | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteRule.mutate(deleteTarget.name, {
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
          ) : rules && rules.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.userId')}</th>
                    <th className="text-left py-3 px-4">{t('table.modelPattern')}</th>
                    <th className="text-left py-3 px-4">{t('table.channel')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {rules.map((rule) => (
                    <tr key={rule.name} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{rule.name}</td>
                      <td className="py-3 px-4">{rule.user_id}</td>
                      <td className="py-3 px-4 text-xs font-mono text-muted-foreground">{rule.model_pattern}</td>
                      <td className="py-3 px-4">{rule.channel_id}</td>
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
