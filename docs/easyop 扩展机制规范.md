# easyop 扩展机制规范

> **版本** v1.0 · 2026-09-18 · 状态：定案
> **范围** 外接（bridged 小程序 + HTTP 桥）与内嵌（component 组件 + 插件体系）两种集成模式
> **核心约束** 零运行时安装（装 easyop 即可用）；搭配 easyop 的 API 配置和键盘配置
> **本文不含代码改动**，已合并原《应用集成框架方案》《插件体系方案》《评审与内容台落点》三份文档并修正 7 处事实错误

---

## 一、两种集成模式

| | 外接（bridged） | 内嵌（component / 插件） |
|---|---|---|
| 代码在哪 | **外部** exe / 脚本 | **easyop 进程内** webview |
| 配置从哪来 | 通过 HTTP 桥调 easyop 代劳 | **继承宿主**（同进程直接拿） |
| UI 在哪 | easyop 之外 | easyop 之内（独立窗口或面板视图） |
| easyop 的角色 | **能力提供者**（HTTP 桥） | **宿主**（承载 + 提供能力） |
| 适合 | 已有外部应用、批处理脚本、其他语言 | 需要复用 easyop 配置的新功能 |

> **内容台是内置插件**（随 easyop 发布，放 `plugins/builtin/studio/`），不是 `bridged`，不是独立 component——它是 easyop 长出来的器官，不是别人装的外设。详见《内容台规范》。

---

## 二、能力内核（Capability Core）—— 一份实现，三个适配面

### 2.1 问题

原方案中同一批能力被定义了两次（桥 `/llm` 与插件 `eo.llm`），两套名字、两套错误语义、两套权限模型，必然分叉。

### 2.2 方案

```
              ┌─ Capability Core（唯一实现 + 唯一错误码表）─┐
              │  inject / chat / chat_stream / get_selection  │
              │  replace_selection / clipboard_read|write     │
              │  reveal / notify / prompts_list / log          │
              │  dispatch_action（白名单具名动作）             │
              └───────┬──────────────┬──────────────┬────────┘
                      │              │              │
        面1 进程内 eo.*  面2 HTTP bridge   面3 CLI 单次调用
        （组件用）         （外部 exe/脚本）  （cron / 批处理）
```

三条硬约定：
1. **三个面的能力集完全一致**——只是传输方式不同。任何"只有面2有"的端点都是错的。
2. **三面共享同一份错误码表**（12 档，`SELECTOR_DRIFT` 单列一码）。
3. **桥只暴露"宿主能力"，绝不暴露"业务动作"**（发布/生图/导出属于内容台）。理由：桥一旦承载业务，easyop 就变成必须常驻的业务服务。

### 2.3 能力清单

| 能力 | Core 函数 | 对应 easyop 内部 | 真实落点（已核实） |
|------|----------|-----------------|-------------------|
| 注入文本 | `inject(text)` | `text_delivery::deliver` | `text_delivery.rs:40` ✓ |
| LLM 补全 | `chat(messages, opts)` | **需新增** | `llm_client.rs` 现只有 `stream_edit`（:134），**阻塞**，见 §七 F1 |
| LLM 流式 | `chat_stream(messages, opts)` | **需新增** | 同上 |
| 读选中文本 | `get_selection()` | `clipboard_util::get_selected_text` | `clipboard_util.rs:28` ✓ |
| 替换选中文本 | `replace_selection(text)` | `clipboard_util::replace_selected_text` | `clipboard_util.rs:65` ✓ |
| 剪贴板读写 | `clipboard_read() / clipboard_write(text)` | **需新增** | 现有 `clipboard_util` 只做"模拟 Ctrl+C 取选中"，无直接读剪贴板 |
| 打开产物目录 | `reveal(path)` | `ss_open_containing_folder` | `lib.rs:466` ✓（已有命令，未进桥清单） |
| 提示词列表 | `prompts_list()` | `commands::workbench_prompt_*` | v0.6.0 ✓ |
| 派发具名动作 | `dispatch_action(name)` | **需新增白名单入口** | `host_action.rs:166` `handle_host_action` 是私有 fn，见 §七 F4 |
| 日志 | `log(level, msg)` | `log_buffer` | ✓ |

---

## 三、外接：HTTP 桥

### 3.1 设计

| 项 | 设计 | 理由 |
|---|---|---|
| 协议 | HTTP/JSON（`127.0.0.1` 仅本机） | 任何语言都能调 |
| 端口 | 启动时动态分配，写入 `app_config_dir()/bridge.port` | 避免冲突；用 `app_config_dir()`（`%APPDATA%\com.easyop.app`），**不用 `~/.easyop`**（会被 Cadence 污染） |
| 鉴权 | 随机 token，写入 `bridge.token` | 防本机未授权调用 |
| 实现 | 裸 `tokio::TcpListener` | 需加 `tokio` 的 `net` + `io-util` feature（见 §七 F2） |

### 3.2 端点（对齐 Capability Core）

| 端点 | 方法 | 作用 |
|------|------|------|
| `/health` | GET | 探活 + 能力清单 |
| `/inject` | POST | 注入文本到光标处 |
| `/llm` | POST | LLM 补全（继承已配置 Key，**不暴露 Key**） |
| `/llm/stream` | POST | SSE 流式 LLM |
| `/selected` | GET | 读取选中文本 |
| `/replace` | POST | 替换选中文本 |
| `/clipboard` | GET/POST | 剪贴板读写 |
| `/reveal` | POST | 打开目录 |
| `/action` | POST | 派发白名单具名动作 |

> **不提供** `/profile/switch` `/key/trigger`——桥只暴露宿主能力，不暴露业务动作。

### 3.3 `bridged` kind（小程序注册表扩展）

```rust
pub const KINDS: [&str; 5] = ["app", "url", "folder", "component", "bridged"];
```

`bridged` 启动时注入环境变量：`EASYOP_BRIDGE_URL` / `EASYOP_BRIDGE_TOKEN`。外部应用读环境变量即可调桥，easyop 不在则降级独立运行。

---

## 四、内嵌：组件 + 插件

### 4.1 组件（component）—— 进程内窗口

| 项 | 设计 |
|---|---|
| 运行环境 | `WebviewWindowBuilder` 同进程建窗 |
| 配置继承 | 同进程直接拿 `AppState`（LLM/ASR/按键方案/应用库），**只有一份，只有 easyop 能改** |
| 窗口契约 | `decorations(true)` / `always_on_top(false)` / 显示在任务栏 / 单实例 toggle |
| UI 组织 | Vite 第二入口，一份 React 代码两种宿主（独立窗口 + 主窗页签） |

> 内容台不走 component 路线——见 §九 落地顺序，内容台走**内置插件**路线（不做沙箱时与 component 权限相同，但 UI 可独立迭代不编译 Rust）。

### 4.2 插件（plugin）—— VSCode 扩展风格

插件**不只是被加载的页面**，而是能**扩展 easyop 自身行为**的模块：让自己的命令出现在 ⌘SS 搜索里、让自己的动作绑到物理按键上。

**插件清单 `plugin.json`**（对标 VSCode `package.json.contributes`）：

```json
{
  "id": "markdown-tools",
  "name": "Markdown 工具集",
  "main": "index.js",
  "activationEvents": ["onCommand:md.preview", "onKeyword:markdown", "onKey:KEY8"],
  "contributes": {
    "commands": [
      { "id": "md.preview", "title": "Markdown 预览" },
      { "id": "md.format",  "title": "Markdown 格式化" }
    ],
    "tools": [
      { "command": "md.preview", "keywords": ["md", "markdown", "预览"] }
    ],
    "keyActions": [
      { "key": "KEY8", "command": "md.preview", "label": "MD 预览" }
    ],
    "panels": [
      { "id": "md.preview.panel", "title": "Markdown 预览", "html": "preview.html",
        "window": { "width": 800, "height": 600 } }
    ],
    "contextActions": [
      { "when": "hasSelection", "command": "md.format", "label": "格式化选中 MD" }
    ]
  },
  "capabilities": ["inject", "llm", "selected-text", "storage"]
}
```

**插件代码 `index.js`（ESM）**：

```javascript
export function activate(context) {
  const eo = context.api;
  context.registerCommand("md.format", async () => {
    const selected = await eo.get_selection();
    const result = await eo.chat({ prompt: `格式化：\n${selected}` });
    await eo.replace_selection(result.text);
  });
}
export function deactivate() {}
```

### 4.3 已定决策

| 决策点 | 选择 |
|--------|------|
| 插件运行环境 | **隐藏 webview**（每插件一个 `WebviewWindowBuilder.visible(false)`，天然隔离 + 完整 DOM） |
| 按键动作贡献 | **自动加入按键配置页**（`contributes.keyActions` 注入 `NAMED_ACTIONS`） |
| 插件分发 | **手动放目录**（`app_config_dir()/plugins/<id>/`，重启即生效） |

### 4.4 激活机制

| 事件 | 触发时机 |
|------|---------|
| `onCommand:<id>` | ⌘SS 搜到并执行 |
| `onKeyword:<kw>` | 搜索关键词命中 |
| `onKey:<key>` | 物理按键触发 |
| `onSelection` | 选中文本时 |

**懒激活**：未激活时命令仍出现在搜索结果中（从 `plugin.json` 静态读取），执行时才加载 `index.js`。

---

## 五、宿主 API（`eo.*`）—— 三个适配面统一命名

| API | 作用 | 对应 Core |
|-----|------|-----------|
| `eo.inject(text)` | 注入光标处 | `inject` |
| `eo.chat({prompt, system?})` | LLM 补全 | `chat` |
| `eo.chat_stream({prompt})` | 流式 LLM | `chat_stream` |
| `eo.get_selection()` | 读选中文本 | `get_selection` |
| `eo.replace_selection(text)` | 替换选中文本 | `replace_selection` |
| `eo.clipboard_read() / clipboard_write(text)` | 剪贴板读写 | `clipboard_read/write` |
| `eo.reveal(path)` | 打开目录 | `reveal` |
| `eo.prompts_list()` | 提示词列表 | `prompts_list` |
| `eo.dispatch_action(name)` | 派发具名动作 | `dispatch_action` |
| `eo.log.info/warn/error(msg)` | 日志 | `log` |
| `eo.storage.get/set(key, value)` | 插件存储（沙箱） | 每插件独立 `storage.json` |
| `eo.panel.open(id)` | 打开面板 | **注意**：Tauri `WebviewUrl` 只能静态 URL，传运行时数据需用 host 存载荷 + `panel:ready` 握手（见 §七 F7） |

---

## 六、安全模型

### 6.1 当前配置下 capability 无法强制（已实测）

| 实测项 | 结果 | 位置 |
|--------|------|------|
| `withGlobalTauri` | **`true`** | `tauri.conf.json` |
| 自定义命令进 ACL | **否**（89 个命令命中 0 次） | `gen/schemas/acl-manifests.json` |
| CSP | **`null`** | `tauri.conf.json` |

**推论**：用户自放的 HTML 一行 `invoke('get_api_key')` 就能读到 Key。`manifest.capabilities` 只能是文档承诺，无法强制。

### 6.2 要做真沙箱需 4 项改造（全是"改"不是"加"）

1. 为自定义命令定义权限（新建 `src-tauri/permissions/`，89 个命令按能力分组）
2. 组件窗口配独立 capability，只放行必要项
3. 组件窗口不开 `withGlobalTauri`
4. 全局 CSP 从 `null` 变成实际策略

> **这四项的代价与"只需要 1 个组件"的需求严重不匹配**——这正是推迟插件体系的核心理由。

### 6.3 一期妥协

**不做沙箱时，插件隐藏 webview 与组件显示 webview 权限完全相同**（都不受限）。因此一期不做沙箱，插件与组件运行时安全成本一致。capability 声明作为文档约束 + 代码层 best-effort 过滤，不做强制隔离。内容台走内置插件路线（随 easyop 发布），与外部插件走同一套 plugin_host——一次性投资，后续所有插件复用。

---

## 七、事实修正（F1–F7，原方案与源码不符处）

| # | 原断言 | 源码事实 | 修正 |
|---|--------|---------|------|
| **F1** | `llm_client::complete/stream` | **不存在**，只有 `stream_edit`（:134） | **阻塞**。必须先做 `LlmClient::chat/chat_stream` 泛化 |
| **F2** | "裸 tokio，零新依赖" | `Cargo.toml` tokio 无 `net` feature | 需加 `net` + `io-util`；建议允许引 `httparse`（零运行时依赖） |
| **F3** | `/profile/switch` → `state::switch_profile` | 函数不存在 | 真实落点：`commands::set_active_profile` |
| **F4** | `/key/trigger` → "host_action 模拟" | `handle_host_action` 是私有 fn | 需新增白名单派发入口 |
| **F5** | `~/.easyop/plugins/` | `~` 会被 Cadence 污染 | 用 `app_config_dir()`（`%APPDATA%\com.easyop.app`） |
| **F6** | `manifest.icon: "📝"` | `MiniApp` 无 icon 字段 | 图标由 kind 的 CSS 类决定；icon 删或降为可选覆盖 |
| **F7** | `eo.panel.open(id, data)` 传运行时数据 | Tauri `WebviewUrl` 只能静态 URL | 用 host 存"待交付载荷" + `panel:ready` 握手 |

> **"零依赖"叙事修正**：零运行时安装（装 easyop 即可用）✓ 成立；零依赖变更（不动 Cargo.toml）✗ 不成立（F2）。

---

## 八、与现有机制整合

| 现有机制 | 扩展机制如何接 |
|---------|-------------|
| `BUILTIN_TOOLS` | 变成内置插件的 `contributes.tools` |
| `MINI_APP_RENDERERS` | 变成插件的 `contributes.panels` |
| `mini_apps` kind="component" | target 指向组件 id，`launch()` 调 `component_registry::toggle(id)` |
| `host_action` 保留 UUID | 插件 `contributes.keyActions` 映射到保留 UUID 段（`…0009`~`…00ff` 现空） |
| `NAMED_ACTIONS` | **第一期不动态化**（改 6 处，见下）；内容台 6 个动作走静态登记 |
| `ss_palette` 搜索 | 合并 `plugin_registry::search_hits()` |

### 新增一个主机端具名动作要改 6 处

| # | 位置 |
|---|------|
| 1 | `constants.rs` `NAMED_ACTIONS` + `UUID_*` 常量 |
| 2 | `payload.rs::reserved_uuid_for_named`（:18） |
| 3 | `payload.rs::named_for_reserved_uuid`（:35）——**必须成对改** |
| 4 | `src/constants.ts` `NAMED_ACTION_LABELS`（打进 bundle） |
| 5 | `host_action.rs` 分流处（:228 之后） |
| 6 | 前端分组数组 |

> **建议第一期不动态化**：内容台需要 6 个动作，走静态登记即可。将来真有插件生态时，把前端词表从 TS 常量改为运行时拉取（`named_actions_list()`）。

---

## 九、落地顺序

| 阶段 | 内容 | 理由 |
|------|------|------|
| **阶段 0 · 框架级三项前置** ✅ | ① `LlmClient::chat/chat_stream` 泛化 ② 可观测执行器 ③ 能力内核骨架 + 统一错误码表 | 四条线的公共前置 |
| **阶段 1 · plugin_host + eo.\* commands** | 激活层（建隐藏 webview + 注入 `eo.*` + 执行 `activate()`）+ 把能力内核每个函数暴露为 `#[tauri::command]` | 插件路线的一次性前置；写完后续所有插件复用 |
| **阶段 2 · 内容台 = 第一个内置插件** | 纯 JS + `eo.*` API 实现（`eo.chat` / `eo.run` / `eo.storage`），放 `plugins/builtin/studio/` | 用真实插件倒推 `eo.*` 形状，避免凭空设计；后端逻辑推到 JS，改 UI 不编译 Rust |
| **阶段 3 · HTTP 桥** | 能力与面1对齐，服务外部 exe / 批处理 | 能力内核已被内容台验证过 |
| **阶段 4 · 外部插件生态** | 需先做 §六 4 项安全改造（真沙箱） | 有第二个外部插件需求时才做；内置插件不受此限 |

### 为什么 plugin_host 在阶段 1 而非阶段 0

1. **阶段 0 是"不依赖插件的基础设施"**——LLM 泛化、执行器、错误码表，即使最终不走插件路线也需要。
2. **plugin_host 必须有真实插件消费才能压准形状**——内容台是第一个消费者，但 plugin_host 要先于内容台可用。因此阶段 1 写 plugin_host + eo.* commands，阶段 2 用内容台验证。
3. **不做沙箱时插件与组件运行时成本相同**——§六.3 已论证。"插件很重"只在"要做真沙箱"时成立，一期不成立。

### 什么时候做真沙箱（阶段 4 前置）

有**外部**插件需求时——用户自放第三方插件，此时才需 §六 4 项改造。内置插件（随 easyop 发布）不受此限，因为作者就是 easyop 自己。

---

## 十、内置工具迁移（阶段 3 时做）

| 现有工具 | 迁移后 | 收益 |
|---------|--------|------|
| calculator / todo / screenshot / translate | `builtin/*/` | 从 `ss_palette.html`（100KB+）拆出，瘦身到 ~30KB |
| studio（内容台） | `builtin/studio/` | 纳入统一注册表 |

---

## 十一、与 MCP 的关系（远期）

| | easyop 插件 | MCP server |
|---|---|---|
| 运行 | easyop 进程内（隐藏 webview） | 独立进程（JSON-RPC） |
| 发现 | `plugin.json.contributes` | `tools/list` |
| 适合 | UI 交互、轻量工具 | 重计算、独立服务 |

远期 easyop 可作为 MCP client 接入外部 MCP server，把 MCP tools 映射为插件 commands。