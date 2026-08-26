import type {
  UsageAnalyticsBucket,
  UsageAnalyticsModel,
} from '../api/usage';

export type UsageChartBucket = UsageAnalyticsBucket & {
  success_rate: number | null;
};

export type UsageChartModel = UsageAnalyticsModel & {
  is_other?: boolean;
};

export const USAGE_CHART_COLORS = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-3)',
  'var(--chart-4)',
  'var(--chart-5)',
];

export function toUsageChartBuckets(buckets: UsageAnalyticsBucket[], days?: number): UsageChartBucket[] {
  if (!days || buckets.length === 0) {
    return buckets.map((bucket) => ({
      ...bucket,
      success_rate: bucket.requests > 0 ? (bucket.succeeded / bucket.requests) * 100 : null,
    }));
  }

  const byDate = new Map(buckets.map((bucket) => [bucket.date, bucket]));
  const latest = new Date(`${buckets[buckets.length - 1].date}T00:00:00Z`);
  return Array.from({ length: days }, (_, index) => {
    const date = new Date(latest);
    date.setUTCDate(latest.getUTCDate() - days + index + 1);
    const dateKey = date.toISOString().slice(0, 10);
    const bucket = byDate.get(dateKey) ?? {
      date: dateKey,
      requests: 0,
      succeeded: 0,
      failed: 0,
      input_tokens: 0,
      cache_read_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      latency_ms: 0,
    };
    return {
      ...bucket,
      success_rate: bucket.requests > 0 ? (bucket.succeeded / bucket.requests) * 100 : null,
    };
  });
}

export function topUsageModels(models: UsageAnalyticsModel[], limit = 8, otherLabel = 'Other'): UsageChartModel[] {
  const sorted = [...models].sort((a, b) => b.requests - a.requests);
  const visible = sorted.slice(0, limit);
  const other = sorted.slice(limit).reduce<UsageChartModel | null>((sum, model) => {
    if (!sum) {
      return {
        model: otherLabel,
        requests: 0,
        succeeded: 0,
        failed: 0,
        input_tokens: 0,
        cache_read_tokens: 0,
        output_tokens: 0,
        is_other: true,
      };
    }
    return {
      ...sum,
      requests: sum.requests + model.requests,
      succeeded: sum.succeeded + model.succeeded,
      failed: sum.failed + model.failed,
      input_tokens: sum.input_tokens + model.input_tokens,
      cache_read_tokens: sum.cache_read_tokens + model.cache_read_tokens,
      output_tokens: sum.output_tokens + model.output_tokens,
    };
  }, null);

  return other && other.requests > 0 ? [...visible, other] : visible;
}

export function formatCompactNumber(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toLocaleString();
}
