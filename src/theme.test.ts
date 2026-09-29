// 主题迁移的纯逻辑单测（存储对象注入，不碰真实 localStorage）。
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  migrateLegacyTheme,
  THEME_MIGRATION_KEY,
  THEME_STORAGE_KEY,
} from "./theme.ts";

const memoryStorage = (initial: Record<string, string> = {}) => {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
  };
};

test("migrateLegacyTheme：旧默认 dark 被清除，落到浅色默认", () => {
  const storage = memoryStorage({ [THEME_STORAGE_KEY]: "dark" });
  migrateLegacyTheme(storage);
  assert.equal(storage.getItem(THEME_STORAGE_KEY), null);
  assert.equal(storage.getItem(THEME_MIGRATION_KEY), "1");
});

test("migrateLegacyTheme：升级后用户重选的 dark 不会被反复清除", () => {
  const storage = memoryStorage({
    [THEME_MIGRATION_KEY]: "1",
    [THEME_STORAGE_KEY]: "dark",
  });
  migrateLegacyTheme(storage);
  assert.equal(storage.getItem(THEME_STORAGE_KEY), "dark");
});

test("migrateLegacyTheme：主动选过 light 的用户不受影响", () => {
  const storage = memoryStorage({ [THEME_STORAGE_KEY]: "light" });
  migrateLegacyTheme(storage);
  assert.equal(storage.getItem(THEME_STORAGE_KEY), "light");
  assert.equal(storage.getItem(THEME_MIGRATION_KEY), "1");
});

test("migrateLegacyTheme：空存储（新安装）只置标记", () => {
  const storage = memoryStorage();
  migrateLegacyTheme(storage);
  assert.equal(storage.getItem(THEME_STORAGE_KEY), null);
  assert.equal(storage.getItem(THEME_MIGRATION_KEY), "1");
});
