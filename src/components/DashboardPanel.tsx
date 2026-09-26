import React from "react";
import { RefreshCw, Settings, Shirt, X } from "lucide-react";
import { todayStr } from "../format";
import type { BalanceData, LoadState, ModelName, UsageResult } from "../types";
import { useTheme } from "../theme";
import { BrandIcon } from "./BrandIcon";
import { BalanceCard } from "./BalanceCard";
import { UsageRow } from "./UsageRow";
import { UsageChart } from "./UsageChart";

function DashboardPanel({
  balance,
  balanceState,
  balanceError,
  usage,
  usageState,
  usageError,
  onRefresh,
  onClose,
  onSettings,
  onDetail,
}: {
  balance: BalanceData | null;
  balanceState: LoadState;
  balanceError: string;
  usage: UsageResult | null;
  usageState: LoadState;
  usageError: string;
  onRefresh: () => void;
  onClose: () => void;
  onSettings: () => void;
  onDetail: (model: ModelName) => void;
}) {
  const { theme, toggleTheme } = useTheme();
  const flash = usage?.models.find((item) => item.key === "flash") ?? null;
  const pro = usage?.models.find((item) => item.key === "pro") ?? null;
  const other = usage?.models.find((item) => item.key === "other") ?? null;
  const maxTokens = Math.max(
    flash?.totalTokens ?? 0,
    pro?.totalTokens ?? 0,
    other?.totalTokens ?? 0,
    1,
  );
  const today = usage?.days.find((day) => day.date === todayStr()) ?? null;
  const todayCost = usageState === "ok" && today ? today.totalCost : null;
  const monthCost = usageState === "ok" && usage ? usage.monthCost : null;

  return (
    <section className="panel dashboard-panel" data-testid="dashboard-panel">
      <header className="panel-header" data-tauri-drag-region>
        <div className="title-lockup" data-tauri-drag-region>
          <BrandIcon size={36} />
          <h1>DeepSeek Monitor</h1>
        </div>
        <div className="header-actions">
          <button aria-label="刷新" onClick={onRefresh}>
            <RefreshCw size={22} />
          </button>
          <div className="skin-menu-wrap">
            <button
              aria-label="Toggle theme"
              className="skin-toggle"
              title={theme === "dark" ? "Switch to light" : "Switch to dark"}
              onClick={toggleTheme}
            >
              <Shirt size={21} />
            </button>
          </div>
          <button aria-label="设置" onClick={onSettings}>
            <Settings size={23} />
          </button>
          <button aria-label="关闭" onClick={onClose}>
            <X size={25} />
          </button>
        </div>
      </header>

      <BalanceCard
        balance={balance}
        state={balanceState}
        error={balanceError}
        todayCost={todayCost}
        monthCost={monthCost}
      />

      <div className="usage-stack">
        <UsageRow
          modelKey="flash"
          data={flash}
          maxTokens={maxTokens}
          state={usageState}
          onClick={() => onDetail("flash")}
        />
        <UsageRow
          modelKey="pro"
          data={pro}
          maxTokens={maxTokens}
          state={usageState}
          onClick={() => onDetail("pro")}
        />
        {other && (
          <UsageRow
            modelKey="other"
            data={other}
            maxTokens={maxTokens}
            state={usageState}
            onClick={() => onDetail("other")}
          />
        )}
      </div>

      <UsageChart usage={usage} state={usageState} error={usageError} />
    </section>
  );
}
export { DashboardPanel };
