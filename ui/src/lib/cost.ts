export function calculateCost(
  promptTokens: number,
  completionTokens: number,
  pricing?: { prompt_price: number; completion_price: number },
): number {
  if (!pricing) return 0;
  return (promptTokens / 1000) * pricing.prompt_price + (completionTokens / 1000) * pricing.completion_price;
}

export function formatCost(
  promptTokens: number,
  completionTokens: number,
  pricing: { prompt_price: number; completion_price: number } | undefined,
  currency: 'usd' | 'cny',
  rate: number,
): string {
  const usd = calculateCost(promptTokens, completionTokens, pricing);
  if (usd === 0) return '—';
  const value = currency === 'cny' ? usd * rate : usd;
  const symbol = currency === 'cny' ? '¥' : '$';
  return `${symbol}${value.toFixed(6)}`;
}

/** Use stored pricing from the usage record if available, falling back to model lookup. */
export function getRecordPricing(
  r: { prompt_price?: number; completion_price?: number; model?: string },
  modelPricing: Record<string, { prompt_price: number; completion_price: number } | undefined>,
): { prompt_price: number; completion_price: number } | undefined {
  if ((r.prompt_price ?? 0) > 0 || (r.completion_price ?? 0) > 0) {
    return { prompt_price: r.prompt_price ?? 0, completion_price: r.completion_price ?? 0 };
  }
  return r.model ? (modelPricing[r.model] ?? undefined) : undefined;
}
