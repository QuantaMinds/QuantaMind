export const DAY_MS = 24 * 60 * 60 * 1000;

/// True when a background update check is due: never checked, an
/// unparseable timestamp, or more than 24h since the last check.
export function shouldCheck(last: string | null | undefined, nowMs: number): boolean {
  if (!last) return true;
  const t = Date.parse(last);
  if (Number.isNaN(t)) return true;
  return nowMs - t >= DAY_MS;
}
