import { DashboardGatewayHealthStrip } from './DashboardGatewayHealthStrip';
import { DashboardRequestFlowCard } from './DashboardRequestFlowCard';
import { DashboardRoutingPerformanceCard } from './DashboardRoutingPerformanceCard';
import { DashboardRiskAlertsCard } from './DashboardRiskAlertsCard';
import { DashboardRequestLogsCard } from './DashboardRequestLogsCard';
import type { UsageRecord } from '@/types';

type RoutingPerformanceRow = {
  channelId: string;
  channelName: string;
  routeRole: string;
  share: number;
  requests: number;
  avgLatency: number;
};

type RiskAlert = {
  id: string;
  title: string;
  description: string;
  severity: 'warn' | 'info';
};

type DashboardAdminSectionProps = {
  availability: number;
  modelCount: number;
  apiKeyCount: number;
  channelCount: number;
  requests24h: number;
  totalTokens24h: number;
  avgLatencyMs24h: number;
  cost24hLabel: string;
  routingRows: RoutingPerformanceRow[];
  isRoutingLoading: boolean;
  isRoutingError: boolean;
  alerts: RiskAlert[];
  requestLogs: UsageRecord[];
  isLogsLoading: boolean;
  isLogsError: boolean;
};

export function DashboardAdminSection({
  availability,
  modelCount,
  apiKeyCount,
  channelCount,
  requests24h,
  totalTokens24h,
  avgLatencyMs24h,
  cost24hLabel,
  routingRows,
  isRoutingLoading,
  isRoutingError,
  alerts,
  requestLogs,
  isLogsLoading,
  isLogsError,
}: DashboardAdminSectionProps) {
  return (
    <section className="space-y-6">
      <DashboardGatewayHealthStrip
        availability={availability}
        modelCount={modelCount}
        apiKeyCount={apiKeyCount}
        channelCount={channelCount}
        requests24h={requests24h}
        totalTokens24h={totalTokens24h}
        avgLatencyMs24h={avgLatencyMs24h}
        cost24hLabel={cost24hLabel}
      />

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.7fr)_minmax(320px,0.9fr)]">
        <div className="space-y-4">
          <DashboardRequestFlowCard
            requests24h={requests24h}
            successRate24h={availability}
          />
          <DashboardRoutingPerformanceCard
            rows={routingRows}
            isLoading={isRoutingLoading}
            isError={isRoutingError}
          />
        </div>

        <div className="space-y-4">
          <DashboardRiskAlertsCard alerts={alerts} />
        </div>
      </div>

      <DashboardRequestLogsCard
        records={requestLogs}
        isLoading={isLogsLoading}
        isError={isLogsError}
      />
    </section>
  );
}
