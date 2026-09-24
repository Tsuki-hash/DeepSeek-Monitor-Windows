import React from "react";
import ReactDOM from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import {
  BarChart3,
  Brain,
  CalendarDays,
  CheckCircle2,
  CreditCard,
  Info,
  KeyRound,
  Power,
  RefreshCw,
  Settings,
  Shirt,
  SunMedium,
  X,
  Zap,
} from "lucide-react";
import "./styles.css";
import {
  addDays,
  chartPointFromDay,
  currencySymbol,
  fmtInt,
  fmtMoney,
  fmtTokensShort,
  mmdd,
  previousMonth,
  recentUsageDays,
  todayStr,
  type UsageDay,
} from "./format";

type ViewName = "dashboard" | "settings" | "detail";
type ModelName = "flash" | "pro";
type AppConfig = {
  apiKeyConfigured: boolean;
  apiKeyPreview: string | null;
  usageTokenConfigured: boolean;
  refreshIntervalSeconds: number;
  autoRefreshEnabled: boolean;
  autostart: boolean;
  configPath: string;
};
type BalanceData = {
  isAvailable: boolean;
  currency: string;
  totalBalance: string;
  grantedBalance: string;
  toppedUpBalance: string;
};
// 通用的异步加载状态，余额与用量共用。刻意不绑定具体业务，避免复用时名字误导。
type LoadState = "loading" | "ok" | "error" | "nokey";

type UsageModel = {
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
type UsageResult = {
  models: UsageModel[];
  days: UsageDay[];
  monthCost: number;
};

const fetchMonthUsage = (month: number, year: number) => {
  return invoke<UsageResult>("fetch_usage", { month, year });
};
const fetchCurrentUsage = async () => {
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

const refreshOptions = [
  { label: "1 分钟", value: 60 },
  { label: "5 分钟", value: 300 },
  { label: "30 分钟", value: 1800 },
  { label: "1 小时", value: 3600 },
];

function App() {
  const [view, setView] = React.useState<ViewName>("dashboard");
  const [model, setModel] = React.useState<ModelName>("flash");

  const [balance, setBalance] = React.useState<BalanceData | null>(null);
  const [balanceState, setBalanceState] = React.useState<LoadState>("loading");
  const [balanceError, setBalanceError] = React.useState("");

  const [usage, setUsage] = React.useState<UsageResult | null>(null);
  const [usageState, setUsageState] = React.useState<LoadState>("loading");
  const [usageError, setUsageError] = React.useState("");
  const [refreshIntervalSeconds, setRefreshIntervalSeconds] = React.useState(60);
  const [autoRefreshEnabled, setAutoRefreshEnabled] = React.useState(false);

  // silent=true 表示后台刷新：已有数据时保留旧值，不再把面板打回「查询中…」骨架，
  // 否则开启 1 分钟自动刷新后会周期性闪烁。若当前没有可用数据（首次或上次失败），
  // 仍显示加载态，避免用户对着「查询失败」无从判断是否正在重试。
  const loadBalance = React.useCallback((silent = false) => {
    if (silent) {
      setBalanceState((prev) => (prev === "ok" ? prev : "loading"));
    } else {
      setBalanceState("loading");
    }
    void invoke<BalanceData>("fetch_balance")
      .then((data) => {
        setBalance(data);
        setBalanceState("ok");
      })
      .catch((error) => {
        const message = typeof error === "string" ? error : "查询失败";
        setBalanceError(message);
        setBalanceState(message.includes("未配置") ? "nokey" : "error");
      });
  }, []);

  const loadUsage = React.useCallback((silent = false) => {
    if (silent) {
      setUsageState((prev) => (prev === "ok" ? prev : "loading"));
    } else {
      setUsageState("loading");
    }
    void fetchCurrentUsage()
      .then((data) => {
        setUsage(data);
        setUsageState("ok");
        setUsageError("");
      })
      .catch((error) => {
        const message = typeof error === "string" ? error : "查询失败";
        setUsageError(message);
        // 静默刷新失败时保留上一份可用快照，避免网络抖动把面板打回「查询失败」
        if (!silent) {
          setUsage(null);
        }
        setUsageState((prev) => {
          if (silent && prev === "ok") {
            return "ok";
          }
          return message.includes("未配置") ? "nokey" : "error";
        });
      });
  }, []);

  const refreshAll = React.useCallback(
    (silent = false) => {
      loadBalance(silent);
      loadUsage(silent);
    },
    [loadBalance, loadUsage],
  );

  React.useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  React.useEffect(() => {
    void invoke<AppConfig>("get_app_config")
      .then((config) => {
        setRefreshIntervalSeconds(config.refreshIntervalSeconds || 60);
        setAutoRefreshEnabled(config.autoRefreshEnabled);
      })
      .catch(() => {
        setRefreshIntervalSeconds(60);
        setAutoRefreshEnabled(false);
      });
  }, []);

  // 面板是否可见。窗口隐藏不会卸载 WebView，所以自动刷新定时器必须靠这个状态显式暂停，
  // 否则面板整天收在托盘里也会按时打接口。该状态由 Rust 侧在显隐时发的事件驱动
  // （见 lib.rs 的 EVENT_MAIN_WINDOW_SHOWN / EVENT_MAIN_WINDOW_HIDDEN），
  // 这样从托盘唤出、托盘左键切换、程序内点关闭三条路径都覆盖得到。
  const [windowVisible, setWindowVisible] = React.useState(true);
  // 首帧后按实际窗口可见性校正，避免启动时若已在托盘仍多跑一轮刷新
  React.useEffect(() => {
    void invoke<boolean>("is_main_window_visible")
      .then((visible) => setWindowVisible(visible))
      .catch(() => undefined);
  }, []);

  React.useEffect(() => {
    const shown = listen("main-window-shown", () => {
      setWindowVisible(true);
      // 面板被唤出即拉最新数据，避免托盘唤出后看到的是旧快照。
      // 走静默刷新：唤出瞬间面板上已有上一轮的数据，不该闪一下「查询中…」。
      refreshAll(true);
    });
    const hidden = listen("main-window-hidden", () => {
      setWindowVisible(false);
    });
    return () => {
      void shown.then((unlisten) => unlisten());
      void hidden.then((unlisten) => unlisten());
    };
  }, [refreshAll]);

  React.useEffect(() => {
    if (!autoRefreshEnabled || !windowVisible) {
      return;
    }
    // 自动刷新属于后台更新，走静默模式。
    // 用 setTimeout 链而非 setInterval：睡眠唤醒后 interval 可能连发补帧，
    // 链式调度总是等上一次结束后再计时，避免睡眠唤醒后 interval 连发补帧。
    let cancelled = false;
    let timer = 0;
    const schedule = () => {
      timer = window.setTimeout(() => {
        if (cancelled) {
          return;
        }
        refreshAll(true);
        schedule();
      }, refreshIntervalSeconds * 1000);
    };
    schedule();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [autoRefreshEnabled, refreshAll, refreshIntervalSeconds, windowVisible]);

  const hideWindow = React.useCallback(() => {
    void invoke("hide_main_window").catch(() => {
      // Browser preview has no Tauri IPC. Keep it non-blocking for visual checks.
    });
  }, []);

  return (
    <div className="stage">
      {view === "dashboard" && (
        <DashboardPanel
          balance={balance}
          balanceState={balanceState}
          balanceError={balanceError}
          usage={usage}
          usageState={usageState}
          usageError={usageError}
          onRefresh={() => refreshAll()}
          onClose={hideWindow}
          onSettings={() => setView("settings")}
          onDetail={(nextModel) => {
            setModel(nextModel);
            setView("detail");
          }}
        />
      )}
      {view === "settings" && (
        <SettingsPanel
          onUsageLoaded={(nextUsage) => {
            setUsage(nextUsage);
            setUsageState("ok");
            setUsageError("");
          }}
          onUsageCleared={() => {
            setUsage(null);
            setUsageState("nokey");
            setUsageError("未配置用量 Token");
          }}
          onRefreshIntervalChanged={setRefreshIntervalSeconds}
          onAutoRefreshChanged={setAutoRefreshEnabled}
          onBack={() => setView("dashboard")}
        />
      )}
      {view === "detail" && (
        <ModelDetailPanel
          model={model}
          usage={usage}
          usageState={usageState}
          usageError={usageError}
          onBack={() => setView("dashboard")}
        />
      )}
    </div>
  );
}

function BrandIcon({ size = 32 }: { size?: number }) {
  // 静态资源缺失时（打包遗漏、被杀软清理）不显示裂图，退化为一个同尺寸的占位块。
  const [failed, setFailed] = React.useState(false);
  return (
    <div className="brand-icon" style={{ width: size, height: size }}>
      {failed ? (
        <span className="brand-icon-fallback" aria-hidden="true">
          DS
        </span>
      ) : (
        <img
          src="/assets/deepseek-color.png"
          alt="DeepSeek"
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}

// 主题的唯一来源：存储键、DOM 属性名、默认值只在这里定义一次。
// 首屏渲染前的引导代码与 React 内的 useTheme() 都调用同一组函数，
// 避免「组件内改一次 + 模块顶层再改一次」那种靠巧合保持一致的状态分散。
const THEME_STORAGE_KEY = "ui-theme";
const THEME_ATTR = "data-theme";
type Theme = "dark" | "light";
const readStoredTheme = (): Theme =>
  localStorage.getItem(THEME_STORAGE_KEY) === "light" ? "light" : "dark";
const applyTheme = (theme: Theme) =>
  document.documentElement.setAttribute(THEME_ATTR, theme);

function useTheme() {
  const [theme, setTheme] = React.useState<Theme>(readStoredTheme);
  // 状态与 DOM 属性、持久化三者在这里同步；调用方只关心 theme 与 toggleTheme
  React.useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme]);
  const toggleTheme = React.useCallback(() => {
    setTheme((prev) => (prev === "dark" ? "light" : "dark"));
  }, []);
  return { theme, toggleTheme };
}

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
  const maxTokens = Math.max(flash?.totalTokens ?? 0, pro?.totalTokens ?? 0, 1);
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
      </div>

      <UsageChart usage={usage} state={usageState} error={usageError} />
    </section>
  );
}

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
  const statusText = state === "ok" ? (balance?.isAvailable ? "可用" : "余额不足") : "—";
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
      <div className={`balance-amount ${state !== "ok" ? "balance-dim" : ""}`}>{amount}</div>
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

function UsageRow({
  modelKey,
  data,
  maxTokens,
  state,
  onClick,
}: {
  modelKey: ModelName;
  data: UsageModel | null;
  maxTokens: number;
  state: LoadState;
  onClick: () => void;
}) {
  const isFlash = modelKey === "flash";
  // 显示名优先取后端 model_slot 的 name，避免前后端双源漂移
  const name = data?.name ?? (isFlash ? "V4.1 Flash" : "V4 Pro");
  const tokensText = data
    ? `${fmtInt(data.totalTokens)} Tokens`
    : state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置 Token"
        : state === "error"
          ? "用量不可用"
          : "—";
  const cost = data ? fmtMoney(data.cost) : "—";
  const ratio = data && data.cost > 0 ? `${fmtTokensShort(data.totalTokens / data.cost)} T/¥` : "—";
  const width = data ? `${Math.max(2, (data.totalTokens / maxTokens) * 100)}%` : "0%";

  return (
    <button className="card usage-row" onClick={onClick}>
      <div className={`model-badge ${isFlash ? "flash" : "pro"}`}>
        {isFlash ? <Zap size={27} fill="currentColor" /> : <Brain size={25} />}
      </div>
      <div className="usage-main">
        <h2>
          {name}
          {!isFlash && (
            <span className="deprecation-tag" title="V4 Pro 正在逐步下线，2026-09-14 起请求路由至 V4.1 Flash">
              逐步下线
            </span>
          )}
        </h2>
        <div className="token-line">
          <span>{tokensText}</span>
          <div className="progress-track">
            <i className={isFlash ? "flash-fill" : "pro-fill"} style={{ width }} />
          </div>
        </div>
        {data && data.cacheHitTokens + data.cacheMissTokens > 0 && (
          <span className={`cache-hit-rate ${isFlash ? "flash" : "pro"}`}>
            缓存命中{" "}
            {((data.cacheHitTokens / (data.cacheHitTokens + data.cacheMissTokens)) * 100).toFixed(0)}%
          </span>
        )}
      </div>
      <div className="usage-price">
        <strong>{cost}</strong>
        <span>{ratio}</span>
      </div>
    </button>
  );
}

type StackedPoint = {
  date: string;
  hit: number;
  miss: number;
  response: number;
  other: number;
  total: number;
};

// 主面板（Flash + Pro 合并）与详情页（单模型）的柱状图此前是两份近乎逐行相同的 JSX
// （约 70 行 × 2），本次评审就需要人工比对两处确认逻辑一致。抽成同一个组件后，
// 分段顺序、tooltip 结构、可访问性只有一处实现，改样式或加分段不必再同步两处。
// 两处仅类名与日期标签元素不同，用 variant 区分，保持各自的渲染结果不变。
function StackedBarChart({
  points,
  variant,
  hasOther,
}: {
  points: StackedPoint[];
  variant: "summary" | "detail";
  hasOther: boolean;
}) {
  const [hoveredIdx, setHoveredIdx] = React.useState<number | null>(null);
  const MIN_BAR = 3; // 整根柱子的最小可见高度百分比（含空数据占位）
  const maxVal = Math.max(...points.map((point) => point.total), 1);
  const isSummary = variant === "summary";

  return (
    <>
      <div
        className={isSummary ? "bars" : "detail-bars"}
        onMouseLeave={() => setHoveredIdx(null)}
      >
        {points.map((point, idx) => {
          // 键盘用户与读屏用户拿到的是同一条信息：日期、合计与各分段明细
          const label =
            `${point.date}：合计 ${fmtInt(point.total)} tokens，` +
            `命中 ${fmtInt(point.hit)}，未命中 ${fmtInt(point.miss)}，输出 ${fmtInt(point.response)}` +
            (point.other > 0 ? `，其他 ${fmtInt(point.other)}` : "");
          return (
            <div className={isSummary ? "bar-column" : "detail-bar-column"} key={point.date}>
              {hoveredIdx === idx && point.total > 0 && (
                <div
                  className={`bar-tooltip${
                    idx <= 1 ? " align-left" : idx >= points.length - 2 ? " align-right" : ""
                  }`}
                >
                  <div className="bar-tooltip-head">
                    <span className="bar-tooltip-date">{point.date}</span>
                    <strong>{fmtInt(point.total)} tokens</strong>
                  </div>
                  <span className="bar-tooltip-row">
                    <i className="dot hit" />输入（命中缓存）
                    <strong>{fmtInt(point.hit)} tokens</strong>
                  </span>
                  <span className="bar-tooltip-row">
                    <i className="dot miss" />输入（未命中缓存）
                    <strong>{fmtInt(point.miss)} tokens</strong>
                  </span>
                  <span className="bar-tooltip-row">
                    <i className="dot response" />输出
                    <strong>{fmtInt(point.response)} tokens</strong>
                  </span>
                  {point.other > 0 && (
                    <span className="bar-tooltip-row">
                      <i className="dot other" />其他（含未识别模型）
                      <strong>{fmtInt(point.other)} tokens</strong>
                    </span>
                  )}
                </div>
              )}
              {isSummary ? (
                <span className="bar-value">
                  {point.total > 0 ? fmtTokensShort(point.total) : "0"}
                </span>
              ) : (
                <span>{point.total > 0 ? fmtTokensShort(point.total) : ""}</span>
              )}
              <div className={isSummary ? "bar-slot" : "detail-bar-slot"}>
                <div
                  className={isSummary ? "cache-bar" : "detail-bar-stacked"}
                  role="img"
                  tabIndex={0}
                  aria-label={label}
                  style={{
                    height: `${point.total > 0 ? Math.max(MIN_BAR, (point.total / maxVal) * 100) : MIN_BAR}%`,
                  }}
                  onMouseEnter={() => setHoveredIdx(idx)}
                  onMouseLeave={() => setHoveredIdx(null)}
                  onFocus={() => setHoveredIdx(idx)}
                  onBlur={() => setHoveredIdx(null)}
                >
                  {point.total > 0 ? (
                    <>
                      {point.hit > 0 && <i className="seg hit" style={{ flexGrow: point.hit }} />}
                      {point.miss > 0 && <i className="seg miss" style={{ flexGrow: point.miss }} />}
                      {point.response > 0 && (
                        <i className="seg response" style={{ flexGrow: point.response }} />
                      )}
                      {point.other > 0 && <i className="seg other" style={{ flexGrow: point.other }} />}
                    </>
                  ) : (
                    <i className="seg empty" />
                  )}
                </div>
              </div>
              {isSummary ? (
                <span className="bar-day">{mmdd(point.date)}</span>
              ) : (
                <em>{mmdd(point.date)}</em>
              )}
            </div>
          );
        })}
      </div>
      <div className="chart-legend-bottom">
        <span className="chart-legend-item">
          <i className="dot hit" />命中
        </span>
        <span className="chart-legend-item">
          <i className="dot miss" />未命中
        </span>
        <span className="chart-legend-item">
          <i className="dot response" />输出
        </span>
        {hasOther && (
          <span className="chart-legend-item">
            <i className="dot other" />其他
          </span>
        )}
      </div>
    </>
  );
}

function UsageChart({
  usage,
  state,
  error,
}: {
  usage: UsageResult | null;
  state: LoadState;
  error: string;
}) {
  const days = recentUsageDays(usage?.days ?? []);
  // all：合计含未识别模型（差额并入 other），避免按日图静默丢量
  const points = days.map((day) => chartPointFromDay(day, "all"));
  const hasOther = points.some((point) => point.other > 0);
  const sumHit = points.reduce((sum, point) => sum + point.hit, 0);
  const sumMiss = points.reduce((sum, point) => sum + point.miss, 0);
  const sumTotal = points.reduce((sum, point) => sum + point.total, 0);
  const hitRate = sumHit + sumMiss > 0 ? ((sumHit / (sumHit + sumMiss)) * 100).toFixed(0) : "0";
  const placeholder =
    state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置用量 Token"
        : state === "error"
          ? error
          : "暂无数据";

  return (
    <article className="card chart-card">
      <div className="card-title-row">
        <div className="caption-with-icon">
          <BarChart3 size={16} className="brand-blue" />
          <span>缓存命中明细</span>
        </div>
        <span className="chart-total">
          {state === "ok" ? (
            <>
              命中率 {hitRate}% · 合计 {fmtTokensShort(sumTotal)}
              {error ? " · 上次刷新失败" : ""}
            </>
          ) : (
            "—"
          )}
        </span>
      </div>
      {state === "ok" && points.length > 0 ? (
        <StackedBarChart points={points} variant="summary" hasOther={hasOther} />
      ) : (
        <div className="chart-placeholder">{placeholder}</div>
      )}
    </article>
  );
}

function SettingsPanel({
  onBack,
  onUsageLoaded,
  onUsageCleared,
  onRefreshIntervalChanged,
  onAutoRefreshChanged,
}: {
  onBack: () => void;
  onUsageLoaded: (usage: UsageResult) => void;
  onUsageCleared: () => void;
  onRefreshIntervalChanged: (seconds: number) => void;
  onAutoRefreshChanged: (enabled: boolean) => void;
}) {
  const [apiKey, setApiKey] = React.useState("");
  const [config, setConfig] = React.useState<AppConfig | null>(null);
  const [status, setStatus] = React.useState("正在读取本地配置");
  const [busy, setBusy] = React.useState(false);
  const [refresh, setRefresh] = React.useState(60);
  const [autoRefresh, setAutoRefresh] = React.useState(false);
  const [autostart, setAutostart] = React.useState(false);
  const [usageToken, setUsageToken] = React.useState("");
  const [usageStatus, setUsageStatus] = React.useState("");
  const [usageSyncing, setUsageSyncing] = React.useState(false);
  const [showManualPaste, setShowManualPaste] = React.useState(false);
  // 空串表示「版本未知」。刻意不写死一个兜底版本号：那个数字会随着发版过期，
  // 显示出来反而是错的信息，不如显示「—」。
  const [appVersion, setAppVersion] = React.useState("");
  const configPath = config?.configPath ?? "%APPDATA%\\DeepSeekMonitorWindows\\config.json";

  React.useEffect(() => {
    void invoke<AppConfig>("get_app_config")
      .then((nextConfig) => {
        setConfig(nextConfig);
        setRefresh(nextConfig.refreshIntervalSeconds || 60);
        setAutoRefresh(nextConfig.autoRefreshEnabled);
        setAutostart(nextConfig.autostart);
        setStatus(nextConfig.apiKeyConfigured ? `已配置 ${nextConfig.apiKeyPreview}` : "未配置 API Key");
        setUsageStatus(nextConfig.usageTokenConfigured ? "用量 Token 已配置" : "未配置用量 Token");
      })
      .catch((error) => {
        // 只有"根本没有 Tauri IPC"才是浏览器预览，属预期情况；真机上失败一律是真实错误，
        // 必须原样透出——否则配置文件损坏会被伪装成"浏览器预览模式"，用户完全无从定位。
        if (!isTauri()) {
          setStatus("浏览器预览模式，未连接本地配置");
          return;
        }
        setStatus(typeof error === "string" ? error : "读取本地配置失败");
      });
  }, []);

  React.useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(""));
  }, []);

  // 保存 Token 之后刷新用量。这里刻意不向外抛出：调用它的两条路径（事件回调、保存按钮）
  // 一旦抛出，要么在回调里变成 unhandled rejection，要么被外层笼统的「保存或验证失败」
  // 覆盖掉下面这句更准确的提示——而 Token 其实已经成功落盘，用户会被误导去重新粘贴。
  const refreshUsageAfterToken = React.useCallback(
    (prefix: string) => {
      setUsageStatus(`${prefix}，正在刷新用量数据…`);
      return fetchCurrentUsage()
        .then((usage) => {
          onUsageLoaded(usage);
          setUsageStatus(`${prefix}，本月消费 ${fmtMoney(usage.monthCost)}`);
          return usage;
        })
        .catch((error) => {
          const message = typeof error === "string" ? error : "用量刷新失败";
          setUsageStatus(`${prefix}，但用量刷新失败：${message}`);
          return null;
        });
    },
    [onUsageLoaded],
  );

  React.useEffect(() => {
    const unlistenPromise = listen<AppConfig>("usage-token-captured", (event) => {
      setConfig(event.payload);
      setUsageSyncing(false);
      void refreshUsageAfterToken("已通过网页登录自动同步用量 Token");
    });
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [refreshUsageAfterToken]);

  React.useEffect(() => {
    const unlistenPromise = listen("usage-sync-ended", () => {
      setUsageSyncing(false);
      setUsageStatus("登录窗口已关闭，Token 未获取到。可重新点击同步或使用方式二手动粘贴。");
    });
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const pasteApiKey = React.useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      setApiKey(text.trim());
      setStatus("已从剪贴板读取");
    } catch {
      setStatus("剪贴板读取失败");
    }
  }, []);

  const saveApiKey = React.useCallback(() => {
    setBusy(true);
    void invoke<AppConfig>("save_api_key", { apiKey })
      .then((nextConfig) => {
        setConfig(nextConfig);
        setApiKey("");
        setStatus("已保存，正在验证 Key…");
        return invoke<BalanceData>("fetch_balance");
      })
      .then((balance) => {
        const symbol = currencySymbol(balance.currency);
        const tip = balance.isAvailable ? "" : "（余额不足）";
        setStatus(`验证通过，当前余额 ${symbol}${balance.totalBalance}${tip}`);
      })
      .catch((error) => {
        // 保存已成功、仅后续验证失败时不能笼统说「保存或验证失败」
        const message = typeof error === "string" ? error : "验证失败";
        setStatus(`Key 已保存，但验证未通过：${message}`);
      })
      .finally(() => setBusy(false));
  }, [apiKey]);

  const clearApiKey = React.useCallback(() => {
    setBusy(true);
    void invoke<AppConfig>("clear_api_key")
      .then((nextConfig) => {
        setConfig(nextConfig);
        setApiKey("");
        setStatus("已清除 API Key");
      })
      .catch((error) => {
        setStatus(typeof error === "string" ? error : "清除失败");
      })
      .finally(() => setBusy(false));
  }, []);

  const pasteUsageToken = React.useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      setUsageToken(text.trim());
      setUsageStatus("已从剪贴板读取");
    } catch {
      setUsageStatus("剪贴板读取失败");
    }
  }, []);

  const startUsageSync = React.useCallback(() => {
    setUsageSyncing(true);
    setUsageStatus("正在打开登录窗口…");
    void invoke<boolean>("start_usage_sync")
      .then((synced) => {
        if (!synced) {
          setUsageStatus("登录完成后，再次点击本按钮即可同步用量（可多点几次）");
        }
        // synced=true 时由 usage-token-captured 事件刷新数据并更新状态
      })
      .catch((error) => {
        setUsageStatus(typeof error === "string" ? error : "打开登录窗口失败");
      })
      .finally(() => {
        // 短暂忙碌后自动恢复可点击，允许用户登录后反复点击触发同步
        window.setTimeout(() => setUsageSyncing(false), 2500);
      });
  }, []);

  const saveUsageToken = React.useCallback(() => {
    setBusy(true);
    void invoke<AppConfig>("save_usage_token", { usageToken })
      .then((nextConfig) => {
        setConfig(nextConfig);
        setUsageToken("");
        setUsageStatus("已保存，正在验证用量 Token…");
        return refreshUsageAfterToken("手动 Token 已保存");
      })
      .catch((error) => {
        // 走到这里说明是「保存」这一步失败（刷新阶段的失败已在 refreshUsageAfterToken
        // 内处理并给出更准确的文案），所以不再笼统地说"保存或验证失败"。
        setUsageStatus(typeof error === "string" ? error : "用量 Token 保存失败");
      })
      .finally(() => setBusy(false));
  }, [refreshUsageAfterToken, usageToken]);

  const clearUsageToken = React.useCallback(() => {
    setBusy(true);
    void invoke<AppConfig>("clear_usage_token")
      .then((nextConfig) => {
        setConfig(nextConfig);
        setUsageToken("");
        setUsageStatus("已清除用量 Token");
        onUsageCleared();
      })
      .catch((error) => {
        setUsageStatus(typeof error === "string" ? error : "清除失败");
      })
      .finally(() => setBusy(false));
  }, [onUsageCleared]);

  const saveRefreshInterval = React.useCallback(
    (seconds: number) => {
      const previous = refresh;
      setRefresh(seconds);
      onRefreshIntervalChanged(seconds);
      void invoke<AppConfig>("save_refresh_interval", { refreshIntervalSeconds: seconds })
        .then((nextConfig) => {
          setConfig(nextConfig);
          setRefresh(nextConfig.refreshIntervalSeconds || 60);
          onRefreshIntervalChanged(nextConfig.refreshIntervalSeconds || 60);
        })
        .catch(() => {
          setRefresh(previous);
          onRefreshIntervalChanged(previous);
        });
    },
    [onRefreshIntervalChanged, refresh],
  );

  const saveAutoRefreshEnabled = React.useCallback(
    (enabled: boolean) => {
      const previous = autoRefresh;
      setAutoRefresh(enabled);
      onAutoRefreshChanged(enabled);
      void invoke<AppConfig>("save_auto_refresh_enabled", { autoRefreshEnabled: enabled })
        .then((nextConfig) => {
          setConfig(nextConfig);
          setAutoRefresh(nextConfig.autoRefreshEnabled);
          onAutoRefreshChanged(nextConfig.autoRefreshEnabled);
        })
        .catch(() => {
          setAutoRefresh(previous);
          onAutoRefreshChanged(previous);
        });
    },
    [autoRefresh, onAutoRefreshChanged],
  );

  const saveAutostart = React.useCallback((enabled: boolean) => {
    const previous = autostart;
    setAutostart(enabled);
    void invoke<AppConfig>("save_autostart", { autostart: enabled })
      .then((nextConfig) => {
        setConfig(nextConfig);
        setAutostart(nextConfig.autostart);
      })
      .catch(() => setAutostart(previous));
  }, [autostart]);

  return (
    <section className="settings-panel" data-testid="settings-panel">
      <button className="floating-close settings-close" onClick={onBack} aria-label="返回主面板">
        <X size={20} />
      </button>
      <div className="settings-inner">
        <header className="settings-header" data-tauri-drag-region>
          <BrandIcon size={42} />
          <div>
            <h1>DeepSeek Monitor</h1>
            <p>设置</p>
          </div>
        </header>

        <SettingsSection icon={<KeyRound size={15} />} title="API Key">
          <p>仅用于查询账户余额（用量需网页登录 Token，见下一节）。会保存在应用本地设置中。</p>
          <p className="muted">API Key 只在当前这台 Windows 电脑本地保留。</p>
          <p className="muted config-path">
            <span>本地位置：</span>
            <span>{configPath}</span>
          </p>
          <div className="key-row">
            <input
              aria-label="API Key"
              type="password"
              value={apiKey}
              placeholder={config?.apiKeyConfigured ? "••••••••••••••••••••••••••••••••••••••••••••••••••" : "sk-..."}
              onChange={(event) => setApiKey(event.target.value)}
            />
          </div>
          <div className="settings-actions">
            <button className="primary" onClick={saveApiKey} disabled={busy || !apiKey.trim()}>
              验证并保存
            </button>
            <button className="secondary" onClick={pasteApiKey} disabled={busy}>
              粘贴
            </button>
            <span className={config?.apiKeyConfigured ? "configured" : "configured muted-status"}>
              <CheckCircle2 size={17} />
              {config?.apiKeyConfigured ? "已配置" : "未配置"}
            </span>
            <button className="secondary" onClick={clearApiKey} disabled={busy || !config?.apiKeyConfigured}>
              清除 Key
            </button>
          </div>
          <p className="muted">{status}</p>
        </SettingsSection>

        <SettingsSection icon={<BarChart3 size={15} />} title="用量同步 Token">
          <p>用于同步 Token 用量、消费和趋势图。DeepSeek 无官方用量 API，需网页登录 token（与上面的 API Key 不同）。</p>
          <p className="muted">方式一网页登录自动同步</p>
          <div className="settings-actions usage-sync-actions">
            <button className="primary" onClick={startUsageSync} disabled={usageSyncing}>
              {usageSyncing ? "等待登录" : "网页登录自动同步"}
            </button>
            <span className={config?.usageTokenConfigured ? "configured" : "configured muted-status"}>
              <CheckCircle2 size={17} />
              {config?.usageTokenConfigured ? "已配置" : "未配置"}
            </span>
            <button className="secondary" onClick={clearUsageToken} disabled={busy || !config?.usageTokenConfigured}>
              清除 Token
            </button>
          </div>
          <p className="muted">{usageStatus}</p>
          <button
            className="link-button"
            onClick={() => setShowManualPaste((value) => !value)}
          >
            {showManualPaste ? "收起手动粘贴" : "方式二：手动粘贴 token"}
          </button>
          {showManualPaste && (
            <>
              <p className="muted">
                获取：浏览器登录 platform.deepseek.com，按 F12 打开控制台，输入
                JSON.parse(localStorage.userToken).value 回车，复制返回的字符串。
              </p>
              <p className="muted">token 会过期，用量查询失败时重新获取一次即可。</p>
              <div className="key-row">
                <input
                  aria-label="用量 Token"
                  type="password"
                  value={usageToken}
                  placeholder={config?.usageTokenConfigured ? "••••••••••••••••••••••••••••••••••••••••••••••••••" : ""}
                  onChange={(event) => setUsageToken(event.target.value)}
                />
              </div>
              <div className="settings-actions">
                <button className="primary" onClick={saveUsageToken} disabled={busy || !usageToken.trim()}>
                  保存 Token
                </button>
                <button className="secondary" onClick={pasteUsageToken} disabled={busy}>
                  粘贴
                </button>
              </div>
            </>
          )}
        </SettingsSection>

        <SettingsSection icon={<Power size={15} />} title="开机自启">
          <p>开启后，每次登录 Windows 时自动启动 DeepSeek Monitor。</p>
          <Toggle label="登录时自动启动" checked={autostart} onChange={saveAutostart} />
        </SettingsSection>

        <SettingsSection icon={<RefreshCw size={15} />} title="自动刷新">
          <p>开启后，按设定周期自动从 DeepSeek API 拉取最新数据。</p>
          <Toggle label="启用自动刷新" checked={autoRefresh} onChange={saveAutoRefreshEnabled} />
          {autoRefresh && (
            <div className="segmented">
              {refreshOptions.map((option) => (
                <button
                  key={option.value}
                  className={refresh === option.value ? "selected" : ""}
                  onClick={() => saveRefreshInterval(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          )}
        </SettingsSection>

        <SettingsSection icon={<Info size={15} />} title="关于">
          <div className="version-row">
            <span>当前版本</span>
            <strong>{appVersion ? `v${appVersion}` : "—"}</strong>
          </div>
        </SettingsSection>

      </div>
    </section>
  );
}

function SettingsSection({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="settings-section">
      <h2>
        {icon}
        {title}
      </h2>
      {children}
    </section>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      <span>{label}</span>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <i />
    </label>
  );
}

function ModelDetailPanel({
  model,
  usage,
  usageState,
  usageError,
  onBack,
}: {
  model: ModelName;
  usage: UsageResult | null;
  usageState: LoadState;
  usageError: string;
  onBack: () => void;
}) {
  const isFlash = model === "flash";
  const data = usage?.models.find((item) => item.key === model) ?? null;
  // 显示名以后端 model_slot 为准，避免前后端双源漂移
  const title = data?.name ?? (isFlash ? "V4.1 Flash" : "V4 Pro");
  const tintClass = isFlash ? "flash" : "pro";
  const cost = data ? fmtMoney(data.cost) : "—";
  const totalText = data ? fmtTokensShort(data.totalTokens) : "—";

  const days = recentUsageDays(usage?.days ?? []);
  const points = days.map((day) => chartPointFromDay(day, isFlash ? "flash" : "pro"));
  const hasOther = points.some((point) => point.other > 0);
  const rangeText =
    points.length > 0 ? `${mmdd(points[0].date)} - ${mmdd(points[points.length - 1].date)}` : "";

  return (
    <section className="panel detail-panel" data-testid="detail-panel">
      <button className="floating-close" onClick={onBack} aria-label="返回主面板">
        <X size={20} />
      </button>
      <article className="card detail-hero" data-tauri-drag-region>
        <div className={`model-badge large ${tintClass}`}>
          {isFlash ? <Zap size={34} fill="currentColor" /> : <Brain size={33} />}
        </div>
        <div>
          <h1>
            {title}
            {!isFlash && (
              <span className="deprecation-tag" title="V4 Pro 正在逐步下线，2026-09-14 起请求路由至 V4.1 Flash 并按 Flash 计价">
                逐步下线
              </span>
            )}
          </h1>
          <p>{cost}</p>
        </div>
      </article>

      <div className="detail-metrics">
        <article className="card metric-card">
          <span>API 请求次数</span>
          <strong className={tintClass}>{data ? fmtInt(data.requestCount) : "—"}</strong>
        </article>
        <article className="card metric-card">
          <span>Tokens</span>
          <strong className={tintClass}>{totalText}</strong>
        </article>
      </div>

      <article className="card detail-chart">
        <div className="detail-chart-head">
          <div>
            <h2>按日 Token 消耗</h2>
            <span>{rangeText}</span>
          </div>
        </div>
        {usageState === "ok" && points.length > 0 ? (
          <StackedBarChart points={points} variant="detail" hasOther={hasOther} />
        ) : (
          <div className="chart-placeholder">
            {usageState === "nokey"
              ? "未配置用量 Token"
              : usageState === "loading"
                ? "查询中…"
                : usageState === "error"
                  ? usageError || "用量不可用"
                  : "暂无数据"}
          </div>
        )}
      </article>
    </section>
  );
}

// Apply the saved theme before first render to avoid a flash of the wrong skin.
// 仅此一处引导，具体读写规则复用上面的 readStoredTheme / applyTheme。
applyTheme(readStoredTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
