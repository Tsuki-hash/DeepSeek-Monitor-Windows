// 设置页的状态与动作（评审 F-09：从 SettingsPanel 拆出，组件只留视图）。
// 拆分是纯搬迁：invoke / listen / 剪贴板调用与状态流转都在这里，行为与拆分前逐行等价。

import React from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { currencySymbol, fmtMoney } from "./format";
import type { AppConfig, BalanceData, UsageResult } from "./types";
import { fetchCurrentUsage } from "./usage-api";

function useSettingsActions(handlers: {
  onUsageLoaded: (usage: UsageResult) => void;
  setUsageStatus: (status: string) => void;
  setShowManualPaste: (show: boolean) => void;
}) {
  const { onUsageLoaded, setUsageStatus, setShowManualPaste } = handlers;
  return React.useCallback(
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
          setShowManualPaste(true);
          return null;
        });
    },
    [onUsageLoaded, setUsageStatus, setShowManualPaste],
  );
}

export type SettingsState = {
  config: AppConfig | null;
  configPath: string;
  status: string;
  busy: boolean;
  appVersion: string;
  apiKey: string;
  setApiKey: (value: string) => void;
  usageToken: string;
  setUsageToken: (value: string) => void;
  refresh: number;
  autoRefresh: boolean;
  autostart: boolean;
  usageStatus: string;
  usageSyncing: boolean;
  showManualPaste: boolean;
  setShowManualPaste: React.Dispatch<React.SetStateAction<boolean>>;
  saveApiKey: () => void;
  clearApiKey: () => void;
  pasteApiKey: () => void;
  saveUsageToken: () => void;
  clearUsageToken: () => void;
  pasteUsageToken: () => void;
  startUsageSync: () => void;
  saveRefreshInterval: (seconds: number) => void;
  saveAutoRefreshEnabled: (enabled: boolean) => void;
  saveAutostart: (enabled: boolean) => void;
};

export function useSettingsState(handlers: {
  onUsageLoaded: (usage: UsageResult) => void;
  onUsageCleared: () => void;
  onRefreshIntervalChanged: (seconds: number) => void;
  onAutoRefreshChanged: (enabled: boolean) => void;
}): SettingsState {
  const {
    onUsageLoaded,
    onUsageCleared,
    onRefreshIntervalChanged,
    onAutoRefreshChanged,
  } = handlers;

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
  const configPath =
    config?.configPath ?? "%APPDATA%\\DeepSeekMonitorWindows\\config.json";

  React.useEffect(() => {
    void invoke<AppConfig>("get_app_config")
      .then((nextConfig) => {
        setConfig(nextConfig);
        setRefresh(nextConfig.refreshIntervalSeconds || 60);
        setAutoRefresh(nextConfig.autoRefreshEnabled);
        setAutostart(nextConfig.autostart);
        setStatus(
          nextConfig.apiKeyConfigured
            ? `已配置 ${nextConfig.apiKeyPreview}`
            : "未配置 API Key",
        );
        setUsageStatus(
          nextConfig.usageTokenConfigured
            ? "用量 Token 已配置"
            : "未配置用量 Token",
        );
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

  // 保存 Token 之后刷新用量。这里刻意不向外抛出（见 useSettingsActions）。
  const refreshUsageAfterToken = useSettingsActions({
    onUsageLoaded,
    setUsageStatus,
    setShowManualPaste,
  });

  React.useEffect(() => {
    const unlistenPromise = listen<AppConfig>(
      "usage-token-captured",
      (event) => {
        setConfig(event.payload);
        setUsageSyncing(false);
        void refreshUsageAfterToken("已通过网页登录自动同步用量 Token");
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [refreshUsageAfterToken]);

  React.useEffect(() => {
    const unlistenPromise = listen("usage-sync-ended", () => {
      setUsageSyncing(false);
      setUsageStatus(
        "未获取到用量 Token。可再次点击同步，或使用方式二手动粘贴。",
      );
      setShowManualPaste(true);
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
          setUsageStatus(
            "登录完成后，再次点击本按钮即可同步用量（可多点几次）",
          );
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
        setUsageStatus(
          typeof error === "string" ? error : "用量 Token 保存失败",
        );
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
      void invoke<AppConfig>("save_refresh_interval", {
        refreshIntervalSeconds: seconds,
      })
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
      void invoke<AppConfig>("save_auto_refresh_enabled", {
        autoRefreshEnabled: enabled,
      })
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

  const saveAutostart = React.useCallback(
    (enabled: boolean) => {
      const previous = autostart;
      setAutostart(enabled);
      void invoke<AppConfig>("save_autostart", { autostart: enabled })
        .then((nextConfig) => {
          setConfig(nextConfig);
          setAutostart(nextConfig.autostart);
        })
        .catch(() => setAutostart(previous));
    },
    [autostart],
  );

  return {
    config,
    configPath,
    status,
    busy,
    appVersion,
    apiKey,
    setApiKey,
    usageToken,
    setUsageToken,
    refresh,
    autoRefresh,
    autostart,
    usageStatus,
    usageSyncing,
    showManualPaste,
    setShowManualPaste,
    saveApiKey,
    clearApiKey,
    pasteApiKey,
    saveUsageToken,
    clearUsageToken,
    pasteUsageToken,
    startUsageSync,
    saveRefreshInterval,
    saveAutoRefreshEnabled,
    saveAutostart,
  };
}
