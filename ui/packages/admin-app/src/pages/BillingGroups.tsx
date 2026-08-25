import { useState } from 'react';
import { Plus, RefreshCw, Power, Trash2 } from 'lucide-react';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { api } from '@fluxeme/shared/src/api/client';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { toast } from 'sonner';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

type PaymentMode = 'prepaid' | 'postpaid';
type BillingGroup = {
  id: string;
  name: string;
  payment_mode: PaymentMode;
  status: 'active' | 'inactive' | string;
  is_default: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
  deleted_by?: string | null;
};

const modeLabel = (mode: PaymentMode): string => mode === 'postpaid' ? '后付费' : '预付费';
const statusLabel = (status: string): string => status === 'active' ? '生效中' : '已停用';

export default function BillingGroups() {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [mode, setMode] = useState<PaymentMode>('prepaid');
  const [deleteTarget, setDeleteTarget] = useState<BillingGroup | null>(null);
  const groups = useQuery({
    queryKey: ['admin', 'billing-groups'],
    queryFn: () => api<BillingGroup[]>('/admin/billing-groups'),
  });
  const create = useMutation({
    mutationFn: () => api<BillingGroup>('/admin/billing-groups', {
      method: 'POST',
      body: { name: name.trim(), payment_mode: mode },
    }),
    onSuccess: () => {
      setName('');
      setMode('prepaid');
      void queryClient.invalidateQueries({ queryKey: ['admin', 'billing-groups'] });
      void queryClient.invalidateQueries({ queryKey: ['billing-groups', 'active'] });
      toast.success('计费分组已创建');
    },
    onError: (error) => toast.error(error instanceof Error ? error.message : '创建计费分组失败'),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/admin/billing-groups/${id}`, { method: 'DELETE' }),
    onSuccess: () => {
      setDeleteTarget(null);
      void queryClient.invalidateQueries({ queryKey: ['admin', 'billing-groups'] });
      void queryClient.invalidateQueries({ queryKey: ['billing-groups', 'active'] });
      toast.success('计费分组已删除');
    },
    onError: (error) => toast.error(error instanceof Error ? error.message : '删除计费分组失败'),
  });
  const toggle = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) => api(`/admin/billing-groups/${id}`, {
      method: 'PATCH',
      body: { status },
    }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'billing-groups'] });
      void queryClient.invalidateQueries({ queryKey: ['billing-groups', 'active'] });
    },
    onError: (error) => toast.error(error instanceof Error ? error.message : '更新计费分组失败'),
  });

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title="计费分组"
        description="为 API Key 配置预付费或后付费模式。预付费实时扣余额；后付费只记账并形成应收。"
        actions={<Button variant="outline" size="sm" onClick={() => void groups.refetch()} disabled={groups.isFetching}><RefreshCw className={`mr-1 size-4 ${groups.isFetching ? 'animate-spin' : ''}`} />刷新</Button>}
      />
      <Card>
        <CardContent className="space-y-4 p-5">
          <div className="flex items-center gap-2 font-semibold"><Plus className="size-4" />创建计费分组</div>
          <div className="grid gap-4 md:grid-cols-[1fr_240px_auto] md:items-end">
            <div className="space-y-2"><Label htmlFor="billing-group-name">分组名称</Label><Input id="billing-group-name" value={name} onChange={(event) => setName(event.target.value)} placeholder="例如：生产环境预付费" /></div>
            <div className="space-y-2"><Label htmlFor="billing-group-mode">计费模式</Label><select id="billing-group-mode" className="h-10 w-full rounded-md border bg-background px-3 text-sm" value={mode} onChange={(event) => setMode(event.target.value as PaymentMode)}><option value="prepaid">预付费（余额实时扣费）</option><option value="postpaid">后付费（只记账/形成应收）</option></select></div>
            <Button onClick={() => create.mutate()} disabled={!name.trim() || create.isPending}><Plus className="mr-1 size-4" />创建</Button>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardContent className="p-0">
          {groups.isLoading ? <div className="p-10 text-center text-sm text-muted-foreground">正在加载计费分组…</div> : groups.isError ? <div className="p-10 text-center text-sm text-destructive">计费分组加载失败</div> : groups.data?.length ? <div className="overflow-x-auto"><table className="w-full min-w-[900px] text-sm"><thead><tr className="border-b text-left text-xs text-muted-foreground"><th className="px-5 py-3">分组名称</th><th className="px-5 py-3">计费模式</th><th className="px-5 py-3">状态</th><th className="px-5 py-3">默认分组</th><th className="px-5 py-3">创建时间</th><th className="px-5 py-3 text-right">操作</th></tr></thead><tbody>{groups.data.map((group) => <tr key={group.id} className="border-b last:border-0"><td className="px-5 py-4"><div className="font-medium">{group.name}</div><div className="mt-1 font-mono text-[11px] text-muted-foreground">{group.id}</div></td><td className="px-5 py-4"><span className={`rounded-full px-2 py-1 text-xs ${group.payment_mode === 'postpaid' ? 'bg-amber-100 text-amber-700' : 'bg-emerald-100 text-emerald-700'}`}>{modeLabel(group.payment_mode)}</span><div className="mt-1 text-xs text-muted-foreground">{group.payment_mode === 'postpaid' ? '记录 Token 与理论成本，形成应收，不预扣钱包' : '资源包优先，余额实时结算'}</div></td><td className="px-5 py-4">{group.deleted_at ? '已删除' : statusLabel(group.status)}</td><td className="px-5 py-4">{group.is_default ? '是' : '否'}</td><td className="px-5 py-4 text-xs text-muted-foreground">{new Date(group.created_at).toLocaleString('zh-CN')}</td><td className="px-5 py-4 text-right"><div className="flex justify-end gap-2">{!group.is_default && !group.deleted_at && <Button variant="outline" size="sm" onClick={() => toggle.mutate({ id: group.id, status: group.status === 'active' ? 'inactive' : 'active' })} disabled={toggle.isPending || remove.isPending}><Power className="mr-1 size-3.5" />{group.status === 'active' ? '停用' : '启用'}</Button>}{!group.is_default && !group.deleted_at && <Button variant="destructive" size="sm" onClick={() => setDeleteTarget(group)} disabled={remove.isPending}><Trash2 className="mr-1 size-3.5" />删除</Button>}</div></td></tr>)}</tbody></table></div> : <div className="p-10 text-center text-sm text-muted-foreground">暂无计费分组</div>}
        </CardContent>
      </Card>
      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => { if (!open && !remove.isPending) setDeleteTarget(null); }}
        title="删除计费分组？"
        description={deleteTarget ? `“${deleteTarget.name}”删除后不可恢复。历史账单会保留；仍被 API Key 使用或存在进行中请求时，删除会被拒绝。` : ''}
        onConfirm={async () => { if (deleteTarget) await remove.mutateAsync(deleteTarget.id); }}
        isPending={remove.isPending}
      />
    </div>
  );
}
