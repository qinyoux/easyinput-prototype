# StyleCam 变更日志

## 0.5.3 — 修复配置写入必败 / Enter 无响应 / 命令丢参（2026-10-13）

### 修复（线上事故，四个叠加 bug）

1. **所有配置写入必然失败**（硬件键开关切不动、设置面板「保存配置」静默失效）：
   `cmds.js` `configFailure()` 是 `async function`，但 `cmdConfigSet` / `cmdConfigValidate`
   调用处**漏了 `await`** —— Promise 恒为 truthy → 每次都走失败分支，返回
   `{ok:false, code:null, message:'', data:{errors:[null]}}`，什么都没存进去。
   修复：两处调用补 `await`。
2. **`configFailure` 给 `provider list` 传 `--model`**：`stylecam_io.py` 的 `provider list`
   子命令没有这个参数，argparse 直接报错退出。修复：不再传 `--model`。
3. **browse 阶段按 Enter 无响应**：`dispatchAction` 的 switch 缺 `case 'confirm'`，
   Enter 产出的 confirm 落到 default 直接吞掉。修复：browse 有素材 → 进 adjusting
   （与点风格卡的阶段推进一致）；无素材 → 给 STAGE_HINTS.browse_empty 提示，不跳阶段。
4. **注册命令丢弃调用参数**：`main.js` 注册时写死 `() => fn({})`，⌘SS 面板 / 硬件键
   绑定带参调命令（如 `stylecam.style.pick {styleId}`）时参数全丢。修复：`(args) => fn(args || {})`。

### 变更

- `ui.js` 硬件键开关按钮：切换失败时弹 toast 报错（此前静默失效，用户无从感知）。

### 测试

- 新增回归组 **⑰**（8 条断言）：config.set 成功落盘、provider list 不带 --model、
  注册命令透传参数、browse+Enter 阶段推进 / 无素材提示、Esc 退回、config.validate 同路径。
- `test/harness.mjs` **295/295 通过**；`test/clamp-extra.mjs` **29/29**；
  `test/test_stylecam_io.py` **18/18**。
- 新增 `test/repro-hwkeys.mjs`：真实子进程 + 运行目录同构配置的端到端复现脚本
  （`node plugins/builtin/stylecam/test/repro-hwkeys.mjs`，不联网、不计费）。

## 0.5.2 — 防跳顶 + 风格样品图（2026-10-13）

### 修复：点击按钮后页面跳顶
- **根因**：任何按钮点击 → `renderAll` 全量 `clear()` 重建各分区，重建瞬间内容高度塌缩 + 聚焦按钮被销毁，浏览器把滚动容器 scrollTop 钳到 0 → 界面跳回顶部。
- **ui.js**：`renderAll` 重建前保存滚动位置（宿主容器 / 面板根 / 文档滚动元素，防御式兼容测试桩 DOM）与焦点描述符（`data-style-id` / `data-action` / `data-sc`），重建后原样放回；重建全程同步完成，塌缩中间态不触发重排。风格卡新增 `data-style-id` 属性作为焦点恢复锚点。

### 新增：风格轨道样品展示图
- 以「猫型轮足机器人形象.jpg」为基础素材，10 组风格各按 strength=100（大改风格档）经 EasySU `gpt-image-2.5-flare` 图生图，1K 原图存 `test/artifacts/style-samples/`。
- 新增 `src/samples.js`：10 张 240px WebP 缩略图内嵌（blob ESM 单文件无相对路径可用），由 `tools/build_style_samples_js.py` 自动生成。
- **ui.js**：风格卡色块优先渲染样品图（无样品回退渐变）；色块 40→52px。
- **styles.js**：`.sc-style-swatch` 支持 `img.sc-style-sample` 铺满。
- 新增 `tools/gen_style_samples.py`（批量生成样品的维护脚本）。

### 修复：prompt 中文档位名被画进图里
- **logic.js `buildPrompt`**：强度档位词改为英文（轻度点缀→light accent / 平衡融合→balanced blend / 大改风格→maximum restyle）。首批样品实测 5/10 张把「大改风格」四字原样画进图，改后重生成全部干净。UI 档位中文名不受影响。

### 契约调整
- **harness.mjs**：bundle 体积上限 160KB→512KB（内嵌样品约 +130KB，index.js 现 299.6KB），注释已注明原因。

### 测试
- `test/harness.mjs` **287/287 通过**；`test/clamp-extra.mjs` **29/29**；`test/test_stylecam_io.py` **18/18**。

## 0.5.1 — UI 紧凑化（2026-09-28）

### 变更
- **styles.js（整体间距收紧）**：面板 `padding` 18→14、`gap` 14→10；卡片 `padding` 14→12；圆角 18/10→14/9；section 标题 `margin-bottom` 10→7；`machine-head` `margin-bottom` 14→10；`stage-zone` `margin-top` 14→9；标题字号 27→24。
- **styles.js（消除分区间空白）**：`machine-grid` 增加 `align-items:start`，两列不再等高拉伸导致矮列底部大留白；预览区 `min-height` 270→230、结果区 240→210；`rail-head` `margin-top` 16→10。
- **styles.js（设置面板紧凑）**：`overlay-body` 改为 `align-items:flex-start` 且可滚动，表单不再垂直居中造成上下大留白；`settings-form` 去掉 `max-height/overflow`、`gap` 10/12→8/10；section 标题 `padding` 收紧；字段 `gap` 4→3、输入 `padding` 7→6、`radius` 8→7；`empty` `padding` 18→12。
- **styles.js（Key 行就近）**：新增 `.sc-key-row` 紧凑布局（API Key 输入框与「保存 Key」按钮同行）。
- **ui.js（按钮就近排布）**：设置面板「保存 Key」按钮由浮层底部 `foot` 移入 API Key 输入框右侧同行，保留 `action='key-set'`；`foot` 仅保留「保存配置 / 自检连接 / 重置 / 关闭」。

### 测试
- `test/harness.mjs` **287/287 通过**（含设置面板结构、12 个非敏感配置字段、Key 输入框不预填、input 防溢出样式等契约断言）。

### ⚠ 未完成
- 无。本次为纯布局/间距优化，未改动任何功能、接口、命令或 `data-action` 契约。
