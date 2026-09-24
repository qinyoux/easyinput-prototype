# easyop 插件系统设计文档

> **版本** v2.0 · 2026-09-20 · 状态：定案
> **合并自** 《easyop 扩展机制规范》《插件集成指南》《内容台规范》《路径A-插件页签接口契约》《ARCHITECTURE》
> **反映** v0.17.0 当前状态：`registerPage` 已实现、`contributes.pages` 不再被丢弃、侧栏页签 + ⌘SS 双入口并存

---

## 一、系统总体架构

### 1.1 顶层目录结构

```
easyinput-prototype/
├── src/                     # 前端（React + Vite + TypeScript）
├── src-tauri/               # 后端（Rust + Tauri 2）
│   ├── src/                 # Rust 源码（全部业务逻辑）
│   ├── Cargo.toml           # Rust 依赖清单
│   ├── tauri.conf.json      # Tauri 应用配置
│   ├── capabilities/        # Tauri 2 权限声明
│   └── gen/                 # Tauri 自动生成的 schema（已 gitignore）
├── public/                  # 静态资源（浮层 HTML）
├── docs/                    # 文档
├── plugins/                 # 插件源码
│   ├── builtin/             # 内置插件（随 easyop 发布）
│   │   ├── studio/          # 内容台插件
│   │   └── demo-quick-note/ # 示范插件
│   └── README.md            # 插件目录约定
├── scripts/                 # 工具脚本（版本管理 / 插件部署）
├── package.json             # 前端依赖与脚本
└── README.md                # 项目说明
```

### 1.2 前端架构（`src/`）

```
src/
├── main.tsx                 # React 挂载入口
├── App.tsx                  # 主壳：侧栏 + 页签主体 + 插件页签容器
├── api.ts                   # Tauri invoke 封装
├── types.ts                 # TypeScript 类型定义
├── constants.ts             # 输入 ID、动作中文标签、固定常量
├── styles.css               # 全局样式
├── pluginHost.ts            # 前端宿主：插件激活 + registerPage + registerCommand
└── components/
    ├── Sidebar.tsx           # 侧栏：静态导航 + 插件页签
    ├── Workbench.tsx         # 工作台：使用记录 + 提示词库 + 词库管理
    ├── ProfilePanel.tsx      # 方案库
    ├── RemapEditor.tsx       # 按键映射
    ├── AppLibrary.tsx        # 本机应用
    ├── VoicePanel.tsx        # 语音输入
    ├── VoiceSettings.tsx     # 语音设置
    ├── PluginPageContainer.tsx  # 插件页签容器（v0.17.0 新增）
    └── LogPanel.tsx          # 日志面板
```

### 1.3 后端架构（`src-tauri/src/`）

| 分组 | 模块 | 职责 |
|------|------|------|
| **入口** | `main.rs`, `lib.rs` | 程序启动、Tauri 命令注册、托盘、浮层、PTT 调度 |
| **命令层** | `commands.rs` | 前端 invoke 的后端入口 |
| **数据模型** | `model.rs`, `state.rs` | ActionSpec / Profile / AppEntry 定义 + 持久化 |
| **固件通信** | `payload.rs`, `transport.rs`, `device.rs`, `constants.rs` | HID 枚举、配置下发、回执解析 |
| **Host Action** | `host_action.rs`, `launcher.rs`, `system_apps.rs` | 后台监听 → UUID 查找 → 打开目标 |
| **语音** | `ptt_hook.rs`, `session.rs`, `audio.rs`, `asr.rs`, `text_delivery.rs` | PTT → 会话 → 麦克风 → ASR → 文本注入 |
| **LLM** | `llm_client.rs`, `voice_edit.rs`, `clipboard_util.rs` | LLM 流式编辑 / 选中文本替换 |
| **插件** | `plugin_registry.rs`, `plugin_host.rs` | 发现层 + 激活层 + `eo.*` 能力层 |
| **搜索** | `ss_palette.rs`, `mini_apps.rs`, `pinyin.rs` | ⌘SS 面板 / 小程序注册表 / 拼音匹配 |
| **工具** | `validate.rs`, `crc16.rs`, `log_buffer.rs` | 校验 / CRC16 / 日志缓冲 |

### 1.4 三通道通信架构

| 通道 | 协议 | 方向 | 用途 |
|------|------|------|------|
| **HID** | USB/BLE HID Feature Report 0x10/0x11 | 双向 | 按键配置下发 + Host Action 回执 |
| **Wi-Fi UDP** | EIAU/EIHB/EICC/EICA（端口 17333） | 双向 | 16kHz PCM 音频流 + 心跳 + 流控 |
| **云端 ASR** | WebSocket WSS | APP→云端 | 火山引擎流式语音识别 |

### 1.5 技术栈

| 层 | 技术 |
|----|------|
| 前端 | React 18 + Vite 5 + TypeScript 5 |
| 桌面 | Tauri 2 |
| 后端 | Rust（edition 2021） |
| Windows FFI | `windows` crate（HID / 键盘钩子 / SendInput / ShellExecuteW） |
| 异步 | tokio（rt-multi-thread） |
| WebSocket | tokio-tungstenite（ASR） |
| HTTP | reqwest（LLM SSE） |
| 音频 | cpal（麦克风采集） |

---

## 二、插件系统架构

### 2.1 两种集成模式

| | 外接（bridged） | 内嵌（plugin） |
|---|---|---|
| 代码在哪 | **外部** exe / 脚本 | **easyop 进程内** webview |
| 配置从哪来 | 通过 HTTP 桥调 easyop | **继承宿主**（同进程直接拿） |
| UI 在哪 | easyop 之外 | easyop 之内（页签或独立窗口） |
| 适合 | 已有外部应用、批处理脚本 | 需要复用 easyop 配置的新功能 |

> 内容台是**内置插件**（随 easyop 发布，放 `plugins/builtin/studio/`），不是 bridged，不是独立 component。

### 2.2 三层分离架构

```
┌─ 发现层 plugin_registry.rs ──────────────────────────────┐
│  启动时扫 app_config_dir()/plugins/<id>/plugin.json       │
│  解析 → 校验 → 内存注册表（static Mutex）                  │
│  对外：search_hits() / all_pages() / find_loaded()        │
└──────────────────────────────────────────────────────────┘
                          ↓ 命令被触发 / 页签被点开
┌─ 激活层 plugin_host.rs + pluginHost.ts ───────────────────┐
│  Rust: 读 index.js → emit("plugin:activate", {code, ...}) │
│  前端: listen → 定义 eo.* → blob import(code) →           │
│        activate(context) → exec(commandId) / renderPage   │
└──────────────────────────────────────────────────────────┘
                          ↓ 调用宿主能力
┌─ 能力层 eo.* Tauri commands ──────────────────────────────┐
│  eo_get_selection / eo_replace_selection / eo_inject      │
│  eo_storage_get / eo_storage_set / eo_log_*               │
│  eo_chat / eo_run                                         │
└──────────────────────────────────────────────────────────┘
```

| 层 | 职责 | 代码位置 | 状态 |
|---|---|---|---|
| **发现层** | 扫目录、读清单、校验、检索 | `plugin_registry.rs` | ✅ |
| **激活层** | 读入口代码、推给前端、执行 activate | `plugin_host.rs` + `pluginHost.ts` | ✅ |
| **能力层** | 10 个 `eo.*` Tauri commands | `plugin_host.rs` | ✅ |
| **接入层 · ⌘SS 搜索** | 插件命令进搜索结果 | `ss_palette.rs` | ✅ |
| **接入层 · 侧栏页签** | `contributes.pages` → 侧栏导航 | `Sidebar.tsx` + `App.tsx` | ✅ v0.17.0 |
| **接入层 · 按键配置** | `contributes.keyActions` 绑物理键 | — | ⬜ |
| **沙箱** | capability 强制隔离 | — | ⬜ |

### 2.3 能力内核（Capability Core）

```
              ┌─ Capability Core（唯一实现 + 唯一错误码表）─┐
              │  inject / chat / chat_stream / get_selection  │
              │  replace_selection / clipboard_read|write     │
              │  reveal / notify / prompts_list / log          │
              │  dispatch_action（白名单具名动作）             │
              └───────┬──────────────┬──────────────┬────────┘
                      │              │              │
        面1 进程内 eo.*  面2 HTTP bridge   面3 CLI 单次调用
```

三条硬约定：
1. **三个面的能力集完全一致**——只是传输方式不同
2. **三面共享同一份错误码表**（12 档，`SELECTOR_DRIFT` 单列一码）
3. **桥只暴露"宿主能力"，绝不暴露"业务动作"**

### 2.4 落地路线图

| 阶段 | 内容 | 状态 |
|------|------|------|
| **阶段 0** · 框架级前置 | LLM 泛化 + 执行器 + 能力内核 | ✅ |
| **阶段 1** · plugin_host + eo.* | 激活层 + 10 个 Tauri commands | ✅ |
| **阶段 2** · 内容台 = 第一个内置插件 | 纯 JS + `eo.*` 实现 | 进行中 |
| **阶段 2.5** · 侧栏页签 | `registerPage` + `contributes.pages` | ✅ v0.17.0 |
| **阶段 3** · HTTP 桥 | 服务外部 exe / 批处理 | ⬜ |
| **阶段 4** · 外部插件生态 | 真沙箱（4 项安全改造） | ⬜ |

---

## 三、插件接口定义与规范

### 3.1 插件清单 `plugin.json`

对标 VSCode `package.json.contributes`：

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "description": "一句话说明",
  "main": "index.js",
  "activationEvents": [
    "onCommand:my.action",
    "onKeyword:关键词",
    "onKey:KEY8",
    "onPage:my.page"
  ],
  "contributes": {
    "commands": [
      {
        "id": "my.action",
        "title": "执行动作",
        "keywords": ["执行", "run"]
      }
    ],
    "tools": [
      {
        "command": "my.action",
        "subtitle": "副标题",
        "keywords": ["工具关键词"]
      }
    ],
    "pages": [
      {
        "id": "my.page",
        "title": "页面标题",
        "icon": "board",
        "order": 50,
        "keywords": ["页面", "page"]
      }
    ]
  },
  "capabilities": ["storage", "selected-text", "llm", "inject", "log", "runner"]
}
```

#### 字段说明

| 字段 | 必需 | 说明 |
|---|---|---|
| `id` | ✅ | 全局唯一，建议与目录名一致 |
| `name` | ✅ | 展示名 |
| `main` | ✅ | 入口文件，**相对路径**（禁止 `..` 与绝对路径） |
| `version` | — | 版本号 |
| `description` | — | 一句话说明 |
| `activationEvents` | — | 激活时机。**当前只收录，不消费**——懒激活由命令/页签触发驱动 |
| `contributes.commands` | — | 命令声明：`id` / `title` / `keywords` |
| `contributes.tools` | — | 把命令挂进 ⌘SS 内置工具区 |
| `contributes.pages` | — | 页签声明（v0.17.0 已实现），见 §3.4 |
| `capabilities` | — | 声明需要的能力。**当前无法强制隔离**，仅作文档约束 |

#### `pages` 字段规则（v0.17.0 已实现）

| 字段 | 必需 | 类型 | 规则 |
|---|---|---|---|
| `pages[].id` | ✅ | string | 全局唯一，建议 `<pluginId>.<name>`；**不得与 `commands[].id` / `tools[].command` 重复** |
| `pages[].title` | ✅ | string | 页签显示名。超过 6 个汉字截断加省略号 |
| `pages[].icon` | — | string | 图标**名**（非 SVG 字符串）。内置集：`board` `note` `spark` `gear` `doc` `chat`；未知名 → 默认图标，**不报错** |
| `pages[].order` | — | number | 排序权重，越小越靠前，缺省 **100**。静态页签占 0–49，插件页从 50 起 |
| `pages[].keywords` | — | string[] | 供 ⌘SS 搜索 |

#### 搜索可见性规则

- 声明了 `tools` → **只有被 `tools` 引用的命令**进 ⌘SS 搜索
- 没声明 `tools` → 全部命令都可搜到

#### 命名空间铁律

`<id>.<action>` 的 `<id>` 必须全局唯一。两个插件都叫 `studio.*` 会导致 `find_plugin_by_command` 只认第一个命中的。

### 3.2 插件入口 `index.js`

ESM 单文件，自包含（blob import 后不能再 import 相对路径文件）：

```javascript
export function activate(context) {
  const eo = context.api;

  // 注册命令（⌘SS 里一次性动作）
  context.registerCommand('my.action', async () => {
    const text = await eo.get_selection();
    const result = await eo.chat(
      [{ role: 'user', content: `处理：${text}` }],
      { temperature: 0.3 }
    );
    await eo.replace_selection(result);
    eo.log.info('完成');
  });

  // 注册页签（常驻界面，v0.17.0 已实现）
  context.registerPage('my.page', function (container) {
    // container: HTMLElement —— 宿主已清空、已给好最终尺寸
    renderInto(container);
    return {
      refresh() { /* 可选：宿主切回本页时调用 */ },
      dispose() { /* 可选：切走 / 卸载时调用 */ },
    };
  });
}

export function deactivate() {}
```

#### `context` 对象

| 属性 | 类型 | 说明 |
|---|---|---|
| `context.api` | `eo` | 宿主能力 API，见 §3.3 |
| `context.pluginId` | `string` | 当前插件 id |
| `context.pluginDir` | `string` | 插件目录绝对路径 |
| `context.registerCommand(id, fn)` | `function` | 声明命令实现 |
| `context.registerPage(id, renderFn)` | `function` | 声明页签渲染函数（v0.17.0） |

### 3.3 宿主 API（`eo.*`）

#### 已实现 API

| API | 对应 Tauri 命令 | 说明 |
|-----|----------------|------|
| `eo.get_selection()` | `eo_get_selection` | 读选中文本 |
| `eo.replace_selection(text)` | `eo_replace_selection` | 替换选中文本 |
| `eo.inject(text)` | `eo_inject` | 注入光标处 |
| `eo.storage.get(key)` | `eo_storage_get` | 读插件存储（返回 `null` 表示不存在） |
| `eo.storage.set(key, value)` | `eo_storage_set` | 写插件存储（原子写，每插件独立 `storage.json`） |
| `eo.log.info/warn/error(msg)` | `eo_log_*` | 日志 |
| `eo.chat(messages, opts)` | `eo_chat` | LLM 补全（非流式） |
| `eo.run(program, args, opts)` | `eo_run` | 子进程执行 |

#### `eo.chat` 参数

```typescript
// messages
[{ role: "user", content: "你好" }, ...]

// opts（可选）
{ system: "系统提示词", temperature: 0.3, max_tokens: 1000, clean: true }

// 返回：string
```

#### `eo.run` 参数

```typescript
eo.run(program: string, args?: string[], opts?: {
  timeoutSecs?: number, cwd?: string,
})
// 返回 { exit_code, stdout, stderr, timed_out, cancelled }
```

#### 尚未实现的 API

| API | 影响 | 现状替代 |
|---|---|---|
| `eo.chat_stream` | 拿不到流式增量 | `eo.chat` 一次性返回 |
| `eo.panel.open` | 开不了独立窗口 | 主窗内 DOM 浮层 |
| `eo.fs.write` / `paths` | 无法自己写文件 | 自写脚本 + `eo.run` |
| `eo.reveal` / `eo.search` | 打开产物目录 / 搜文件 | 无 |
| `eo.clipboard_read` / `clipboard_write` | 剪贴板读写 | 无 |
| `eo.dispatch_action` | 插件动作绑不到物理键 | `contributes.keyActions` 也还没消费 |
| `eo.config` | 读不到 easyop 配置 | 只能硬编码路径 |

### 3.4 页签接口契约（`registerPage`）

> v0.17.0 已实现。此契约已冻结，变更需双方确认并升 major 版本。

#### 五条铁律

| # | 铁律 | 原因 |
|---|---|---|
| **P1** | 渲染函数**同步返回**（返回值可为 `undefined` 或 lifecycle 对象，**不可为 Promise**） | 宿主按同步调用处理 |
| **P2** | **不得使用 `position:fixed`** | 页签里宿主负责定位 |
| **P3** | **不得把 `container` 移出 DOM** | 容器生命周期归宿主 |
| **P4** | 样式只作用于**容器内部**：用插件自己的 class 前缀 + 注入 `<style>` | 宿主不承诺任何 class 契约 |
| **P5** | 同一 `pageId` 重复注册 → **后注册覆盖先注册** | 热重载 / 重复 activate 的幂等 |

#### `registerPage` 与 `registerCommand` 的关系

并存、互不影响。同一插件可以既有页签（常驻界面）又有命令（⌘SS 里一次性动作）。**⌘SS 命令路径必须继续可用**——页签是新增入口，不是替换入口。

#### 「同步返回」不等于「立刻有内容」

P1 要求渲染函数同步返回，但内容可以异步填充（骨架同步画好，数据随后 `await eo.storage.get()` 填进去）。宿主**不得**假定 `renderFn()` 返回后容器里已有内容。

### 3.5 校验规则（加载期即挡）

坏插件**不会拖垮其他插件**：加载失败只记 warn 并跳过。校验项：

- `id` / `name` / `main` 非空
- `main` 不越界（无 `..` / 无绝对路径）
- 入口文件存在
- 命令 id 非空且不重复
- `tools` 不得引用未声明的命令
- `pages[].id` 非空 · 页内 id 不重复 · **不与 `commands[].id` 重复**（v0.17.0）
- 清单 ≤ 256KB

---

## 四、插件加载与生命周期管理

### 4.1 生命周期

| 阶段 | 时机 | 行为 |
|------|------|------|
| **发现** | easyop 启动 / `plugins_rescan` | 扫目录、读 `plugin.json`、校验、进内存注册表 |
| **懒激活** | 命令首次被触发 / 页签首次被点开 | `emit("plugin:activate")` → 前端 blob import → `activate(context)` |
| **执行** | 每次命令触发 | 已激活则直接调注册的 fn；未激活先走激活 |
| **页签渲染** | 页签被点开 | 激活 → `getPluginPageRenderer` → `renderFn(container)` → 返回 lifecycle |
| **页签切走** | 切到其他页签 | `lifecycle.dispose?.()` |
| **停用** | easyop 退出 / 插件移除 | 调 `deactivate()`（当前未实现主动调用） |

### 4.2 激活机制

| 事件 | 触发时机 |
|------|---------|
| `onCommand:<id>` | ⌘SS 搜到并执行 |
| `onPage:<pageId>` | 侧栏页签被点开（v0.17.0，仅声明，实际由 `plugin_activate_page` 驱动） |
| `onKeyword:<kw>` | 搜索关键词命中 |
| `onKey:<key>` | 物理按键触发 |
| `onSelection` | 选中文本时 |

**懒激活**：未激活时命令/页签仍出现在搜索结果和侧栏中（从 `plugin.json` 静态读取），首次触发时才加载 `index.js`。

### 4.3 页签激活流程（v0.17.0）

```
用户点侧栏插件页签
  → App.tsx 渲染 PluginPageContainer
  → invoke("plugin_trigger_activate", { pluginId })
  → Rust emit("plugin:activate", { id, dir, commandId: "" })
  → pluginHost.ts: blob import(index.js) → activate(context)
  → context.registerPage(pageId, renderFn) 存入 pageRenderers Map
  → PluginPageContainer 取 renderFn → renderFn(container) → 返回 lifecycle
  → 切走时 lifecycle.dispose?.()
```

**幂等**：已激活的插件再次 `plugin_trigger_activate` → 直接返回，不重复 `activate()`。

### 4.4 向前兼容

`Contributes` **没有** `deny_unknown_fields`，未识别字段被 serde 静默忽略。

| 时刻 | `plugin.json` 有 `pages` | 宿主是否支持 | 结果 |
|---|---|---|---|
| 插件先做完 | ✅ | ❌ | 旧宿主静默忽略 → 插件照常工作 |
| 宿主后做完 | ✅ | ✅ | 插件一行不改，页签自动出现 |
| 宿主先做完 | ❌ | ✅ | `pages` 为空 → 只剩静态页签 |

> **不要给 `Contributes` 加 `deny_unknown_fields`**——那会让并行开发这条路堵死。

---

## 五、插件间通信与依赖管理

### 5.1 通信机制：Tauri Commands（进程内 invoke）

插件 JS 运行在主窗浏览器上下文，通过 `invoke()` 调用 Rust 侧 `#[tauri::command]`：

| 优点 | 缺点 |
|------|------|
| 零网络开销（进程内消息） | 插件必须运行在 easyop 进程内 |
| 类型安全（Rust 端 serde） | 无法被外部进程调用 |
| 无需鉴权 | — |

### 5.2 HTTP 桥（阶段 3 规划，面向外部应用）

| 项 | 设计 |
|---|---|
| 协议 | HTTP/JSON，`127.0.0.1` 仅本机 |
| 端口 | 启动时动态分配，写 `app_config_dir()/bridge.port` |
| 鉴权 | 随机 token，写 `bridge.token` |

| 端点 | 方法 | 作用 |
|------|------|------|
| `/health` | GET | 探活 + 能力清单 |
| `/inject` | POST | 注入文本 |
| `/llm` | POST | LLM 补全（不暴露 Key） |
| `/llm/stream` | POST | SSE 流式 LLM |
| `/selected` | GET | 读选中文本 |
| `/replace` | POST | 替换选中文本 |
| `/clipboard` | GET/POST | 剪贴板读写 |
| `/reveal` | POST | 打开目录 |
| `/action` | POST | 派发白名单动作 |

> **不提供** `/profile/switch` `/key/trigger`——桥只暴露宿主能力，不暴露业务动作。

### 5.3 通信方式对比

| 方式 | 延迟 | 隔离 | 适用场景 |
|------|------|------|--------|
| **Tauri invoke**（当前） | ~0.1ms | 无 | 内嵌插件 ✅ |
| **HTTP 桥**（规划） | ~1ms | 进程级 | 外部应用 |
| MCP（远期） | ~1ms | 进程级 | 外部 MCP server |

### 5.4 与现有机制整合

| 现有机制 | 插件如何接 | 状态 |
|---------|-----------|------|
| `BUILTIN_TOOLS` | 变成内置插件的 `contributes.tools` | — |
| `MINI_APP_RENDERERS` | 变成插件的 `contributes.panels` | — |
| `host_action` 保留 UUID | 插件 `contributes.keyActions` 映射保留 UUID 段 | ⬜ |
| `NAMED_ACTIONS` | 第一期不动态化；内容台 6 个动作走静态登记 | — |
| `ss_palette` 搜索 | 合并 `plugin_registry::search_hits()` | ✅ |
| MCP server（远期） | easyop 作为 MCP client，把 MCP tools 映射为插件 commands | — |

### 5.5 与 MCP 的关系（远期）

| | easyop 插件 | MCP server |
|---|---|---|
| 运行 | easyop 进程内（隐藏 webview） | 独立进程（JSON-RPC） |
| 发现 | `plugin.json.contributes` | `tools/list` |
| 适合 | UI 交互、轻量工具 | 重计算、独立服务 |

---

## 六、安全模型

### 6.1 当前状态：无沙箱

| 配置项 | 值 | 位置 |
|--------|---|---|
| `withGlobalTauri` | `true` | `tauri.conf.json` |
| CSP | `null` | `tauri.conf.json` |
| 自定义命令进 ACL | 否（89 个命令命中 0 次） | `gen/schemas/acl-manifests.json` |

**风险**：

| 风险 | 影响 |
|------|------|
| 插件读 LLM API Key | `invoke('get_api_key')` 可读 |
| 插件调任意 Tauri 命令 | 无 ACL 拦截 |
| 插件写任意文件 | `eo.run` 可执行任意程序 |
| 插件间隔离 | 无，可访问其他插件的命令表 |

### 6.2 真沙箱改造路径（4 项，阶段 4 前置）

1. 为自定义命令定义权限（`src-tauri/permissions/`，按能力分组）
2. 插件窗口配独立 capability，只放行必要项
3. 不开 `withGlobalTauri`
4. 全局 CSP 从 `null` 变成实际策略

> 有**外部**插件需求时才做。内置插件（随 easyop 发布）不受此限。

### 6.3 页签安全铁律

1. **`icon` 只接受「名字」**，绝不接受内联 SVG / HTML 字符串
2. **宿主不 eval 插件下发的 HTML**。渲染只走 `renderFn(container)`
3. **不要给 `Contributes` 加 `deny_unknown_fields`**

---

## 七、内容台插件规范

### 7.1 定位

内容台是 easyop 的内置插件，不是独立应用，也不是编译进 easyop 的 component。它随 easyop 发布（`plugins/builtin/studio/`），通过 `eo.*` API 继承 LLM/ASR/按键方案/应用库。

> 为什么是插件而非 component：**不做沙箱时两者运行时权限相同，但插件路线把后端逻辑推到 JS，改 UI 不编译 Rust**。

### 7.2 与 easyop 的关系

| 继承项 | eo.* API | 说明 |
|--------|----------|------|
| LLM 配置 | `eo.chat` / `eo.chat_stream` | 复用已配置通道，不再问 API Key |
| 文本注入 | `eo.inject` | 生成完直接注入光标处 |
| 选中文本 | `eo.get_selection` / `eo.replace_selection` | 选中素材送进内容台 |
| 子进程执行 | `eo.run` | 调 `pub.py` / `gen_figs.py` / `qa_check.py` |
| 本地存储 | `eo.storage` | 台账（topics.json / runs.json） |
| Host Action | `contributes.keyActions` | 物理键直驱看板 |

**关键约束：配置只有一份，且只有 easyop 能改。**

### 7.3 入口与窗口契约

#### 四条入口

| # | 入口 | 触发 | 状态 |
|---|------|------|------|
| ① | 侧栏页签 | 点侧栏「内容台」 | ✅ v0.17.0 |
| ② | ⌘SS 面板搜索 | 输入「内容台」「选题」 | ✅ |
| ③ | ⌘SS 图标区 | 面板空闲时点图标 | ✅ |
| ④ | 物理键 / 旋钮 | 按键触发 `host_action:<保留UUID>` | ⬜ |

#### 插件命令

| 命令 id | 作用 |
|---------|------|
| `studio.toggle` | 开合窗口（幂等） |
| `studio.state` | 取当前状态 |
| `studio.topics.list` / `save` / `remove` | 台账增删改 |
| `studio.stage.advance` | 推进阶段 |
| `studio.gen.title` / `body` / `humanize` / `figures` | 生成类 |
| `studio.export` | 导出本地成品包 |
| `studio.task.cancel` | 取消当前任务 |

### 7.4 本地数据层

```
%APPDATA%\com.easyop.app\studio\
├── topics.json      # 选题台账
├── runs.json        # 运行记录
└── publish.json     # 二期：草稿映射
```

`eo.storage` 提供 KV 读写（每插件独立 `storage.json`）；`schema_version` 字段做版本迁移。

### 7.5 七项功能路径

| # | 功能 | 数据流 | 失败表现 |
|---|------|--------|---------|
| 1 | 灵感收集 | 来源 → 采集器 → `Vec<Inspiration>` → 转选题 | 失败明确提示并跳过 |
| 2 | 情报卡 | 表单 → 校验 → 写 `Topic.intel` | 凑不满 3 条角度 → 退回 |
| 3 | 正文 AI 填充 | 选题 + 情报卡 → `eo.chat` → 950 字正文 | Key 未配 → 报错 |
| 4 | 去 AI 味 | 正文 → humanizer → 四层自检 | L4 不过 → 重写段落 |
| 5 | 配图生成 | 叠字或 AI 生图 → `qa_check` | 按 item 报错不中断 |
| 6 | 文章排版 | `文章.md` → HTML → 交付包 | 导出失败保留源稿 |
| 7 | 台账 | 本地 JSON → 可选平台回写 | 无票 → 跳过 |

### 7.6 硬件直驱看板

| UUID | 具名动作 | 行为 | 期 |
|------|---------|------|---|
| `…0009` | `studio` | 开合内容台 | 一期 |
| `…000a` | `studio_prev` | 旋钮左旋：上一张 | 一期 |
| `…000b` | `studio_next` | 旋钮右旋：下一张 | 一期 |
| `…000c` | `studio_confirm` | 推进到下一阶段 | 一期 |
| `…000d` | `studio_gen` | 生成正文 | 一期 |
| `…000e` | `studio_export` | 导出成品包 | 一期 |
| `…000f` | `studio_publish` | 发公众号草稿 | 二期 |

**固件零改动**——新增动作需改 6 处（`constants.rs`/`payload.rs`×2/`constants.ts`/`host_action.rs`/前端分组）。

### 7.7 执行器

通过 `eo.run` 调子进程，自动注入 UTF-8 环境。动作白名单：

| 动作 | argv | 期 |
|------|------|---|
| 校验稿件 | `pub.py check <post>` | 一期 |
| 排版导出 | `pub.py export --post <post> --out <dir>` | 一期 |
| 叠字配图 | `gen_figs.py --spec <spec.json>` | 一期 |
| 结构自检 | `qa_check.py <png…>` | 一期 |
| 发草稿 | `pub.py publish wechat --save-draft` | 二期 |

### 7.8 二期：公众号回补

手动配图 7 张约 7~14 分钟；自动 `upload_body_image()` 逐张上传 + 插到光标处。一期预留 `Publisher` trait + `publish.json` 字段。

---

## 八、调试机制与错误处理

### 8.1 错误处理策略

| 层 | 策略 | 代码位置 |
|---|---|---|
| 发现层 | 坏插件不拖垮其他插件：加载失败只记 warn 并跳过 | `plugin_registry.rs` |
| 校验 | id/name/main 非空 · main 不越界 · 入口存在 · 命令不重复 · pages 不冲突 · ≤ 256KB | `plugin_registry.rs` |
| 激活层 | `activate()` 抛异常 → 记 error → 标记失败，不影响其他插件 | `pluginHost.ts` |
| 命令执行 | fn 抛异常 → 记 error → 返回 null | `pluginHost.ts` |
| 页签渲染 | renderFn 抛异常 → 容器显示错误 + `eo_log_error` | `PluginPageContainer.tsx` |
| 宿主 API | 每个 `eo_*` 返回 `Result<T, String>` | `plugin_host.rs` |

**失败不静默**（页签三条分支）：
- `plugin_trigger_activate` 失败 → 容器显示"插件「X」激活失败：<原因>"
- 取不到 renderFn → 显示"页签「Y」未注册渲染函数"
- renderFn 抛异常 → catch 后显示错误

### 8.2 调试与监控

| 手段 | 用法 |
|------|------|
| **日志面板** | `eo.log.*` 输出带 `[plugin]` 前缀 |
| **`storage.json`** | 直接看 `%APPDATA%\com.easyop.app\plugins\<id>\storage.json` |
| **`plugins:list`** | `npm run plugins:list` 干跑列出将部署的插件 |
| **`plugins:deploy --dry`** | 只演练不写盘 |
| **验证台** | 仿内容台 `test/harness.mjs`，`npm run plugins:test` |
| **Tauri devtools** | `npm run tauri dev` 时 F12 |
| **性能 profiling** | `getPluginStats()` 返回各插件 activate 耗时 / 命令调用次数 |

### 8.3 性能影响

| 环节 | 开销 | 评估 |
|------|------|------|
| 启动扫描 | 读 N 个 `plugin.json` | 可忽略（2 插件 < 1ms） |
| 懒激活 | blob import + `activate()` | ~10-50ms，仅一次 |
| 命令执行 | 已激活后调 fn | 零开销 |
| `eo.*` invoke | Tauri IPC | ~0.1ms/次 |
| `eo.chat` | 调 LLM API | 网络延迟主导 |
| `eo.run` | spawn 子进程 | ~50ms |

### 8.4 版本兼容性

- `plugin.json` 有 `version` 字段，但**当前无兼容性检查**
- **建议**：插件在 `capabilities` 里声明所需能力，宿主激活前检查
- **breaking change**：`eo.*` API 签名变更属于 `major` 版本

---

## 九、当前已知问题及后续优化方向

### 9.1 v0.17.0 已知问题

| # | 问题 | 影响 | 优先级 |
|---|------|------|--------|
| 1 | `eo_run` 无动作白名单 | 可执行任意程序 | 高 |
| 2 | `eo.chat_stream` 未暴露 | 内容台拿不到流式增量 | 中 |
| 3 | `studio.gen.figures` 未实现 | 配图生成不可用 | 中 |
| 4 | 硬编码绝对路径（`CP_DIR`/`PY_EXE`） | 跨机器不可移植 | 中 |
| 5 | `contributes.keyActions` 未消费 | 物理键直驱不可用 | 低 |
| 6 | 无沙箱隔离 | 插件可读 API Key / 调任意命令 | 低（内置插件信任边界内） |
| 7 | 页签模式未在真机验证 | 仅编译通过 | 高 |

### 9.2 后续优化方向

#### 近期

- **真机验证页签模式**：启动 easyop → 侧栏出现"内容台" → 点开激活 → 台账可用
- **`eo.chat_stream` 暴露**：内容台流式生成依赖此 API
- **`eo_run` 白名单**：动作枚举，argv 由模板生成
- **硬编码路径消除**：改用 `eo.config` 或环境变量

#### 中期

- **HTTP 桥（阶段 3）**：能力内核已被内容台验证，服务外部 exe / 批处理
- **内置工具迁移**：calculator / todo / screenshot / translate 从 `ss_palette.html` 拆为 `builtin/*/`
- **`contributes.keyActions` 消费**：物理键直驱看板
- **ASR / LLM 健壮性**：多 ASR 引擎切换 / LLM 失败重试

#### 远期

- **外部插件生态（阶段 4）**：真沙箱 4 项安全改造
- **MCP server 接入**：easyop 作为 MCP client
- **跨平台评估**：Win32 FFI → macOS IOKit / Linux uinput

### 9.3 事实修正记录

| # | 原断言 | 源码事实 | 修正 |
|---|--------|---------|------|
| F1 | `llm_client::complete/stream` | 不存在，只有 `stream_edit` | 需新增 `LlmClient::chat/chat_stream` |
| F2 | "裸 tokio，零新依赖" | tokio 无 `net` feature | 需加 `net` + `io-util` |
| F3 | `/profile/switch` → `state::switch_profile` | 函数不存在 | 真实落点：`commands::set_active_profile` |
| F4 | `/key/trigger` → "host_action 模拟" | `handle_host_action` 是私有 fn | 需新增白名单派发入口 |
| F5 | `~/.easyop/plugins/` | `~` 会被 Cadence 污染 | 用 `app_config_dir()` |
| F6 | `manifest.icon: "📝"` | `MiniApp` 无 icon 字段 | 图标由 kind CSS 类决定 |
| F7 | `eo.panel.open(id, data)` 传运行时数据 | Tauri `WebviewUrl` 只能静态 URL | 用 host 存载荷 + `panel:ready` 握手 |

---

## 十、集成步骤：从零开发一个插件

### Step 1：创建目录结构

```
plugins/builtin/my-plugin/
├── plugin.json
├── index.js
└── test/
    └── harness.mjs
```

### Step 2：编写 `plugin.json`

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "main": "index.js",
  "contributes": {
    "commands": [
      { "id": "my.run", "title": "运行", "keywords": ["执行", "run"] }
    ],
    "pages": [
      { "id": "my.page", "title": "页面", "icon": "board", "order": 50 }
    ]
  },
  "capabilities": ["storage", "llm"]
}
```

### Step 3：编写 `index.js`

```javascript
export function activate(context) {
  const eo = context.api;

  context.registerCommand('my.run', async () => {
    const input = await eo.get_selection();
    const result = await eo.chat(
      [{ role: 'user', content: `处理：${input}` }],
      { temperature: 0.3 }
    );
    await eo.replace_selection(result);
    eo.log.info('完成');
  });

  context.registerPage('my.page', function (container) {
    container.innerHTML = '<h1>我的插件</h1>';
    return { dispose() {} };
  });
}

export function deactivate() {}
```

### Step 4：部署到运行目录

```bash
npm run plugins:deploy -- my-plugin   # 拷到 %APPDATA%\com.easyop.app\plugins\
```

### Step 5：验证

```bash
npm run plugins:test                  # 验证台
# 重启 easyop
# 侧栏出现页签 → 点开 → 确认渲染
# ⌘SS 搜"运行" → 回车 → 确认执行
```

### 命令速查

```bash
npm run plugins:list         # 干跑：列出将部署的插件
npm run plugins:deploy       # 部署全部内置插件
npm run plugins:deploy -- my-plugin   # 只部署某个
npm run plugins:test         # 跑插件验证台
```

---

## 附录 A：完整示例——灵感速记插件

> 源码：`plugins/builtin/demo-quick-note/`

```json
{
  "id": "demo-quick-note",
  "name": "灵感速记（示范）",
  "version": "0.2.0",
  "main": "index.js",
  "contributes": {
    "commands": [
      { "id": "demo.quick_note", "title": "记一条灵感", "keywords": ["灵感", "速记", "note"] }
    ],
    "tools": [
      { "command": "demo.quick_note", "subtitle": "记下一句话", "keywords": ["灵感", "速记"] }
    ]
  },
  "capabilities": ["storage", "selected-text", "log"]
}
```

```javascript
const KEY = 'inbox';
export function activate(context) {
  const eo = context.api;
  context.registerCommand('demo.quick_note', async () => {
    let seed = '';
    try { seed = await eo.get_selection(); } catch (_) { seed = ''; }
    const notes = (await eo.storage.get(KEY)) || [];
    notes.push({ text: (seed || '').trim(), at: Date.now() });
    await eo.storage.set(KEY, notes);
    await eo.log.info(`灵感速记：已保存 ${notes.length} 条`);
    return { count: notes.length };
  });
}
export function deactivate() {}
```

---

## 附录 B：内容台实施步骤

### 前置（阶段 1）✅

| 阶段 | 内容 | 验收点 |
|------|------|--------|
| P0 | plugin_host + eo.* 注入 | 测试插件能调 `eo.log` |
| P1 | eo.* commands | `eo.chat` / `eo.run` / `eo.storage` 各跑通 |

### 一期（阶段 2）

| 阶段 | 内容 | 验收点 |
|------|------|--------|
| M0 | 底座：plugin.json + model + store + 看板 | 建/改/推进选题，重启后仍在 |
| M1 | 灵感 + 情报卡 | 本地来源可拉条目转选题 |
| M2 | 生成 + 润色 | 一键出 950 字正文 |
| M3 | 配图 | 封面 1080×460 / 正文 16:9 正确 |
| M4 | 排版 + 导出 | 交付包六产物齐全 |
| M5 | 硬件直驱 | 旋钮换卡、按键推进各跑一次 |

### 二期（回补）

| 阶段 | 内容 | 验收点 |
|------|------|--------|
| P0 | `publish.rs` 填实现 | 面板显示三态 |
| P1 | `pub.py status` 预检 | 未登录不发请求 |
| P2 | `publish wechat --save-draft` | 草稿箱新增一条 |
| P3 | `--appmsgid` 更新 | 连跑两次仍 1 条 |

---

## 附录 C：内容台风险登记

| # | 风险 | 等级 | 期 | 对策 |
|---|------|------|---|------|
| 1 | 管道下中文乱码 | 高 | 一期 | runner 设 `PYTHONIOENCODING=utf-8` |
| 2 | 长任务阻塞事件循环 | 高 | 一期 | `async` + `spawn_blocking` |
| 3 | 任意命令注入 | 高 | 一期 | 动作白名单枚举 |
| 4 | 导出 HTML 样式失效 | 中 | 一期 | 复用已验证 `INLINE_STYLE` |
| 5 | 用户误认为导出=已发布 | 中 | 一期 | 导出后强制提示 |
| 6 | Python/Chrome/字体未就位 | 中 | 一期 | 启动自检 |
| 7 | AI 生图账号池枯竭 | 中 | 一期 | 按 item 报错不中断 |
| 8 | LLM Key 未配 | 中 | 一期 | 复用现有「测试连接」 |
| 9 | Edge profile 互斥 | 高 | 二期 | 单并发队列 |
| 10 | 登录态过期 | 中 | 二期 | `pub.py status` 预检 |

---

## 附录 D：内容台回滚策略

- **纯增量**：不动固件协议、不动 `payload` 既有分支、不动 `host_action` 既有分流
- **数据面**：只新增 `plugins/builtin/studio/` 目录（可删），不碰 `config.json`/`profiles.json`/`apps.json`
- **熔断**：`plugin.json` 不存在时 plugin_registry 自然跳过
- **二期独立性**：一期完全不依赖二期