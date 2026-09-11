#!/usr/bin/env node
// 校验三处配置里的版本号是否一致。
//
// 背景：Tauri 2 的 tauri.conf.json 不会去读 package.json，Cargo.toml 的版本号也是
// 独立的一份，发版时三处必须手动同步。前端原先还硬编码了两处兜底版本号，已改为
// 不显示（见 main.tsx），所以现在只需守住这三处。
//
// 用法：npm run check:version    —— 不一致时以退出码 1 失败，可直接用于 CI。

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const readJson = (rel) => JSON.parse(readFileSync(join(root, rel), "utf8"));

/** 从 Cargo.toml 的 [package] 段里取 version，避免为此引入 TOML 解析依赖 */
const readCargoVersion = (rel) => {
  const text = readFileSync(join(root, rel), "utf8");
  const pkg = text.split(/^\[/m).find((section) => section.startsWith("package]"));
  if (!pkg) throw new Error(`${rel} 中找不到 [package] 段`);
  const match = /^\s*version\s*=\s*"([^"]+)"/m.exec(pkg);
  if (!match) throw new Error(`${rel} 的 [package] 段中找不到 version`);
  return match[1];
};

const sources = [
  { label: "package.json", value: readJson("package.json").version },
  { label: "src-tauri/tauri.conf.json", value: readJson("src-tauri/tauri.conf.json").version },
  { label: "src-tauri/Cargo.toml", value: readCargoVersion("src-tauri/Cargo.toml") },
];

const distinct = [...new Set(sources.map((s) => s.value))];

for (const { label, value } of sources) {
  console.log(`${label.padEnd(30)} ${value}`);
}

if (distinct.length !== 1) {
  console.error(
    `\n版本号不一致：${distinct.join(" / ")}。发版时请把上列三处改成同一个版本号。`,
  );
  process.exit(1);
}

console.log(`\n版本号一致：${distinct[0]}`);
