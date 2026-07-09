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
  return `${symbol}${value.toFixed(4)}`;
}
