import React from "react";
import { CalendarDays, CreditCard, SunMedium } from "lucide-react";
import { currencySymbol, fmtMoney } from "../format";
import type { BalanceData, LoadState } from "../types";

function BalanceCard({
  balance,
  state,
  error,
  todayCost,
  monthCost,
}: {
  balance: BalanceData | null;
  state: LoadState;
  error: string;
  todayCost: number | null;
  monthCost: number | null;
}) {
  const symbol = currencySymbol(balance?.currency);
  const amount =
    state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置"
        : state === "error"
          ? "查询失败"
          : `${symbol}${balance?.totalBalance ?? "0.00"}`;
  const statusText =
    state === "ok" ? (balance?.isAvailable ? "可用" : "余额不足") : "—";
  const statusOff = state === "ok" && balance != null && !balance.isAvailable;

  return (
    <article className="card balance-card">
      <div className="card-title-row">
        <div className="caption-with-icon">
          <CreditCard size={15} />
          <span>账户余额</span>
        </div>
        <div className={`status-pill ${statusOff ? "off" : ""}`}>
          <span />
          {statusText}
        </div>
      </div>
      <div className={`balance-amount ${state !== "ok" ? "balance-dim" : ""}`}>
        {amount}
      </div>
      {state === "error" && <div className="balance-error">{error}</div>}
      <div className="metric-grid">
        <div className="mini-card">
          <div className="caption-with-icon orange">
            <SunMedium size={15} />
            <span>当日消耗</span>
          </div>
          {/* 用量费用恒为人民币计价，不能跟余额币种（可能是 USD）走 */}
          <strong>{todayCost != null ? fmtMoney(todayCost, "¥") : "—"}</strong>
        </div>
        <div className="mini-card">
          <div className="caption-with-icon orange">
            <CalendarDays size={15} />
            <span>本月消费</span>
          </div>
          <strong>{monthCost != null ? fmtMoney(monthCost, "¥") : "—"}</strong>
        </div>
      </div>
    </article>
  );
}
export { BalanceCard };
