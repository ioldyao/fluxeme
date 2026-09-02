import { useEffect, useState } from 'react';
import { Globe, KeyRound, Loader2, Network, Plus, RefreshCw, Route as RouteIcon, Trash2, X } from 'lucide-react';
import { toast } from 'sonner';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@fluxeme/shared/src/components/ui/card';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import { Guard } from '@fluxeme/shared/src/permissions';
import type { GatewayRoute } from '@fluxeme/shared/src/types';
import {
  useCreateGatewayRoute, useDeleteGatewayRoute, useGatewayRoutes, useUpdateGatewayRoute,
  type GatewayRouteInput,
} from '@fluxeme/shared/src/api/gateway';

const DEFAULT_METHODS = 'GET,POST,PUT,PATCH,DELETE';
const makeEmptyForm = (): GatewayRouteInput => ({
  name: '', path_prefix: '', upstream_url: '', methods: DEFAULT_METHODS,
  timeout_ms: 30000, enabled: true, preserve_query: true, strip_prefix: false, upstream_headers: {},
});

function routeInput(route: GatewayRoute): GatewayRouteInput { return { ...route, upstream_headers: {} }; }

const PROTECTED_HEADERS = ['host', 'content-length', 'connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade'];

function isProtectedHeader(name: string): boolean {
  return PROTECTED_HEADERS.includes(name.trim().toLowerCase());
}

export default function GatewayRoutes() {
  const routes = useGatewayRoutes();
  const create = useCreateGatewayRoute();
  const update = useUpdateGatewayRoute();
  const remove = useDeleteGatewayRoute();
  const [form, setForm] = useState<GatewayRouteInput>(makeEmptyForm);
  const [editing, setEditing] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [headerText, setHeaderText] = useState('');
  const [newHeaderName, setNewHeaderName] = useState('');
  const [newHeaderValue, setNewHeaderValue] = useState('');

  useEffect(() => {
    if (create.isSuccess || update.isSuccess) { setForm(makeEmptyForm()); setHeaderText(''); setEditing(null); setDialogOpen(false); }
  }, [create.isSuccess, update.isSuccess]);

  const set = <K extends keyof GatewayRouteInput>(key: K, value: GatewayRouteInput[K]) => setForm((p) => ({ ...p, [key]: value }));
  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const upstream_headers: Record<string, string> = {};
    for (const line of headerText.split('\n')) { const i = line.indexOf(':'); if (i > 0) upstream_headers[line.slice(0, i).trim()] = line.slice(i + 1).trim(); }
    const input = { ...form, path_prefix: form.path_prefix.trim(), upstream_url: form.upstream_url.trim(), upstream_headers };
    try { if (editing) await update.mutateAsync({ id: editing, input }); else await create.mutateAsync(input); toast.success(editing ? 'API 网关路由已更新' : 'API 网关路由已创建'); }
    catch (error) { toast.error(error instanceof Error ? error.message : '保存网关路由失败'); }
  };
  const beginEdit = (route: GatewayRoute) => { setEditing(route.id); setForm(routeInput(route)); setHeaderText(''); setDialogOpen(true); };
  const beginCreate = () => { setEditing(null); setForm(makeEmptyForm()); setHeaderText(''); setDialogOpen(true); };
  const closeDialog = () => { if (create.isPending || update.isPending) return; setDialogOpen(false); setEditing(null); setForm(makeEmptyForm()); setHeaderText(''); };
  const deleteRoute = async (id: string) => { if (!window.confirm('确定删除这个 API 网关路由吗？')) return; try { await remove.mutateAsync(id); toast.success('API 网关路由已删除'); } catch (error) { toast.error(error instanceof Error ? error.message : '删除网关路由失败'); } };

  const routeCount = routes.data?.length ?? 0;
  const activeCount = routes.data?.filter((route) => route.enabled).length ?? 0;
  const methodOptions = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
  const selectedMethods = form.methods.split(',').map((m) => m.trim()).filter(Boolean);
  const toggleMethod = (m: string) => {
    const next = selectedMethods.includes(m) ? selectedMethods.filter((x) => x !== m) : [...selectedMethods, m];
    set('methods', methodOptions.filter((m) => next.includes(m)).join(','));
  };
  const headerLines = headerText.split('\n').map((line) => line.trim()).filter(Boolean);
  const parsedHeaders = headerLines.filter((line) => line.includes(':') && line.indexOf(':') > 0);
  const validHeaders = parsedHeaders.filter((line) => {
    const name = line.slice(0, line.indexOf(':')).trim();
    return name.length > 0 && !isProtectedHeader(name);
  });
  const previewPath = form.path_prefix.trim() || '/your-prefix';
  const previewUpstream = form.upstream_url.trim().replace(/\/+$/, '') || 'https://upstream.example.com';
  const previewSuffix = form.strip_prefix ? '/jobs' : `${previewPath}/jobs`;
  const saving = create.isPending || update.isPending;
  const addHeaderLine = () => {
    const name = newHeaderName.trim();
    if (!name) return;
    if (isProtectedHeader(name)) { toast.error(`「${name}」是受保护的请求头，不能注入`); return; }
    setHeaderText((prev) => (prev ? `${prev}\n${name}: ${newHeaderValue}` : `${name}: ${newHeaderValue}`));
    setNewHeaderName('');
    setNewHeaderValue('');
  };

  return <Guard perm="admin:gateway"><div className="min-h-full space-y-6 animate-fade-in">
    <PageHeader
      title="API网关"
      description="统一代理外部 API，通过 Fluxeme API Key 的 gateway 权限安全访问。"
      actions={<div className="flex items-center gap-2"><Button variant="outline" size="sm" onClick={() => void routes.refetch()} disabled={routes.isFetching}><RefreshCw className={`mr-1 size-4 ${routes.isFetching ? 'animate-spin' : ''}`} />刷新</Button><Button size="sm" onClick={beginCreate}><Plus className="mr-1 size-4" />新增网关路由</Button></div>}
    />

    <div className="grid gap-3 sm:grid-cols-3">
      <div className="rounded-xl border border-border/70 bg-card p-4 shadow-sm"><p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">已配置路由</p><p className="mt-2 text-2xl font-semibold tracking-tight">{routeCount}</p><p className="mt-1 text-xs text-muted-foreground">对外提供的代理入口</p></div>
      <div className="rounded-xl border border-border/70 bg-card p-4 shadow-sm"><p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">运行中</p><p className="mt-2 text-2xl font-semibold tracking-tight text-emerald-600">{activeCount}</p><p className="mt-1 text-xs text-muted-foreground">正在接受请求</p></div>
      <div className="rounded-xl border border-border/70 bg-gradient-to-br from-brand/10 via-card to-card p-4 shadow-sm"><p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">调用方式</p><p className="mt-2 font-mono text-lg font-semibold tracking-tight">/apigw/…</p><p className="mt-1 text-xs text-muted-foreground">使用 gateway scope 的 API Key</p></div>
    </div>

    <Card className="overflow-hidden border-border/70 shadow-sm"><CardHeader className="border-b border-border/60 bg-muted/20 px-5 py-4"><div className="flex items-center justify-between gap-4"><div><CardTitle className="text-base">代理路由</CardTitle><p className="mt-1 text-xs text-muted-foreground">将公开路径映射到受控的上游 API，凭据仅在服务端注入。</p></div><span className="rounded-full bg-muted px-2.5 py-1 text-xs text-muted-foreground">{routeCount} routes</span></div></CardHeader><CardContent className="p-0">{routes.isLoading ? <div className="p-14 text-center text-sm text-muted-foreground">正在加载网关路由…</div> : routes.isError ? <div className="p-14 text-center text-sm text-destructive">网关路由加载失败</div> : routes.data?.length ? <div className="overflow-x-auto"><table className="w-full min-w-[850px] text-sm"><thead><tr className="border-b bg-muted/10 text-left text-xs text-muted-foreground"><th className="px-5 py-3 font-medium">路由</th><th className="px-5 py-3 font-medium">上游服务</th><th className="px-5 py-3 font-medium">允许方法</th><th className="px-5 py-3 font-medium">状态</th><th className="px-5 py-3 text-right font-medium">操作</th></tr></thead><tbody>{routes.data.map((route) => <tr key={route.id} className="border-b border-border/60 transition-colors last:border-0 hover:bg-muted/20"><td className="px-5 py-4"><div className="font-medium">{route.name || '未命名路由'}</div><code className="mt-1 inline-block rounded bg-brand/10 px-1.5 py-0.5 text-xs text-brand">/apigw{route.path_prefix}</code></td><td className="max-w-[280px] truncate px-5 py-4 font-mono text-xs text-muted-foreground">{route.upstream_url}</td><td className="px-5 py-4 text-xs text-muted-foreground">{route.methods}</td><td className="px-5 py-4"><span className={`inline-flex items-center gap-1.5 text-xs font-medium ${route.enabled ? 'text-emerald-600' : 'text-muted-foreground'}`}><span className={`size-1.5 rounded-full ${route.enabled ? 'bg-emerald-500' : 'bg-muted-foreground/50'}`} />{route.enabled ? '启用' : '停用'}</span>{route.upstream_headers.length > 0 && <div className="mt-1 text-xs text-muted-foreground">{route.upstream_headers.length} 个注入头</div>}</td><td className="px-5 py-4 text-right"><div className="flex justify-end gap-2"><Button variant="outline" size="sm" onClick={() => beginEdit(route)}>编辑</Button><Button variant="ghost" size="sm" className="text-destructive hover:bg-destructive/10 hover:text-destructive" onClick={() => void deleteRoute(route.id)} disabled={remove.isPending} aria-label={`删除 ${route.name || route.path_prefix}`}><Trash2 className="size-3.5" /></Button></div></td></tr>)}</tbody></table></div> : <div className="flex flex-col items-center justify-center px-6 py-20 text-center"><div className="flex size-12 items-center justify-center rounded-2xl bg-brand/10 text-brand"><Network className="size-6" /></div><h3 className="mt-4 font-medium">还没有网关路由</h3><p className="mt-1 max-w-sm text-sm text-muted-foreground">创建第一条路由，把外部 API 接入 Fluxeme 的统一访问入口。</p><Button className="mt-5" size="sm" onClick={beginCreate}><Plus className="mr-1 size-4" />新增网关路由</Button></div>}</CardContent></Card>

    <Dialog open={dialogOpen} onOpenChange={(open) => { if (open) setDialogOpen(true); else closeDialog(); }}>
      <DialogContent className="max-h-[92vh] overflow-y-auto p-0 sm:max-w-2xl">
        <div className="border-b border-border/60 px-6 pb-5 pt-6">
          <div className="flex items-center gap-3">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-brand/10 text-brand"><RouteIcon className="size-5" /></div>
            <div className="min-w-0 flex-1">
              <DialogTitle className="text-base font-semibold">{editing ? '编辑网关路由' : '新增网关路由'}</DialogTitle>
              <DialogDescription className="mt-0.5 text-xs">{editing ? '已保存的上游凭据不会回显，留空即可保留。' : '配置一个受控的上游 API 代理入口。'}</DialogDescription>
            </div>
          </div>
        </div>
        <form id="gateway-route-form" onSubmit={submit} className="px-6 py-5">
          <section className="space-y-4">
            <h4 className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-widest text-muted-foreground"><Globe className="size-3.5" />基本信息</h4>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-1.5"><Label htmlFor="gateway-name" className="text-xs">名称</Label><Input id="gateway-name" value={form.name} onChange={(e) => set('name', e.target.value)} placeholder="例如：天气 API" className="h-9" /></div>
              <div className="space-y-1.5"><Label htmlFor="gateway-prefix" className="text-xs">路径前缀</Label><div className="flex h-9 items-center rounded-md border border-input bg-muted/40 px-2.5 text-xs text-muted-foreground"><span className="font-mono">/apigw</span><input id="gateway-prefix" required value={form.path_prefix} onChange={(e) => set('path_prefix', e.target.value)} placeholder="/api" className="h-full w-full bg-transparent pl-1 font-mono text-sm text-foreground outline-none" /></div><p className="text-[11px] text-muted-foreground">前缀是上游路径的一部分，默认原样转发。仅入口前缀 /apigw 会被剥离。</p></div>
            </div>
            <div className="space-y-1.5"><Label htmlFor="gateway-upstream" className="text-xs">上游 URL</Label><Input id="gateway-upstream" required type="url" value={form.upstream_url} onChange={(e) => set('upstream_url', e.target.value)} placeholder="https://api.example.com" className="h-9 font-mono text-sm" /><p className="text-[11px] text-muted-foreground">仅允许 HTTP(S)；私网与回环地址会被 SSRF 策略拦截。</p></div>
          </section>

          <section className="mt-6 space-y-4">
            <h4 className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-widest text-muted-foreground"><Network className="size-3.5" />转发规则</h4>
            <div className="space-y-1.5"><Label className="text-xs">允许方法</Label>
              <div className="flex flex-wrap gap-1.5" role="group" aria-label="允许方法">
                {methodOptions.map((m) => <button key={m} type="button" aria-pressed={selectedMethods.includes(m)} onClick={() => toggleMethod(m)} className={`h-7 rounded-md border px-2.5 font-mono text-[11px] font-medium transition-colors ${selectedMethods.includes(m) ? 'border-brand/40 bg-brand/10 text-brand' : 'border-border/70 bg-background text-muted-foreground hover:border-brand/30 hover:text-foreground'}`}>{m}</button>)}
              </div>
            </div>
            <div className="space-y-1.5"><Label htmlFor="gateway-timeout" className="text-xs">超时（毫秒）</Label><Input id="gateway-timeout" type="number" min={1} max={60000} required value={form.timeout_ms} onChange={(e) => set('timeout_ms', Number(e.target.value))} className="h-9" /></div>
            <div className="grid gap-2 rounded-xl border border-border/70 bg-muted/20 p-1.5 sm:grid-cols-3">
              {([['enabled', '启用路由', '启用后立即生效'], ['preserve_query', '透传 Query', '转发查询参数'], ['strip_prefix', '剥掉路径前缀', '通常关闭：/api 是上游路径，应原样转发']] as const).map(([key, label, hint]) => (
                <label key={key} className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 transition-colors hover:bg-background/60"><Switch aria-label={label} checked={form[key]} onCheckedChange={(v) => set(key, v)} /><span className="leading-tight"><span className="block text-xs font-medium">{label}</span><span className="block text-[10px] text-muted-foreground">{hint}</span></span></label>
              ))}
            </div>
          </section>

          <section className="mt-6 space-y-2">
            <h4 className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-widest text-muted-foreground"><KeyRound className="size-3.5" />上游请求头</h4>
            <p className="text-[11px] text-muted-foreground">值加密保存在服务端并注入上游请求，不会回显。</p>
            <div className="rounded-xl border border-border/70">
              {headerLines.length === 0 && <div className="border-b border-border/60 bg-muted/20 px-3 py-2 text-[11px] text-muted-foreground">暂无注入头{editing ? '（留空保留已保存的凭据）' : ''}</div>}
              {headerLines.map((line, idx) => {
                const name = line.slice(0, line.indexOf(':')).trim();
                const value = line.slice(line.indexOf(':') + 1).trim();
                const invalid = !line.includes(':') || line.indexOf(':') === 0 || isProtectedHeader(name);
                return <div key={idx} className={`flex items-center gap-2 border-b border-border/60 px-3 py-2 last:border-0 ${invalid ? 'bg-destructive/5' : ''}`}>
                  <code className={`w-32 shrink-0 truncate font-mono text-[11px] ${invalid ? 'text-destructive' : 'text-foreground'}`}>{name || '—'}</code>
                  <code className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">{invalid && name ? '受保护的头，无法注入' : value ? '••••••••' : '（空值）'}</code>
                  <button type="button" aria-label={`移除 ${name || `第 ${idx + 1} 行`}`} onClick={() => setHeaderText(headerLines.filter((_, i) => i !== idx).join('\n'))} className="shrink-0 rounded p-1 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"><X className="size-3.5" /></button>
                </div>;
              })}
            </div>
            <div className="flex gap-2">
              <Input placeholder="Header 名称，如 x-api-key" aria-label="新增 Header 名称" className="h-9 flex-1 font-mono text-xs" value={newHeaderName} onChange={(e) => setNewHeaderName(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addHeaderLine(); } }} />
              <Input placeholder="值（服务端加密）" aria-label="新增 Header 值" className="h-9 flex-1" type="password" value={newHeaderValue} onChange={(e) => setNewHeaderValue(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addHeaderLine(); } }} />
              <Button type="button" variant="outline" size="sm" className="h-9 shrink-0" onClick={addHeaderLine} disabled={!newHeaderName.trim()}><Plus className="mr-1 size-3.5" />添加</Button>
            </div>
          </section>

          <section className="mt-6 rounded-xl border border-brand/20 bg-brand/5 p-4">
            <p className="text-[11px] font-semibold uppercase tracking-widest text-brand">请求预览</p>
            <div className="mt-2 flex flex-wrap items-center gap-2 font-mono text-xs">
              <span className="rounded bg-brand/15 px-1.5 py-0.5 font-semibold text-brand">{selectedMethods[0] ?? 'GET'}</span>
              <code className="text-foreground">{previewUpstream}{previewSuffix}</code>
            </div>
            <div className="mt-2 flex items-center gap-1.5 font-mono text-[10px] text-muted-foreground">
              <code>/apigw{previewPath}/current</code><span className="not-italic">→</span><code className="text-brand">{previewUpstream}{previewSuffix}</code>
            </div>
          </section>
        </form>
        <div className="flex items-center justify-between gap-3 border-t border-border/60 bg-muted/20 px-6 py-4">
          <p className="text-[11px] text-muted-foreground">{headerLines.length > 0 ? `${validHeaders.length}/${headerLines.length} 个有效注入头` : '未配置注入头'}</p>
          <div className="flex gap-2"><Button type="button" variant="outline" size="sm" onClick={closeDialog} disabled={saving}>取消</Button><Button type="submit" form="gateway-route-form" size="sm" disabled={saving}>{saving && <Loader2 className="mr-1 size-3.5 animate-spin" />}{editing ? '保存修改' : '创建路由'}</Button></div>
        </div>
      </DialogContent>
    </Dialog>
  </div></Guard>;
}