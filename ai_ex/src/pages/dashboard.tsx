import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { TrendingUp, TrendingDown, Download, CreditCard } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { cn } from '@/lib/utils'
import {
  statCards,
  usageTrend,
  modelUsageShare,
  latencyByHour,
  billingSummary,
} from '@/lib/mock-data'

function ChartTooltip({ active, payload, label }: any) {
  if (!active || !payload?.length) return null
  return (
    <div className="rounded-lg border border-border bg-popover px-3 py-2 text-xs shadow-md">
      {label && <p className="mb-1 font-medium text-popover-foreground">{label}</p>}
      {payload.map((entry: any, i: number) => (
        <div key={i} className="flex items-center gap-2 text-muted-foreground">
          <span
            className="size-2 rounded-full"
            style={{ background: entry.color || entry.payload?.fill }}
          />
          <span>{entry.name}</span>
          <span className="ml-auto font-mono font-medium text-popover-foreground">
            {typeof entry.value === 'number'
              ? entry.value.toLocaleString('zh-CN')
              : entry.value}
          </span>
        </div>
      ))}
    </div>
  )
}

export default function DashboardPage() {
  const budgetPercent = (billingSummary.currentSpend / billingSummary.budget) * 100

  return (
    <div>
      <PageHeader
        title="仪表盘"
        description="实时监控你的 AI 网关调用量、延迟表现与费用支出。"
        actions={
          <>
            <Button variant="outline">
              <Download className="size-4" />
              导出报表
            </Button>
            <Button>
              <CreditCard className="size-4" />
              管理账单
            </Button>
          </>
        }
      />

      {/* 统计卡片 */}
      <div className="mb-4 grid grid-cols-2 gap-4 lg:grid-cols-4">
        {statCards.map((stat) => (
          <Card key={stat.label}>
            <CardContent className="p-5">
              <p className="text-sm text-muted-foreground">{stat.label}</p>
              <p className="mt-2 text-2xl font-semibold tracking-tight">{stat.value}</p>
              <div
                className={cn(
                  'mt-2 inline-flex items-center gap-1 text-xs font-medium',
                  stat.trend === 'up' ? 'text-success' : 'text-primary',
                )}
              >
                {stat.trend === 'up' ? (
                  <TrendingUp className="size-3.5" />
                ) : (
                  <TrendingDown className="size-3.5" />
                )}
                {stat.change}
                <span className="text-muted-foreground">较上周</span>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* 用量趋势 */}
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>请求量趋势</CardTitle>
            <CardDescription>近 14 天每日 API 调用次数</CardDescription>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={280}>
              <AreaChart data={usageTrend} margin={{ left: -12, right: 8, top: 4 }}>
                <defs>
                  <linearGradient id="reqFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.35} />
                    <stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                <XAxis
                  dataKey="date"
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                />
                <YAxis
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                  tickFormatter={(v) => `${v / 1000}K`}
                />
                <Tooltip content={<ChartTooltip />} />
                <Area
                  type="monotone"
                  dataKey="requests"
                  name="请求数"
                  stroke="var(--chart-1)"
                  strokeWidth={2}
                  fill="url(#reqFill)"
                />
              </AreaChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>

        {/* 模型占比 */}
        <Card>
          <CardHeader>
            <CardTitle>模型调用占比</CardTitle>
            <CardDescription>本月各模型请求分布</CardDescription>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={200}>
              <PieChart>
                <Pie
                  data={modelUsageShare}
                  dataKey="value"
                  nameKey="name"
                  innerRadius={52}
                  outerRadius={80}
                  paddingAngle={2}
                  strokeWidth={0}
                >
                  {modelUsageShare.map((entry) => (
                    <Cell key={entry.name} fill={entry.fill} />
                  ))}
                </Pie>
                <Tooltip content={<ChartTooltip />} />
              </PieChart>
            </ResponsiveContainer>
            <div className="mt-2 space-y-2">
              {modelUsageShare.map((m) => (
                <div key={m.name} className="flex items-center gap-2 text-sm">
                  <span
                    className="size-2.5 rounded-full"
                    style={{ background: m.fill }}
                  />
                  <span className="text-muted-foreground">{m.name}</span>
                  <span className="ml-auto font-medium">{m.value}%</span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* 延迟分布 */}
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>响应延迟分布</CardTitle>
            <CardDescription>按时段的 P50 / P95 延迟(毫秒)</CardDescription>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={260}>
              <BarChart data={latencyByHour} margin={{ left: -12, right: 8, top: 4 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                <XAxis
                  dataKey="hour"
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                  tickFormatter={(v) => `${v}:00`}
                />
                <YAxis
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                />
                <Tooltip content={<ChartTooltip />} cursor={{ fill: 'var(--muted)', opacity: 0.4 }} />
                <Bar dataKey="p50" name="P50" fill="var(--chart-1)" radius={[4, 4, 0, 0]} />
                <Bar dataKey="p95" name="P95" fill="var(--chart-2)" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>

        {/* 账单 */}
        <Card>
          <CardHeader>
            <CardTitle>本月账单</CardTitle>
            <CardDescription>计费周期 07-01 至 07-31</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <div className="flex items-end justify-between">
                <span className="text-3xl font-semibold tracking-tight">
                  ${billingSummary.currentSpend.toLocaleString('zh-CN')}
                </span>
                <span className="text-sm text-muted-foreground">
                  / ${billingSummary.budget.toLocaleString('zh-CN')}
                </span>
              </div>
              <Progress value={budgetPercent} className="mt-3" />
              <p className="mt-2 text-xs text-muted-foreground">
                已使用预算 {budgetPercent.toFixed(0)}%,预计月末支出 $
                {billingSummary.projected.toLocaleString('zh-CN')}
              </p>
            </div>

            <div className="space-y-2 border-t border-border pt-4">
              <p className="text-sm font-medium">历史账单</p>
              {billingSummary.invoices.map((inv) => (
                <div key={inv.id} className="flex items-center justify-between text-sm">
                  <div>
                    <p className="font-medium">{inv.period}</p>
                    <p className="text-xs text-muted-foreground">{inv.id}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="font-mono">${inv.amount.toLocaleString('zh-CN')}</span>
                    <Badge variant="success">已支付</Badge>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
