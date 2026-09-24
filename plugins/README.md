# easyop 插件目录

一个子目录 = 一个插件。easyop 启动时扫 `plugin.json`，把插件声明的**命令**读进注册表，
使它们出现在 ⌘SS 搜索结果里；命令被触发时由 `plugin_host` 激活并执行 `index.js`。

## 目录约定

```
plugins/
└── builtin/                 # 内置插件源（随 easyop 发布）
    ├── studio/              # 内容台    —— 命名空间 studio.*
    └── demo-quick-note/     # 最小样板  —— 命名空间 demo.*
```

**命名空间铁律**：`<id>.<action>` 的 `<id>` 必须全局唯一。
一个插件一份前缀，别两个插件都叫 `studio.*`——`find_plugin_by_command` 只认第一个命中的，
搜索会指错插件。

## 装到运行目录

`plugin_registry::scan_dir` 目前**只扫一层**，读的是 `app_config_dir()/plugins/<id>/`；
而内置插件源放在 `plugins/builtin/<id>/`。中间差一次拷贝，用脚本跨过去：

```bash
npm run plugins:list         # 看有哪些、装在哪
npm run plugins:deploy       # 全装
npm run plugins:deploy -- studio   # 只装某个（按需）
npm run plugins:deploy -- --dry    # 只演练
```

装完重启 easyop（或在插件管理页点刷新 → `plugins_rescan`），然后 ⌘SS 搜关键词。

> 长期方案是让 easyop 直接把 `builtin/` 也扫进来（或 `tauri.conf.json` 打包 `resources`），
> 那样就不需要这一步。当前是权宜。

## `plugin.json` 字段

| 字段 | 必需 | 说明 |
|---|---|---|
| `id` | ✅ | 全局唯一，建议与目录名一致 |
| `name` | ✅ | 展示名 |
| `main` | ✅ | 入口文件，**必须是插件目录内的相对路径**（禁止 `..` 与绝对路径） |
| `version` | — | 版本号 |
| `description` | — | 一句话说明 |
| `activationEvents` | — | `onCommand:<id>` / `onKeyword:<kw>` / `onKey:<KEYn>` / `onSelection`。**当前只收录，不消费**（懒激活由命令触发驱动） |
| `contributes.commands` | — | 命令声明：`id` / `title` / `keywords` |
| `contributes.tools` | — | 把某命令挂进 ⌘SS 内置工具区：`command` / `subtitle` / `keywords` |
| `capabilities` | — | 声明需要的能力。**当前无法强制隔离**，仅作文档约束，见《easyop 扩展机制规范》§六 |

### 搜索可见性规则

- 声明了 `tools` → **只有被 `tools` 引用的命令**进 ⌘SS 搜索；
- 没声明 `tools` → 全部命令都可搜到（按 `title` + `keywords`）。

### 校验（加载期即挡，不留到执行期）

坏插件**不会拖垮其他插件**：加载失败只记一条 warn 并跳过。校验项：
`id`/`name`/`main` 非空 · `main` 不越界 · 入口文件存在 · 命令 id 非空且不重复 ·
`tools` 不得引用未声明的命令 · 清单 ≤ 256KB。

> 注意：`PluginManifest` 里**只有** `commands` 与 `tools` 两个贡献点。
> 规范文档里出现过的 `keyActions` / `panels` / `contextActions` 会被 serde **静默丢弃**
> ——写了不报错、也不生效。要支持得先扩 `Contributes`。

## 宿主 API（`eo.*`）

| API | 对应 Tauri 命令 | 实测状态 |
|---|---|---|
| `eo.get_selection()` | `eo_get_selection` | ✅ |
| `eo.replace_selection(text)` | `eo_replace_selection` | ✅ |
| `eo.inject(text)` | `eo_inject` | ✅ |
| `eo.storage.get/set(key, value)` | `eo_storage_get` / `eo_storage_set` | ✅（每插件一个 `storage.json`，原子写） |
| `eo.log.info/warn/error(msg)` | `eo_log_*` | ✅ |
| `eo.chat(messages, opts)` | `eo_chat` | ✅ 非流式（`opts`: `system` / `temperature` / `max_tokens` / `clean`） |
| `eo.run(program, args, opts)` | `eo_run` | ✅（`opts`: `timeoutSecs` / `cwd`；返回 `exit_code` / `stdout` / `stderr` / `timed_out` / `cancelled`） |

`context` 还带：`pluginId` / `pluginDir`（插件目录绝对路径，用于定位自带脚本）/ `registerCommand(id, fn)`。

### 还没有的能力（写插件前先知道）

| 缺的东西 | 影响 | 现状替代 |
|---|---|---|
| `eo.fs.write` / `paths` | 插件**无法自己写文件** | 自写脚本 + `eo.run`（内容台就是这么绕的） |
| `eo.panel.open` | 插件**开不了独立窗口**，UI 只能挂主窗浮层 | 主窗内 DOM 浮层（内容台面板） |
| `eo.chat_stream` | 拿不到流式增量 | `eo.chat` 一次性返回 |
| `eo.reveal` / `eo.search` / `eo.clipboard_*` | 打开产物目录 / 搜文件 / 剪贴板 | 无 |
| `eo.dispatch_action` | 插件动作绑不到物理键 | `contributes.keyActions` 也还没消费 |
| `eo.config` | 插件**读不到 easyop 配置**（ASR / 按键方案 / 应用库） | 只能硬编码路径 |

> 也就是说：《内容台规范》§2.1 列的 11 项"继承"，目前真兑现的是 **6 项**
> （LLM / 选中文本 / 文本注入 / 日志 / 子进程 / 本地存储）。

## 进度

| 层 | 状态 | 证据 |
|---|---|---|
| **发现层**（扫描 / 解析 / 校验 / 检索） | ✅ | `plugin_registry.rs` + 3 条命令；`cargo test --lib plugin_registry -- --ignored` 真机扫到 2 个插件 |
| **接入层 · ⌘SS 搜索** | ✅ | `ss_palette::search` 已并入 `kind="plugin"`，前端有青色图标 |
| **激活层**（`plugin:activate` 事件 → 前端 blob import → `activate()`） | ✅ | 真机跑过：`demo-quick-note/storage.json` 里有实际写入记录 |
| **接入层 · 按键配置页** | ⬜ | `contributes.keyActions` 未消费 |
| **沙箱**（capability 强制隔离） | ⬜ | 需 4 项改造，见《easyop 扩展机制规范》§六 |

## 写一个插件（最小路径）

1. `plugins/builtin/<id>/plugin.json` + `index.js`（`export function activate(context){}`）；
2. `npm run plugins:deploy -- <id>`；
3. `npm run plugins:test`（如果你照内容台建了 `test/harness.mjs`）；
4. 重启 easyop → ⌘SS 搜关键词 → 回车。

**入口文件必须自包含**：宿主用 `blob:` URL 动态 `import()`，blob 模块里**不能再 import 相对路径**
的文件。要拆多文件就先打包成单文件产物再放进插件目录。
