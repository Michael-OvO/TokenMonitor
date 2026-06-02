# CHANGELOG

## v0.13.1 — Settings 重构 + SSH 搜索扩展 + FloatBall 修复

### UI
- **Settings → Visibility 卡片**：Header Tabs / Models / Remote Devices 合并为单张 `.card`，每项显示 "x of y" 计数，折叠展开
- **Status Displays** 改用单 `.card`（移除 `.group`），预览图移至最顶端
- **FloatBall 预览修复**：移除 `.fb-ball` 的渐变圆圈背景，改为透明（与真实 FloatBall 一致）
- **Settings header** 添加 `box-shadow` 分隔头部与滚动内容
- **SSH hosts** 全部使用 toggle 滑块（移除 Add 按钮），未注册 host 首次开启时自动注册
- **Cost Alert / Model Change Stats** 放回 Display card

### SSH
- **远程搜索扩展**：Codex 搜索从固定 `~/.codex/sessions` 改为 glob `~/.codex*`，匹配所有 codex 变体目录
- **Sync 进度反馈**：逐 host 显示 "x of y servers connected"，完成后显示 "Finished syncing in Xs"

### 修复
- `fix(floatball)`: 速率利用率回退逻辑——当主窗口过期时，回退到任意未过期窗口（与主窗口行为统一）

## v0.12.3 — Visibility 合并 + 窗口尺寸记忆

### UI
- **Settings → Visibility 合并为单卡片**：`Provider` / `Model Visibility` / `SSH Hosts` 三个面板现在共享一张 card，视觉上与 `Display` 分组一致，行间用 `border-top` 分隔
- 修复折叠面板（Cursor / Permissions）收起时会漏出内部内容的问题——`grid-template-rows: 0fr/1fr` 方案在嵌套子组件根节点时 `:global(*)` 作用域不稳，回退到显式 `max-height` + `overflow: hidden`

### 启动体验
- **窗口尺寸记忆**：新增 `window_state.height` 字段持久化到 `settings.json`，启动时在 `loadInitialData()` 之前立刻把窗口恢复到上次关闭时的高度，消除"先小后大"的跳变
- **首帧 shrink 门控**：`resizeOrchestrator` 新增 `initialContentReady` 闸门，首次数据就绪前屏蔽所有 shrink 方向的 resize 请求；首帧渲染后再放闸并做一次同步修正
- `resizeOrchestrator` 新增 `onHeightApplied` 回调，每次 `set_window_size_and_align` 成功后节流 400ms 写回磁盘

## v0.12.2 — 模型家族过滤 + 图表排序

- `fix(usage)`: 按 model family 过滤跨集成聚合行，避免 Claude/Codex/Cursor 计费交叉污染
- `feat(ui)`: 统一模型名格式化并扩展品牌调色板
- `feat(chart)`: 按使用成本对图例模型排序
- `fix(ci)`: 移除 `config.rs` 中未用的 `AtomicBool/Ordering` 引用

## v0.12.1 — 签名与 CI 修复

- `fix(macos)`: 修复 Windows-only 代码路径中的 `SetWindowRgn` 导入路径
- `fix(ci)`: 使用平台相关的 SHA256SUMS 文件名，避免 release 资产冲突
- `fix(ci)`: 默认将 release 发布为非草稿
- `fix(ci)`: `cargo fmt` 应用到 `window.rs`
- `fix(ci)`: 移除 clippy 报出的多余 `return`

## v0.12.0 — Codex 速率限制 + View Transitions + SSH 加固

### 新功能
- 新增 Codex CLI 速率限制面板（`rate_limits/codex.rs`）
- 新增 View Transitions（`view-enter-right` / `view-enter-left` / `view-enter-fade`），在 settings / calendar / devices / single-device 之间切换时做滑入动画
- 新增 Skeleton loading 骨架屏（`HiddenModelsSettings` 等）
- 新增 `usage_access_enabled` 原子开关，首次启动通过 welcome card 显式授权后才开始解析本地日志

### 安全与正确性
- SSH alias 验证加固、缓存失效修正
- Windows 空白时区 / SHA256 / 磁盘 I/O 错误路径强化
- 应用内告警添加一键关闭按钮
- SSH 缓存改为流式读取，降低 Windows 上的内存峰值

## v0.11.1 — Cursor Rate Limits & Updater Signing Fixes

- 新增 Cursor IDE 速率限制支持（`rate_limits/cursor.rs`），通过 Cursor API 获取计划用量和支出限额
- 新增 `secrets/cursor.rs` 管理 Cursor access token（OS keyring 优先，文件回退）
- 修复 Windows/Linux 上 updater 签名密钥缺失时跳过签名的问题
- 修复 workflow 条件表达式中无效的 secrets 引用
- 修复通过 `GITHUB_ENV` 设置 updater 签名密钥

## v0.11.0 — Major Module Refactor & Dynamic Exchange Rates

- 大模块拆分重构（`float_ball` → `float_ball/mod.rs` + `layout.rs`）
- 新增动态汇率（`usage/exchange_rates.rs`，Frankfurter API，24h TTL 缓存）
- 新增 `usage/device_aggregation.rs` 远程设备数据聚合
- 新增 `usage/claude_parser.rs` Claude Code 专用深度解析器（含 change-event 分类和 subagent scope 检测）
- 新增 `usage/archive.rs` 持久化每小时聚合存储，防止日志删除导致数据丢失
- 新增 `usage/openrouter.rs` OpenRouter 动态定价

## v0.10.x — Permissions, Keychain Hardening, SSH Fixes

- 新增权限系统（`permissions/surfaces.ts`、`permissions/keychain.ts`、`PermissionDisclosure.svelte`）
- 新增首次启动欢迎卡片（`WelcomeCard.svelte`），含速率限制和自启动的 opt-in 开关
- macOS Keychain 读取切换到 Security.framework，避免旧版 Keychain 弹窗
- 一次性交互式 Keychain 提示（`feat(macos): one-time interactive Keychain prompt`）
- 硬化权限披露和 TCC 作用域修复
- SSH 远程设备去重修复、jq 依赖移除
- Claude 会话刷新和速率限制窗口修复

## v0.9.0 — Pie Chart & Capitalization Cleanup

- 新增饼图模式（`feat: add pie chart mode with model-share breakdown`）
- 标准化 section label 大小写

## v0.8.0 — Auto-Updater

- 完整的应用内自动更新系统
- 新增 `updater/` Rust 模块（state、persistence、scheduler）
- 新增 `stores/updater.ts` 前端 store
- 新增 `UpdateBanner.svelte` 应用内更新横幅
- 托盘图标红色 badge dot（更新可用时显示）
- OS 通知（每版本去重，6h 检查间隔，指数退避）
- Skip / Later / Update Now 操作
- CI/CD 更新：生成 `latest.json` manifest，构建 AppImage、NSIS updater 产物
- 新增 `docs/testing/auto-update.md` 手动测试矩阵

## v0.7.x — Cache Tiers, Fast Mode, SSH Archive

- Claude fast mode 作为独立模型支持（单独定价）
- 缓存层级数据管道暴露，统一缓存定价
- Web search 追踪
- SSH archive 更新
- 速率限制百分比系统统一

## v0.6.0 — Cross-Platform Architecture Overhaul

> 基准对比：[Michael-OvO/TokenMonitor](https://github.com/Michael-OvO/TokenMonitor) main 分支 (v0.5.0)
>
> 差异规模：158 files changed, +22,583 / -10,787

---

### 一、Rust 后端模块化重构

上游将所有 IPC 命令、速率限制、解析逻辑集中在少数大文件中。本 fork 按领域拆分为独立子模块，每个文件职责单一。

| 上游（monolithic） | 本 fork（modular） | 说明 |
|---|---|---|
| `commands.rs` (2189 行) | `commands/usage_query.rs` (1356 行) | 用量数据查询 |
| | `commands/calendar.rs` (488 行) | 日历热力图 |
| | `commands/config.rs` (177 行) | 设置持久化 |
| | `commands/tray.rs` (440 行) | 托盘标题/利用率 |
| | `commands/ssh.rs` (1058 行) | SSH 远程设备管理 |
| | `commands/float_ball.rs` (977 行) | 浮球悬浮窗状态 |
| | `commands/period.rs` (98 行) | 时间范围选择 |
| | `commands/logging.rs` (24 行) | 日志级别控制 |
| `rate_limits.rs` (1124 行) | `rate_limits/claude.rs` (303 行) | Claude OAuth + API |
| | `rate_limits/claude_cli.rs` (526 行) | CLI 探测（全平台） |
| | `rate_limits/codex.rs` (140 行) | Codex 会话解析 |
| | `rate_limits/http.rs` (257 行) | 共享 HTTP 客户端 |
| | `rate_limits/mod.rs` (125 行) | 模块入口 |
| `change_stats.rs` | `stats/change.rs` | 重命名 + 微调 |
| `subagent_stats.rs` | `stats/subagent.rs` | 重命名 + 接口优化 |
| `tray_render.rs` | `tray/render.rs` (含 126 行改动) | 托盘渲染升级 |
| `parser.rs` (根级) | `usage/parser.rs` | 移入 usage/ 模块 |
| `pricing.rs` (根级) | `usage/pricing.rs` (+85 行) | 移入 + 扩展定价 |
| `integrations.rs` (根级) | `usage/integrations.rs` | 移入 usage/ 模块 |
| _(不存在)_ | `usage/ccusage.rs` (986 行) | **新增**：Claude Code 专用解析器 |
| _(不存在)_ | `usage/litellm.rs` (331 行) | **新增**：LiteLLM 动态定价（24h TTL） |
| _(不存在)_ | `usage/ssh_remote.rs` (469 行) | **新增**：SSH 远程同步 + 缓存 |
| _(不存在)_ | `usage/ssh_config.rs` (485 行) | **新增**：SSH config 自动发现 |
| _(不存在)_ | `logging.rs` (108 行) | **新增**：tracing + rolling file appender |

**净新增 Rust 代码：~10,287 行（含重构迁移）**

---

### 二、跨平台支持（Windows + Linux）

上游仅支持 macOS。本 fork 实现了完整的 Windows 和 Linux 支持。

#### 新增平台模块 `platform/`

| 文件 | 行数 | 功能 |
|---|---|---|
| `platform/mod.rs` | 78 | 跨平台工具函数（`clamp_window_to_work_area` 等） |
| `platform/windows/taskbar.rs` | 585 | **Windows 任务栏面板**：GDI 嵌入式面板，位于应用列表和系统托盘之间 |
| `platform/windows/window.rs` | 263 | Windows 窗口定位，对齐任务栏 |
| `platform/macos/mod.rs` | 13 | macOS 特定代码 |
| `platform/linux/mod.rs` | 2 | Linux 入口（预留） |

#### 平台差异矩阵

| 功能 | macOS | Windows | Linux |
|---|---|---|---|
| 系统托盘 | 菜单栏 | 系统托盘 | 系统托盘 |
| 费用显示 | `set_title()` 文字 | Tooltip 悬浮 | Tooltip 悬浮 |
| 速率限制(Claude) | OAuth Keychain + API | CLI 探测 | CLI 探测 |
| 毛玻璃效果 | 可切换 | 不可用（不透明） | 不可用（不透明） |
| Dock 图标 | 可切换 | N/A | N/A |
| 自启动 | LaunchAgent | 注册表 | XDG autostart |
| 安装包 | DMG (签名+公证) | NSIS .exe | .deb |

#### 前端平台检测

- 新增 `src/lib/utils/platform.ts`：从 User Agent 检测 macOS/Windows/Linux，结果缓存
- UI 组件通过 `isMacOS()` 条件渲染 macOS 专属设置（毛玻璃、Dock 图标）
- `set_glass_effect`、`set_window_surface`、`set_dock_icon_visible` 在非 macOS 平台作为 noop 保留

---

### 三、FloatBall 浮球悬浮窗（全新功能）

一个独立的 always-on-top 可拖拽悬浮球，显示实时费用概览。

| 文件 | 类型 | 行数 |
|---|---|---|
| `float-ball.html` | HTML 入口 | 新增 |
| `src/float-ball.ts` | TS 入口 | 新增 |
| `src/lib/components/FloatBall.svelte` | Svelte 组件 | 473 |
| `src-tauri/src/commands/float_ball.rs` | Rust 后端 | 977 |
| `vite.config.ts` | 多入口配置 | +9 |

- 独立的 Vite 入口点（`rollupOptions.input` 多入口）
- 独立挂载目标 `#float-ball`，与主 `App.svelte` 窗口完全隔离
- 支持拖拽定位、展开/收起状态持久化

---

### 四、SSH 远程设备管理（全新功能）

通过 SSH 从远程机器抓取使用日志，统一合并到本地视图。

**后端：**

| 文件 | 行数 | 功能 |
|---|---|---|
| `usage/ssh_remote.rs` | 469 | 每主机同步状态跟踪 + 文件缓存 |
| `usage/ssh_config.rs` | 485 | 自动从 `~/.ssh/config` 发现主机 |
| `commands/ssh.rs` | 1058 | SSH 主机 CRUD + 同步操作 IPC |

**前端：**

| 文件 | 行数 | 功能 |
|---|---|---|
| `DevicesView.svelte` | 419 | 设备列表 + 统计概览 |
| `SingleDeviceView.svelte` | 205 | 单设备用量详情 |
| `SshHostsSettings.svelte` | 285 | SSH 设备管理设置 UI |
| `views/deviceStats.ts` + 测试 | 113 | 设备数据聚合 |

---

### 五、定价引擎增强

| 改进 | 说明 |
|---|---|
| LiteLLM 动态定价 | 新增 `usage/litellm.rs` (331 行)，从 LiteLLM API 获取实时价格，24h TTL 缓存 |
| Claude Code 专用解析 | 新增 `usage/ccusage.rs` (986 行)，独立于通用 parser 的 Claude Code 深度解析 |
| 定价表扩展 | `pricing.rs` +85 行，覆盖更多模型 |

---

### 六、前端架构重组

#### 目录结构变化

```
# 上游（扁平结构）
src/lib/
  traySync.ts, trayTitle.ts, footerView.ts, rateLimitsView.ts,
  windowAppearance.ts, calendar-utils.ts, rateLimitMonitor.ts

# 本 fork（按领域分目录）
src/lib/
  tray/sync.ts, tray/title.ts
  views/footer.ts, views/rateLimits.ts, views/rateLimitMonitor.ts, views/deviceStats.ts
  window/appearance.ts, window/sizing.ts
  utils/calendar.ts, utils/format.ts, utils/logger.ts, utils/platform.ts
  types/index.ts
```

#### 新增 / 重构组件

| 组件 | 行数 | 说明 |
|---|---|---|
| `HeaderTabsSettings.svelte` | 162 | Tab 头部设置（从 Settings 拆出） |
| `HiddenModelsSettings.svelte` | 200 | 模型过滤设置（从 Settings 拆出） |
| `ThemeSettings.svelte` | 163 | 主题设置（从 Settings 拆出） |
| `TrayConfigSettings.svelte` | 381 | 托盘配置（全新） |
| `SshHostsSettings.svelte` | 285 | SSH 主机管理（全新） |
| `DevicesView.svelte` | 419 | 设备视图（全新） |
| `SingleDeviceView.svelte` | 205 | 单设备视图（全新） |
| `FloatBall.svelte` | 473 | 浮球组件（全新） |
| `Settings.svelte` | 大幅瘦身 | 拆分为上述子组件 |

#### 删除

| 文件 | 说明 |
|---|---|
| `ResizeDebugOverlay.svelte` (190 行) | 移除调试组件 |
| `resizeDebug.ts` (211 行) + 测试 (145 行) | 移除调试工具 |

#### 新增工具

| 文件 | 行数 | 说明 |
|---|---|---|
| `resizeOrchestrator.ts` | 606 | 窗口大小调整编排器（替代 resizeDebug） |
| `uiStability.ts` | 54 | UI 稳定性工具 |
| `utils/logger.ts` | 54 | 前端日志通过 IPC 路由到 Rust 文件写入器 |
| `utils/platform.ts` | 38 | 平台检测 + 缓存 |
| `bootstrap.test.ts` | 49 | 启动流程测试 |

#### Store 增强

- `usage.ts`：+229 行，SSH 设备数据合并、payload 缓存优化
- `rateLimits.ts`：+17 行，per-provider 可配置刷新间隔
- `settings.ts`：+69 行，新增 SSH 主机配置、浮球状态等持久化字段
- `types/index.ts`：+91 行，SSH、FloatBall、设备统计等新类型定义

---

### 七、日志系统（全新）

| 文件 | 说明 |
|---|---|
| `src-tauri/src/logging.rs` (108 行) | `tracing` + rolling file appender，后端日志 |
| `src-tauri/src/commands/logging.rs` (24 行) | 运行时日志级别控制 IPC |
| `src/lib/utils/logger.ts` (54 行) | 前端日志通过 IPC 路由到同一 Rust 文件写入器 |

前后端日志统一写入平台 app-data 目录下的滚动日志文件，支持运行时动态调整日志级别。

---

### 八、构建系统 & CI/CD

#### 新增模块化构建系统 `build/`

| 文件 | 说明 |
|---|---|
| `build/index.mjs` | 构建入口 |
| `build/lib/cli.mjs` | CLI 参数解析 |
| `build/lib/platform.mjs` | 平台特定构建逻辑 |
| `build/lib/platform.test.mjs` | 构建脚本测试 |
| `build/lib/workflow.mjs` (297 行) | 构建工作流编排 |
| `build/config/tauri.{linux,macos,windows}.json` | 平台特定 Tauri 配置 |

#### CI 更新

- `.github/workflows/ci.yml`：适配跨平台构建矩阵
- `.github/workflows/release.yml`：新增 Windows NSIS / Linux .deb 构建流程

#### package.json 新增脚本

```json
"build:installers": "node build/index.mjs"
```

---

### 九、文档变更

| 变更 | 说明 |
|---|---|
| 新增 `docs/tutorial.md` (338 行) | 用户教程 |
| 新增 `docs/ecl/code-optimization.yaml` (376 行) | 工程变更记录 |
| 删除 6 个设计文档 (~4,480 行) | `change-stats-plan`、`subagent-stats-plan` 等设计文档移除（已完成实施） |

---

### 变更统计

| 类别 | 新增文件 | 新增行数 | 删除行数 |
|---|---|---|---|
| Rust 后端 | 23 | ~10,287 | ~3,548 |
| 前端组件/工具 | 15 | ~4,822 | ~1,623 |
| 构建系统 | 7 | ~536 | — |
| CI/CD | 2 (改) | ~102 | — |
| 文档 | 2 | ~714 | ~4,480 |
| **合计** | **~47 新文件** | **+22,583** | **-10,787** |
