import { useEffect, useMemo, useState } from 'react';

type RangeKey = '5m' | '15m' | '1h' | '6h' | '24h';
type FlowTabKey = 'flow' | 'endpoint' | 'compare';
type Tone = 'blue' | 'cyan' | 'green' | 'yellow' | 'red';

type ModelCatalogItem = {
  name: string;
  channels: number;
  successRate: string;
  rpm: string;
  status: 'healthy' | 'warn' | 'error' | 'offline';
  currentRps: string;
  inflight: string;
  queued: string;
  p95Latency: string;
  p95Ttft: string;
  activeChannels: string;
};

type EndpointRow = {
  channel: string;
  path: string;
  protocol: string;
  statusLabel: string;
  statusTone: 'healthy' | 'warn' | 'error';
  latency: string;
  timeline: Array<'healthy' | 'warn' | 'error' | 'idle'>;
};

type IncidentItem = {
  title: string;
  severity: 'WARN' | 'HIGH';
  description: string;
  time: string;
  model: string;
};

type KpiValues = {
  inflight: number;
  generating: number;
  streaming: number;
  queued: number;
  success: number;
  failed: number;
};

const RANGE_OPTIONS: Array<{ key: RangeKey; short: string; long: string; label: string }> = [
  { key: '5m', short: '5M', long: '5 分钟', label: '5 分钟' },
  { key: '15m', short: '15M', long: '15 分钟', label: '15 分钟' },
  { key: '1h', short: '1H', long: '1 小时', label: '1 小时' },
  { key: '6h', short: '6H', long: '6 小时', label: '6 小时' },
  { key: '24h', short: '24H', long: '24 小时', label: '24 小时' },
];

const BASE_KPIS: KpiValues = {
  inflight: 52,
  generating: 37,
  streaming: 29,
  queued: 11,
  success: 2846,
  failed: 24,
};

const SUCCESS_HISTORY_BASE = [127, 148, 140, 154, 165, 175, 158, 148, 178, 186, 172, 160, 181, 193, 184];
const FAILURE_HISTORY_BASE = [9, 13, 8, 16, 12, 10, 21, 15, 9, 12, 18, 23, 13, 19, 15];

const IP_ROWS = [
  { ip: '10.12.8.41', count: 628, ratio: 100 },
  { ip: '10.12.8.73', count: 491, ratio: 78 },
  { ip: '10.233.45.216', count: 414, ratio: 66 },
  { ip: '172.19.0.14', count: 332, ratio: 53 },
  { ip: '10.121.18.20', count: 270, ratio: 43 },
  { ip: '10.12.9.12', count: 204, ratio: 32 },
  { ip: '172.19.0.27', count: 171, ratio: 27 },
  { ip: '10.12.11.93', count: 119, ratio: 19 },
] as const;

const MODEL_SHARE_ROWS = [
  { name: 'DeepSeek-V4-Flash', percent: 36.8, width: 100 },
  { name: 'GPT-5.6-Luna', percent: 27.4, width: 75 },
  { name: 'Claude-Sonnet-4.6', percent: 20.1, width: 55 },
  { name: 'Qwen3.5-Plus', percent: 9.8, width: 27 },
  { name: 'GPT-5.4', percent: 4.2, width: 12 },
  { name: 'Other', percent: 1.7, width: 5 },
] as const;

const MODEL_CATALOG: ModelCatalogItem[] = [
  {
    name: 'DeepSeek-V4-Flash',
    channels: 3,
    successRate: '99.6%',
    rpm: '12.8K rpm',
    status: 'healthy',
    currentRps: '71.2',
    inflight: '22',
    queued: '7',
    p95Latency: '41.8 s',
    p95Ttft: '3.6 s',
    activeChannels: '3 / 3',
  },
  {
    name: 'GPT-5.6-Luna',
    channels: 2,
    successRate: '99.9%',
    rpm: '9.4K rpm',
    status: 'healthy',
    currentRps: '52.4',
    inflight: '14',
    queued: '2',
    p95Latency: '17.2 s',
    p95Ttft: '1.1 s',
    activeChannels: '2 / 2',
  },
  {
    name: 'Claude-Sonnet-4.6',
    channels: 2,
    successRate: '97.8%',
    rpm: '7.2K rpm',
    status: 'warn',
    currentRps: '38.5',
    inflight: '10',
    queued: '4',
    p95Latency: '28.7 s',
    p95Ttft: '2.4 s',
    activeChannels: '2 / 2',
  },
  {
    name: 'Qwen3.5-Plus',
    channels: 2,
    successRate: '99.5%',
    rpm: '3.5K rpm',
    status: 'healthy',
    currentRps: '18.7',
    inflight: '6',
    queued: '1',
    p95Latency: '21.4 s',
    p95Ttft: '1.9 s',
    activeChannels: '2 / 2',
  },
  {
    name: 'GPT-5.4',
    channels: 1,
    successRate: '99.7%',
    rpm: '1.9K rpm',
    status: 'healthy',
    currentRps: '9.1',
    inflight: '3',
    queued: '0',
    p95Latency: '16.9 s',
    p95Ttft: '0.9 s',
    activeChannels: '1 / 1',
  },
  {
    name: 'Legacy-Fallback',
    channels: 1,
    successRate: 'unavailable',
    rpm: '0 rpm',
    status: 'offline',
    currentRps: '0.0',
    inflight: '0',
    queued: '0',
    p95Latency: '—',
    p95Ttft: '—',
    activeChannels: '0 / 1',
  },
];

const ENDPOINT_ROWS: EndpointRow[] = [
  {
    channel: 'channel-a',
    path: '/v1/chat/completions',
    protocol: 'OpenAI',
    statusLabel: '200 OK',
    statusTone: 'healthy',
    latency: '31 ms',
    timeline: ['healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy'],
  },
  {
    channel: 'channel-b',
    path: '/v1/messages',
    protocol: 'Anthropic',
    statusLabel: '200 OK',
    statusTone: 'healthy',
    latency: '46 ms',
    timeline: ['healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy', 'healthy'],
  },
  {
    channel: 'channel-c',
    path: '/v1/chat/completions',
    protocol: 'vLLM',
    statusLabel: '429 WARN',
    statusTone: 'warn',
    latency: '118 ms',
    timeline: ['healthy', 'healthy', 'warn', 'healthy', 'warn', 'healthy', 'healthy', 'warn', 'healthy', 'healthy'],
  },
  {
    channel: 'fallback-us',
    path: '/v1/responses',
    protocol: 'OpenAI',
    statusLabel: '503 ERR',
    statusTone: 'error',
    latency: '—',
    timeline: ['healthy', 'healthy', 'healthy', 'error', 'error', 'error', 'error', 'error', 'idle', 'idle'],
  },
];

const COMPARE_ROWS = [
  { model: 'DeepSeek-V4-Flash', rps: '71.2', successRate: '99.6%', latency: '41.8s', ttft: '3.6s', errors: '4' },
  { model: 'GPT-5.6-Luna', rps: '52.4', successRate: '99.9%', latency: '17.2s', ttft: '1.1s', errors: '1' },
  { model: 'Claude-Sonnet-4.6', rps: '38.5', successRate: '97.8%', latency: '28.7s', ttft: '2.4s', errors: '14' },
  { model: 'Qwen3.5-Plus', rps: '18.7', successRate: '99.5%', latency: '21.4s', ttft: '1.9s', errors: '3' },
] as const;

const INCIDENTS: IncidentItem[] = [
  {
    title: 'channel-c 触发限流',
    severity: 'WARN',
    description: '上游连续返回 429，路由权重已从 28% 自动降至 17%。',
    time: '03:49:12',
    model: 'DeepSeek-V4-Flash',
  },
  {
    title: 'Claude Sonnet 错误率抬升',
    severity: 'HIGH',
    description: '5 分钟错误率达到 2.2%，主要为 upstream_timeout。',
    time: '03:44:37',
    model: 'Claude-Sonnet-4.6',
  },
  {
    title: 'fallback-us 探测失败',
    severity: 'HIGH',
    description: '连续 5 次健康检查失败，端点已进入熔断状态。',
    time: '03:39:02',
    model: 'GPT-5.4',
  },
  {
    title: 'TTFT P99 短时升高',
    severity: 'WARN',
    description: 'P99 一度达到 12.3s，目前已恢复至 9.4s。',
    time: '03:31:18',
    model: 'all models',
  },
];

const FLOW_METRICS = [
  { label: 'Gateway RPS', value: '191.3' },
  { label: 'Route P95', value: '13 ms' },
  { label: 'Upstream TTFT', value: '3.9 s' },
  { label: 'Retry Rate', value: '0.42%' },
] as const;

const FLOW_TABS: Array<{ key: FlowTabKey; label: string }> = [
  { key: 'flow', label: '请求流' },
  { key: 'endpoint', label: '端点状态' },
  { key: 'compare', label: '模型对比' },
];

const SHARE_FILTER_OPTIONS = MODEL_SHARE_ROWS.filter((item) => item.name !== 'Other');

const RANGE_TOTAL_MINUTES: Record<RangeKey, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
};

function buildRangeLabels(range: RangeKey) {
  const totalMinutes = RANGE_TOTAL_MINUTES[range];
  const now = new Date();

  return Array.from({ length: 5 }, (_, index) => {
    const offsetMinutes = totalMinutes - (totalMinutes / 4) * index;
    const point = new Date(now.getTime() - offsetMinutes * 60_000);
    const month = String(point.getMonth() + 1).padStart(2, '0');
    const day = String(point.getDate()).padStart(2, '0');
    const hours = String(point.getHours()).padStart(2, '0');
    const minutes = String(point.getMinutes()).padStart(2, '0');

    return totalMinutes >= 1440 ? `${month}-${day}` : `${hours}:${minutes}`;
  });
}

function matchesChannelFilter(row: EndpointRow, selectedChannel: string) {
  if (selectedChannel === '全部渠道') return true;
  if (selectedChannel === 'vLLM Cluster') return row.protocol === 'vLLM';
  return row.protocol === selectedChannel;
}

function randomInt(min: number, max: number) {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

function mutateHistory(values: number[], min: number, max: number) {
  return values.map((value) => Math.max(min, Math.min(max, value + randomInt(-12, 12))));
}

function formatNumber(value: number) {
  return value.toLocaleString('zh-CN');
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

function statusSquareClasses(status: ModelCatalogItem['status']) {
  switch (status) {
    case 'healthy':
      return 'bg-[#16a36a]';
    case 'warn':
      return 'bg-[#d99a18]';
    case 'error':
      return 'bg-[#e24f4f]';
    case 'offline':
      return 'bg-[#c4cad3]';
  }
}

function endpointStatusClasses(tone: EndpointRow['statusTone']) {
  switch (tone) {
    case 'healthy':
      return 'text-[#179767]';
    case 'warn':
      return 'text-[#bd7c10]';
    case 'error':
      return 'text-[#d14848]';
  }
}

function timelineSquareClasses(state: EndpointRow['timeline'][number]) {
  switch (state) {
    case 'healthy':
      return 'bg-[#28b477]';
    case 'warn':
      return 'bg-[#e3aa37]';
    case 'error':
      return 'bg-[#e46262]';
    case 'idle':
      return 'bg-[#cbd2dc]';
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
  right?: React.ReactNode;
  children: React.ReactNode;
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
  subtext: React.ReactNode;
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

export default function FlowTowerContent() {
  const [selectedRange, setSelectedRange] = useState<RangeKey>('15m');
  const [selectedFilter, setSelectedFilter] = useState<string>('all');
  const [selectedModelName, setSelectedModelName] = useState<string>(MODEL_CATALOG[0].name);
  const [selectedChannel, setSelectedChannel] = useState<string>('全部渠道');
  const [modelSearch, setModelSearch] = useState('');
  const [activeTab, setActiveTab] = useState<FlowTabKey>('flow');
  const [kpis, setKpis] = useState<KpiValues>(BASE_KPIS);
  const [successHistory, setSuccessHistory] = useState<number[]>(SUCCESS_HISTORY_BASE);
  const [failureHistory, setFailureHistory] = useState<number[]>(FAILURE_HISTORY_BASE);

  const rangeMeta = useMemo(
    () => RANGE_OPTIONS.find((item) => item.key === selectedRange) ?? RANGE_OPTIONS[1],
    [selectedRange],
  );

  const selectedModel = useMemo(
    () => MODEL_CATALOG.find((item) => item.name === selectedModelName) ?? MODEL_CATALOG[0],
    [selectedModelName],
  );

  const filteredShareRows = useMemo(() => {
    if (selectedFilter === 'all') return MODEL_SHARE_ROWS;
    return MODEL_SHARE_ROWS.filter((item) => item.name === selectedFilter);
  }, [selectedFilter]);

  const visibleModels = useMemo(() => {
    const keyword = modelSearch.trim().toLowerCase();
    if (!keyword) return MODEL_CATALOG;
    return MODEL_CATALOG.filter((model) => model.name.toLowerCase().includes(keyword));
  }, [modelSearch]);

  const visibleEndpointRows = useMemo(
    () => ENDPOINT_ROWS.filter((row) => matchesChannelFilter(row, selectedChannel)),
    [selectedChannel],
  );

  const rangeAxisLabels = useMemo(() => buildRangeLabels(selectedRange), [selectedRange]);

  const refreshMetrics = () => {
    setKpis({
      inflight: randomInt(48, 61),
      generating: randomInt(33, 44),
      streaming: randomInt(24, 35),
      queued: randomInt(7, 16),
      success: randomInt(2825, 2915),
      failed: randomInt(20, 31),
    });
    setSuccessHistory(mutateHistory(SUCCESS_HISTORY_BASE, 130, 198));
    setFailureHistory(mutateHistory(FAILURE_HISTORY_BASE, 7, 25));
  };

  useEffect(() => {
    const timer = window.setInterval(refreshMetrics, 10_000);

    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    refreshMetrics();
  }, [selectedRange]);

  const handleRefresh = () => {
    refreshMetrics();
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <section className="overflow-hidden rounded-2xl border border-border bg-[linear-gradient(180deg,rgba(255,255,255,0.98)_0%,rgba(248,250,252,0.96)_100%)] shadow-sm">
        <div className="flex flex-col gap-4 border-b border-border px-5 py-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
            <p className="mt-1 text-sm text-muted-foreground">实时请求态势、模型流量、端点健康与异常定位</p>
            <p className="mt-2 text-xs text-[#98a2b3]">演示用 mock 页面，当前展示的是静态/随机模拟数据，不代表真实线上状态。</p>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex h-9 items-center gap-2 rounded-lg border border-border bg-background px-3 text-[#475467]">
              <span className="h-2 w-2 rounded-full bg-[#16a36a] shadow-[0_0_0_4px_rgba(22,163,106,0.12)]" />
              Mock Live
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-[#475467]">
              自动刷新 · 10s
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
                    onClick={() => setSelectedRange(option.key)}
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
                value={selectedFilter}
                onChange={(event) => setSelectedFilter(event.target.value)}
              >
                <option value="all">全部模型</option>
                {SHARE_FILTER_OPTIONS.map((model) => (
                  <option key={model.name} value={model.name}>
                    {model.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label="渠道筛选"
                className="h-9 rounded-lg border border-border bg-background px-3 text-xs text-[#475467]"
                value={selectedChannel}
                onChange={(event) => setSelectedChannel(event.target.value)}
              >
                <option>全部渠道</option>
                <option>OpenAI</option>
                <option>Anthropic</option>
                <option>vLLM Cluster</option>
              </select>
              <button
                type="button"
                onClick={handleRefresh}
                className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-xs text-[#475467] transition hover:bg-muted"
              >
                ↻ 刷新
              </button>
            </div>
          </div>

          <section className="grid gap-3 md:grid-cols-2 2xl:grid-cols-6">
            <KpiCard
              label="当前在途请求数"
              badge="LIVE"
              value={formatNumber(kpis.inflight)}
              tone="blue"
              subtext={
                <>
                  较 5 分钟均值 <span className="font-semibold text-[#16a36a]">+4.2%</span>
                </>
              }
            />
            <KpiCard
              label="上游生成中"
              badge="LIVE"
              value={formatNumber(kpis.generating)}
              tone="cyan"
              subtext="71.2% 的在途请求正在推理"
            />
            <KpiCard
              label="上游输出中"
              badge="LIVE"
              value={formatNumber(kpis.streaming)}
              tone="green"
              subtext="当前活跃 Streaming 连接"
            />
            <KpiCard
              label="排队请求数"
              badge="APPROX"
              value={formatNumber(kpis.queued)}
              tone="yellow"
              subtext={
                <>
                  队列压力 <span className="font-semibold text-[#b67c0c]">中等</span> · P95 1.8s
                </>
              }
            />
            <KpiCard
              label="成功完成"
              badge={rangeMeta.short}
              value={formatNumber(kpis.success)}
              tone="green"
              subtext={
                <>
                  成功率 <span className="font-semibold text-[#16a36a]">99.18%</span>
                </>
              }
            />
            <KpiCard
              label="失败完成"
              badge={rangeMeta.short}
              value={formatNumber(kpis.failed)}
              tone="red"
              subtext={
                <>
                  错误率 <span className="font-semibold text-[#e24f4f]">0.82%</span> · +0.17%
                </>
              }
            />
          </section>

          <section className="grid gap-4 2xl:grid-cols-[minmax(0,1.2fr)_minmax(380px,0.8fr)]">
            <div className="grid gap-4">
              <Panel
                title="成功 / 失败完成量趋势"
                subtitle={`按 1 分钟聚合 · 最近 ${rangeMeta.long}`}
                right={
                  <div className="flex gap-3 text-[11px] text-[#667085]">
                    <span className="inline-flex items-center gap-1.5">
                      <i className="h-2 w-2 rounded-sm bg-[#27ad74]" />成功
                    </span>
                    <span className="inline-flex items-center gap-1.5">
                      <i className="h-2 w-2 rounded-sm bg-[#e45d5d]" />失败
                    </span>
                  </div>
                }
              >
                <div className="h-[240px] w-full">
                  <svg viewBox="0 0 760 240" preserveAspectRatio="none" className="h-full w-full">
                    {[20, 70, 120, 170, 220].map((y) => (
                      <line key={y} x1="45" y1={y} x2="742" y2={y} className="stroke-[#edf1f6]" strokeWidth="1" />
                    ))}
                    <text x="7" y="23" className="fill-[#98a2b3] text-[10px]">300</text>
                    <text x="7" y="73" className="fill-[#98a2b3] text-[10px]">225</text>
                    <text x="7" y="123" className="fill-[#98a2b3] text-[10px]">150</text>
                    <text x="12" y="173" className="fill-[#98a2b3] text-[10px]">75</text>
                    <text x="17" y="223" className="fill-[#98a2b3] text-[10px]">0</text>

                    {successHistory.map((height, index) => (
                      <rect
                        key={`success-${index}`}
                        x={65 + index * 42}
                        y={220 - height}
                        width="28"
                        height={height}
                        rx="3"
                        fill="#54bd8b"
                      />
                    ))}

                    {failureHistory.map((height, index) => (
                      <rect
                        key={`failure-${index}`}
                        x={94 + index * 42}
                        y={220 - height}
                        width="7"
                        height={height}
                        rx="2"
                        fill="#e45d5d"
                      />
                    ))}

                    {rangeAxisLabels.map((label, index) => {
                      const xPositions = [52, 211, 374, 535, 690];
                      return (
                        <text key={`${label}-${index}`} x={xPositions[index]} y="237" className="fill-[#98a2b3] text-[10px]">
                          {label}
                        </text>
                      );
                    })}
                  </svg>
                </div>
              </Panel>

              <Panel
                title="客户端 IP Top N"
                subtitle="请求来源排行 · Top 8"
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-[#475467]">
                    按请求数
                  </div>
                }
              >
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
                      {IP_ROWS.map((row) => (
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
                          <td className="border-t border-[#f2f4f7] py-2.5 text-right text-[11px] text-[#475467]">{row.count}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Panel>
            </div>

            <div className="grid gap-4">
              <Panel
                title="模型请求占比"
                subtitle="成功 + 失败请求总量"
                right={<div className="text-[11px] text-[#98a2b3]">2,870 requests</div>}
              >
                <div className="flex flex-col gap-3">
                  {filteredShareRows.map((row) => (
                    <div key={row.name} className="grid grid-cols-[minmax(120px,145px)_minmax(0,1fr)_52px] items-center gap-3">
                      <div className="truncate text-[11px] text-[#475467]">{row.name}</div>
                      <div className="h-2 overflow-hidden rounded-full bg-[#eef2f7]">
                        <div
                          className="h-full rounded-full bg-[linear-gradient(90deg,#7eb0ff,#3678e8)]"
                          style={{ width: `${row.width}%` }}
                        />
                      </div>
                      <div className="text-right text-[11px] text-[#667085]">{row.percent.toFixed(1)}%</div>
                    </div>
                  ))}
                </div>
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
                  <div className="rounded-xl border border-[#edf0f4] bg-[#fbfcfe] p-4">
                    <div className="mb-3 flex items-center justify-between gap-2">
                      <div className="text-[11px] font-semibold text-[#475467]">请求延迟</div>
                      <div className="text-[10px] text-[#98a2b3]">tail latency ↑</div>
                    </div>
                    <div className="grid grid-cols-3 gap-3">
                      {[
                        { label: 'P50', value: '8,482' },
                        { label: 'P90', value: '33,552' },
                        { label: 'P99', value: '158,537' },
                      ].map((item, index) => (
                        <div key={item.label} className={index < 2 ? 'border-r border-[#e9edf2]' : ''}>
                          <small className="block text-[10px] text-[#98a2b3]">{item.label}</small>
                          <strong className="mt-1 block text-lg tracking-[-0.02em] text-[#f08b32]">{item.value}</strong>
                        </div>
                      ))}
                    </div>
                  </div>

                  <div className="rounded-xl border border-[#edf0f4] bg-[#fbfcfe] p-4">
                    <div className="mb-3 flex items-center justify-between gap-2">
                      <div className="text-[11px] font-semibold text-[#475467]">TTFT</div>
                      <div className="text-[10px] text-[#98a2b3]">first token stable</div>
                    </div>
                    <div className="grid grid-cols-3 gap-3">
                      {[
                        { label: 'P50', value: '1,326' },
                        { label: 'P90', value: '3,904' },
                        { label: 'P99', value: '9,411' },
                      ].map((item, index) => (
                        <div key={item.label} className={index < 2 ? 'border-r border-[#e9edf2]' : ''}>
                          <small className="block text-[10px] text-[#98a2b3]">{item.label}</small>
                          <strong className="mt-1 block text-lg tracking-[-0.02em] text-[#0ca8bd]">{item.value}</strong>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              </Panel>
            </div>
          </section>

          <section className="grid gap-4 xl:grid-cols-[240px_minmax(0,1fr)] 2xl:grid-cols-[240px_minmax(0,1fr)_310px]">
            <Panel title="模型目录" subtitle="6 个模型 · 5 可用" className="min-h-[580px]">
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
                  {visibleModels.length > 0 ? (
                    visibleModels.map((model) => (
                      <button
                        key={model.name}
                        type="button"
                        onClick={() => {
                          setSelectedModelName(model.name);
                          setSelectedFilter(
                            SHARE_FILTER_OPTIONS.some((item) => item.name === model.name) ? model.name : 'all',
                          );
                        }}
                        className={`w-full rounded-xl border px-3 py-3 text-left transition ${
                          selectedModel.name === model.name
                            ? 'border-[#d8e6ff] bg-[#eff5ff]'
                            : 'border-transparent hover:bg-[#f7f9fc]'
                        }`}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="text-[11px] font-semibold text-[#344054]">{model.name}</div>
                          <span className={`h-2 w-2 rounded-sm ${statusSquareClasses(model.status)}`} />
                        </div>
                        <div className="mt-2 flex flex-wrap gap-2 text-[10px] text-[#98a2b3]">
                          <span>{model.channels} channels</span>
                          <span>{model.successRate}</span>
                          <span>{model.rpm}</span>
                        </div>
                      </button>
                    ))
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
                <div
                  id="flowtower-panel-flow"
                  role="tabpanel"
                  aria-labelledby="flowtower-tab-flow"
                  className="space-y-5 p-4"
                >
                  <div className="grid gap-2 px-2 py-2 md:grid-cols-[130px_1fr_170px_1fr_170px_1fr_140px] md:items-center">
                    {[
                      { label: 'Client / SDK', sub: `${kpis.inflight} in-flight`, classes: 'border-[#d6e5ff] bg-[#f6f9ff]' },
                      { label: 'Fluxeme Gateway', sub: 'auth · quota · route', classes: 'border-[#dfe5ed] bg-[#fbfcfe]' },
                      { label: 'Model Router', sub: 'healthy · 3 targets', classes: 'border-[#cfeedd] bg-[#f5fcf8]' },
                      { label: 'Upstream', sub: `${kpis.queued} queued`, classes: 'border-[#f3dfae] bg-[#fffaf0]' },
                    ].map((node, index, array) => (
                      <div key={node.label} className="contents md:contents">
                        <div className={`flex min-h-[86px] flex-col justify-center rounded-xl border px-3 py-4 text-center ${node.classes}`}>
                          <strong className="text-xs text-[#344054]">{node.label}</strong>
                          <span className="mt-1.5 text-[10px] text-[#98a2b3]">{node.sub}</span>
                        </div>
                        {index < array.length - 1 ? (
                          <div className="mx-auto h-6 w-0.5 bg-[#cdd6e1] md:h-0.5 md:w-full md:self-center" />
                        ) : null}
                      </div>
                    ))}
                  </div>

                  <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
                    {FLOW_METRICS.map((metric) => (
                      <div key={metric.label} className="rounded-xl border border-[#edf0f4] bg-white p-3">
                        <small className="text-[10px] text-[#98a2b3]">{metric.label}</small>
                        <strong className="mt-1 block text-sm text-[#182230]">{metric.value}</strong>
                      </div>
                    ))}
                  </div>

                  <div className="border-t border-[#edf0f4] pt-4">
                    <div className="mb-3 flex items-center justify-between gap-2">
                      <div className="text-[11px] font-semibold text-[#475467]">{selectedModel.name} · Channel Flow</div>
                      <div className="text-[10px] text-[#98a2b3]">动态权重路由</div>
                    </div>
                    <svg viewBox="0 0 760 190" preserveAspectRatio="none" className="h-[190px] w-full">
                      <path d="M110 95 C210 95,230 42,330 42" fill="none" stroke="#8ab1f1" strokeWidth="4" />
                      <path d="M110 95 C210 95,230 95,330 95" fill="none" stroke="#73c99d" strokeWidth="5" />
                      <path d="M110 95 C210 95,230 148,330 148" fill="none" stroke="#e1b85e" strokeWidth="2.5" />
                      <path d="M430 42 C535 42,545 65,650 65" fill="none" stroke="#8ab1f1" strokeWidth="4" />
                      <path d="M430 95 C535 95,545 95,650 95" fill="none" stroke="#73c99d" strokeWidth="5" />
                      <path d="M430 148 C535 148,545 125,650 125" fill="none" stroke="#e1b85e" strokeWidth="2.5" />
                      <rect x="35" y="65" width="75" height="60" rx="10" fill="#f6f9ff" stroke="#d6e5ff" />
                      <text x="72" y="91" textAnchor="middle" fontSize="11" fill="#344054" fontWeight="700">Router</text>
                      <text x="72" y="109" textAnchor="middle" fontSize="9" fill="#98a2b3">191 rps</text>
                      <rect x="330" y="20" width="100" height="44" rx="9" fill="#fff" stroke="#dfe5ed" />
                      <text x="380" y="47" textAnchor="middle" fontSize="10" fill="#475467">channel-a · 44%</text>
                      <rect x="330" y="73" width="100" height="44" rx="9" fill="#fff" stroke="#cfeedd" />
                      <text x="380" y="100" textAnchor="middle" fontSize="10" fill="#475467">channel-b · 39%</text>
                      <rect x="330" y="126" width="100" height="44" rx="9" fill="#fffaf0" stroke="#f3dfae" />
                      <text x="380" y="153" textAnchor="middle" fontSize="10" fill="#475467">channel-c · 17%</text>
                      <rect x="650" y="43" width="80" height="44" rx="9" fill="#f5fcf8" stroke="#cfeedd" />
                      <text x="690" y="69" textAnchor="middle" fontSize="10" fill="#475467">P Cluster</text>
                      <rect x="650" y="103" width="80" height="44" rx="9" fill="#f5fcf8" stroke="#cfeedd" />
                      <text x="690" y="129" textAnchor="middle" fontSize="10" fill="#475467">D Cluster</text>
                    </svg>
                  </div>
                </div>
              ) : null}

              {activeTab === 'endpoint' ? (
                <div
                  id="flowtower-panel-endpoint"
                  role="tabpanel"
                  aria-labelledby="flowtower-tab-endpoint"
                  className="overflow-x-auto p-4"
                >
                  <table className="w-full min-w-[760px] border-collapse">
                    <thead>
                      <tr>
                        <th className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#98a2b3]">渠道 / 端点</th>
                        <th className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#98a2b3]">协议</th>
                        <th className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#98a2b3]">最近探测</th>
                        <th className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#98a2b3]">耗时</th>
                        <th className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#98a2b3]">状态时间线</th>
                      </tr>
                    </thead>
                    <tbody>
                      {visibleEndpointRows.map((row) => (
                        <tr key={row.channel}>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-[10.5px] text-[#475467]">
                            <div className="font-semibold">{row.channel}</div>
                            <div className="font-mono text-[10px] text-[#98a2b3]">{row.path}</div>
                          </td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-[10.5px] text-[#475467]">{row.protocol}</td>
                          <td className={`border-b border-[#eef1f5] px-2 py-2.5 text-[10.5px] ${endpointStatusClasses(row.statusTone)}`}>
                            {row.statusLabel}
                          </td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-[10.5px] text-[#475467]">{row.latency}</td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5">
                            <div className="flex gap-1">
                              {row.timeline.map((state, index) => (
                                <i key={`${row.channel}-${index}`} className={`h-2.5 w-2.5 rounded-sm ${timelineSquareClasses(state)}`} />
                              ))}
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : null}

              {activeTab === 'compare' ? (
                <div
                  id="flowtower-panel-compare"
                  role="tabpanel"
                  aria-labelledby="flowtower-tab-compare"
                  className="overflow-x-auto p-4"
                >
                  <table className="w-full min-w-[680px] border-collapse">
                    <thead>
                      <tr>
                        {['模型', 'RPS', '成功率', 'P95 延迟', 'TTFT P95', '错误'].map((header) => (
                          <th key={header} className="border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] font-semibold text-[#98a2b3] first:text-left">
                            {header}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {COMPARE_ROWS.map((row) => (
                        <tr key={row.model}>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-left text-[10.5px] font-semibold text-[#475467]">{row.model}</td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] text-[#475467]">{row.rps}</td>
                          <td className={`border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] ${row.model === 'Claude-Sonnet-4.6' ? 'text-[#c98616]' : 'text-[#475467]'}`}>
                            {row.successRate}
                          </td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] text-[#475467]">{row.latency}</td>
                          <td className="border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] text-[#475467]">{row.ttft}</td>
                          <td className={`border-b border-[#eef1f5] px-2 py-2.5 text-right text-[10.5px] ${row.model === 'Claude-Sonnet-4.6' ? 'text-[#d14848]' : 'text-[#475467]'}`}>
                            {row.errors}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : null}
            </article>

            <Panel title="模型检查器" subtitle="快速定位当前选中模型" className="min-h-[580px]">
              <div className="rounded-xl border border-[#dfe7f3] bg-[#f8fbff] p-4">
                <div className="flex items-center justify-between gap-2">
                  <strong className="text-sm text-[#182230]">{selectedModel.name}</strong>
                  <span className={`rounded-md px-2 py-1 text-[10px] font-semibold ${
                    selectedModel.status === 'warn'
                      ? 'bg-[#fff5dc] text-[#ad7411]'
                      : selectedModel.status === 'offline'
                        ? 'bg-[#f1f3f6] text-[#667085]'
                        : 'bg-[#eaf8f1] text-[#15865a]'
                  }`}>
                    {selectedModel.status === 'warn'
                      ? 'Warning'
                      : selectedModel.status === 'offline'
                        ? 'Offline'
                        : 'Healthy'}
                  </span>
                </div>
                <div className="mt-4 grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
                  <span className="text-[#98a2b3]">当前 RPS</span><span className="text-right text-[#475467]">{selectedModel.currentRps}</span>
                  <span className="text-[#98a2b3]">在途请求</span><span className="text-right text-[#475467]">{selectedModel.inflight}</span>
                  <span className="text-[#98a2b3]">排队请求</span><span className="text-right text-[#b37b19]">{selectedModel.queued}</span>
                  <span className="text-[#98a2b3]">成功率</span><span className="text-right text-[#475467]">{selectedModel.successRate}</span>
                  <span className="text-[#98a2b3]">P95 延迟</span><span className="text-right text-[#475467]">{selectedModel.p95Latency}</span>
                  <span className="text-[#98a2b3]">TTFT P95</span><span className="text-right text-[#475467]">{selectedModel.p95Ttft}</span>
                  <span className="text-[#98a2b3]">活跃渠道</span><span className="text-right text-[#475467]">{selectedModel.activeChannels}</span>
                </div>
              </div>

              <div className="mt-4 border-t border-border pt-4">
                <div className="mb-3 text-[11px] font-semibold text-[#475467]">异常事件 · 最近 30 分钟</div>
                <div className="space-y-2">
                  {INCIDENTS.map((incident) => (
                    <div key={`${incident.title}-${incident.time}`} className="rounded-xl border border-[#edf0f4] bg-white p-3">
                      <div className="flex items-center justify-between gap-2">
                        <div className="text-[10.5px] font-semibold text-[#344054]">{incident.title}</div>
                        <span className={`rounded-md px-1.5 py-0.5 text-[9px] font-bold ${
                          incident.severity === 'HIGH'
                            ? 'bg-[#ffecec] text-[#c83333]'
                            : 'bg-[#fff5dc] text-[#ad7411]'
                        }`}>
                          {incident.severity}
                        </span>
                      </div>
                      <p className="mt-1.5 text-[9.8px] leading-6 text-[#7a8697]">{incident.description}</p>
                      <time className="mt-2 block text-[9px] text-[#b0b8c4]">{incident.time} · {incident.model}</time>
                    </div>
                  ))}
                </div>
              </div>
            </Panel>
          </section>

          <div className="text-right text-[10px] text-[#98a2b3]">Mock data · Fluxeme Model Observability Console</div>
        </div>
      </section>
    </div>
  );
}
