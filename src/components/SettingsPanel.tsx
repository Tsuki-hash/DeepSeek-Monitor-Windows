import React from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import {
  BarChart3,
  CheckCircle2,
  Info,
  KeyRound,
  Power,
  RefreshCw,
  X,
} from "lucide-react";
import { currencySymbol, fmtMoney } from "../format";
import type { AppConfig, BalanceData, UsageResult } from "../types";
import { fetchCurrentUsage, refreshOptions } from "../usage-api";
import { BrandIcon } from "./BrandIcon";
import { SettingsSection, Toggle } from "./ui";

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
export { SettingsPanel };
