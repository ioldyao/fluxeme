import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import {
  useAdminBillingActivities,
  useAdminBillingMonths,
  type BillingActivity,
  type BillingActivityDimensionRow,
} from '@fluxeme/shared/src/api/billing';

const PAGE_SIZE = 50;
type DetailTab = 'activities' | 'keys' | 'models' | 'sources';
type Props = { userId: string; onBack: () => void };

function numberValue(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value ?? 0);
  return Number.isFinite(parsed) ? parsed : 0;
}
function formatMoney(value: unknown): string {
  const amount = numberValue(value);
  return amount === 0 ? '¥0.00' : `¥${amount.toFixed(6)}`;
}
function formatCompact(value: unknown): string {
  const amount = numberValue(value);
  if (amount >= 1_000_000_000) return `${(amount / 1_000_000_000).toFixed(2)}B`;
  if (amount >= 1_000_000) return `${(amount / 1_000_000).toFixed(2)}M`;
  if (amount >= 1_000) return `${(amount / 1_000).toFixed(1)}K`;
  return Math.round(amount).toLocaleString('en-US');
}
function sourceLabel(source: string): string {
  return { package: '资源包', wallet: '钱包', package_and_wallet: '资源包 + 钱包', prepaid: '预付费', prepaid_package: '预付费 + 资源包', free_model: '免费模型', none: '无扣费', zero_cost: '零金额', unknown: '待确认' }[source] ?? source;
}
function sourceClass(source: string): string {
  if (source === 'package') return 'bg-violet-100 text-violet-700';
  if (source === 'wallet') return 'bg-blue-100 text-blue-700';
  if (source === 'package_and_wallet') return 'bg-amber-100 text-amber-700';
  if (source === 'free_model') return 'bg-emerald-100 text-emerald-700';
  return 'bg-slate-100 text-slate-600';
}
function statusLabel(activity: BillingActivity): string {
  if (activity.status_code === 499 || activity.activity_status === 'interrupted') return '客户端中断';
  if (!activity.success || activity.activity_status === 'failed') return '失败';
  if (activity.activity_status === 'zero_cost') return '零金额活动';
  return '成功';
}
function statusClass(activity: BillingActivity): string {
  const status = statusLabel(activity);
  if (status === '成功') return 'bg-emerald-100 text-emerald-700';
  if (status === '失败') return 'bg-red-100 text-red-700';
  return 'bg-amber-100 text-amber-700';
}

function ActivityRow({ activity, onClick }: { activity: BillingActivity; onClick: () => void }) {
  return (
    <tr className="cursor-pointer border-b border-border/70 hover:bg-accent/30" role="button" tabIndex={0} aria-label={`查看请求 ${activity.request_id} 的活动详情`} onClick={onClick} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onClick(); } }}>
      <td className="whitespace-nowrap px-3 py-3 text-xs text-muted-foreground">{new Date(activity.timestamp).toLocaleString('zh-CN')}</td>
      <td className="whitespace-nowrap px-3 py-3 font-mono text-xs">{activity.request_id}</td>
      <td className="whitespace-nowrap px-3 py-3 text-xs font-semibold">{activity.api_key_name ?? '未命名 Key'}</td>
      <td className="whitespace-nowrap px-3 py-3 text-xs font-semibold">{activity.model}</td>
      <td className="whitespace-nowrap px-3 py-3"><span className={`inline-flex rounded-full px-2 py-1 text-[11px] font-bold ${statusClass(activity)}`}>{statusLabel(activity)} · {activity.status_code}</span></td>
      <td className="whitespace-nowrap px-3 py-3 text-right text-xs tabular-nums">{formatCompact(activity.total_tokens)}</td>
      <td className="whitespace-nowrap px-3 py-3"><div className={`inline-flex rounded-full px-2 py-1 text-[11px] font-bold ${activity.billing_payment_mode === 'prepaid' ? 'bg-amber-100 text-amber-700' : 'bg-blue-100 text-blue-700'}`}>{activity.billing_payment_mode === 'prepaid' ? '预付费' : '按量计费'}</div><div className="mt-1 text-[10px] text-muted-foreground">{activity.billing_group_name ?? '默认分组'}</div></td>
      <td className={`whitespace-nowrap px-3 py-3 text-right text-xs tabular-nums ${activity.package_units ? 'font-bold text-violet-700' : 'text-muted-foreground'}`}>{formatCompact(activity.package_units)}</td>
      <td className={`whitespace-nowrap px-3 py-3 text-right text-xs tabular-nums ${activity.wallet_amount ? 'font-bold' : 'text-muted-foreground'}`}>{formatMoney(activity.wallet_amount)}</td>
      <td className={`whitespace-nowrap px-3 py-3 text-right text-xs tabular-nums ${activity.priced_cost_amount ? 'font-bold' : 'text-muted-foreground'}`}>{formatMoney(activity.priced_cost_amount)}</td>
    </tr>
  );
}

function DimensionTable({ tab, rows }: { tab: Exclude<DetailTab, 'activities'>; rows: BillingActivityDimensionRow[] }) {
  const title = tab === 'keys' ? 'API Key' : tab === 'models' ? '模型' : '结算来源';
  const relatedTitle = tab === 'keys' ? '模型分布' : tab === 'models' ? 'API Key 分布' : '示例 API Key';
  const secondaryTitle = tab === 'sources' ? '示例模型' : '结算来源分布';
  return (
    <div className="overflow-auto">
      <table className="w-full min-w-[1100px] border-collapse">
        <thead><tr className="bg-muted text-[11px] text-muted-foreground"><th className="px-3 py-3 text-left">{title}</th><th className="px-3 py-3 text-right">活动数</th><th className="px-3 py-3 text-right">涉及 Key 数</th><th className="px-3 py-3 text-right">涉及模型数</th><th className="px-3 py-3 text-left">{relatedTitle}</th><th className="px-3 py-3 text-left">{secondaryTitle}</th><th className="px-3 py-3 text-right">Token</th><th className="px-3 py-3 text-right">资源包 units</th><th className="px-3 py-3 text-right">钱包实扣</th><th className="px-3 py-3 text-right">理论成本</th></tr></thead>
        <tbody>{rows.map((row) => <tr key={row.name} className="border-b border-border/70"><td className="px-3 py-3 text-xs font-bold">{tab === 'sources' ? <span className={`inline-flex rounded-full px-2 py-1 text-[11px] ${sourceClass(row.name)}`}>{sourceLabel(row.name)}</span> : row.name}</td><td className="px-3 py-3 text-right text-xs tabular-nums">{formatCompact(row.activity_count)}</td><td className="px-3 py-3 text-right text-xs tabular-nums">{formatCompact(row.key_count)}</td><td className="px-3 py-3 text-right text-xs tabular-nums">{formatCompact(row.model_count)}</td><td className="max-w-[270px] px-3 py-3 text-xs text-muted-foreground">{(row.related_names ?? []).join(', ') || '—'}</td><td className="max-w-[220px] px-3 py-3 text-xs text-muted-foreground">{(row.source_names ?? []).map((name) => secondaryTitle === '结算来源分布' ? sourceLabel(name) : name).join(', ') || '—'}</td><td className="px-3 py-3 text-right text-xs tabular-nums">{formatCompact(row.total_tokens)}</td><td className="px-3 py-3 text-right text-xs font-bold tabular-nums text-violet-700">{formatCompact(row.package_units)}</td><td className="px-3 py-3 text-right text-xs font-bold tabular-nums">{formatMoney(row.wallet_amount)}</td><td className="px-3 py-3 text-right text-xs font-bold tabular-nums">{formatMoney(row.priced_cost_amount)}</td></tr>)}</tbody>
      </table>
      {rows.length === 0 && <div className="px-4 py-12 text-center text-sm text-muted-foreground">本期暂无维度活动</div>}
    </div>
  );
}

export default function BillingUserActivityDetail({ userId, onBack }: Props) {
  const [searchParams, setSearchParams] = useSearchParams();
  const year = Number(searchParams.get('year')) || new Date().getFullYear();
  const month = Number(searchParams.get('month')) || new Date().getMonth() + 1;
  const [tab, setTab] = useState<DetailTab>('activities');
  const [query, setQuery] = useState('');
  const [keyFilter, setKeyFilter] = useState('all');
  const [modelFilter, setModelFilter] = useState('all');
  const [sourceFilter, setSourceFilter] = useState('all');
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<BillingActivity | null>(null);
  const { data, isLoading } = useAdminBillingActivities(year, month, PAGE_SIZE, (page - 1) * PAGE_SIZE, userId, {
    search: query,
    api_key_name: keyFilter,
    model: modelFilter,
    charge_source: sourceFilter,
  });
  const { data: months } = useAdminBillingMonths();
  const queryClient = useQueryClient();
  const activities = data?.activities ?? [];
  const summary = data?.summary;
  const dimensions = data?.dimensions;
  const filteredActivities = activities;
  const totalPages = Math.max(1, Math.ceil((data?.total ?? 0) / PAGE_SIZE));
  const keys = dimensions?.api_keys ?? [];
  const models = dimensions?.models ?? [];
  const sources = dimensions?.sources ?? [];
  const resetFilters = () => { setQuery(''); setKeyFilter('all'); setModelFilter('all'); setSourceFilter('all'); setPage(1); };
  const handleFilterChange = (update: () => void) => { update(); setPage(1); };
  const refresh = () => { void queryClient.invalidateQueries({ queryKey: ['admin-billing', 'activities', year, month, PAGE_SIZE, (page - 1) * PAGE_SIZE, userId] }); };

  return (
    <div className="space-y-4">
      <section className="flex items-start justify-between gap-5"><div><h1 className="text-[22px] font-bold leading-tight">用户账单活动</h1><p className="mt-2 text-[13px] text-muted-foreground">以请求活动为事实主体，分别从 API Key、模型、结算来源等维度查看和聚合，不建立任何固定绑定关系。</p><div className="mt-2 flex flex-wrap gap-2 text-xs font-semibold"><span className="rounded-full bg-emerald-100 px-2 py-1 text-emerald-700">● 正常</span><span className="rounded-full bg-muted px-2 py-1 text-muted-foreground">用户：{userId}</span><span className="rounded-full bg-accent px-2 py-1 text-accent-foreground">个人账户</span><span className="rounded-full bg-muted px-2 py-1 text-muted-foreground">{year} 年 {month} 月</span></div></div><div className="flex items-center gap-2"><select className="h-9 rounded-lg border border-border bg-card px-2 text-sm" value={`${year}-${String(month).padStart(2, '0')}`} aria-label="选择账期" onChange={(event) => { const [nextYear, nextMonth] = event.target.value.split('-'); setPage(1); setSearchParams({ user: userId, year: nextYear, month: nextMonth }); }}>{(months ?? []).map((value) => <option key={value}>{value}</option>)}</select><button type="button" className="rounded-lg border border-border bg-card px-3 py-1.5 text-sm font-semibold hover:bg-muted" onClick={refresh}>刷新</button><button type="button" className="rounded-lg border border-border bg-card px-3 py-1.5 text-sm font-semibold hover:bg-muted" onClick={onBack}>返回账单</button></div></section>
      <section className="rounded-xl border border-blue-200 bg-blue-50 px-4 py-3 text-sm text-blue-800"><strong>Request / Billing Event 是唯一活动事实。</strong><p className="mt-1 text-xs text-blue-900/75">API Key 表示谁发起，模型表示调用什么，结算来源表示这一笔最终如何结算。同一个 Key 可以调用多个模型，并在不同请求中分别使用资源包、钱包或混合结算。</p></section>
      <section className="grid grid-cols-6 gap-3 max-[1400px]:grid-cols-3 max-[850px]:grid-cols-2">{[['账单活动', formatCompact(summary?.activity_count ?? data?.total), '一请求一条 billing_event'], ['活跃 API Key', formatCompact(summary?.api_key_count), '活动事实中的调用维度'], ['使用模型', formatCompact(summary?.model_count), '活动事实中的模型维度'], ['总 Token', formatCompact(summary?.total_tokens), '包含免费与资源包活动'], ['理论成本', formatMoney(summary?.priced_cost_amount), '按每笔请求价格快照计算'], ['钱包实扣', formatMoney(summary?.wallet_amount), '资源包用完后可继续钱包结算']].map(([label, value, note]) => <div key={label} className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="text-[11px] text-muted-foreground">{label}</div><div className="mt-2 text-[22px] font-bold">{value}</div><div className="mt-1 text-[10px] text-muted-foreground">{note}</div></div>)}</section>
      <section className="grid grid-cols-4 gap-3 max-[1000px]:grid-cols-2">{[['API Key 维度', `${formatCompact(summary?.api_key_count)} 个 Key`, '同一 Key 多模型、多来源'], ['模型维度', `${formatCompact(summary?.model_count)} 个模型`, '同一模型可被不同 Key 调用'], ['结算来源维度', `${sources.length} 种来源`, '来源由每笔活动结算结果决定'], ['请求事实', `${formatCompact(summary?.activity_count ?? data?.total)} 条`, '所有维度最终回到 request_id']].map(([label, value, note]) => <div key={label} className="rounded-xl border border-border bg-card p-4"><div className="font-bold">{label}</div><div className="mt-1 text-[11px] text-muted-foreground">{note}</div><div className="mt-3 text-lg font-bold">{value}</div></div>)}</section>
      <section className="rounded-xl border border-border bg-card shadow-sm"><div className="border-b border-border/70 px-4 py-4"><div className="font-bold">账单活动与多维汇总</div><div className="mt-1 text-xs text-muted-foreground">各汇总视图只是对同一批 billing_events 的不同 GROUP BY，不代表对象之间存在绑定关系。</div></div><div className="flex gap-1 overflow-auto border-b border-border/70 px-4">{([['activities', '活动明细'], ['keys', '按 API Key'], ['models', '按模型'], ['sources', '按结算来源']] as const).map(([value, label]) => <button type="button" key={value} className={`whitespace-nowrap border-b-2 px-3 py-3 text-sm font-semibold ${tab === value ? 'border-accent-foreground text-accent-foreground' : 'border-transparent text-muted-foreground'}`} onClick={() => setTab(value)}>{label}</button>)}</div>
        {tab === 'activities' ? <><div className="flex flex-wrap items-center gap-2 border-b border-border/70 px-3 py-3"><label htmlFor="billing-activity-search" className="sr-only">搜索 Request ID、API Key 名称或模型</label><input id="billing-activity-search" className="h-9 min-w-[260px] rounded-lg border border-border bg-card px-3 text-sm outline-none" placeholder="搜索 Request ID / API Key 名称 / 模型" title="API Key 搜索匹配名称（例如 testkey），不匹配 sk- 开头的密钥值" value={query} onChange={(event) => handleFilterChange(() => setQuery(event.target.value))} /><select className="h-9 rounded-lg border border-border bg-card px-2 text-sm" value={keyFilter} onChange={(event) => handleFilterChange(() => setKeyFilter(event.target.value))}><option value="all">全部 API Key</option>{keys.map((row) => <option key={row.name}>{row.name}</option>)}</select><select className="h-9 rounded-lg border border-border bg-card px-2 text-sm" value={modelFilter} onChange={(event) => handleFilterChange(() => setModelFilter(event.target.value))}><option value="all">全部模型</option>{models.map((row) => <option key={row.name}>{row.name}</option>)}</select><select className="h-9 rounded-lg border border-border bg-card px-2 text-sm" value={sourceFilter} onChange={(event) => handleFilterChange(() => setSourceFilter(event.target.value))}><option value="all">全部结算来源</option>{sources.map((row) => <option key={row.name} value={row.name}>{sourceLabel(row.name)}</option>)}</select><button type="button" className="rounded-lg border border-border bg-card px-3 py-1.5 text-sm font-semibold" onClick={resetFilters}>重置</button></div><div className="overflow-auto"><table className="w-full min-w-[1560px] border-collapse"><thead><tr className="bg-muted text-[11px] text-muted-foreground">{['时间', 'Request ID', 'API Key', '模型', '请求结果', 'Token', '结算方式', '资源包 units', '钱包实扣', '理论成本'].map((label) => <th key={label} className={`px-3 py-3 ${label === 'Token' || label.includes('units') || label.includes('扣') || label.includes('成本') ? 'text-right' : 'text-left'}`}>{label}</th>)}</tr></thead><tbody>{filteredActivities.map((activity) => <ActivityRow key={activity.request_id} activity={activity} onClick={() => setSelected(activity)} />)}</tbody></table></div>{isLoading && <div className="px-4 py-8 text-center text-sm text-muted-foreground">正在加载账单活动…</div>}{!isLoading && filteredActivities.length === 0 && <div className="px-4 py-8 text-center text-sm text-muted-foreground">本期暂无匹配活动</div>}<div className="flex items-center justify-between border-t border-border/70 px-3 py-3 text-xs text-muted-foreground"><span>显示本页 {filteredActivities.length} 条 · 共 {data?.total ?? 0} 条活动</span><div className="flex items-center gap-2"><span>第 {page} / {totalPages} 页</span><button type="button" className="rounded border border-border px-3 py-1 disabled:opacity-40" disabled={page <= 1} onClick={() => setPage((value) => value - 1)}>上一页</button><button type="button" className="rounded border border-border px-3 py-1 disabled:opacity-40" disabled={page >= totalPages} onClick={() => setPage((value) => value + 1)}>下一页</button></div></div></> : <DimensionTable tab={tab} rows={tab === 'keys' ? keys : tab === 'models' ? models : sources} />}
      </section>
      {selected && <div className="fixed inset-0 z-50 flex justify-end bg-slate-900/40" role="presentation" onClick={() => setSelected(null)}><aside className="h-full w-[min(680px,96vw)] overflow-auto bg-card p-6 shadow-2xl" role="dialog" aria-modal="true" aria-labelledby="billing-activity-drawer-title" onClick={(event) => event.stopPropagation()}><div className="flex items-start justify-between border-b border-border pb-4"><div><div id="billing-activity-drawer-title" className="text-lg font-bold">活动事实详情</div><div className="mt-1 text-xs text-muted-foreground">一笔 Request 的全部独立维度</div></div><button type="button" aria-label="关闭活动详情" className="rounded-lg bg-muted px-3 py-1" onClick={() => setSelected(null)}>×</button></div><div className="mt-4 space-y-2 text-sm">{[['Request ID', selected.request_id], ['时间', new Date(selected.timestamp).toLocaleString('zh-CN')], ['API Key', selected.api_key_name ?? '未命名 Key'], ['模型', selected.model], ['请求结果', `${statusLabel(selected)} · ${selected.status_code}`], ['计费模式', selected.billing_payment_mode === 'prepaid' ? '预付费' : '按量计费'], ['计费分组', selected.billing_group_name ?? '默认分组'], ['结算来源', sourceLabel(selected.charge_source)]].map(([label, value]) => <div key={label} className="grid grid-cols-[140px_1fr] gap-3 border-b border-border/70 py-2"><span className="text-muted-foreground">{label}</span><strong className="break-all">{value}</strong></div>)}</div><h3 className="mt-6 font-bold">Token</h3><div className="mt-2 rounded-lg border border-border">{[['Prompt Token', selected.prompt_tokens], ['其中缓存输入', selected.cache_hit_input_tokens], ['Completion Token', selected.completion_tokens], ['Total Token', selected.total_tokens]].map(([label, value]) => <div key={label} className="flex justify-between border-b border-border/70 px-3 py-2 last:border-0"><span>{label}</span><strong>{formatCompact(value)}</strong></div>)}</div><h3 className="mt-6 font-bold">本次请求的结算结果</h3><div className="mt-2 rounded-lg border border-border">{[['资源包消耗', `${formatCompact(selected.package_units)} units`], ['理论成本', formatMoney(selected.priced_cost_amount)], ['钱包实际扣款', formatMoney(selected.wallet_amount)]].map(([label, value]) => <div key={label} className="flex justify-between border-b border-border/70 px-3 py-2 last:border-0"><span>{label}</span><strong>{value}</strong></div>)}</div><div className="mt-4 rounded-lg border border-border bg-muted p-3 text-xs text-muted-foreground">结算来源只属于当前 Request，不代表 API Key 或模型自身属性。同一个 Key 的下一次请求可以使用其他结算方式。</div></aside></div>}
    </div>
  );
}
