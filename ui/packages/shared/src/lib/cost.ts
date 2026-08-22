import { useCurrency } from '../store/currency';

export function calculateCost(
  promptTokens: number,
  completionTokens: number,
  cacheHitTokens: number,
  cacheWriteTokens: number,
  pricing?: { prompt_price: number; completion_price: number; cache_read_price: number; cache_write_price?: number },
): number {
  if (!pricing) return 0;
  return (promptTokens / 1000000) * pricing.prompt_price
    + (completionTokens / 1000000) * pricing.completion_price
    + (cacheHitTokens / 1000000) * pricing.cache_read_price
    + (cacheWriteTokens / 1000000) * (pricing.cache_write_price ?? 0);
}

export function formatCost(
  promptTokens: number,
  completionTokens: number,
  cacheHitTokens: number,
  cacheWriteTokens: number,
  pricing: { prompt_price: number; completion_price: number; cache_read_price: number; cache_write_price?: number } | undefined,
): string {
  const value = calculateCost(promptTokens, completionTokens, cacheHitTokens, cacheWriteTokens, pricing);
  if (value === 0) return '—';
  const symbol = useCurrency.getState().currency === 'cny' ? '¥' : '$';
  return `${symbol}${value.toFixed(6)}`;
}

/** Use only the pricing snapshot stored with the usage record — the price
 *  billed when the request completed. Never falls back to current model
 *  pricing, so historical fees don't change when model pricing is edited
 *  later. A record with no stored pricing (all zero) is a zero-fee request. */
export function getRecordPricing(
  r: { prompt_price?: number; completion_price?: number; cache_read_price?: number; cache_write_price?: number },
): { prompt_price: number; completion_price: number; cache_read_price: number; cache_write_price: number } | undefined {
  if ((r.prompt_price ?? 0) > 0 || (r.completion_price ?? 0) > 0 || (r.cache_read_price ?? 0) > 0 || (r.cache_write_price ?? 0) > 0) {
    return { prompt_price: r.prompt_price ?? 0, completion_price: r.completion_price ?? 0, cache_read_price: r.cache_read_price ?? 0, cache_write_price: r.cache_write_price ?? 0 };
  }
  return undefined;
}
