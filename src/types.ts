import type { UsageDay } from "./format";

export type ViewName = "dashboard" | "settings" | "detail";
export type ModelName = "flash" | "pro" | "other";
export type AppConfig = {
  configWarnings?: string[];
  apiKeyConfigured: boolean;
  apiKeyPreview: string | null;
  usageTokenConfigured: boolean;
  refreshIntervalSeconds: number;
  autoRefreshEnabled: boolean;
  autostart: boolean;
  configPath: string;
};
export type BalanceData = {
  isAvailable: boolean;
  currency: string;
  totalBalance: string;
  grantedBalance: string;
  toppedUpBalance: string;
};
export type SavedApiKey = { config: AppConfig; balance: BalanceData };
// 通用的异步加载状态，余额与用量共用。
export type LoadState = "loading" | "ok" | "error" | "nokey";
export type UsageModel = {
  key: string;
  name: string;
  totalTokens: number;
  requestCount: number;
  cacheHitTokens: number;
  cacheMissTokens: number;
  responseTokens: number;
  otherTokens: number;
  cost: number;
};
export type UsageResult = {
  warnings?: string[];
  unavailableDates?: string[];
  models: UsageModel[];
  days: UsageDay[];
  monthCost: number;
};
