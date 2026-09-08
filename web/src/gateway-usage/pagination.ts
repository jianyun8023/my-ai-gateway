import { type UsageEventViewModel } from '@/gateway-usage';

export const appendStableEventPage = (
  current: readonly UsageEventViewModel[],
  incoming: readonly UsageEventViewModel[],
): UsageEventViewModel[] => {
  const seen = new Set(current.map((event) => `${event.id}:${event.createdAt}`));
  return [...current, ...incoming.filter((event) => {
    const key = `${event.id}:${event.createdAt}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  })];
};
