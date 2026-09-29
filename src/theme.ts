// 主题的唯一来源：存储键、DOM 属性名、默认值只在这里定义一次。
// 首屏渲染前的引导代码与 React 内的 useTheme() 都调用同一组函数。
import React from "react";

export const THEME_STORAGE_KEY = "ui-theme";
export const THEME_ATTR = "data-theme";
export type Theme = "dark" | "light";

/** 一次性迁移的标记键：置位后不再动用户的主题存储 */
export const THEME_MIGRATION_KEY = "ui-theme-1.3.0-migrated";

/** 迁移只用到 storage 的这三个方法；注入以便在 Node 单测里锁定行为 */
export type ThemeStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/**
 * 一次性主题迁移（1.3.0 浅色默认上线）。
 *
 * 1.2.x 每次启动都会把当时的默认值写进存储：从未主动选过主题的老用户带着
 * 的是 `dark`，升级后会一直停在深色。本函数置入迁移标记并清除这份旧默认，
 * 让其落到 1.3.0 的浅色；标记置位后用户再选深色会被正常记住，不会被反复清除。
 */
export function migrateLegacyTheme(storage: ThemeStorage = localStorage): void {
  if (storage.getItem(THEME_MIGRATION_KEY)) {
    return;
  }
  storage.setItem(THEME_MIGRATION_KEY, "1");
  if (storage.getItem(THEME_STORAGE_KEY) === "dark") {
    storage.removeItem(THEME_STORAGE_KEY);
  }
}

// 默认皮肤为浅色「明亮工作台」：只有明确存过 "dark" 才回到深色。
// 已保存偏好的用户不受影响，新安装用户不会落在旧默认值上。
export const readStoredTheme = (): Theme =>
  localStorage.getItem(THEME_STORAGE_KEY) === "dark" ? "dark" : "light";

export const applyTheme = (theme: Theme) =>
  document.documentElement.setAttribute(THEME_ATTR, theme);

export function useTheme() {
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
