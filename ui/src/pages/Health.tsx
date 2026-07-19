import { useTranslation } from 'react-i18next';
import { useRoutingHealth } from '@/api/health';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { RefreshCw, Activity } from 'lucide-react';
import { cn } from '@/lib/utils';

export default function HealthPage() {
  const { t } = useTranslation();
  const { data, isLoading, isError, refetch } = useRoutingHealth();
  const summary = data?.summary;

  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

  return (
    <div className="space-y-6 animate-fade-in">
      {/* ── Header ── */}
      <div className="flex items-end justify-between">
        <div>
          <div className="text-xs font-mono tracking-wider text-primary mb-1.5 flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
            实时监控
          </div>
          <h1 className="text-2xl font-bold tracking-tight">模型路由 / 负载均衡</h1>
          <p className="text-sm text-muted-foreground mt-1">按模型分组展示渠道绑定、请求分配占比与成功率</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
        </Button>
      </div>

      {/* ── Summary Cards ── */}
      <div className="grid grid-cols-4 gap-3">
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">总请求数 / 24h</p>
            <p className="text-xl font-semibold mt-1">{summary?.total_requests_24h?.toLocaleString() ?? '-'}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">整体成功率</p>
            <p className={cn('text-xl font-semibold mt-1', (summary?.overall_success_rate ?? 1) > 0.9 ? 'text-green-600' : 'text-yellow-500')}>
              {summary ? pct(summary.overall_success_rate) : '-'}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">活跃渠道数</p>
            <p className="text-xl font-semibold mt-1 text-green-600">{summary?.active_channels ?? '-'}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">熔断中渠道</p>
            <p className={cn('text-xl font-semibold mt-1', (summary?.broken_channels ?? 0) > 0 ? 'text-yellow-500' : 'text-muted-foreground')}>
              {summary?.broken_channels ?? '-'}
            </p>
          </CardContent>
        </Card>
      </div>

      {/* ── Content ── */}
      {isLoading ? (
        <div className="p-12 text-center text-sm text-muted-foreground">加载中...</div>
      ) : isError ? (
        <div className="p-12 text-center">
          <p className="text-sm text-destructive mb-3">加载失败</p>
          <Button variant="outline" onClick={() => refetch()}>重试</Button>
        </div>
      ) : !data || data.models.length === 0 ? (
        <div className="p-16 text-center text-muted-foreground">
          <Activity className="w-10 h-10 mx-auto mb-3 opacity-50" />
          <div className="text-sm">暂无路由数据</div>
        </div>
      ) : (
        <div className="space-y-4">
          {data.models.map((model) => {
            const totalReq = model.channels.reduce((s, c) => s + c.requests, 0) || 1;
            return (
              <Card key={model.id}>
                {/* Model Header */}
                <div className="flex items-center justify-between px-5 py-3.5 border-b bg-muted/20">
                  <div className="flex items-baseline gap-2.5">
                    <span className="font-semibold text-foreground">{model.name}</span>
                    <span className="text-xs font-mono text-muted-foreground bg-muted px-2 py-0.5 rounded">{model.model_pattern}</span>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    共 <b className="text-foreground">{model.total_requests.toLocaleString()}</b> 次请求
                  </span>
                </div>

                {/* Channel Table */}
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="text-left text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">渠道</th>
                        <th className="text-left text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">请求占比</th>
                        <th className="text-right text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">请求数</th>
                        <th className="text-right text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">成功率</th>
                        <th className="text-right text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">P95 延迟</th>
                        <th className="text-left text-[11px] font-medium uppercase tracking-wider px-5 py-2.5">状态</th>
                      </tr>
                    </thead>
                    <tbody>
                      {model.channels.map((ch) => {
                        const pctVal = totalReq > 0 ? ch.requests / totalReq : 0;
                        const barColor = ch.circuit_ok ? 'bg-primary' : 'bg-destructive';
                        const successRate = ch.success_rate;
                        const rateBadge = successRate > 0.95 ? 'ok' : successRate > 0.8 ? 'warn' : 'bad';
                        return (
                          <tr key={ch.channel_id} className="border-b last:border-0 hover:bg-muted/30">
                            <td className="px-5 py-3">
                              <div className="flex items-center gap-2">
                                <span className="text-[10px] font-semibold text-muted-foreground bg-muted px-1.5 py-0.5 rounded">
                                  P{ch.priority}
                                </span>
                                <span className="font-mono text-sm">{ch.channel_name || ch.channel_id}</span>
                              </div>
                            </td>
                            <td className="px-5 py-3">
                              <div className="flex items-center gap-2 max-w-[200px]">
                                <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
                                  <div className={cn('h-full rounded-full', barColor)} style={{ width: `${Math.max(pctVal * 100, 2)}%` }} />
                                </div>
                                <span className="text-xs text-muted-foreground min-w-[36px] text-right">{(pctVal * 100).toFixed(0)}%</span>
                              </div>
                            </td>
                            <td className="px-5 py-3 text-right font-mono text-sm">{ch.requests.toLocaleString()}</td>
                            <td className="px-5 py-3 text-right">
                              <span className={cn(
                                'inline-flex items-center gap-1.5 text-xs font-medium px-2 py-1 rounded',
                                rateBadge === 'ok' ? 'text-green-600 bg-green-500/10' :
                                rateBadge === 'warn' ? 'text-yellow-600 bg-yellow-500/10' :
                                'text-destructive bg-destructive/10'
                              )}>
                                <span className="w-1.5 h-1.5 rounded-full bg-current" />
                                {pct(successRate)}
                              </span>
                            </td>
                            <td className="px-5 py-3 text-right font-mono text-sm text-muted-foreground">
                              {ch.p95_latency_ms > 0 ? `${ch.p95_latency_ms.toFixed(0)}ms` : '-'}
                            </td>
                            <td className="px-5 py-3">
                              {!ch.circuit_enabled ? (
                                <span className="text-xs text-muted-foreground">已禁用</span>
                              ) : ch.circuit_ok ? (
                                <span className="text-xs text-green-600 font-medium">健康</span>
                              ) : (
                                <span className="text-xs text-destructive font-medium">熔断中</span>
                              )}
                              {ch.endpoints.length > 1 && (
                                <div className="flex items-center gap-1 mt-1.5">
                                  {ch.endpoints.map((ep) => (
                                    <span
                                      key={ep.endpoint_id}
                                      className={cn(
                                        'inline-block w-2 h-2 rounded-full',
                                        !ep.enabled ? 'bg-muted-foreground/30' :
                                        ep.available ? 'bg-green-500' : 'bg-destructive'
                                      )}
                                      title={`端点 #${ep.endpoint_id}: ${ep.enabled ? (ep.available ? '正常' : '熔断') : '已禁用'}`}
                                    />
                                  ))}
                                </div>
                              )}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}
