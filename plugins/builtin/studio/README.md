# 内容台（studio）—— easyop 内置插件

> v0.3.0 · 2026-09-21 · 状态：**能跑通的最小闭环 + 页签模式待宿主侧就位**
>
> **v0.3.0 变更**：公众号导出新增降级路径——登录态有效走 `pub.py publish wechat --save-draft` 自动填草稿，
> 失效时调 `eo.chat`（排版提示词提炼自 wechat-typesetting skill 的 blue-minimal 模板）生成全内联样式 HTML，
> 写入本地投放包供手动粘贴，与小红书 `pack xhs` 对齐。面板加「+公众号」按钮。`studio_io.py` 加 `write-html` 子命令。
>
> **v0.2.1 变更**：外部工具路径（Python / content-publisher）改为从 `eo.storage` 的 `config` 键读、
> 回退默认值（换机器写一次配置即可）；稿件输出目录改读 `EASYOP_DRAFTS_DIR` 环境变量；
> 修了 `studio.doctor` 一处 `!!x === false` 笔误（原恒误报「插件目录」检查失败）。
> 规范：[docs/内容台规范.md](../../../docs/内容台规范.md)（定案）· 机制：[plugins/README.md](../README.md)
> 页签契约：[docs/路径A-插件页签接口契约.md](../../../docs/路径A-插件页签接口契约.md)（冻结）

## 两种挂载形态

同一份界面代码服务两个入口，靠「视图」抽象统一（`buildView(container, mode)`）：

| 形态 | 谁挂的 | 定位 | 现状 |
|---|---|---|---|
| **overlay** | `studio.toggle`（⌘SS 里搜"内容台"） | `.studio-overlay` 包装承担 `position:fixed`（右上 340px 窄栏） | ✅ 现在就能用 |
| **page** | 宿主主窗页签（`contributes.pages` + `context.registerPage`） | 面板**不带任何定位**，填满宿主给的容器 | ⏳ 插件侧已完成并验证，**等宿主侧 H1–H5** |

`refreshPanel()` / `setStatus()` 会**遍历所有已挂载视图**——所以两个入口同时开着时数据是同步的，
不会出现"在浮层里点了按钮，页签里不更新"。

**宿主未实现 `registerPage` 时插件静默跳过**：老宿主照常走 ⌘SS 命令路径，不报错、不崩。
这是两侧能独立开发、最后直接集成的前提。

## 这一版做了什么

选题台账 → AI 生成（标题 / 正文 / 去 AI 味）→ 导出稿件 → 调 `pub.py` 校验 / 出小红书投放包。
全部逻辑在 `index.js`，持久化走 `eo.storage`，LLM 走 `eo.chat`，子进程走 `eo.run`。

| 命令 | 做什么 |
|---|---|
| `studio.toggle` | 开合主窗浮层面板 |
| `studio.state` | 状态：选题数 / 当前选题 / 阶段 / 工具路径 / **已挂载的页签** |
| `studio.topics.list` / `save` / `remove` | 台账增删改 |
| `studio.stage.advance` | 六阶段推进（带准入校验） |
| `studio.gen.title` | 生成 3 个标题候选（自动剥掉 `①` `1.` 这类编号） |
| `studio.gen.body` | 生成正文（情报卡 <3 条直接拒绝） |
| `studio.gen.humanize` | 去 AI 味，并报字数漂移 |
| `studio.export` | 组稿 → `pub.py check` →（可选）`pub.py pack xhs` |
| `studio.doctor` | 自检：Python / pub.py / 自带脚本是否就位 |

**贡献点**：11 命令 · 5 tools（进 ⌘SS）· **1 page**（`studio.board`，主窗页签，`order: 50`）。

**六阶段**：`inspiration 灵感 → intel 情报卡 → body 正文 → humanized 去AI味 → figures 配图 → exported 已导出`。
推进的门禁语义 = **"该阶段的输入是否就绪"**：进 `intel` 不需前置；进 `body` 要情报卡 ≥3 条；
进 `humanized`/`figures`/`exported` 要有正文。缺前置时返回 `ok:false` + 说明缺什么，**不静默跳过**。

## 文件

```
studio/
├── plugin.json                  清单（11 命令 / 5 tools / 1 page）
├── index.js                     实现（单文件自包含 ESM）
├── tools/studio_io.py           组稿落盘（读 storage.json → 写 .md → 打印 POST=<路径>）
└── test/
    ├── harness.mjs              Node 验证台：桩 eo.* + 极简 DOM 桩，144 项断言
    ├── panel-preview.html       浮层模式真渲染页（暗色，配 Chrome headless 截图）
    └── panel-preview-page.html  页签模式真渲染页（亮色，含 16 项页面内自断言）
```

## 验证怎么做（以及对不上的地方）

```bash
npm run plugins:test                                     # 144 项断言
npm run plugins:deploy -- studio                         # 装进运行目录
cd src-tauri && cargo test --lib plugin_registry -- --ignored --nocapture   # 真机扫描
```

**渲染验证（浮层 + 页签两种形态）**：

```bash
CH="/c/Program Files/Google/Chrome/Application/chrome.exe"
"$CH" --headless=new --disable-gpu --allow-file-access-from-files \
  --hide-scrollbars --virtual-time-budget=4000 --window-size=1120,980 \
  --screenshot="panel-preview-page.png" "file:///…/test/panel-preview-page.html"
```

> ⚠️ **`--allow-file-access-from-files` 不能省**。预览页要 `import('../index.js')`，
> 而 `file://` 源下这个跨源 import 会被 CORS 直接拒绝——**报错只进浏览器 console，
> Chrome 照样写出一张"空白页"截图**，看上去像"面板渲染正常但没内容"。
> 判断脚本到底跑没跑完：看页面 `<title>` 有没有被脚本改写，或 `--dump-dom` 里有没有断言框。

面板渲染（不启动 easyop 也能看）：

```bash
"/c/Program Files/Google/Chrome/Application/chrome.exe" --headless=new \
  --allow-file-access-from-files --window-size=780,720 \
  --screenshot=plugins/builtin/studio/test/panel-preview.png \
  plugins/builtin/studio/test/panel-preview.html
```

**已验证**：命令逻辑 / 信封 / 失败路径（111 项）· `studio_io.py` 真跑（含 3 条失败路径）·
`pub.py check` 真跑（退出码 0）· `pub.py pack xhs` 真跑（缺图时退出码 3，插件如实上报）·
真机扫描到插件 · 面板真渲染（3 选题 / 4 详情块 / 3 标题候选 / 8 按钮 / toggle 幂等）。

**未验证**：easyop 里 ⌘SS 真按回车触发激活的完整链路（需要 GUI 手点一次）。

## 与规范的偏差（都是被现实逼的，不是偷懒）

| 规范写的 | 实际做的 | 为什么 |
|---|---|---|
| `plugins/builtin/studio/` 直接被 easyop 扫到 | 需 `plugins:deploy` 拷到 `%APPDATA%\...\plugins\studio\` | `scan_dir` 只扫一层，"内置"概念还没落地 |
| 独立窗口 1080×720，`decorations(true)` | 主窗内浮层面板（340px 宽） | **没有 `eo.panel.open`**；插件代码跑在主窗 webview 里，只能挂 DOM |
| `app/*.ts` 多文件 + `dist/` | 单文件 `index.js` | 宿主用 `blob:` import，blob 模块不能 import 相对路径 |
| `eo.run(pub.py export --post …)` | `eo.run(pub.py check/pack)` + 自带 `studio_io.py` 组稿 | **`pub.py` 没有 `export` 子命令**（实际有 `check`/`pack`/`draft`/`wb`） |
| 台账落 `%APPDATA%\...\studio\topics.json` | `eo.storage` 的 `plugins/studio/storage.json` 里一个 `topics` 键 | `eo.storage` 就是 KV 单文件 |
| `eo.run` 有"动作白名单" | **没有**，`runner` 不限 program | 未实现（见 §九的承诺） |
| `contributes.keyActions` 6 个硬件动作 | 未写 | registry 的 `Contributes` 只有 `commands`+`tools`，写了会被静默丢掉 |
| `studio.gen.figures` / `studio.task.cancel` | 未实现 | 前者要接生图通道 + `e_qc`，后者要异步任务队列——这版没有队列 |
| `eo.chat_stream` 生成正文 | `eo.chat` 一次性返回 | `eo_chat_stream` 未暴露（需要事件推增量） |

## 一期真正的拦路石

**小红书投放包必须有配图**（`pub.py pack xhs` 缺图退出码 3），而 `studio.gen.figures` 还没做。
所以现在 `studio.export` 默认只做「组稿 + 校验」，要投放包得单独点「+小红书包」，
**而且会因为没有图而失败**——这是当前最该补的一环。

## 回滚

纯增量：删掉 `%APPDATA%\com.easyop.app\plugins\studio\` 即净；
不动固件协议、不动 `config.json` / `profiles.json` / `apps.json`。
