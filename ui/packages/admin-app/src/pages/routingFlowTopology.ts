import type { Channel, Model } from '@fluxeme/shared/src/types';

export interface TopoEndpoint { key: string; matchId: number | null; label: string; url: string }
export interface TopoChannel { id: string; name: string; endpoints: TopoEndpoint[] }
export interface TopoModel { model: string; pattern: string; channels: TopoChannel[] }

/** Stable binding identity for UI metadata. Endpoint IDs are only unique within
 * a model/channel binding, so never use endpoint_id alone as a key. */
export function bindingKey(channelId: string, endpointId: number | null): string {
  return `${channelId}:id:${endpointId ?? 'unknown'}`;
}

export const keyFor = (...parts: (string | number)[]) => parts.join('>');

export function matchPattern(text: string, pattern: string): boolean {
  if (pattern === '*') return true;
  if (!pattern.includes('*')) return text === pattern;
  const parts = pattern.split('*');
  if (parts.length === 2) {
    const [pfx, sfx] = parts;
    return (pfx === '' || text.startsWith(pfx)) && (sfx === '' || text.endsWith(sfx));
  }
  if (parts.length === 3) {
    const [pfx, mid, sfx] = parts;
    return text.startsWith(pfx) && text.includes(mid) && text.endsWith(sfx);
  }
  return pattern === text;
}

export function resolveEvent(
  topology: TopoModel[],
  ev: { model: string; channel_id: string; endpoint_id?: number | null },
): { modelName: string; channelId: string; endpointKey: string | null } | null {
  const m = topology.find((t) => t.model === ev.model) || topology.find((t) => matchPattern(ev.model, t.pattern));
  if (!m) return null;
  const ch = m.channels.find((c) => c.id === ev.channel_id);
  if (!ch) return null;
  let ep: TopoEndpoint | undefined;
  if (ev.endpoint_id != null) ep = ch.endpoints.find((e) => e.matchId === ev.endpoint_id);
  if (!ep) ep = ch.endpoints[0];
  return { modelName: m.model, channelId: ch.id, endpointKey: ep ? ep.key : null };
}

/** Build the display topology while preserving every model→channel→endpoint
 * binding. A physical endpoint may occur in more than one binding. */
export function buildTopology(models: Model[], channels: Channel[]): TopoModel[] {
  const channelMap = new Map(channels.map((c) => [c.id, c]));
  const merged = new Map<string, TopoModel>();
  for (const m of models) {
    const key = m.name;
    let entry = merged.get(key);
    if (!entry) { entry = { model: m.name, pattern: m.name, channels: [] }; merged.set(key, entry); }
    for (const mc of m.channels) {
      const ch = channelMap.get(mc.channel_id);
      if (!ch || entry.channels.some((ec) => ec.id === ch.id)) continue;
      entry.channels.push({
        id: ch.id, name: ch.name || ch.id,
        endpoints: ch.endpoints.map((e, i) => ({
          key: e.id != null ? `id:${e.id}` : `${ch.id}#${i}`,
          matchId: e.id ?? null, label: `${i + 1}`, url: e.url,
        })),
      });
    }
  }
  return [...merged.values()];
}
