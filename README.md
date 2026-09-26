# DeepSeek Monitor Windows

把 DeepSeek 的余额和用量钉在 Windows 托盘上的小面板。点一下图标，就能看到还剩多少钱、今天花了多少、这个月花了多少，以及每个模型的 Token 和缓存命中情况。

只面向 Windows 10 / 11，技术栈是 Tauri 2 + React 18 + Rust。

**本项目不是 DeepSeek 官方产品，与 DeepSeek 公司无关。**

## 界面

| 深色皮肤 | 浅色皮肤 |
| :---: | :---: |
| <img src="screenshots/dashboard-dark.png" width="330" alt="DeepSeek Monitor Windows 深色皮肤主面板"> | <img src="screenshots/dashboard-light.png" width="330" alt="DeepSeek Monitor Windows 浅色皮肤主面板"> |

*上图均为演示数据，非真实账户。*

## 为什么会有这个项目

DeepSeek 官方只开放了余额接口，没有账户级的用量接口。网页端的用量页虽然能看到消费，但看不到「缓存命中 / 缓存未命中 / 输出」这种按模型拆开的明细，也没办法常驻桌面随时瞄一眼。

这个项目把三件事拼在一起：

- **余额** —— 用官方 API Key 直接调 `/user/balance`。
- **用量** —— 复用网页登录态换来的用量 Token，调平台内部接口，拿到当月消费、Token 总量、请求数和缓存明细。
- **呈现** —— 合并成一个常驻托盘、不占任务栏的窄面板，随开随关。

## 安装与上手

### 安装

从 [Releases](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows/releases/latest) 下载 `DeepSeekMonitorWindows_1.2.4_x64-setup.exe` 安装。覆盖安装不需要先卸载旧版本。

运行环境：Windows 10 或 Windows 11，以及 Microsoft Edge WebView2 Runtime（Windows 11 自带，Windows 10 若缺失需单独安装）。

### 填 API Key

打开主面板 → 右上角设置 → 「API Key」区块，粘贴你的 Key（在 DeepSeek 开放平台的 API Keys 页面生成），点 **验证并保存**。验证通过后，设置页会直接显示当前余额，主面板也会开始刷新。

### 同步用量

用量要用另一个凭据，见「两种凭据，别混」一节。

### 日常使用

- 点窗口右上角的关闭按钮 = **隐藏到托盘**，不是退出。
- 左键托盘图标：显示 / 隐藏面板。
- 右键托盘图标：显示主面板 / 退出。

## 两种凭据，别混

这是最容易踩的坑：查余额和查用量用的**不是**同一个东西。

|  | API Key | 用量 Token |
| --- | --- | --- |
| 从哪里来 | DeepSeek 开放平台 → API Keys 页面 | 登录 platform.deepseek.com 之后的会话 token |
| 用来干什么 | 查账户余额 | 查用量与消费 |
| 能不能互换 | 不能 | 不能 |

**为什么用量非得用 Token**：DeepSeek 没有公开账户级用量接口，只能复用网页端自己在调的那套接口，因此必须借你的登录态。这个 Token 属于会话凭据，和 API Key 一样敏感，而且会过期。

### 方式一：网页登录自动同步

在设置页「用量同步 Token」区块点 **网页登录自动同步**，在弹出的 DeepSeek 登录窗口完成登录。

应用会 hook 页面发出的网络请求，直接从 `Authorization` 头里抓取 Bearer token，验证它确实能调通用量接口之后才保存，然后自动刷新数据。

> 登录需要时间，页面登录完成后才会发请求。如果点了没反应，把登录窗口关掉再点一次按钮即可（等待期间按钮显示「等待登录」）。

### 方式二：手动粘贴（兜底）

点 **方式二：手动粘贴 token** 展开。用浏览器登录 platform.deepseek.com，按 F12 打开控制台，输入：

```js
JSON.parse(localStorage.userToken).value
```

复制返回的字符串，粘进输入框，点 **保存 Token**。

**用量 Token 会过期。查不出用量时，重新同步一次就行。**

## 模型口径

DeepSeek 在 2026-09-10 上线了 V4.1 Flash，模型名从 `deepseek-v4-flash` 改为 `deepseek-flash`。这次变更有个坑：**旧模型名没有被立刻禁用，而是被兼容路由到了新模型**，所以迁移期内同一份账单里可能出现新旧两种名字。本项目的处理方式：

| 平台返回的 model | 归入界面上的哪一行 |
| --- | --- |
| `deepseek-flash` | V4.1 Flash |
| `deepseek-v4-flash` | V4.1 Flash（**累加**） |
| `deepseek-v4-flash-vision-exp` | V4.1 Flash（**累加**） |
| `deepseek-v4-pro` | V4 Pro |

同名归入同一行时是**求和**而不是覆盖——否则一旦新旧名字并存，会有一部分用量被静默丢掉，界面上还看不出来。

另外两件事值得知道：

- **V4 Pro 正常提供。** 与 V4.1 Flash 分开统计，可独立查看 Token、费用与缓存明细。
- **未归类的 Token 有兜底统计。** V4.1 Flash 原生支持图片输入，如果平台返回了本项目尚未分类的 token 类型，这些量会被计入总量，并在图表里以「其他（未归类）」单独显示，而不是静默丢掉。
- **未接入的模型有兜底展示。** 平台返回了本项目尚未接入的模型名时（例如后续新模型上线初期），其用量会聚合为「其他」一行展示（Token、费用、请求数求和），不会静默丢失；新模型正式接入仍以 `model_slot()` 的映射为准。

## 界面上的数字分别是什么

### 主面板

| 位置 | 含义 |
| --- | --- |
| 账户余额 | 官方 `/user/balance` 返回的总额度，含赠送额度与充值额度 |
| 当日消耗 / 本月消费 | 来自平台用量接口的费用字段 |
| 模型行 | 该模型当月的 Token 总量、费用，以及每元能买多少 Token（T/¥）；下方是缓存命中率 |
| 缓存命中明细 | 最近 7 天的按日堆叠柱状图，三段分别是输入（命中缓存）、输入（未命中缓存）、输出 |

### 详情页

点主面板上任一模型行进入。展示该模型的请求次数、Token 总量，以及按日的 Token 消耗分解。

### 设置页

API Key 与用量 Token 的配置和清除、开机自启、自动刷新间隔（1 分钟 / 5 分钟 / 30 分钟 / 1 小时）、当前版本号。

## 凭据存在哪里

```text
%APPDATA%\DeepSeekMonitorWindows\config.json
```

API Key 和用量 Token 存在这个文件里，**已用 Windows DPAPI 加密**（`CryptProtectData`，密文以 `DSM1:` 开头的 base64 串保存）。DPAPI 的密钥绑定当前 Windows 用户与这台机器，因此：

- 把 `config.json` 单独拷到别的机器或别的用户下，凭据**解不开**，应用会提示重新填写（其余设置原样保留）。
- 旧版本（v1.2.1 及更早）留下的明文凭据，在应用首次读取时**自动加密回写**，无需手工处理。
- 仍然不要把它提交到任何仓库、分享或备份到云盘——加密只防「文件被顺手拿走」，不防「当前用户下的恶意进程」。
- 在共享电脑上用完，去设置页点 **清除 Key** 和 **清除 Token**。

网页登录产生的 WebView2 缓存位于 `%LOCALAPPDATA%\com.deepseek.monitor.windows\EBWebView`，属于本机运行数据，同样不应提交到仓库。

## 开发

### 环境要求

- Node.js 22+ 与 npm（单测使用 `node --test --experimental-strip-types`）
- Rust 1.77.2+，建议 MSVC 工具链
- Visual Studio Build Tools 2022，勾选 `Desktop development with C++`

### 常用命令

```powershell
git clone https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows.git
cd DeepSeek-Monitor-Windows
npm install
npm run tauri:dev
```

```powershell
npm run tauri:check    # cargo check（全部 target）
npm run check:version  # 校验三处配置里的版本号是否一致
npm test               # 前端单测（类型 + 用例）
npm run lint           # ESLint
npm run build          # 类型检查 + 前端构建
npm run verify         # 本地一键门禁（需 PowerShell 7+）
npm run tauri:build    # 打包 NSIS 安装包
```

`npm run tauri:dev` 与 `npm run tauri:build` 分别是 `tauri dev` / `tauri build` 的封装，会按需探测本机 VS Build Tools 的位置，不需要手动配路径。若想直接调用 Tauri CLI，`npx tauri dev` 效果相同。

版本号散落在 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 三处（Tauri 2 的配置不会去读 `package.json`），发版前跑一次 `npm run check:version` 可以避免打出名字对不上的安装包。

**测试**：前端用 Node 22 内置的 `node --test` 配合 `--experimental-strip-types` 直接跑 TypeScript 测试，**没有引入 vitest / jest 这类测试框架**（因此也没有新增任何 devDependency）。后端就是标准的 `cargo test`：

```powershell
npm test                                  # 前端：tsc -p tsconfig.test.json + node --test
cargo test --manifest-path src-tauri/Cargo.toml --lib   # 后端
```

测试覆盖的是「算错了不容易被发现」的那部分：用量口径（`model_slot` 的模型名映射表、`token_breakdown` 的六类 token 归并与 `PROMPT_TOKEN` 双计边界、`merge_model_slot` 的求和语义）、配置读写（旧配置缺字段回退、损坏配置留证重置、原子写不残留临时文件、凭据加解密与明文迁移）、登录 token 解析（上下文特征匹配、截断输入不崩溃），以及前端的跨月补零与单位换算阈值。改动这些地方时，先跑测试再动手。

本仓库**不再启用 GitHub Actions**。推送前请在本地跑一次 `npm run verify`（PowerShell 7+）：依次执行版本一致性、前端测试/lint/构建，以及 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`。

可选加固：`git config core.hooksPath scripts/hooks` 启用仓库自带的 pre-push 钩子，`git push` 前自动跑一次 `npm run verify`（单次跳过用 `git push --no-verify`）。

安装包产物位于 `src-tauri/target/release/bundle/nsis/`。若报 `Visual Studio Build Tools not found`，请安装 Build Tools 2022 并确认勾选了 C++ 组件。

### 代码结构

```text
DeepSeek-Monitor-Windows/
├── scripts/                     # check-version.mjs、verify.ps1（本地门禁）
├── src/                         # 前端
│   ├── main.tsx                 # 入口
│   ├── App.tsx                  # 路由与全局刷新状态
│   ├── components/              # 主面板、设置、详情、图表等组件
│   ├── format.ts                # 纯格式化 / 日期工具（有单测）
│   ├── format.test.ts           # 上述模块的单测
│   ├── theme.ts / types.ts / usage-api.ts
│   └── styles.css               # 全部样式，含深色 / 浅色两套皮肤
├── src-tauri/                   # 后端
│   ├── src/lib.rs               # 命令装配、托盘、窗口显隐、HTTP 请求
│   ├── src/config.rs            # 配置结构与读写（有单测）
│   ├── src/credentials.rs       # 凭据的 DPAPI 加解密（有单测）
│   ├── src/usage.rs             # 用量口径（有单测）
│   ├── src/token_sync.rs        # 登录 token 抓取与解析（有单测）
│   ├── tauri.conf.json          # 窗口、打包与安全配置
│   └── capabilities/            # Tauri 权限
├── public/assets/               # 图标与静态资源
├── scripts/check-version.mjs    # 发版时校验三处版本号一致
└── screenshots/                 # README 界面截图
```

界面是一个单文件（`main.tsx` + `styles.css`），后端按职责分成了几个小模块但都不大。模型名到界面行的映射集中在 `src-tauri/src/usage.rs` 的 `model_slot()`，改模型或加模型从那里入手——它旁边就是覆盖四类模型名的测试，改完立刻能验证。

## 常见问题

**余额能查到，但用量一直是空的？**
两者用的是不同凭据。余额看 API Key，用量看用量 Token——先确认用量 Token 已经配好了。

**提示「用量不可用」或「未配置用量 Token」？**
用量 Token 过期了。去设置页重新点一次 **网页登录自动同步**，或按方式二手动粘贴。

**登录窗口点了没反应 / 一直显示「等待登录」？**
登录本身需要时间，登录完成后页面才会发请求、token 才会被捕获。如果一直没动静，关掉登录窗口再点一次按钮。

**关掉窗口后程序还在后台？**
这是设计如此——「关闭」是隐藏到托盘。要真正退出，右键托盘图标选「退出」。

**Pro 那一行一直是 ¥0.00？**
说明本月几乎没有用 `deepseek-v4-pro` 调用。V4 Pro 与 V4.1 Flash 是分开统计的两行；若你实际调用的是 Flash，请看上一行。

**用量一直失败，余额却正常？**  
两者接口与凭据不同。按顺序试：① 重新「网页登录自动同步」；② 用方式二粘贴新 Token；③ 稍后重试（平台内部接口可能临时变更）。余额一直可用的话，说明 API Key 正常。

**会不会频繁请求 DeepSeek 的接口？**
余额和用量在三种时机各请求一次：打开面板（含从托盘唤出）、手动刷新、以及你设定的自动刷新周期（默认关闭，开启后最短 1 分钟）。面板收进托盘后自动刷新会暂停，不会在后台轮询。

### 接口失效排查步骤

用量走的是平台内部接口，平台侧调整时可能临时失效。按顺序排查：

1. **看报错文案。** 面板会区分「Token 无效 / 过期」（需重新同步）与「接口暂不可用 / 路径可能变更」（稍后重试）；余额报错与用量 Token 无关。
2. **重新同步 Token。** 设置页点「网页登录自动同步」，登录完成后通常数百毫秒内自动填入；若登录窗口还开着，可多点几次按钮促发刷新。
3. **手动粘贴兜底。** 展开「方式二：手动粘贴 token」，按提示从浏览器取最新 token 粘贴保存。
4. **查诊断日志。** `%LOCALAPPDATA%\com.deepseek.monitor.windows\logs` 记录了每次同步的退出原因与候选校验结果（不含任何凭据内容），可确认卡在哪一步：候选为空、校验未通过、窗口提前关闭或等待超时。
5. **仍无效。** 多半是平台调整了内部接口的路由或鉴权，留意本仓库 Releases 页更新；余额走官方接口，不受影响。

## 版本历史

完整发布记录见 [Releases](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows/releases)。`v1.0.0` – `v1.1.0` 由 [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows) 发布，本仓库自 `v1.2.1` 起接手维护。

### v1.2.4

**同步与性能**

- 登录同步改为监听 WebView2 缓存目录变更（Windows `ReadDirectoryChangesW`）：网页登录落盘后通常数百毫秒内即完成同步（原先最快也要等 1.5 秒的下一轮轮询）；空闲期间不再反复全目录扫描，大缓存目录下 CPU/IO 占用更低。监听失效（如目录被重建）时自动退回原有轮询节奏，不影响可用性。
- 缓存去重表改为惰性 LRU 淘汰：内容频繁变动的「热文件」不会被优先淘汰，淘汰本身也从整段搬移变为 O(1)，长会话下避免反复重读同一批文件。
- 缓存候选改为「先收集后验证」：单个候选 token 的网络校验不再阻塞其余缓存文件的扫描，登录同步的延迟上限更可控。
- Token 校验试调月改为东八区当前月 + 上月，不再固定用历史月份，避免平台归档旧数据后校验恒失效。
- 新增同步链路诊断日志（watcher 退出原因、候选校验结果，不含任何凭据内容），「点同步没反应」类问题可查日志定位。
- 主面板刷新加请求守卫：手动刷新 / 自动刷新 / 托盘唤出并发时，慢的旧响应不再覆盖新数据。
- 未接入的模型名聚合为「其他」行展示（Token / 费用 / 请求数求和），新模型上线初期用量不再「合计对、明细缺」。
- README 增加「接口失效排查步骤」：报错分级、重新同步、手动粘贴、诊断日志位置一次讲清。

**工程**

- `lib.rs` 从 616 行拆分至 82 行（命令层 `commands.rs`、托盘 `tray.rs`、登录同步 `usage_watcher.rs`），命令权限可集中审计。
- 设置页状态与动作拆分至 `settings-state.ts`，组件只留视图；配置写锁新增重入 fail-fast 防护（误用立即报错而非死锁）。
- 全仓接入 Prettier（`npm run format` / verify 门禁检查），并修复 Windows 行尾导致的检查误报。
- 决策记录：不恢复远程 CI（2026-09-26 拍板），推送前以本地 `npm run verify` 为门禁。
- 新增 14 项单元测试锁定缓存监听、LRU 淘汰、试调月口径与配置锁防护（Rust 合计 80 项）。

安装包为 `DeepSeekMonitorWindows_1.2.4_x64-setup.exe`。

### v1.2.3

**产品与体验**

- V4 Pro 恢复为独立模型展示，去掉「逐步下线」标签与说明。
- 用量查询失败时提示更明确（区分 Token 过期 / 接口不可用），引导重新同步或手动粘贴；余额与用量文案分开，避免误以为整机不可用。
- 同步未获取到 Token 时自动展开「手动粘贴」入口。
- 图表支持点按/触摸查看当日明细；设置页可切换深浅色皮肤。
- 开机自启路径写入更安全（含空格的安装目录可正常启动），配置保存失败会回滚自启状态。
- 面板按任务栏所在边（上/下/左/右）贴靠托盘附近显示。

**安全**

- 用量 Token 同步不再经窗口标题传递，降低本机其它程序读取标题的风险。
- 登录窗口仅允许 DeepSeek 站内跳转；敏感命令按窗口校验调用方。

**稳定性**

- 用量统计对 `PROMPT_TOKEN` 与缓存明细采用保守口径，避免重复计算（已用测试锁定）。
- 静默刷新失败时保留上一份用量快照；缓存扫描空转后自动降频，超限按旧优先淘汰。

安装包为 `DeepSeekMonitorWindows_1.2.3_x64-setup.exe`。

### v1.2.2

- **凭据改为加密存储**：API Key 与用量 Token 经 Windows DPAPI 加密后写入 `config.json`。密钥绑定当前用户与机器，配置文件被单独拷走无法解密；旧版本留下的明文凭据在首次读取时自动加密回写。
- **补齐回归测试**：后端 54 项、前端 18 项单元测试，CI 每次推送都会跑。
- **修复详情页柱顶数值被截断**：窗口宽度 356px 时，按日柱状图顶部的数值不再被裁成 `268….`。
- 配置损坏时的备份与回退、刷新语义、图表键盘可访问性等一批细节修缮。
- 安装包为 `DeepSeekMonitorWindows_1.2.2_x64-setup.exe`。

### v1.2.1

- 适配 DeepSeek 2026-09-10 的模型变更：新模型名 `deepseek-flash`（V4.1 Flash），界面名称同步更新。
- 用量统计改为按模型行归并：`deepseek-flash` 与旧名 `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp` 的用量累加到同一行，不再因新旧名字并存而漏统计。
- 适配当时平台对 V4 Pro 的路由策略说明（现已恢复为独立提供，界面无下线提示）。
- 新增未归类 token 类型的兜底统计，计入总量并在图表中单独展示，避免静默漏算。
- 安装包为 `DeepSeekMonitorWindows_1.2.1_x64-setup.exe`。

### v1.1.0（Joyi-code 发布）

- 支持缓存命中、缓存未命中与输出 Token 的明细显示。
- 增加亮色皮肤，可在主面板一键切换并记住选择。
- 设置页增加当前版本号显示。
- 安装包为 `DeepSeekMonitorWindows_1.1.0_x64-setup.exe`。

### v1.0.1（Joyi-code 发布）

- 修复单实例缺失导致的重复多开问题（感谢抖音粉丝群烛阴兄弟反馈）。此前程序已在运行时再次点击图标会不断启动新进程，现在改为唤出已有面板。通过接入 `tauri-plugin-single-instance` 实现。

### v1.0.0（Joyi-code 发布）

- 首个正式版本：余额查询、平台用量统计、消费趋势、Windows 托盘入口、API Key 与用量 Token 管理。

## 来源与致谢

本项目是 DeepSeek Monitor 系列在 Windows 桌面端的延续，**直接承接自 [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows)**，并沿用更早的 macOS 原项目 [JayHome137/DeepSeekMonitor](https://github.com/JayHome137/DeepSeekMonitor) 的产品思路与视觉方向。

| 代次 | 仓库 | 目标平台 | 核心技术 |
| --- | --- | --- | --- |
| 初代 | [JayHome137/DeepSeekMonitor](https://github.com/JayHome137/DeepSeekMonitor) | macOS 菜单栏与 WidgetKit 桌面小组件 | Swift 5.9+、SwiftUI、AppKit、WidgetKit |
| 二代 | [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows) | Windows 桌面端 | Tauri 2、React、TypeScript、Rust |
| 本项目 | [Tsuki-hash/DeepSeek-Monitor-Windows](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows) | Windows 桌面端 | 同上，承接二代继续迭代 |

- **感谢 [@JayHome137](https://github.com/JayHome137/DeepSeekMonitor)**：用 Swift、SwiftUI、AppKit 与 WidgetKit 做出了 macOS 版本，确立了「在桌面角落随手看一眼余额与用量」这个产品形态。没有这个原项目，就没有后面所有版本。
- **感谢 [@Joyi-code](https://github.com/Joyi-code/DeepSeekMonitorWindows)**：把这个想法完整移植到 Windows——重建界面与后端，处理托盘驻留、无边框窗口、WebView2 登录态同步等大量平台细节，并把仓库与安装包完整开源。**本项目的代码基线正是来自这个仓库。**

本仓库接手后的主要工作是跟进 DeepSeek 平台的接口与模型变更、修正用量统计口径，并继续维护 Windows 版本的可用性。本项目不重写实现，而是在二代基线上迭代。

## 许可证与免责声明

MIT License，详见 [LICENSE](LICENSE)，与两代上游项目声明的许可证保持一致。

本项目仅用于学习和研究目的。请遵守 DeepSeek 的使用条款，合理使用相关接口，避免频繁请求。用量接口为网页端内部接口，客户端会携带常见的浏览器/版本请求头以兼容平台侧校验，属非官方调用方式。

DeepSeek 的平台页面结构、登录状态、WebView2 缓存行为和内部用量接口都可能随时变化，本项目不保证长期可用。**API Key 和用量 Token 属于敏感凭据，使用者需自行承担本机存储、账号安全、网络请求和数据展示带来的风险。**
