# DeepSeek Monitor Windows

A small panel that pins your DeepSeek balance and usage to the Windows system tray. One click on the icon shows how much credit is left, what you spent today and this month, and how each model is consuming tokens and cache.

Windows 10 / 11 only. Built with Tauri 2, React 18, and Rust.

**This project is not an official DeepSeek product and is not affiliated with DeepSeek.**

## Screenshots

| Dark skin | Light skin |
| :---: | :---: |
| <img src="screenshots/dashboard-dark.png" width="330" alt="DeepSeek Monitor Windows dashboard, dark skin"> | <img src="screenshots/dashboard-light.png" width="330" alt="DeepSeek Monitor Windows dashboard, light skin"> |

*Both screenshots use sample data, not a real account.*

## Why this exists

DeepSeek only publishes a balance endpoint. There is no account-level usage API. The usage page on the web console shows spending, but not a per-model breakdown of cache hits, cache misses, and output — and it cannot sit on your desktop for a glance.

This project combines three things:

- **Balance** — calls `/user/balance` directly with your official API key.
- **Usage** — reuses a usage token derived from a web login session to call the platform's internal endpoints, retrieving monthly spending, total tokens, request count, and cache detail.
- **Presentation** — merges both into a narrow tray-resident panel that stays off the taskbar.

## Install and get started

### Install

Download `DeepSeekMonitorWindows_1.2.2_x64-setup.exe` from [Releases](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows/releases/latest). Installing over an older version does not require uninstalling it first.

Requirements: Windows 10 or Windows 11, plus the Microsoft Edge WebView2 Runtime (included with Windows 11; install separately on Windows 10 if missing).

### Set your API key

Open the dashboard → Settings in the top right → the `API Key` section. Paste your key (created on the API Keys page of the DeepSeek platform) and click **验证并保存** (Validate and save). Once it validates, the Settings page shows your current balance and the dashboard starts refreshing.

### Sync usage

Usage needs a second credential. See "Two credentials, don't mix them" below.

### Day-to-day use

- The close button in the top right **hides the panel to the tray**; it does not quit.
- Left-click the tray icon: show or hide the panel.
- Right-click the tray icon: show the dashboard, or quit.

## Two credentials, don't mix them

This is the most common pitfall: balance and usage are **not** queried with the same thing.

|  | API key | Usage token |
| --- | --- | --- |
| Where it comes from | DeepSeek platform → API Keys page | Session token after signing in to platform.deepseek.com |
| What it is for | Account balance | Usage and spending |
| Interchangeable | No | No |

**Why usage needs a token**: DeepSeek publishes no account-level usage API, so the app reuses the same endpoints the web console calls, which requires your login session. The token is a session credential — as sensitive as the API key, and it expires.

### Method 1: automatic sync through web login

In the `用量同步 Token` section of Settings, click **网页登录自动同步** (Sync via web login) and sign in through the DeepSeek login window that opens.

The app hooks the network requests the page makes, reads the Bearer token straight out of the `Authorization` header, verifies that it can actually call the usage endpoint, and only then saves it and refreshes the data.

> Signing in takes time; the page only issues requests once login completes. If nothing happens, close the login window and click the button again (while waiting, the button reads 等待登录).

### Method 2: paste it manually (fallback)

Click **方式二：手动粘贴 token** to expand. Sign in to platform.deepseek.com in a browser, press F12 to open the console, and run:

```js
JSON.parse(localStorage.userToken).value
```

Copy the returned string, paste it into the field, and click **保存 Token**.

**The usage token expires. When usage stops loading, just sync it again.**

## Model naming and billing

DeepSeek launched V4.1 Flash on 2026-09-10 and renamed the model from `deepseek-v4-flash` to `deepseek-flash`. There is a catch: **the old model names were not disabled — they were routed to the new model for compatibility**, so during the transition a single billing period can contain both old and new names. Here is how this project handles it:

| `model` returned by the platform | Which row it lands in |
| --- | --- |
| `deepseek-flash` | V4.1 Flash |
| `deepseek-v4-flash` | V4.1 Flash (**summed**) |
| `deepseek-v4-flash-vision-exp` | V4.1 Flash (**summed**) |
| `deepseek-v4-pro` | V4 Pro |

Names that map to the same row are **summed, not overwritten** — otherwise, whenever old and new names coexist, part of the usage would be silently dropped with no visible sign in the interface.

Two more things worth knowing:

- **V4 Pro is available.** Tracked separately from V4.1 Flash, with its own tokens, cost, and cache stats.
- **Unclassified tokens are counted anyway.** V4.1 Flash takes image input natively. If the platform reports a token type this project has not classified yet, those tokens are still counted in the total and shown as 其他（未归类） in the charts, rather than being silently dropped.

## What the numbers on screen mean

### Dashboard

| Where | Meaning |
| --- | --- |
| Account balance | Total credit from the official `/user/balance` endpoint, including granted and topped-up amounts |
| Today / This month | Spending fields from the platform usage endpoint |
| Model rows | That model's token total and cost this month, plus tokens per yuan (T/¥); the line below is the cache hit rate |
| Cache detail chart | A 7-day stacked bar chart with three segments: input (cache hit), input (cache miss), and output |

### Detail page

Click any model row on the dashboard. It shows that model's request count, token total, and a per-day token breakdown.

### Settings

Configure and clear the API key and usage token, autostart on login, refresh interval (1 / 5 / 30 minutes, or 1 hour), and the current version.

## Where credentials are stored

```text
%APPDATA%\DeepSeekMonitorWindows\config.json
```

Both the API key and the usage token are stored in this file, **encrypted with Windows DPAPI** (`CryptProtectData`; the ciphertext is kept as a base64 string prefixed with `DSM1:`). DPAPI ties the key to the current Windows user and this machine, which means:

- Copying `config.json` alone to another machine or another user account makes the credentials **undecryptable**; the app then asks you to re-enter them (all other settings are preserved).
- Plaintext credentials left behind by older versions (v1.2.1 and earlier) are **re-encrypted automatically** the first time the app reads them — no manual step needed.
- Still do not commit it to any repository, share it, or back it up to cloud storage — encryption only guards against the file being taken, not against a malicious process running as the current user.
- On a shared computer, clear credentials when you are done using **清除 Key** and **清除 Token** in Settings.

The WebView2 cache created by the web login lives at `%LOCALAPPDATA%\com.deepseek.monitor.windows\EBWebView`. It is local runtime data and should not be committed either.

## Development

### Requirements

- Node.js 18+ and npm
- Rust 1.77.2+, MSVC toolchain recommended
- Visual Studio Build Tools 2022 with `Desktop development with C++`

### Commands

```powershell
git clone https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows.git
cd DeepSeek-Monitor-Windows
npm install
npm run tauri:dev
```

```powershell
npm run tauri:check    # cargo check (all targets)
npm run check:version  # verify the version number is consistent across config files
npm test               # frontend tests (types + cases)
npm run build          # type check + frontend build
npm run tauri:build    # build the NSIS installer
```

`npm run tauri:dev` and `npm run tauri:build` are thin wrappers around `tauri dev` / `tauri build` that detect your Visual Studio Build Tools installation automatically, so no paths need to be configured by hand. Calling `npx tauri dev` directly works just as well.

The version number lives in three places — `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` (Tauri 2 does not read `package.json`). Running `npm run check:version` before a release prevents shipping an installer whose name does not match its contents.

**Tests**: the frontend runs TypeScript tests directly through Node 22's built-in `node --test` with `--experimental-strip-types`, so **no test framework such as vitest or jest is involved** — and no devDependency was added for it. The backend is plain `cargo test`:

```powershell
npm test                                  # frontend: tsc -p tsconfig.test.json + node --test
cargo test --manifest-path src-tauri/Cargo.toml --lib   # backend
```

Coverage targets the things that break silently: usage accounting (the model-name table in `model_slot`, the six token kinds and the `PROMPT_TOKEN` double-count boundary in `token_breakdown`, the additive semantics of `merge_model_slot`), config I/O (missing fields in old configs falling back to defaults, corrupt configs being quarantined and reset, atomic writes leaving no temp file, credential encryption and plaintext migration), login token parsing (context-feature matching, truncated input not crashing), and the frontend's cross-month padding and unit thresholds. Change any of those and run the tests first.

Pushes and pull requests run `.github/workflows/ci.yml`: version consistency, frontend tests, type check and frontend build, plus `cargo fmt --check`, `cargo check`, `cargo clippy -- -D warnings`, and `cargo test`.

The installer is produced in `src-tauri/target/release/bundle/nsis/`. If you see `Visual Studio Build Tools not found`, install Build Tools 2022 and confirm the C++ workload is selected.

### Code layout

```text
DeepSeek-Monitor-Windows/
├── .github/workflows/           # CI (version consistency, frontend tests + build, cargo check/clippy/test)
├── src/                         # Frontend
│   ├── main.tsx                 # The entire UI: dashboard, settings, detail page
│   ├── format.ts                # Pure formatting / date helpers (unit tested)
│   ├── format.test.ts           # Tests for the above
│   └── styles.css               # All styles, including dark and light skins
├── src-tauri/                   # Backend
│   ├── src/lib.rs               # Command wiring, tray, window visibility, HTTP requests
│   ├── src/config.rs            # Config structs and I/O (unit tested)
│   ├── src/credentials.rs       # DPAPI encryption of credentials (unit tested)
│   ├── src/usage.rs             # Usage accounting (unit tested)
│   ├── src/token_sync.rs        # Login token capture and parsing (unit tested)
│   ├── tauri.conf.json          # Window, bundle, and security configuration
│   └── capabilities/            # Tauri permissions
├── public/assets/               # Icons and static assets
├── scripts/check-version.mjs     # verifies the three version numbers agree
└── screenshots/                 # README screenshots
```

The UI is one file (`main.tsx` plus `styles.css`); the backend is split into a few small modules. The mapping from model name to dashboard row lives in `model_slot()` in `src-tauri/src/usage.rs` — that is where to start when changing or adding a model, and the test covering all four model names sits right next to it.

## FAQ

**Usage keeps failing but balance is fine.**
Different APIs and credentials. Re-sync the usage token, paste a token manually, or retry later. If balance works, the API key is fine.

**The balance loads but usage is always empty.**
They use different credentials. Balance comes from the API key; usage comes from the usage token. Make sure the token is configured.

**It says usage is unavailable, or that no usage token is configured.**
The usage token expired. Go to Settings and either click **网页登录自动同步** again or paste it manually with method 2.

**The login window does nothing, or it stays on 等待登录.**
Signing in takes time, and the page only issues requests once login completes — that is when the token is captured. If nothing happens, close the login window and click the button again.

**The program is still running after I close the window.**
That is by design: closing hides the panel to the tray. To actually quit, right-click the tray icon and choose quit.

**Why is the Pro row always ¥0.00?**
That means almost no `deepseek-v4-pro` calls this month. V4 Pro and V4.1 Flash are tracked separately; if you called Flash, look at the row above.

**Does it hammer the DeepSeek endpoints?**
Balance and usage are each requested once in three situations: when you open the panel (including bringing it back from the tray), when you refresh manually, and on whatever auto-refresh interval you set (off by default, minimum 1 minute). Auto-refresh pauses while the panel is hidden in the tray, so nothing polls in the background.

## Version history

See [Releases](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows/releases) for the complete history. `v1.0.0` through `v1.1.0` were published by [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows); this repository took over from `v1.2.1`.

### v1.2.3

- Clearer usage-fetch errors with guidance to re-sync or paste the token; balance errors stay separate.
- Expands manual paste automatically when sync fails to capture a token.
- Tap/click chart bars for daily details; theme toggle in Settings.

### v1.2.2

- **Credentials are now encrypted at rest**: the API key and usage token are written to `config.json` encrypted with Windows DPAPI. The key is tied to the current user and machine, so a copied config file cannot be decrypted; plaintext credentials left by older versions are re-encrypted automatically on first read.
- **Regression tests added**: 54 backend and 18 frontend unit tests, run by CI on every push.
- **Fixed truncated bar-top values** on the detail page: at a 356px window width the daily bar chart labels are no longer clipped to `268….`.
- A batch of smaller fixes: corrupt-config quarantine and fallback, refresh semantics, chart keyboard accessibility.
- Installer `DeepSeekMonitorWindows_1.2.2_x64-setup.exe`.

### v1.2.1

- Adapted to the DeepSeek model change of 2026-09-10: the new model name is `deepseek-flash` (V4.1 Flash), with labels updated in the interface.
- Usage is now merged by model row: `deepseek-flash` and the retired names `deepseek-v4-flash` and `deepseek-v4-flash-vision-exp` accumulate into a single row, so nothing is lost while old and new names coexist.
- Documented the platform’s V4 Pro routing policy at the time (now available again as its own model; the UI has no phase-out badge).
- Added a fallback for unclassified token types: they are counted in the total and shown separately in the charts instead of being silently dropped.
- Installer `DeepSeekMonitorWindows_1.2.1_x64-setup.exe`.

### v1.1.0 (published by Joyi-code)

- Added detailed cache hit, cache miss, and output token statistics.
- Added a light skin that can be toggled from the dashboard and persists.
- Added the current version to the Settings page.
- Installer `DeepSeekMonitorWindows_1.1.0_x64-setup.exe`.

### v1.0.1 (published by Joyi-code)

- Fixed an issue that allowed multiple instances to run at once (thanks to Zhuyin from the Douyin community for the report). Previously, clicking the icon while the app was already running spawned another process; it now brings the existing panel to the foreground, implemented with `tauri-plugin-single-instance`.

### v1.0.0 (published by Joyi-code)

- First stable release: balance queries, platform usage statistics, spending trends, a Windows tray entry, and API key and usage token management.

## Origin and acknowledgements

This project continues the DeepSeek Monitor family on the Windows desktop. It builds **directly on [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows)** and follows the product idea and visual direction of the earlier macOS original, [JayHome137/DeepSeekMonitor](https://github.com/JayHome137/DeepSeekMonitor).

| Generation | Repository | Target platform | Core technology |
| --- | --- | --- | --- |
| Original | [JayHome137/DeepSeekMonitor](https://github.com/JayHome137/DeepSeekMonitor) | macOS menu bar and WidgetKit desktop widget | Swift 5.9+, SwiftUI, AppKit, WidgetKit |
| Second | [Joyi-code/DeepSeekMonitorWindows](https://github.com/Joyi-code/DeepSeekMonitorWindows) | Windows desktop | Tauri 2, React, TypeScript, Rust |
| This project | [Tsuki-hash/DeepSeek-Monitor-Windows](https://github.com/Tsuki-hash/DeepSeek-Monitor-Windows) | Windows desktop | Same as the second generation, continued |

- **Thanks to [@JayHome137](https://github.com/JayHome137/DeepSeekMonitor)** for building the macOS version with Swift, SwiftUI, AppKit, and WidgetKit, and for establishing the idea of glancing at your balance and usage from the corner of your desktop. Without that original project, none of the later versions would exist.
- **Thanks to [@Joyi-code](https://github.com/Joyi-code/DeepSeekMonitorWindows)** for porting that idea fully to Windows — rebuilding the interface and backend, handling a large amount of platform detail such as tray residency, frameless windows, and WebView2 login-state synchronization, and open-sourcing both the repository and the installer. **The code baseline of this project comes from that repository.**

Since taking over, the main work here has been following DeepSeek platform API and model changes, correcting usage statistics, and keeping the Windows build usable. This project does not rewrite the implementation; it iterates on the second-generation baseline.

## License and disclaimer

MIT License. See [LICENSE](LICENSE). Consistent with the license declared by both upstream generations.

This project is intended only for learning and research. Follow the DeepSeek terms of use, use the relevant endpoints responsibly, and avoid frequent requests.

DeepSeek page structures, login state, WebView2 cache behavior, and internal usage endpoints may change at any time, and long-term availability is not guaranteed. **API keys and usage tokens are sensitive credentials. Users are responsible for the risks associated with local storage, account security, network requests, and displayed data.**
