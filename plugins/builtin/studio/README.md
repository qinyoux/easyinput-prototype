# 内容台（studio）—— easyop 内置插件

> **v0.4.3 · 2026-10-12 · 状态：修「打开目录」真机 bug** —— v0.4.2 的 explorer 参数写法（单字符串 `/select,"路径"`）被 runner 的 argv 转义二次包引号，explorer 解析失败后打开「此电脑」。改为 easyop 核心同款写法：`explorer.exe` + 两个独立参数 `['/select,', '<反斜杠路径>']`，失败退回 `explorer.exe <反斜杠目录>`。新增 `studio.open.dir` 命令（可被快捷动作路由、可回归测试），回归台补 12 项参数形状断言（194 项全绿）。
>
> **v0.4.2 · 2026-10-12 · 状态：工作流四项优化落地** —— ① 移除「配图」阶段（推进流程改为五阶段）② 术语统一（`去AI味`→`内容优化`、`一条龙`→`一键执行`）③ 「平台发布」简化为**只产一个排版好的 HTML**（不再生成 .md / 投放说明 / manifest）④ 「打开目录」改为定位到文件实际所在目录（v0.4.2 版只修了正斜杠，参数写法问题在 v0.4.3 才修全）。新增 `studio.publish.html` 命令与 `write-html-file` 子命令。
>
> v0.4.1 · 2026-09-28 · 状态：**「+公众号」重构为默认产出可复制粘贴的 HTML（按 wechat-writer-pro 规范排版，自动填草稿降为可选）**
>
> v0.4.0 · 2026-09-27 · 状态：**侧边栏页签为唯一入口，支持深浅色双主题，旋钮导航已接入并真机验证通过**
>
> **v0.38.0 变更**：旋钮导航从"功能动作"中拆出，独立成 `Studio` kind（与"任务台"同级）。
> 按键映射下拉里"内容台"是独立选项，不再混在"功能动作"里。
> 修复插件激活失败时 `ensurePluginActivated` 永久挂起的 bug。
>
> **v0.4.0 变更**：移除 `studio.toggle` 命令和 overlay 浮层模式，清空 `contributes.tools`。
> 插件不再出现在 ⌘SS 搜索结果里，统一走侧边栏页签（`contributes.pages`）。
> ⌘SS 命令面板回归搜索/启动文件本职。
> 新增旋钮导航、`studio.quick_action` 命令、`eo.clipboard_read` 能力、双主题支持、使用说明弹窗、情报卡自动生成、一条龙流程、正文字数可配置。
>
> **v0.3.0 变更**：公众号导出新增降级路径——登录态有效走 `pub.py publish wechat --save-draft` 自动填草稿，
> 失效时调 `eo.chat` 生成全内联样式 HTML，写入本地投放包供手动粘贴。
>
> **v0.2.1 变更**：外部工具路径改为从 `eo.storage` 的 `config` 键读、回退默认值；
> 稿件输出目录改读 `EASYOP_DRAFTS_DIR` 环境变量；修了 `studio.doctor` 笔误。
>
> 规范：[docs/内容台规范.md](../../../docs/内容台规范.md)（定案）· 机制：[plugins/README.md](../README.md)
> 页签契约：[docs/路径A-插件页签接口契约.md](../../../docs/路径A-插件页签接口契约.md)（冻结）
> 架构设计：[docs/内容台插件架构设计.md](../../../docs/内容台插件架构设计.md)

## 入口

**侧边栏页签**是唯一入口。插件在 `plugin.json` 声明 `contributes.pages`，
宿主启动时自动在侧边栏渲染「内容台」按钮，点击即打开主窗页签。

⌘SS 命令面板**不再**作为插件入口——它回归搜索/启动文件的本职。
`contributes.tools` 已清空，studio 的命令不会出现在 ⌘SS 搜索结果里。

## 使用说明

面板打开后，点右上角「?」按钮可随时查看内置使用说明。以下是完整版。

### 五阶段流程

```
灵感 → 情报卡 → 正文 → 内容优化 → 已导出
```

每个阶段有**准入校验**：进「正文」要情报卡 ≥3 条；进「内容优化」要有正文。
缺前置时返回 `ok:false` + 说明缺什么，**不静默跳过**。

> 「配图」阶段（原 `figures`）已移除，推进流程只保留实际存在的五个阶段。
> 历史数据若停在 `figures`，会被归一化到 `humanized`，不会把阶段回退到第一步。

### 操作流程

1. **新建选题**：在顶部输入框填标题（或留空自动取选中文本/剪贴板），点「＋ 新建选题」
2. **一键执行**（推荐）：点「一键执行」按钮，自动依次跑 情报卡→标题→正文→内容优化
   - 中途某步失败后，重试会**跳过已落库的步骤**，只补跑未完成的
3. **分步操作**：也可以逐个点「生成情报卡」「生成标题」「生成正文」「内容优化」
4. **选定标题**：在「标题候选」卡片里点选一个标题
5. **字数设置**：动作区「字数」输入框设置正文目标字数（默认 500）
6. **导出**：组稿成 `.md` → `pub.py check` 校验
7. **平台发布**：把整理好的内容交给排版 skill，**只生成一个 HTML 文件**，
   浏览器打开后全选复制、粘贴到公众号即可
   - **不生成** `.md` 稿件、投放说明、`正文.txt`、`_manifest.json` 等任何附加文件
   - LLM 排版不可用时自动降级到本地转换器（零依赖，样式对齐排版规范），状态行会标注「本地兜底」
8. **打开目录**：直接打开最近一次产物（平台发布的 HTML 优先于导出的 `.md`）**所在的目录**
9. **推进阶段**：手动推进到下一阶段（带准入校验）
10. **自检**：检查 Python / pub.py / 自带脚本是否就位
11. **删除**：删除当前选题

> 长正文不再挤占主区：内容卡片默认折叠，点「🗔 浮窗查看/编辑」可在独立浮窗里
> 查看全量内容并直接编辑保存。

### 旋钮操作

在按键映射配置中，选择动作类型「**内容台**」（与"任务台"同级，不在"功能动作"里），
然后选择子动作：

| 旋钮方向 | 动作类型 | 子动作 |
|----------|---------|--------|
| 左旋 | 内容台 | 上一项（左旋切换焦点） |
| 右旋 | 内容台 | 下一项（右旋切换焦点） |
| 按下 | 内容台 | 确认（按下执行） |

配置后下发到开发板。旋转时界面提示当前选中的功能名，按下执行。
功能列表（5 项循环）：新建选题 → 一键执行 → 推进阶段 → 导出 → 平台发布。

### 命令一览

| 命令 | 做什么 |
|---|---|
| `studio.state` | 状态：选题数 / 当前选题 / 阶段 / 工具路径 / 已挂载的页签 |
| `studio.topics.list` / `save` / `remove` | 台账增删改 |
| `studio.stage.advance` | 五阶段推进（带准入校验） |
| `studio.gen.intel` | 生成情报卡（角度/事实/金句/风险四板块） |
| `studio.gen.all` | 一键执行：情报卡→标题→正文→内容优化 顺序执行 |
| `studio.gen.title` | 生成 3 个标题候选（自动剥掉编号前缀） |
| `studio.gen.body` | 生成正文（情报卡 <3 条直接拒绝，字数可配置） |
| `studio.gen.humanize` | 内容优化（去 AI 味），并报字数漂移 |
| `studio.export` | 组稿 `.md` → `pub.py check`（不再打任何投放包） |
| `studio.publish.html` | 平台发布：排版 skill → **只写一个 HTML 文件**，记 `htmlPath` |
| `studio.open.dir` | 打开产物目录：`explorer.exe /select,` + 反斜杠路径（htmlPath 优先，无产物明确报错） |
| `studio.doctor` | 自检：Python / pub.py / 自带脚本是否就位 |
| `studio.quick_action` | 快捷动作（旋钮确认入口，路由到上述功能） |

## 这一版做了什么

选题台账 → AI 生成（标题 / 正文 / 内容优化）→ 导出稿件 或 平台发布（单个排版 HTML）。
全部逻辑在 `index.js`，持久化走 `eo.storage`，LLM 走 `eo.chat`，子进程走 `eo.run`。

**贡献点**：16 命令 · 0 tools · **1 page**（`studio.board`，主窗页签，`order: 50`）。

**五阶段**：`inspiration 灵感 → intel 情报卡 → body 正文 → humanized 内容优化 → exported 已导出`。
推进的门禁语义 = **"该阶段的输入是否就绪"**：进 `intel` 不需前置；进 `body` 要情报卡 ≥3 条；
进 `humanized` / `exported` 要有正文。缺前置时返回 `ok:false` + 说明缺什么，**不静默跳过**。

## 文件

```
studio/
├── plugin.json                  清单（16 命令 / 0 tools / 1 page）
├── index.js                     实现（单文件自包含 ESM，双主题 UI + 旋钮快捷动作）
├── tools/studio_io.py           磁盘窄腰：compose（组稿 .md）/ write-html-file（只写单个 HTML）
│                                / markdown-to-html（LLM 排版的本地兜底）/ read-layout-guide
└── test/
    ├── harness.mjs              Node 验证台：桩 eo.* + 极简 DOM 桩，194 项断言
    ├── panel-preview.html       浮层模式真渲染页（暗色，配 Chrome headless 截图）
    └── panel-preview-page.html  页签模式真渲染页（亮色，含 16 项页面内自断言）
```

## 验证怎么做

```bash
npm run plugins:test                                     # 194 项断言（studio 部分）
npm run plugins:deploy -- studio                         # 装进运行目录
cd src-tauri && cargo test --lib plugin_registry -- --ignored --nocapture   # 真机扫描
```

**已验证**：命令逻辑 / 信封 / 失败路径 · `studio_io.py` 真跑（含 3 条失败路径）·
`pub.py check` 真跑（退出码 0）· 平台发布单 HTML 产出（目录内只有 1 个 `.html`）·
排版 LLM 失败时的本地兜底 · 真机扫描到插件 · 面板真渲染 · 深浅色双主题 ·
旋钮导航逻辑 · **旋钮硬件触发（真机验证通过）**。

**未验证**：easyop 里侧边栏点「内容台」→ 页签渲染的完整链路（需 GUI 手点）；
「打开目录」的 `explorer /select` 真机行为（需手点确认）。

## 与规范的偏差

| 规范写的 | 实际做的 | 为什么 |
|---|---|---|
| `plugins/builtin/studio/` 直接被 easyop 扫到 | 需 `plugins:deploy` 拷到 `%APPDATA%\...\plugins\studio\` | `scan_dir` 只扫一层 |
| 独立窗口 1080×720 | 主窗内页签面板 | **没有 `eo.panel.open`**；插件代码跑在主窗 webview 里 |
| `app/*.ts` 多文件 + `dist/` | 单文件 `index.js` | blob import 不能 import 相对路径 |
| `contributes.keyActions` 6 个硬件动作 | 3 个保留 UUID 桥接（`Studio` kind：`prev`/`next`/`confirm`） | registry 的 `Contributes` 未扩展，走 host_action 通道 |
| `studio.gen.figures` / `studio.task.cancel` | 未实现 | 前者要接生图通道，后者要异步任务队列 |
| `eo.chat_stream` 生成正文 | `eo.chat` 一次性返回 | `eo_chat_stream` 未暴露 |

## 已知限制

- **平台发布只出 HTML**：不再生成小红书投放包 / 自动填草稿，如需多平台产物需另行扩展
- **旋钮功能列表固定 5 项**：不可自定义映射
- **长文编辑走浮窗**：主区内容卡片默认折叠，需点「浮窗查看/编辑」展开全量
- **「打开目录」依赖 explorer.exe 参数解析**：`/select,` + 路径必须是两个独立参数（runner argv 转义会二次包引号，单字符串写法会让 explorer 打开「此电脑」）；真机行为无 GUI 自动化验证，回归台只锁定参数形状


## 回滚

纯增量：删掉 `%APPDATA%\com.easyop.app\plugins\studio\` 即净；
不动固件协议、不动 `config.json` / `profiles.json` / `apps.json`。
