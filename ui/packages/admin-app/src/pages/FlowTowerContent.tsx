import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useFlowMetrics } from '@fluxeme/shared';
import { useModels } from '@fluxeme/shared/src/api/models';
import type { FlowMetricsClientIp, FlowMetricsModelShare, FlowMetricsPercentiles } from '@fluxeme/shared/src/types';

type RangeKey = '5m' | '15m' | '1h' | '6h' | '24h';
type FlowTabKey = 'flow' | 'endpoint' | 'compare';
type Tone = 'blue' | 'cyan' | 'green' | 'yellow' | 'red';

type MetricRow = {
  label: string;
  value: string;
};

const RANGE_OPTIONS: Array<{ key: RangeKey; short: string; long: string; label: string }> = [
  { key: '5m', short: '5M', long: '5 分钟', label: '5 分钟' },
  { key: '15m', short: '15M', long: '15 分钟', label: '15 分钟' },
  { key: '1h', short: '1H', long: '1 小时', label: '1 小时' },
  { key: '6h', short: '6H', long: '6 小时', label: '6 小时' },
  { key: '24h', short: '24H', long: '24 小时', label: '24 小时' },
];

const RANGE_MINUTES: Record<RangeKey, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
};

const FLOW_TABS: Array<{ key: FlowTabKey; label: string }> = [
  { key: 'flow', label: '请求流' },
  { key: 'endpoint', label: '端点状态' },
  { key: 'compare', label: '模型对比' },
];

function formatNumber(value: number | null | undefined) {
  if (value == null) return '—';
  return value.toLocaleString('zh-CN');
}

function formatPercent(value: number | null | undefined) {
  if (value == null || Number.isNaN(value)) return '—';
  return `${value.toFixed(2)}%`;
}

function formatPercentile(value: number | null | undefined) {
  if (value == null || Number.isNaN(value)) return '—';
  return formatNumber(Math.round(value));
}

function formatIpRows(rows: FlowMetricsClientIp[]) {
  const top = rows[0]?.requests ?? 0;
  return rows.slice(0, 8).map((row) => ({
    ...row,
    ratio: top > 0 ? Math.max(8, Math.round((row.requests / top) * 100)) : 0,
  }));
}

function formatTrendBucket(bucket: string, unit: 'minute' | 'hour') {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) {
    return bucket;
  }
  if (unit === 'minute') {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false });
  }
  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    hour12: false,
  }).replace(' ', ' ');
}

function formatRangeBounds(range: RangeKey, nowMs = Date.now()) {
  const end = new Date(nowMs);
  const start = new Date(end.getTime() - RANGE_MINUTES[range] * 60_000);
  return {
    start: start.toISOString(),
    end: end.toISOString(),
  };
}

function toneClasses(tone: Tone) {
  switch (tone) {
    case 'blue':
      return {
        card: 'border-[#dce7ff] bg-[linear-gradient(180deg,#ffffff_0%,#f6f9ff_100%)]',
        badge: 'bg-[#edf4ff] text-[#3276e8]',
        value: 'text-[#3276e8]',
        glow: 'bg-[#edf4ff]',
      };
    case 'cyan':
      return {
        card: 'border-[#d8f0f5] bg-[linear-gradient(180deg,#ffffff_0%,#f3fcfe_100%)]',
        badge: 'bg-[#eafafb] text-[#0ca8bd]',
        value: 'text-[#0ca8bd]',
        glow: 'bg-[#eafafb]',
      };
    case 'green':
      return {
        card: 'border-[#d7f0e3] bg-[linear-gradient(180deg,#ffffff_0%,#f5fcf8_100%)]',
        badge: 'bg-[#ecfbf4] text-[#16a36a]',
        value: 'text-[#16a36a]',
        glow: 'bg-[#ecfbf4]',
      };
    case 'yellow':
      return {
        card: 'border-[#f3e4bc] bg-[linear-gradient(180deg,#ffffff_0%,#fffaf0_100%)]',
        badge: 'bg-[#fff8e8] text-[#d99a18]',
        value: 'text-[#d99a18]',
        glow: 'bg-[#fff8e8]',
      };
    case 'red':
      return {
        card: 'border-[#f2d8d8] bg-[linear-gradient(180deg,#ffffff_0%,#fff6f6_100%)]',
        badge: 'bg-[#fff0f0] text-[#e24f4f]',
        value: 'text-[#e24f4f]',
        glow: 'bg-[#fff0f0]',
      };
  }
}

function Panel({
  title,
  subtitle,
  right,
  children,
  className = '',
}: {
  title: string;
  subtitle?: string;
  right?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <article className={`overflow-hidden rounded-2xl border border-border bg-card shadow-sm ${className}`}>
      <div className="flex min-h-13 items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <div className="text-[13px] font-semibold text-foreground">{title}</div>
          {subtitle ? <div className="mt-1 text-[11px] text-muted-foreground">{subtitle}</div> : null}
        </div>
        {right}
      </div>
      <div className="p-4">{children}</div>
    </article>
  );
}

function PlaceholderCard({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="rounded-xl border border-dashed border-[#d8dee7] bg-[#fbfcfe] px-4 py-6 text-sm text-[#667085]">
      <div className="font-medium text-[#344054]">{title}</div>
      <p className="mt-2 leading-6">{description}</p>
    </div>
  );
}

function KpiCard({
  label,
  badge,
  value,
  subtext,
  tone,
}: {
  label: string;
  badge: string;
  value: string;
  subtext: ReactNode;
  tone: Tone;
}) {
  const palette = toneClasses(tone);

  return (
    <article className={`relative min-h-30 overflow-hidden rounded-2xl border p-4 shadow-sm ${palette.card}`}>
      <div className={`pointer-events-none absolute -right-7 -bottom-8 h-22 w-22 rounded-full ${palette.glow}`} />
      <div className="relative flex items-center justify-between gap-2">
        <div className="text-xs font-semibold text-muted-foreground">{label}</div>
        <span className={`rounded-md px-2 py-0.5 text-[10px] font-bold ${palette.badge}`}>{badge}</span>
      </div>
      <div className={`relative mt-4 text-3xl font-semibold tracking-[-0.04em] ${palette.value}`}>{value}</div>
      <div className="relative mt-3 text-[11px] text-[#98a2b3]">{subtext}</div>
    </article>
  );
}

function PercentileBlock({
  title,
  status,
  tone,
  metrics,
}: {
  title: string;
  status: string;
  tone: 'orange' | 'cyan';
  metrics: FlowMetricsPercentiles;
}) {
  const toneClass = tone === 'orange' ? 'text-[#f08b32]' : 'text-[#0ca8bd]';

  return (
    <div className="rounded-xl border border-[#edf0f4] bg-[#fbfcfe] p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <div className="text-[11px] font-semibold text-[#475467]">{title}</div>
        <div className="text-[10px] text-[#98a2b3]">{status}</div>
      </div>
      <div className="grid grid-cols-3 gap-3">
        {[
          { label: 'P50', value: metrics.p50 },
          { label: 'P90', value: metrics.p90 },
          { label: 'P99', value: metrics.p99 },
        ].map((item, index) => (
          <div key={item.label} className={index < 2 ? 'border-r border-[#e9edf2]' : ''}>
            <small className="block text-[10px] text-[#98a2b3]">{item.label}</small>
            <strong className={`mt-1 block text-lg tracking-[-0.02em] ${toneClass}`}>{formatPercentile(item.value)}</strong>
          </div>
        ))}
      </div>
      <div className="mt-3 text-[10px] text-[#98a2b3]">样本数：{formatNumber(metrics.sample_count)}</div>
    </div>
  );
}

function MetricList({ rows }: { rows: MetricRow[] }) {
  return (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
      {rows.map((row) => (
        <Fragment key={row.label}>
          <span className="text-[#98a2b3]">{row.label}</span>
          <span className="text-right text-[#475467]">{row.value}</span>
        </Fragment>
      ))}
    </div>
  );
}

export default function FlowTowerContent() {
  const [selectedRange, setSelectedRange] = useState<RangeKey>('15m');
  const [selectedModelName, setSelectedModelName] = useState('all');
  const [modelSearch, setModelSearch] = useState('');
  const [activeTab, setActiveTab] = useState<FlowTabKey>('flow');
  const [queryNowMs, setQueryNowMs] = useState(() => Date.now());
  const [displayedData, setDisplayedData] = useState<ReturnType<typeof useFlowMetrics>['data']>(undefined);

  const rangeMeta = useMemo(
    () => RANGE_OPTIONS.find((item) => item.key === selectedRange) ?? RANGE_OPTIONS[1],
    [selectedRange],
  );
  const rangeBounds = useMemo(() => formatRangeBounds(selectedRange, queryNowMs), [selectedRange, queryNowMs]);
  const modelParam = selectedModelName !== 'all' ? selectedModelName : undefined;

  const lastFlowMetricsDataRef = useRef<ReturnType<typeof useFlowMetrics>['data']>(undefined);

  const flowMetrics = useFlowMetrics(
    {
      start: rangeBounds.start,
      end: rangeBounds.end,
      model: modelParam,
    },
    {
      refetchInterval: false,
    },
  );
  const modelsQuery = useModels();

  useEffect(() => {
    const timer = window.setInterval(() => {
      setQueryNowMs(Date.now());
    }, 30_000);

    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (flowMetrics.data) {
      lastFlowMetricsDataRef.current = flowMetrics.data;
      setDisplayedData(flowMetrics.data);
    }
  }, [flowMetrics.data]);

  const modelOptions = useMemo(() => {
    const names = Array.from(new Set((modelsQuery.data ?? []).map((model) => model.name)));
    names.sort((left, right) => left.localeCompare(right));
    return names;
  }, [modelsQuery.data]);

  const visibleModels = useMemo(() => {
    const keyword = modelSearch.trim().toLowerCase();
    if (!keyword) return modelOptions;
    return modelOptions.filter((name) => name.toLowerCase().includes(keyword));
  }, [modelOptions, modelSearch]);

  const effectiveFlowMetricsData = flowMetrics.data ?? displayedData ?? lastFlowMetricsDataRef.current;

  const historical = effectiveFlowMetricsData?.historical;
  const realtime = effectiveFlowMetricsData?.realtime;
  const totalCompleted = historical?.total_completed ?? 0;
  const successRate = totalCompleted > 0
    ? (historical?.success_completed ?? 0) / totalCompleted * 100
    : null;
  const failureRate = totalCompleted > 0
    ? (historical?.failed_completed ?? 0) / totalCompleted * 100
    : null;

  const queueUnavailable = realtime?.queue.count == null;
  const queueValue = queueUnavailable ? '—' : formatNumber(realtime?.queue.count);
  const queueSubtext = queueUnavailable
    ? `当前不可用 · ${realtime?.queue.reason ?? '待后端支持'}`
    : `状态：${realtime?.queue.status}`;

  const shareRows: FlowMetricsModelShare[] = historical?.model_share ?? [];
  const ipRows = useMemo(
    () => formatIpRows(historical?.client_ips ?? []),
    [historical?.client_ips],
  );
  const trendData = useMemo(() => {
    const trend = historical?.trend;
    if (!trend) return [];
    return trend.buckets.map((bucket, index) => ({
      bucket,
      label: formatTrendBucket(bucket, trend.bucket_unit),
      success_completed: trend.success_completed[index] ?? 0,
      failed_completed: trend.failed_completed[index] ?? 0,
    }));
  }, [historical?.trend]);

  const inspectorRows: MetricRow[] = [
    { label: '统计区间', value: rangeMeta.long },
    { label: '筛选模型', value: selectedModelName === 'all' ? '全部模型' : selectedModelName },
    { label: '完成请求', value: formatNumber(totalCompleted) },
    { label: '成功率', value: formatPercent(successRate) },
    { label: 'P99 延迟', value: historical?.latency_ms.p99 != null ? `${formatPercentile(historical.latency_ms.p99)} ms` : '—' },
    { label: 'TTFT P99', value: historical?.ttft_ms.p99 != null ? `${formatPercentile(historical.ttft_ms.p99)} ms` : '—' },
    { label: '实时来源', value: realtime?.source ?? '—' },
  ];

  const lastUpdatedLabel = realtime?.as_of
    ? new Date(realtime.as_of).toLocaleTimeString('zh-CN', { hour12: false })
    : '—';

  if (flowMetrics.isLoading && !effectiveFlowMetricsData) {
    return (
      <div className="space-y-4 animate-fade-in">
        <section className="rounded-2xl border border-border bg-card px-6 py-12 text-center shadow-sm">
          <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
          <p className="mt-3 text-sm text-muted-foreground">正在加载真实观测数据…</p>
        </section>
      </div>
    );
  }

  if (flowMetrics.isError && !effectiveFlowMetricsData) {
    return (
      <div className="space-y-4 animate-fade-in">
        <section className="rounded-2xl border border-border bg-card px-6 py-12 text-center shadow-sm">
          <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
          <p className="mt-3 text-sm text-muted-foreground">加载真实观测数据失败。</p>
          <button
            type="button"
            onClick={() => void flowMetrics.refetch()}
            className="mt-4 inline-flex h-9 items-center rounded-lg border border-border bg-background px-4 text-sm text-[#475467] transition hover:bg-muted"
          >
            重试
          </button>
        </section>
      </div>
    );
  }

  return (
    <div className="space-y-4 animate-fade-in">
      <section className="overflow-hidden rounded-2xl border border-border bg-[linear-gradient(180deg,rgba(255,255,255,0.98)_0%,rgba(248,250,252,0.96)_100%)] shadow-sm">
        <div className="flex flex-col gap-4 border-b border-border px-5 py-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
            <p className="mt-1 text-sm text-muted-foreground">实时请求态势、模型流量、端点健康与异常定位</p>
            <p className="mt-2 text-xs text-[#98a2b3]">本页已接入现有 flow metrics 真实接口；队列与异常事件等区块仍等待后端补充。</p>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex h-9 items-center gap-2 rounded-lg border border-border bg-background px-3 text-[#475467]">
              <span className="h-2 w-2 rounded-full bg-[#16a36a] shadow-[0_0_0_4px_rgba(22,163,106,0.12)]" />
              Real / polled
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-[#475467]">
              自动刷新 · 30s
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-[#475467]">
              最近更新 · {lastUpdatedLabel}
            </div>
          </div>
        </div>

        <div className="space-y-4 px-5 py-5">
          <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
            <div className="flex flex-wrap items-center gap-2">
              <div className="inline-flex rounded-xl bg-[#eaf0f7] p-1">
                {RANGE_OPTIONS.map((option) => (
                  <button
                    key={option.key}
                    type="button"
                    onClick={() => {
                      setSelectedRange(option.key);
                      setQueryNowMs(Date.now());
                    }}
                    className={`rounded-lg px-3 py-1.5 text-xs transition ${
                      selectedRange === option.key
                        ? 'bg-white text-[#1f2937] shadow-sm'
                        : 'text-[#697586] hover:text-[#1f2937]'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
              <select
                aria-label="模型筛选"
                className="h-9 rounded-lg border border-border bg-background px-3 text-xs text-[#475467]"
                value={selectedModelName}
                onChange={(event) => {
                  setSelectedModelName(event.target.value);
                  setQueryNowMs(Date.now());
                }}
              >
                <option value="all">全部模型</option>
                {modelOptions.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label="渠道筛选"
                className="h-9 rounded-lg border border-border bg-muted px-3 text-xs text-[#98a2b3]"
                value="pending"
                disabled
              >
                <option value="pending">渠道筛选（待接口支持）</option>
              </select>
              <button
                type="button"
                onClick={() => setQueryNowMs(Date.now())}
                className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-xs text-[#475467] transition hover:bg-muted"
              >
                {flowMetrics.isFetching ? '刷新中…' : '↻ 刷新'}
              </button>
            </div>
          </div>

          <section className="grid gap-3 md:grid-cols-2 2xl:grid-cols-6">
            <KpiCard
              label="当前在途请求数"
              badge="LIVE"
              value={formatNumber(realtime?.in_flight)}
              tone="blue"
              subtext={<>当前实时快照 · 数据源 <span className="font-semibold text-[#3276e8]">{realtime?.source ?? '—'}</span></>}
            />
            <KpiCard
              label="上游生成中"
              badge="LIVE"
              value={formatNumber(realtime?.upstream_generating)}
              tone="cyan"
              subtext="尚未收到首字节的上游请求数"
            />
            <KpiCard
              label="上游输出中"
              badge="LIVE"
              value={formatNumber(realtime?.upstream_outputting)}
              tone="green"
              subtext="已经开始返回内容的上游请求数"
            />
            <KpiCard
              label="排队请求数"
              badge="PENDING"
              value={queueValue}
              tone="yellow"
              subtext={queueSubtext}
            />
            <KpiCard
              label="成功完成"
              badge={rangeMeta.short}
              value={formatNumber(historical?.success_completed)}
              tone="green"
              subtext={<>成功率 <span className="font-semibold text-[#16a36a]">{formatPercent(successRate)}</span></>}
            />
            <KpiCard
              label="失败完成"
              badge={rangeMeta.short}
              value={formatNumber(historical?.failed_completed)}
              tone="red"
              subtext={<>错误率 <span className="font-semibold text-[#e24f4f]">{formatPercent(failureRate)}</span></>}
            />
          </section>

          <section className="grid gap-4 2xl:grid-cols-[minmax(0,1.2fr)_minmax(380px,0.8fr)]">
            <div className="grid gap-4">
              <Panel
                title="成功 / 失败完成量趋势"
                subtitle={`最近 ${rangeMeta.long} · ${historical?.trend.bucket_unit === 'minute' ? '按分钟聚合' : '按小时聚合'}`}
              >
                {trendData.length > 0 ? (
                  <div className="h-[260px] w-full">
                    <ResponsiveContainer>
                      <AreaChart data={trendData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#edf1f6" />
                        <XAxis dataKey="label" tick={{ fill: '#98a2b3', fontSize: 10 }} minTickGap={20} />
                        <YAxis tick={{ fill: '#98a2b3', fontSize: 10 }} allowDecimals={false} />
                        <RechartsTooltip
                          contentStyle={{ borderRadius: 12, border: '1px solid #e6ebf2', fontSize: 12 }}
                          formatter={(value, name) => [formatNumber(Number(value)), name === 'success_completed' ? '成功完成' : '失败完成']}
                          labelFormatter={(label) => String(label ?? '')}
                        />
                        <Area type="monotone" dataKey="success_completed" stackId="1" stroke="#27ad74" fill="#54bd8b" fillOpacity={0.25} />
                        <Area type="monotone" dataKey="failed_completed" stackId="1" stroke="#e45d5d" fill="#e45d5d" fillOpacity={0.18} />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                ) : (
                  <PlaceholderCard
                    title="当前区间暂无趋势数据"
                    description="当前时间窗口内没有可用的成功 / 失败完成量序列，暂时无法渲染趋势图。"
                  />
                )}
              </Panel>

              <Panel
                title="客户端 IP Top N"
                subtitle={`请求来源排行 · 最近 ${rangeMeta.long}`}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-[#475467]">
                    按请求数
                  </div>
                }
              >
                {ipRows.length > 0 ? (
                  <div className="overflow-x-auto">
                    <table className="w-full min-w-[520px] border-collapse">
                      <thead>
                        <tr>
                          <th className="pb-2 text-left text-[10px] font-semibold text-[#98a2b3]">客户端 IP</th>
                          <th className="pb-2 text-left text-[10px] font-semibold text-[#98a2b3]">请求占比</th>
                          <th className="pb-2 text-right text-[10px] font-semibold text-[#98a2b3]">请求</th>
                        </tr>
                      </thead>
                      <tbody>
                        {ipRows.map((row) => (
                          <tr key={row.ip}>
                            <td className="border-t border-[#f2f4f7] py-2.5 font-mono text-[11px] text-[#475467]">{row.ip}</td>
                            <td className="border-t border-[#f2f4f7] py-2.5">
                              <div className="h-1.5 overflow-hidden rounded-full bg-[#eef2f7]">
                                <div
                                  className="h-full rounded-full bg-[linear-gradient(90deg,#9683ed,#5f7fe5)]"
                                  style={{ width: `${row.ratio}%` }}
                                />
                              </div>
                            </td>
                            <td className="border-t border-[#f2f4f7] py-2.5 text-right text-[11px] text-[#475467]">{formatNumber(row.requests)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                ) : (
                  <PlaceholderCard title="暂无客户端 IP 数据" description="当前区间内没有可用的 client_ip 统计，或观测数据尚未写入。" />
                )}
              </Panel>
            </div>

            <div className="grid gap-4">
              <Panel
                title="模型请求占比"
                subtitle={`成功 + 失败请求总量 · 最近 ${rangeMeta.long}`}
                right={<div className="text-[11px] text-[#98a2b3]">{formatNumber(totalCompleted)} requests</div>}
              >
                {shareRows.length > 0 ? (
                  <div className="flex flex-col gap-3">
                    {shareRows.map((row) => (
                      <div key={row.model} className="grid grid-cols-[minmax(120px,145px)_minmax(0,1fr)_52px] items-center gap-3">
                        <div className="truncate text-[11px] text-[#475467]">{row.model}</div>
                        <div className="h-2 overflow-hidden rounded-full bg-[#eef2f7]">
                          <div
                            className="h-full rounded-full bg-[linear-gradient(90deg,#7eb0ff,#3678e8)]"
                            style={{ width: `${Math.max(6, row.share)}%` }}
                          />
                        </div>
                        <div className="text-right text-[11px] text-[#667085]">{row.share.toFixed(1)}%</div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <PlaceholderCard title="暂无模型占比数据" description="当前区间内没有模型完成请求统计，暂时无法生成模型请求占比。" />
                )}
              </Panel>

              <Panel
                title="延迟 / TTFT 分位数"
                subtitle={`全模型汇总 · 最近 ${rangeMeta.long}`}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-[#475467]">
                    ms
                  </div>
                }
              >
                <div className="grid gap-3">
                  <PercentileBlock
                    title="请求延迟"
                    status="来自 ClickHouse usage_events"
                    tone="orange"
                    metrics={historical?.latency_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                  />
                  <PercentileBlock
                    title="TTFT"
                    status="仅统计已有首字节观测样本"
                    tone="cyan"
                    metrics={historical?.ttft_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                  />
                </div>
              </Panel>
            </div>
          </section>

          <section className="grid gap-4 xl:grid-cols-[240px_minmax(0,1fr)] 2xl:grid-cols-[240px_minmax(0,1fr)_310px]">
            <Panel title="模型目录" subtitle={`${formatNumber(modelOptions.length)} 个模型 · 先接真实模型列表`} className="min-h-[580px]">
              <div className="-mt-1">
                <div className="mb-3">
                  <input
                    aria-label="搜索模型"
                    value={modelSearch}
                    onChange={(event) => setModelSearch(event.target.value)}
                    placeholder="搜索模型"
                    className="h-9 w-full rounded-lg border border-[#e1e7ef] bg-[#fbfcfe] px-3 text-[11px] text-[#475467] outline-none placeholder:text-[#98a2b3]"
                  />
                </div>
                <div className="space-y-1">
                  <button
                    type="button"
                    onClick={() => setSelectedModelName('all')}
                    className={`w-full rounded-xl border px-3 py-3 text-left transition ${
                      selectedModelName === 'all'
                        ? 'border-[#d8e6ff] bg-[#eff5ff]'
                        : 'border-transparent hover:bg-[#f7f9fc]'
                    }`}
                  >
                    <div className="text-[11px] font-semibold text-[#344054]">全部模型</div>
                    <div className="mt-2 text-[10px] text-[#98a2b3]">查看当前范围的聚合 flow metrics</div>
                  </button>
                  {visibleModels.length > 0 ? (
                    visibleModels.map((name) => (
                      <button
                        key={name}
                        type="button"
                        onClick={() => setSelectedModelName(name)}
                        className={`w-full rounded-xl border px-3 py-3 text-left transition ${
                          selectedModelName === name
                            ? 'border-[#d8e6ff] bg-[#eff5ff]'
                            : 'border-transparent hover:bg-[#f7f9fc]'
                        }`}
                      >
                        <div className="text-[11px] font-semibold text-[#344054]">{name}</div>
                        <div className="mt-2 text-[10px] text-[#98a2b3]">点击后按该模型重新查询已接入的 flow metrics</div>
                      </button>
                    ))
                  ) : modelsQuery.isLoading ? (
                    <div className="rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3]">
                      正在加载模型列表…
                    </div>
                  ) : modelsQuery.isError ? (
                    <div className="rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3]">
                      模型列表加载失败，请稍后重试。
                    </div>
                  ) : (
                    <div className="rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3]">
                      没有匹配的模型
                    </div>
                  )}
                </div>
              </div>
            </Panel>

            <article className="overflow-hidden rounded-2xl border border-border bg-card shadow-sm">
              <div className="flex gap-4 border-b border-border px-4 pt-3" role="tablist" aria-label="流控台详情标签页">
                {FLOW_TABS.map((tab) => (
                  <button
                    key={tab.key}
                    id={`flowtower-tab-${tab.key}`}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === tab.key}
                    aria-controls={`flowtower-panel-${tab.key}`}
                    onClick={() => setActiveTab(tab.key)}
                    className={`border-b-2 pb-3 text-[11px] transition ${
                      activeTab === tab.key
                        ? 'border-[#2f6edb] text-[#2f6edb] font-semibold'
                        : 'border-transparent text-[#667085] hover:text-[#2f6edb]'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {activeTab === 'flow' ? (
                <div id="flowtower-panel-flow" role="tabpanel" aria-labelledby="flowtower-tab-flow" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="请求流拓扑待第二阶段接入"
                    description="这一块需要组合 routing snapshot、recent paths、routing health 等接口才能展示真实路由拓扑百分比与端点分布。当前先保留占位态，避免继续展示伪实时路径图。"
                  />
                </div>
              ) : null}

              {activeTab === 'endpoint' ? (
                <div id="flowtower-panel-endpoint" role="tabpanel" aria-labelledby="flowtower-tab-endpoint" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="端点状态待第二阶段接入"
                    description="现有接口可以进一步接 routing health / probe results recent，但本轮先只接 flow metrics 已覆盖的区块，避免把不同接口的字段逻辑混在一起。"
                  />
                </div>
              ) : null}

              {activeTab === 'compare' ? (
                <div id="flowtower-panel-compare" role="tabpanel" aria-labelledby="flowtower-tab-compare" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="模型对比待第二阶段接入"
                    description="完整模型对比需要组合 flow metrics、routing health 以及更多按模型维度的实时/趋势字段；当前接口还不足以支撑现有 mock 表格里的全部列。"
                  />
                </div>
              ) : null}
            </article>

            <Panel title="模型检查器" subtitle="先展示当前已接入的真实 flow metrics 摘要" className="min-h-[580px]">
              <div className="rounded-xl border border-[#dfe7f3] bg-[#f8fbff] p-4">
                <div className="flex items-center justify-between gap-2">
                  <strong className="text-sm text-[#182230]">{selectedModelName === 'all' ? '全部模型' : selectedModelName}</strong>
                  <span className="rounded-md bg-[#edf4ff] px-2 py-1 text-[10px] font-semibold text-[#3276e8]">
                    flow-metrics
                  </span>
                </div>
                <div className="mt-4">
                  <MetricList rows={inspectorRows} />
                </div>
              </div>

              <div className="mt-4 border-t border-border pt-4">
                <PlaceholderCard
                  title="异常事件待后端聚合接口"
                  description="当前仓库里还没有统一的 incidents feed 用于支撑“最近 30 分钟异常事件”。等后端补充事件聚合接口后，再把这个区域切成 real。"
                />
              </div>
            </Panel>
          </section>
        </div>
      </section>
    </div>
  );
}
