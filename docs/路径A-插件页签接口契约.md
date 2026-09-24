# 路径 A · 插件页签接口契约

> **版本** v1.0 · 2026-09-19 · 状态：**冻结（freeze）**
> **目的** 让「插件侧」与「easyop 宿主侧」由**两个 agent 并行开发**、最后**直接集成**。
> **性质** 这是双方唯一的对齐物。**任何一方要改本文件，必须两人都确认后再改。**
> **上游文档**《插件集成指南》v1.0 §一（三层架构）· 《easyop 扩展机制规范》v1.0

---

## 零、为什么先冻结契约

插件侧与宿主侧并行改同一个工作树，最典型的失败不是"代码写错"，而是**两边各自发明了一套接口**：
插件导出 `renderPage`，宿主找 `mount`；插件清单写 `views`，宿主解析 `pages`。
等两人都"做完"了才发现对不上，返工量比从头做还大。

所以本文件**先定接口、后写代码**。接口一旦冻结，两边可以完全独立地推进。

---

## 一、整体形态

```
plugin.json                     index.js                      Sidebar / App
┌──────────────────┐   ┌────────────────────────┐   ┌──────────────────────────┐
│ contributes.pages│   │ activate(ctx)          │   │ 静态 6 页签 + 插件页签   │
│  [{ id, title,   │──▶│  ctx.registerPage(     │──▶│ 点页签 → 激活 → render   │
│     icon, order }]│   │    id, (el) => ({...}))│   │ 到容器 el 里             │
└──────────────────┘   └────────────────────────┘   └──────────────────────────┘
       静态声明                  前端注册                      宿主接线
```

**关键设计选择：宿主给容器，插件画内容。**
不走"插件下发 HTML 字符串"，也不走"插件下发 SVG 字符串"——本项目 `csp: null` + `withGlobalTauri: true`，
任何字符串注入都是 API Key 泄漏面（见《插件集成指南》§5.4）。
**容器 + DOM 对象**是唯一安全的形态：插件只能拿到宿主给的元素，宿主不需要 eval 任何字符串。

---

## 二、清单契约（`plugin.json`）

### 2.1 新增字段

```json
{
  "id": "studio",
  "name": "内容台",
  "main": "index.js",
  "contributes": {
    "commands": [ ... 保持原样 ... ],
    "tools":    [ ... 保持原样 ... ],
    "pages": [
      {
        "id": "studio.board",
        "title": "内容台",
        "icon": "board",
        "order": 50,
        "keywords": ["内容台", "studio", "写稿", "看板"]
      }
    ]
  }
}
```

### 2.2 字段规则（**宿主侧必须按此校验**）

| 字段 | 必需 | 类型 | 规则 |
|---|---|---|---|
| `pages[].id` | ✅ | string | 全局唯一，建议 `<pluginId>.<name>`；**不得与任何 `commands[].id` / `tools[].command` 重复** |
| `pages[].title` | ✅ | string | 页签显示名。宿主显示时**超过 6 个汉字截断加省略号** |
| `pages[].icon` | — | string | **图标「名」，绝不是 SVG 字符串**（见 §五 安全铁律）。宿主内置集见 §2.3；未知名 → 默认图标，**不报错** |
| `pages[].order` | — | number | 排序权重，越小越靠前，缺省 **100**。**宿主静态页签占 0–49**，插件页从 50 起 |
| `pages[].keywords` | — | string[] | 可选。供 ⌘SS 搜到"打开 XX"（宿主自行决定是否做搜索结果） |

### 2.3 `icon` 内置集（宿主侧需实现）

`board` · `note` · `spark` · `gear` · `doc` · `chat`

> 未知名一律回退到 `board`。**不报错、不空白** —— 对齐 `ss_palette.html` 的
> "「x」还没有渲染器。"那条既有约定：宁可显式降级，不要静默空白。

### 2.4 `activationEvents` 里可写 `onPage:<pageId>`（**仅作文档，宿主不消费**）

与既有的 `onKey:KEY8` 同样性质：《插件集成指南》§1.2 已定 "`activationEvents` **当前只收录，不消费**"。
写 `onPage:studio.board` 只是**声明意图**，真正的激活由 H3 的 `plugin_activate_page` 驱动。
<br>**不要**在宿主侧为 `activationEvents` 实现分发逻辑 —— 那会引入第二套激活路径。

---

## 三、插件侧契约（`index.js`）

### 3.1 注册

```javascript
export function activate(context) {
  const eo = context.api;

  context.registerPage('studio.board', function (container) {
    // container: HTMLElement —— 宿主已清空、已给好最终尺寸，插件直接往里画
    // 返回可选的生命周期对象（同步返回，不要 Promise）
    renderInto(container);
    return {
      refresh() { /* 可选：宿主切回本页 / 数据变更时调用 */ },
      dispose() { /* 可选：切走 / 插件卸载时调用，用来摘监听器 */ },
    };
  });
}
```

### 3.2 五条铁律

| # | 铁律 | 原因 |
|---|---|---|
| **P1** | 渲染函数**同步返回**（返回值可为 `undefined` 或 lifecycle 对象，**不可为 Promise**） | 宿主按同步调用处理；异步会让"切页签"时序不可预测 |
| **P2** | **不得使用 `position:fixed`** | 页签里宿主负责定位，插件只管填充。带了 fixed 的 UI 是"浮层"做法，塞进页签会盖住整个窗口 |
| **P3** | **不得把 `container` 移出 DOM**（不 `appendChild` 到别处、不 `remove()` 它） | 容器生命周期归宿主 |
| **P4** | 样式只作用于**容器内部**：用插件自己的 class 前缀 + 注入 `<style>`；**不得依赖宿主提供任何 class** | 宿主不承诺任何 class 契约，依赖即脆断 |
| **P5** | 同一 `pageId` 重复注册 → **后注册覆盖先注册**（宿主按覆盖处理，不报错） | 热重载 / 重复 activate 的幂等 |

### 3.3 与 `registerCommand` 的关系

`registerPage` 与 `registerCommand` **并存、互不影响**。

同一插件可以：
- 既有页签（`registerPage`，常驻界面），
- 又有命令（`registerCommand`，可绑硬件键或被编程式调用）。

内容台就是这种：页签 = 台账主界面；命令 = `studio.gen.title` / `studio.export` 等动作。
**页签是插件主入口**。`contributes.tools` 机制保留给需要进 ⌘SS 搜索的插件用，
但内容台选择不声明 tools，不进 ⌘SS——⌘SS 回归搜索/启动文件本职。

### 3.4 「同步返回」不等于「立刻有内容」

P1 要求渲染函数**同步返回**，但**内容可以异步填充**（内容台就是这样：骨架同步画好，
台账数据随后 `await eo.storage.get()` 填进去）。

- 宿主**不得**假定 `renderFn()` 返回后容器里已有内容；
- 宿主**不得**因为"容器暂时是空的"就判定失败或重试；
- 插件想让宿主主动重拉数据时，用返回的 `lifecycle.refresh()`。

> 这条是**实测踩出来的**：预览脚本在 `renderFn()` 返回后立刻数 `选题行`，
> 得到 0，而截图里明明是 3 行——因为数早了。

---

## 四、宿主侧任务清单（交给宿主 agent）

> 顺序即依赖顺序。**H1 → H2 → H3 → H4 → H5**。

### H1 · `plugin_registry.rs`：解析并校验 `pages`

- 新增结构体：
  ```rust
  #[derive(Clone, Debug, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct PageDecl {
      pub id: String,
      pub title: String,
      #[serde(default)] pub icon: Option<String>,
      #[serde(default)] pub order: Option<u32>,
      #[serde(default)] pub keywords: Vec<String>,
  }
  ```
- `Contributes` 加 `#[serde(default)] pub pages: Vec<PageDecl>`
- `validate()` 加三条：`pages[].id` 非空 · 页内 id 不重复 · **`pages[].id` 不与 `commands[].id` 重复**
- 新增对外查询 `pub fn pages() -> Vec<PluginPage>`，返回项含 `plugin_id` + 上面的字段，
  **排序规则：`order` 升序（缺省 100），同 order 按 `plugin_id` 字典序**
- **单测**：空 id 被拒 / 重复 pageId 被拒 / pageId 与 commandId 撞车被拒 / 排序正确

### H2 · 新增 Tauri 命令 `plugins_pages`

- 签名：`#[tauri::command] fn plugins_pages() -> Vec<PluginPage>`
- 返回 `Vec<{ pluginId, id, title, icon, order }>`
- 注册进 `lib.rs` 的 `generate_handler![...]`
- **前端在 Sidebar 挂载时调用一次**（不是每个 tick 调）

### H3 · 新增激活入口 `plugin_activate_page`

- 签名：`#[tauri::command] fn plugin_activate_page(plugin_id: String, page_id: String) -> Result<(), String>`
- 行为：读该插件 `index.js` → emit `plugin:activate`，**`commandId` 传空串 `""`**
- 空串语义 = **"只为注册页面而激活，不执行任何命令"**
- **`pluginHost.ts` 必须加守卫**：
  ```ts
  if (!commandId) return;   // execCommand 开头，否则会报"命令未注册："
  ```
  这是本契约里**最容易漏的一处**，漏了会在日志里刷 `命令未注册：`。
- 若插件入口读不到 / 不存在 → 返回 `Err`，**不要返回 Ok**（对齐"失败不静默"）

### H4 · `pluginHost.ts`：`registerPage` + 取渲染函数

- `context` 增加 `registerPage: (id: string, renderFn: PageRenderFn) => void`
- 类型：
  ```ts
  type PageLifecycle = { refresh?: () => void; dispose?: () => void };
  type PageRenderFn = (container: HTMLElement) => PageLifecycle | void;
  ```
- 内部 `const pages: Record<string, PageRenderFn> = {}`，与已有 `commands` **并列**
- 已激活插件表 `activatedPlugins` 需要**同时**存 `commands` 与 `pages`
  （建议改成一个 `{ commands, pages }` 的对象；**注意这是内部结构改动，别动 `getPluginStats()` 的对外形状**）
- 新增导出 `export function getPluginPages(): Map<string, Record<string, PageRenderFn>>`
- **幂等**：已激活的插件再次 `plugin_activate_page` → 直接返回，不重复 `activate()`

### H5 · `Sidebar.tsx` + `App.tsx` 接线

**Sidebar**
- `NAV` 从硬编码数组改为：`[...STATIC_NAV, ...pluginPages.map(toNavItem)]`
- 静态 6 项**保持原顺序、原 id**（`workbench`/`remap`/`apps`/`voice`/`profiles`/`settings`），
  插件页一律排在最后
- `Tab` 类型需要放宽（现在是字面量联合类型 `"workbench" | "remap" | ...`）→
  建议改成 `type Tab = StaticTab | \`plugin:${string}\``，**避免把插件页硬塞进静态联合类型**

**App.tsx**
- `tab` 为 `plugin:<pageId>` 时渲染容器分支：
  1. 取一个 `<div ref={mountRef}>`，样式 `flex:1; min-height:0; overflow:hidden`
  2. 首次进入 → `await invoke("plugin_activate_page", { pluginId, pageId })`
  3. `getPluginPages().get(pluginId)?.[pageId]` → 拿到 renderFn
  4. `const life = renderFn(mountRef.current)`
  5. 离开该页签（或组件卸载）→ `life?.dispose?.()`
- **懒激活**：侧边栏在插件未激活时就已显示页签（`plugins_pages` 静态读 `plugin.json`）；
  首次点开才激活。这是既有"懒激活"设计的延续，**不要改成启动全激活**
- **失败不静默**（三条分支都要给可见文案，不能空白）：
  - `plugin_activate_page` 失败 → 容器内显示"插件「X」激活失败：<原因>"
  - 取不到 renderFn → 显示"页签「Y」未注册渲染函数"（对齐 `ss_palette.html` 既有措辞风格）
  - renderFn 抛异常 → catch 后显示错误 + `eo_log_error`

---

## 五、安全铁律（**不要破坏**）

1. **`icon` 只接受「名字」**，绝不接受内联 SVG / HTML 字符串。
   本项目 `csp: null` + `withGlobalTauri: true`，任何字符串注入面都等价于
   "任何 webview 里一行 `invoke('get_api_key')` 就能读走 API Key"。
2. **宿主不 eval 插件下发的 HTML**。渲染只走 `renderFn(container)` —— 插件拿 DOM 对象，自己建节点。
3. **不要给 `Contributes` 加 `deny_unknown_fields`** —— 见 §六，这是并行开发能成立的前提。

---

## 六、向前兼容铁律（**并行开发能成立的前提**）

`Contributes` 当前**没有** `deny_unknown_fields`，未识别字段被 **serde 静默忽略**。

由此得到一条极有价值的时间差：

| 时刻 | `plugin.json` 有 `pages` | 宿主是否支持 `pages` | 结果 |
|---|---|---|---|
| 插件先做完 | ✅ | ❌ | **旧宿主静默忽略 → 插件照常工作，不报错、不崩** |
| 宿主后做完 | ✅ | ✅ | **插件一行不改，页签自动出现** |
| 宿主先做完 | ❌ | ✅ | `pages` 为空 → `plugins_pages` 返回空数组 → 只剩静态页签 |

**所以两侧可以完全独立地做完，最后直接合。**

⚠️ **宿主 agent 注意**：千万别为了"严格校验"给 `Contributes` 或 `PluginManifest` 加
`deny_unknown_fields` —— 那会让"插件先做完"的那段时间里**旧宿主拒绝加载新清单**，
直接把并行开发这条路堵死。

---

## 七、验收口径

### 插件侧（内容台已完成）

```bash
npm run plugins:test     # 验证台断言：
                         #   · 清单含 pages，且 pages[].id 与 commands[].id 不冲突
                         #   · activate() 调用了 registerPage('studio.board', fn)
                         #   · 渲染函数同步返回（非 Promise）
                         #   · 渲染函数内部不出现 position:fixed
```

### 宿主侧（宿主 agent 完成后）

```bash
cd src-tauri && cargo test --lib plugin_registry   # pages 解析 / 校验 / 排序 单测
npm run build                                      # tsc 过（Tab 类型放宽后不能有类型错）
```

**真机冒烟（两人都做完后，钦友手点）**：
1. 启动 easyop → 侧边栏**已出现**"内容台"页签（未点开 = 未激活）
2. 点"内容台" → 首次触发激活 → 台账界面向左填充整页
3. 切到其他页签再切回 → 界面还在、数据在
4. ⌘SS 搜"内容台" → **不应出现** studio 命令（v0.4.0 起内容台退出 ⌘SS，走页签为唯一入口）

---

## 八、变更纪律

- 本文件**冻结后**，任一侧要改接口 → **先改本文件并说明理由**，再改代码。
- 接口变更属于**对外契约变更** → 版本档位 **major**（见 `AGENTS.md` §3）。
- 各自收工照常走 `npm run version:bump` + `CHANGELOG.md` + `README.md` + `version:check`。
