import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useContractPrices, useCreateContractPrice, useDeleteContractPrice } from '@/api/pricingChain';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Card, CardContent } from '@/components/ui/card';
import { Plus, Trash2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { ContractPrice } from '@/types';

export default function ContractPrices() {
  const { t } = useTranslation();
  const { data: prices, isLoading, isError, refetch } = useContractPrices();
  const createPrice = useCreateContractPrice();
  const deletePrice = useDeleteContractPrice();
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ContractPrice | null>(null);

  // Add form state
  const [formUserId, setFormUserId] = useState('');
  const [formModelId, setFormModelId] = useState('');
  const [formPromptPrice, setFormPromptPrice] = useState('');
  const [formCompletionPrice, setFormCompletionPrice] = useState('');
  const [formEffectiveFrom, setFormEffectiveFrom] = useState('');
  const [formEffectiveUntil, setFormEffectiveUntil] = useState('');
  const [formDescription, setFormDescription] = useState('');

  function resetForm() {
    setFormUserId('');
    setFormModelId('');
    setFormPromptPrice('');
    setFormCompletionPrice('');
    setFormEffectiveFrom('');
    setFormEffectiveUntil('');
    setFormDescription('');
  }

  function handleAdd() {
    const body: Record<string, unknown> = {
      user_id: formUserId,
      model_id: formModelId,
      prompt_price: parseFloat(formPromptPrice),
      completion_price: parseFloat(formCompletionPrice),
      effective_from: formEffectiveFrom,
    };
    if (formEffectiveUntil) body.effective_until = formEffectiveUntil;
    if (formDescription) body.description = formDescription;

    createPrice.mutate(body as Partial<ContractPrice>, {
      onSuccess: () => {
        toast.success(t('toast.created'));
        setShowAdd(false);
        resetForm();
        refetch();
      },
      onError: (err) => toast.error(err.message),
    });
  }

  const handleDelete = () => {
    if (!deleteTarget) return;
    deletePrice.mutate(deleteTarget.id, {
      onSuccess: () => {
        toast.success(t('toast.deleted'));
        setDeleteTarget(null);
        refetch();
      },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('contractPrice.title')}
        description={t('contractPrice.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('contractPrice.add')}
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
          ) : prices && prices.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.userId')}</th>
                    <th className="text-left py-3 px-4">{t('table.model')}</th>
                    <th className="text-right py-3 px-4">{t('contractPrice.promptPrice')}</th>
                    <th className="text-right py-3 px-4">{t('contractPrice.completionPrice')}</th>
                    <th className="text-left py-3 px-4">{t('contractPrice.effectiveFrom')}</th>
                    <th className="text-left py-3 px-4">{t('contractPrice.effectiveUntil')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {prices.map((p) => (
                    <tr key={p.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{p.user_id}</td>
                      <td className="py-3 px-4 font-mono text-xs">{p.model_id}</td>
                      <td className="py-3 px-4 text-right font-mono text-xs">{p.prompt_price}</td>
                      <td className="py-3 px-4 text-right font-mono text-xs">{p.completion_price}</td>
                      <td className="py-3 px-4 text-xs whitespace-nowrap">{p.effective_from}</td>
                      <td className="py-3 px-4 text-xs whitespace-nowrap text-muted-foreground">
                        {p.effective_until || '—'}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(p)}>
                          <Trash2 className="size-3.5 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noContractPrices')} />
          )}
        </CardContent>
      </Card>

      <Dialog open={showAdd} onOpenChange={(open) => { if (!open) { setShowAdd(false); resetForm(); }}}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t('contractPrice.new')}</DialogTitle>
            <DialogDescription>{t('contractPrice.newHint')}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label>{t('form.userId')}</Label>
                <Input value={formUserId} onChange={(e) => setFormUserId(e.target.value)} placeholder="user-1" />
              </div>
              <div className="space-y-2">
                <Label>{t('form.modelName')}</Label>
                <Input value={formModelId} onChange={(e) => setFormModelId(e.target.value)} placeholder="gpt-4" />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label>{t('contractPrice.promptPrice')}</Label>
                <Input type="number" step="0.000001" min="0" value={formPromptPrice} onChange={(e) => setFormPromptPrice(e.target.value)} />
              </div>
              <div className="space-y-2">
                <Label>{t('contractPrice.completionPrice')}</Label>
                <Input type="number" step="0.000001" min="0" value={formCompletionPrice} onChange={(e) => setFormCompletionPrice(e.target.value)} />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label>{t('contractPrice.effectiveFrom')}</Label>
                <Input type="datetime-local" value={formEffectiveFrom} onChange={(e) => setFormEffectiveFrom(e.target.value)} />
              </div>
              <div className="space-y-2">
                <Label>{t('contractPrice.effectiveUntil')} <span className="text-muted-foreground">({t('common.optional')})</span></Label>
                <Input type="datetime-local" value={formEffectiveUntil} onChange={(e) => setFormEffectiveUntil(e.target.value)} />
              </div>
            </div>
            <div className="space-y-2">
              <Label>{t('form.description')} <span className="text-muted-foreground">({t('common.optional')})</span></Label>
              <Textarea value={formDescription} onChange={(e) => setFormDescription(e.target.value)} placeholder={t('contractPrice.descPlaceholder')} />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" onClick={() => { setShowAdd(false); resetForm(); }}>{t('common.cancel')}</Button>
              <Button onClick={handleAdd} disabled={!formUserId || !formModelId || !formPromptPrice || !formCompletionPrice || !formEffectiveFrom || createPrice.isPending}>
                {t('common.save')}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteContractPrice')}${deleteTarget?.id}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
