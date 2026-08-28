import { useMemo } from 'react';
import { Package, RefreshCw } from 'lucide-react';
import { useTokenPackageGrants, type TokenPackageGrant } from '@fluxeme/shared/src/api/wallet';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';

const remainingUnits = (grant: TokenPackageGrant): number => grant.status === 'active'
  ? Math.max(0, grant.total_units - grant.consumed_units - grant.reserved_units)
  : 0;
const statusLabel = (status: string): string => ({ active: '生效中', revoked: '已撤回', exhausted: '已耗尽', expired: '已失效' }[status] ?? status);
const formatDate = (value: string | null): string => value ? new Date(value).toLocaleString('zh-CN') : '—';
const formatUnits = (value: number): string => `${Math.max(0, value).toLocaleString()} units`;
const packageLabel = (grant: TokenPackageGrant): string => grant.plan_name || grant.plan_code || grant.plan_id;

function ProgressBar({ grant }: { grant: TokenPackageGrant }) {
  const remaining = remainingUnits(grant);
  const percent = grant.total_units > 0 ? Math.min(100, remaining / grant.total_units * 100) : 0;
  return (
    <div className="min-w-[220px] space-y-1.5">
      <div className="flex justify-between gap-3 text-xs">
        <span className="whitespace-nowrap font-medium tabular-nums">{formatUnits(remaining)} / {formatUnits(grant.total_units)}</span>
        <span className="text-muted-foreground tabular-nums">{percent.toFixed(1)}%</span>
      </div>
      <div
        className="h-2 overflow-hidden rounded-full bg-muted"
        role="progressbar"
        aria-label={`${packageLabel(grant)} 资源包余量`}
        aria-valuemin={0}
        aria-valuemax={grant.total_units}
        aria-valuenow={remaining}
      >
        <div className={`h-full rounded-full ${grant.status === 'active' ? 'bg-brand' : 'bg-muted-foreground/40'}`} style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

export default function TokenPackages() {
  const { data: grants, isLoading, isError, refetch } = useTokenPackageGrants();
  const summary = useMemo(() => {
    const items = grants ?? [];
    return {
      total: items.length,
      active: items.filter((grant) => grant.status === 'active').length,
      units: items.reduce((sum, grant) => sum + grant.total_units, 0),
      remaining: items.reduce((sum, grant) => sum + remainingUnits(grant), 0),
    };
  }, [grants]);

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title="资源包管理"
        description="查看当前账号已获得的 Token 资源包、使用情况和有效期。"
        actions={
          <Button variant="outline" size="sm" onClick={() => void refetch()} disabled={isLoading}>
            <RefreshCw className={`mr-1 size-4 ${isLoading ? 'animate-spin' : ''}`} />
            刷新
          </Button>
        }
      />

      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        {[
          ['资源包数量', summary.total],
          ['生效中', summary.active],
          ['总量', formatUnits(summary.units)],
          ['可用余量', formatUnits(summary.remaining)],
        ].map(([label, value]) => (
          <Card key={String(label)}>
            <CardContent className="p-4">
              <div className="text-[11px] text-muted-foreground">{label}</div>
              <div className="mt-2 text-lg font-bold">{value}</div>
            </CardContent>
          </Card>
        ))}
      </div>

      {isLoading ? (
        <Card>
          <CardContent className="p-10 text-center text-sm text-muted-foreground">正在加载资源包…</CardContent>
        </Card>
      ) : isError ? (
        <Card>
          <CardContent className="space-y-3 p-10 text-center text-sm text-muted-foreground">
            <div>资源包加载失败</div>
            <Button variant="outline" size="sm" onClick={() => void refetch()}>重试</Button>
          </CardContent>
        </Card>
      ) : grants && grants.length > 0 ? (
        <Card>
          <CardContent className="p-0">
            <div className="flex items-center gap-2 border-b px-5 py-3 font-semibold">
              <Package className="size-4" />
              我的资源包
            </div>
            <div className="overflow-x-auto">
              <table className="w-full min-w-[1180px] text-sm">
                <caption className="sr-only">我的资源包</caption>
                <thead>
                  <tr className="border-b bg-muted/20 text-xs text-muted-foreground">
                    <th scope="col" className="px-4 py-3 text-left font-medium">资源包</th>
                    <th scope="col" className="px-4 py-3 text-left font-medium">状态</th>
                    <th scope="col" className="px-4 py-3 text-left font-medium">可用余量</th>
                    <th scope="col" className="px-4 py-3 text-left font-medium">计量模式</th>
                    <th scope="col" className="px-4 py-3 text-right font-medium">已消耗</th>
                    <th scope="col" className="px-4 py-3 text-right font-medium">已预留</th>
                    <th scope="col" className="px-4 py-3 text-left font-medium">发放时间</th>
                    <th scope="col" className="px-4 py-3 text-left font-medium">失效时间</th>
                  </tr>
                </thead>
                <tbody>
                  {grants.map((grant) => (
                    <tr key={grant.id} className="border-b last:border-0 hover:bg-muted/30">
                      <th scope="row" className="max-w-[220px] px-4 py-3 text-left align-middle font-medium">
                        <div className="truncate" title={packageLabel(grant)}>{packageLabel(grant)}</div>
                        <div className="mt-1 truncate font-mono text-[11px] font-normal text-muted-foreground" title={grant.plan_code || grant.plan_id}>
                          {grant.plan_code || grant.plan_id}
                        </div>
                      </th>
                      <td className="whitespace-nowrap px-4 py-3 align-middle">
                        <span className={grant.status === 'active' ? 'text-emerald-600' : 'text-muted-foreground'}>
                          {statusLabel(grant.status)}
                        </span>
                      </td>
                      <td className="px-4 py-3 align-middle"><ProgressBar grant={grant} /></td>
                      <td className="whitespace-nowrap px-4 py-3 align-middle text-xs">
                        {grant.accounting_mode === 'raw_tokens' ? 'Raw tokens' : 'Standardized credits'}
                        <div className="mt-1 text-muted-foreground">Token 资源包</div>
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 text-right align-middle font-mono text-xs tabular-nums">
                        {formatUnits(grant.consumed_units)}
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 text-right align-middle font-mono text-xs tabular-nums">
                        {formatUnits(grant.reserved_units)}
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 align-middle text-xs text-muted-foreground">
                        {formatDate(grant.created_at)}
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 align-middle text-xs text-muted-foreground">
                        {formatDate(grant.expires_at)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </CardContent>
        </Card>
      ) : (
        <Card>
          <CardContent className="p-10 text-center text-sm text-muted-foreground">当前账号暂无资源包</CardContent>
        </Card>
      )}
    </div>
  );
}
