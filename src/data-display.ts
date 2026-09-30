export function updateTime(
  timestamp: number | null | undefined,
  full = false,
  timeZone?: string,
): string {
  if (!timestamp) return "尚未更新";
  return new Intl.DateTimeFormat("zh-CN", {
    ...(timeZone ? { timeZone } : {}),
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    ...(full ? { month: "2-digit", day: "2-digit", second: "2-digit" } : {}),
  }).format(timestamp);
}
