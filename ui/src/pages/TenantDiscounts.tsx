import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useTenantDiscounts, useCreateTenantDiscount, useDeleteTenantDiscount } from '@/api/pricingChain';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Card, CardContent } from '@/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Plus, Trash2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { TenantDiscount } from '@/types';

export default function TenantDiscounts() {
  const { t } = useTranslation();
  const { data: discounts, isLoading, isError, refetch } = useTenantDiscounts();
  const createDiscount = useCreateTenantDiscount();
  const deleteDiscount = useDeleteTenantDiscount();
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<TenantDiscount | null>(null);

  // Add form state
  const [formUserId, setFormUserId] = useState('');
  const [formModelId, setFormModelId] = useState('');
  const [formDiscountType, setFormDiscountType] = useState<'Percentage' | 'Fixed'>('Percentage');
  const [formDiscountValue, setFormDiscountValue] = useState('');
  const [formDescription, setFormDescription] = useState('');

  function resetForm() {
    setFormUserId('');
    setFormModelId('');
    setFormDiscountType('Percentage');
    setFormDiscountValue('');
    setFormDescription('');
  }

  function handleAdd() {
    const body: Record<string, unknown> = {
      user_id: formUserId,
      model_id: formModelId,
      discount_type: formDiscountType,
      discount_value: parseFloat(formDiscountValue),
    };
    if (formDescription) body.description = formDescription;

    createDiscount.mutate(body as Partial<TenantDiscount>, {
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
    deleteDiscount.mutate(deleteTarget.id, {
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
        title={t('tenantDiscount.title')}
        description={t('tenantDiscount.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('tenantDiscount.add')}
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
          ) : discounts && discounts.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.userId')}</th>
                    <th className="text-left py-3 px-4">{t('table.model')}</th>
                    <th className="text-center py-3 px-4">{t('tenantDiscount.type')}</th>
                    <th className="text-right py-3 px-4">{t('tenantDiscount.value')}</th>
                    <th className="text-left py-3 px-4">{t('form.description')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {discounts.map((d) => (
                    <tr key={d.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{d.user_id}</td>
                      <td className="py-3 px-4 font-mono text-xs">{d.model_id}</td>
                      <td className="py-3 px-4 text-center text-xs">
                        {d.discount_type === 'Percentage' ? '%' : t('tenantDiscount.fixed')}
                      </td>
                      <td className="py-3 px-4 text-right font-mono text-xs">
                        {d.discount_type === 'Percentage' ? `${d.discount_value}%` : d.discount_value}
                      </td>
                      <td className="py-3 px-4 text-xs text-muted-foreground max-w-[200px] truncate">
                        {d.description || '—'}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(d)}>
                          <Trash2 className="size-3.5 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noTenantDiscounts')} />
          )}
        </CardContent>
      </Card>

      <Dialog open={showAdd} onOpenChange={(open) => { if (!open) { setShowAdd(false); resetForm(); }}}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t('tenantDiscount.new')}</DialogTitle>
            <DialogDescription>{t('tenantDiscount.newHint')}</DialogDescription>
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
                <Label>{t('tenantDiscount.type')}</Label>
                <Select value={formDiscountType} onValueChange={(v: 'Percentage' | 'Fixed') => setFormDiscountType(v)}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="Percentage">{t('tenantDiscount.percentage')}</SelectItem>
                    <SelectItem value="Fixed">{t('tenantDiscount.fixedPrice')}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label>{t('tenantDiscount.value')}</Label>
                <Input
                  type="number"
                  step={formDiscountType === 'Percentage' ? '1' : '0.000001'}
                  min="0"
                  max={formDiscountType === 'Percentage' ? '100' : undefined}
                  value={formDiscountValue}
                  onChange={(e) => setFormDiscountValue(e.target.value)}
                  placeholder={formDiscountType === 'Percentage' ? '10' : '0.001'}
                />
              </div>
            </div>
            <div className="space-y-2">
              <Label>{t('form.description')} <span className="text-muted-foreground">({t('common.optional')})</span></Label>
              <Textarea value={formDescription} onChange={(e) => setFormDescription(e.target.value)} placeholder={t('tenantDiscount.descPlaceholder')} />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" onClick={() => { setShowAdd(false); resetForm(); }}>{t('common.cancel')}</Button>
              <Button onClick={handleAdd} disabled={!formUserId || !formModelId || !formDiscountValue || createDiscount.isPending}>
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
        description={`${t('confirm.deleteTenantDiscount')}${deleteTarget?.id}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
