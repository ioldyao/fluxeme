export type RoutingLineState = 'healthy-zero' | 'traffic' | 'unroutable' | 'selected' | 'retry';

/** Connector width scaled by absolute request count (sqrt), bounded 1–6px.
 *  Small counts stay readable; huge counts do not blow out the diagram. */
export function getTrafficWidth(count: number, maxCount: number): number {
  if (count <= 0 || maxCount <= 0) return 1;
  const scaled = Math.sqrt(count) / Math.sqrt(maxCount);
  return 1 + scaled * 5;
}

/** Pure visual state of a connector, derived only from presentation inputs.
 *  `routeEligible` is backend truth (topology routing_available) — this helper
 *  never computes eligibility itself. */
export function getEdgeVisualState({
  count,
  routeEligible,
  selected = false,
  retry = false,
}: {
  count: number;
  routeEligible: boolean;
  selected?: boolean;
  retry?: boolean;
}): RoutingLineState {
  if (retry) return 'retry';
  if (selected) return 'selected';
  if (!routeEligible) return 'unroutable';
  return count > 0 ? 'traffic' : 'healthy-zero';
}

export function routingLineStyle(state: RoutingLineState): {
  stroke: string;
  opacity: number;
  dasharray?: string;
} {
  switch (state) {
    case 'unroutable':
      return { stroke: 'var(--muted-foreground)', opacity: 0.55, dasharray: '6 5' };
    case 'selected':
      return { stroke: 'var(--chart-1)', opacity: 1 };
    case 'retry':
      return { stroke: 'var(--destructive)', opacity: 0.95, dasharray: '8 4' };
    case 'traffic':
      return { stroke: 'var(--chart-1)', opacity: 0.72 };
    case 'healthy-zero':
    default:
      return { stroke: 'var(--border)', opacity: 0.32 };
  }
}

/** Relative request-heat bucket (low/mid/high) of a node vs its siblings.
 *  Represents request share only — never CPU/concurrency/capacity saturation. */
export function getTrafficIntensity(count: number, siblingCounts: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblingCounts);
  const ratio = count / max;
  if (ratio >= 0.66) return 'high';
  if (ratio >= 0.33) return 'mid';
  return 'low';
}
