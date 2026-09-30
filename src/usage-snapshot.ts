import type { UsageResult } from "./types";

/** 只保留当前记账月的快照；取得快照之后的新日期是未知，而不是零。 */
export function currentUsageSnapshot(
  snapshot: UsageResult | null,
  today: string,
): UsageResult | null {
  const fetchedDate = snapshot?.accountingDate;
  if (
    !snapshot ||
    !fetchedDate ||
    fetchedDate.slice(0, 7) !== today.slice(0, 7)
  )
    return null;
  if (fetchedDate >= today) return snapshot;
  const now = Date.parse(`${today}T00:00:00Z`);
  const unknown = Array.from({ length: 7 }, (_, i) =>
    new Date(now - i * 86400_000).toISOString().slice(0, 10),
  ).filter((date) => date > fetchedDate);
  return {
    ...snapshot,
    // 平台可能提前返回本月未来日期的零占位，不能将其当作后来已取得的数据。
    days: snapshot.days.filter((day) => day.date <= fetchedDate),
    unavailableDates: [
      ...new Set([...(snapshot.unavailableDates ?? []), ...unknown]),
    ],
  };
}
