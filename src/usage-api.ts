import { invoke } from "@tauri-apps/api/core";
import { addDays, previousMonth } from "./format";
import type { UsageResult } from "./types";

export const fetchMonthUsage = (month: number, year: number) => {
  return invoke<UsageResult>("fetch_usage", { month, year });
};
export const fetchCurrentUsage = async () => {
  const now = new Date();
  const current = await fetchMonthUsage(now.getMonth() + 1, now.getFullYear());
  const needsPreviousMonth = addDays(now, -6).getMonth() !== now.getMonth();
  if (!needsPreviousMonth) {
    return current;
  }
  try {
    const previous = previousMonth(now);
    const previousUsage = await fetchMonthUsage(previous.month, previous.year);
    return {
      ...current,
      days: [...previousUsage.days, ...current.days],
    };
  } catch {
    return current;
  }
};

export const refreshOptions = [
  { label: "1 分钟", value: 60 },
  { label: "5 分钟", value: 300 },
  { label: "30 分钟", value: 1800 },
  { label: "1 小时", value: 3600 },
];
