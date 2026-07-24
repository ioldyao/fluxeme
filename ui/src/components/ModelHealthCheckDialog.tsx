import { useEffect, useMemo, useState } from 'react';
import { Activity, CheckCircle2, Loader2, Search, XCircle } from 'lucide-react';
import { toast } from 'sonner';
import { useModelHealthCheck } from '@/api/models';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Switch } from '@/components/ui/switch';
import type { Model, ModelHealthCheckResult, Endpoint } from '@/types';

interface Props {
  model: Model | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  channelName: (id: string) => string;
  channelEndpoints: (id: string) => Endpoint[];
}

export function ModelHealthCheckDialog({ model, open, onOpenChange, channelName, channelEndpoints }: Props) {
  const healthCheck = useModelHealthCheck();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [stream, setStream] = useState(false);
  const [filter, setFilter] = useState('');
  const [result, setResult] = useState<ModelHealthCheckResult | null>(null);

  useEffect(() => {
    if (!open || !model) return;
    setSelected(new Set(model.channels.map((binding) => binding.channel_id)));
    setStream(false);
    setFilter('');
    setResult(null);
    healthCheck.reset();
  // healthCheck is intentionally excluded: mutation state changes must not reset this dialog.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, model]);

  const bindings = useMemo(() => {
    const query = filter.trim().toLowerCase();
    return (model?.channels ?? []).filter((binding) =>
      !query
      || binding.channel_id.toLowerCase().includes(query)
      || channelName(binding.channel_id).toLowerCase().includes(query)
      || (binding.upstream_model ?? '').toLowerCase().includes(query));
  }, [model, filter, channelName]);

  const toggle = (channelId: string) => setSelected((current) => {
    const next = new Set(current);
    if (next.has(channelId)) next.delete(channelId);
    else next.add(channelId);
    return next;
  });

  const run = async (channelIds = [...selected]) => {
    if (!model || channelIds.length === 0) return;
    try {
      const response = await healthCheck.mutateAsync({ modelId: model.id, channelIds, stream });
      setResult(response);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : '健康检测失败');
    }
  };

  const resultFor = (channelId: string) =>
    result?.channel_results.find((item) => item.channel_id === channelId);

  return (
    <Dialog open={open} onOpenChange={(next) => !healthCheck.isPending && onOpenChange(next)}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>测试模型连接：{model?.name}</DialogTitle>
          <p className="text-sm text-muted-foreground">选择要检测的渠道，确认后才会向上游发送最小化测试请求。</p>
        </DialogHeader>

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-1.5">
            <div className="text-sm font-medium">测试模型</div>
            <div className="h-9 rounded-md border bg-muted/30 px-3 flex items-center text-sm">
              {model?.model_pattern}
            </div>
          </div>
          <div className="space-y-1.5">
            <div className="text-sm font-medium">流式模式</div>
            <div className="h-9 flex items-center gap-2">
              <Switch checked={stream} onCheckedChange={setStream} disabled={healthCheck.isPending} />
              <span className="text-sm">{stream ? '已启用' : '已禁用'}</span>
            </div>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="font-medium">绑定渠道</div>
            <div className="text-xs text-muted-foreground mt-1">可以批量检测，也可以只检测单个渠道。</div>
          </div>
          <div className="relative w-64 max-w-full">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
            <input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="筛选渠道…"
              className="h-9 w-full rounded-md border bg-background pl-9 pr-3 text-sm outline-none focus:ring-1 focus:ring-ring" />
          </div>
        </div>

        <div className="border rounded-lg overflow-hidden">
          <div className="grid grid-cols-[40px_minmax(180px,1fr)_minmax(160px,1fr)_120px_90px] items-center border-b bg-muted/40 px-3 py-2 text-xs font-semibold text-muted-foreground">
            <Checkbox
              checked={!!model?.channels.length && selected.size === model.channels.length}
              onCheckedChange={() => setSelected(selected.size === model?.channels.length ? new Set() : new Set(model?.channels.map((item) => item.channel_id)))}
              disabled={healthCheck.isPending}
            />
            <span>渠道 / 端点</span><span>上游模型</span><span>结果</span><span className="text-right">操作</span>
          </div>
          <div className="max-h-80 overflow-y-auto divide-y">
            {bindings.length === 0 ? (
              <div className="py-10 text-center text-sm text-muted-foreground">没有可检测的绑定渠道</div>
            ) : bindings.map((binding) => {
              const item = resultFor(binding.channel_id);
              const endpoints = channelEndpoints(binding.channel_id);
              return (
                <div key={binding.channel_id} className="border-b last:border-b-0">
                <div className="grid grid-cols-[40px_minmax(180px,1fr)_minmax(160px,1fr)_120px_90px] items-center px-3 py-3 text-sm">
                  <Checkbox checked={selected.has(binding.channel_id)} onCheckedChange={() => toggle(binding.channel_id)} disabled={healthCheck.isPending} />
                  <div className="min-w-0"><div className="font-medium truncate">{channelName(binding.channel_id)}</div><div className="text-xs text-muted-foreground truncate">{item?.endpoint_url || binding.channel_id}</div></div>
                  <span className="truncate text-muted-foreground">{binding.upstream_model || model?.name}</span>
                  <div>{!item ? <span className="text-muted-foreground">未测试</span> : item.success
                    ? <span className="inline-flex items-center gap-1 text-green-600"><CheckCircle2 className="size-4" />{item.latency_ms}ms</span>
                    : <span className="inline-flex items-center gap-1 text-destructive" title={item.error ?? undefined}><XCircle className="size-4" />失败</span>}</div>
                  <div className="text-right"><Button variant="ghost" size="sm" title="仅检测此渠道" disabled={healthCheck.isPending} onClick={() => run([binding.channel_id])}><Activity className="size-4" /></Button></div>
                </div>
                {endpoints.map((endpoint, index) => <div key={endpoint.id ?? `${endpoint.url}-${index}`} className="grid grid-cols-[40px_minmax(180px,1fr)_minmax(160px,1fr)_120px_90px] items-center bg-muted/20 px-3 py-2 text-xs text-muted-foreground"><span /><span className="pl-4">↳ 端点{index + 1}<span className="block truncate">{endpoint.url}</span></span><span /><span>未单独测试</span><span /></div>)}
                </div>
              );
            })}
          </div>
        </div>

        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={healthCheck.isPending}>关闭</Button>
          <Button onClick={() => run()} disabled={selected.size === 0 || healthCheck.isPending}>
            {healthCheck.isPending && <Loader2 className="size-4 mr-1 animate-spin" />}
            检测选中渠道（{selected.size}）
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
