import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppConfig,
  BalanceData,
  LoadState,
  ModelName,
  UsageResult,
  ViewName,
} from "./types";
import { fetchCurrentUsage } from "./usage-api";
import { DashboardPanel } from "./components/DashboardPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { ModelDetailPanel } from "./components/ModelDetailPanel";

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
export { App };
