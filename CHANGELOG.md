# 更新日志

所有版本的重要变更都记录在此文件中。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，  
版本号遵循 [语义化版本（Semantic Versioning）](https://semver.org/lang/zh-CN/)。

---

## [v0.45.0] — 2026-10-16

> 署名：qinyoux

### 新增

- **本地激活码（授权码）系统**：让 easyop 成为"可以注册"的正式产品。采用 Ed25519 非对称签名，私钥由作者离线保管、公钥硬编码进二进制；激活码 = `base64url(载荷) + "~" + base64url(签名)`，本机校验、**无需联网、无法伪造**。载荷含版次/过期时间/备注/可选机器绑定。
  - 后端：`src-tauri/src/license.rs`（纯逻辑 + 9 条单测）、`state.rs` 存取、命令 `activate_license` / `license_status`。
  - 前端：状态条 `注册 / 激活` 徽标 + 激活弹窗（`src/App.tsx`、`api.ts`、`types.ts`、`styles.css`）。
  - 作者专用生成器：`scripts/gen_license.mjs`（`genkey` / `pub` / `sign`），私钥 `scripts/.license_private.key` 已被 `.gitignore` 排除。
- **NSIS 安装包**：`tauri.conf.json` 的 `bundle.targets` 由 `[]` 改为 `["nsis"]`，`tauri build` 现在会产出标准 Windows 安装程序（开始菜单 + 卸载），不再只是便携 exe。

### 变更

- 状态条新增"注册 / 激活"入口，未激活时显示"未注册（试用）"，激活后显示"已注册 · 版次"；激活状态存于本机 `%APPDATA%\com.easyop.app\license.json`。
- **试用版功能完整**，激活只改变"是否注册"标识，不阉割任何能力。

### 修复

- 修复了"打包只出便携 exe、没有安装包"的问题：根因是 `bundle.targets: []` 在 Tauri 2.11 被当作"无目标"，显式声明 `["nsis"]` 后正常产出安装包。

### ⚠️ 未完成

- 自动更新（Tauri updater）尚未接入：需要作者生成 updater 密钥并托管更新文件，本期只做安装包 + 便携版，升级暂靠手动覆盖。
- 机器绑定为可选能力（默认发不绑定的码），如需按设备授权，生成时传 `mid` 参数即可。

---

## [v0.44.0] — 2026-10-15

> 署名：workbuddy/easyinput-prototype/锐

> minor：**关闭任务台播报排队机制的三条遗留**（钦友点名要求写清楚的那三条）。
> ① 进度类不再"被占即丢"；② 合并提醒从"只有条数"变成"带槽位与事项"；
> ③ 排队等待期与队首**实时可见**（新事件 `agent-desk-channel` + 前端喇叭实况区）。

### 新增

- **`agent-desk-channel` 事件**（`ChannelState { playing, notice_queue }`）：
  喇叭占用与播报队列一变化就推，"真变化才发"（内容比对，非版本号 —— 并发播放下
  版本号必然错序）。⚠️ **刻意不并入 `agent-desk-state`**：后者由任务状态驱动，
  而排队与播完发生在**任务状态完全没变**的时刻；等它再变，等待期已经结束。
  （即三条遗留里的"队首等待没有实时进度"。）
- **`project_channel(mgr)`**：喇叭实况的**唯一投影**。快照（5s 轮询）与实时事件
  同源取值 —— 两处各算一遍早晚会漂移成"喇叭响着 A、界面写着在播 B"，
  而这种 bug 恰恰在排查时最难重现。
- **前端喇叭实况区**（`DeskView`）：正在播什么（槽位 + 文本 + 是否结果播报）、
  排队几条、**队首单独点出**"下一个轮到谁"、其余折叠成"之后：…"。
  快照只作**初值兜底**（收到过事件后不再覆盖，避免往返延迟导致界面来回跳）。
- **回归测试 4 条**（投影）：空闲态、队列位置与槽位标签、全局进度无槽位标签、
  快照与事件同源不漂移。`agent_desk` 共 **289 passed / 0 failed**。

### 变更

- **进度类播报改为"收敛入队"**（遗留 1）：此前被占用即丢，用户对排队期间
  完全没有感知。现在**最多留一条最新的，且绝不压在终态前面**
  （队里有终态 ⇒ 进度直接过期丢弃：FIFO 下它永远排在终态后，而终态播完时
  任务多已结束，"正在推进"成了废话）。规则抽成纯函数 `enqueue_decide` 以便单测 ——
  排队规则的失败形态是**静默的**（听不到、也无报错），只能靠测试钉死。
- **合并提醒记明细而非计数**（遗留 2）：`notice_dropped` 从 `AtomicUsize`
  改为 `Mutex<Vec<DroppedNotice { kind, slot }>>`，正文改为
  "还有 2 条任务结果未播报（S1 任务已完成、S3 任务失败），请点击查看。"
  超 3 条退化为纯条数（TTS 按字数计费，一句提醒念半分钟比不念更糟）。
- **`speak_notice` 增加 `app` 参数**：播放的开始与结束都发生在它内部，
  调用方在外面拿不到准确时刻，传进来才能在两处各推一次状态事件。

### 修复

- **修 v0.42.0 中断留下的编译错误**：`a_progress_notice_expires_when_a_terminal_is_queued`
  与 `non_terminal_events_never_queue` 两条测试用 `EnqueueDecision::Drop { reason: _ }`
  做模式匹配 —— `_` 不能出现在表达式位置，`cargo test` 直接编译失败（`E0599`）。
  改用 `matches!(..., EnqueueDecision::Drop { .. })`。

### ⚠️ 未完成

- **喇叭硬件下行未闭环**：板载扬声器/键盘喇叭的 PCM 通道仍未接通，
  播报只走 PC 声卡。本批改的是**排队与可见性**，不涉及输出设备。
- **排队等待只有"静态快照"，没有进度条**：队首等待期间不会报"还要等约 12 秒"。
  需要 `PlaybackControl` 暴露剩余时长并让前端定时刷新，属于下一步。
- **进度类合并提醒仍不记明细**：设计如此（过期进度没有告知价值，
  报进合并提醒是噪音）。

---

## [v0.43.3] — 2026-10-14

> 署名：workbuddy/easyinput-prototype/锐

> patch：**任务台的 Codex CLI 恢复联网能力**（钦友实测要求"去掉沙箱"）。
> 预置参数加 `--sandbox danger-full-access`，老配置升级时自动补（只补一个沙箱旗标都没有的）。
> **不改协议、不改前端、不动用户已设的值。**

### 新增

- **`config.rs` 迁移方向三**：`migrate_presets()` 世代 2 → 3，给老配置的 codex-cli
  补上去沙箱旗标。只改 `default_drivers()` 对老用户**不生效**——
  `normalize()` 只按 id 补"从未出现过"的预置，钦友磁盘上的 codex-cli 条目一直在，
  表现为"升级了还是不能联网"。用户已设 `-s` / `--sandbox` /
  `--dangerously-bypass-approvals-and-sandbox` 任意一种都不碰（`SANDBOX_FLAG_CANDIDATES`）。
- **回归测试 4 条**（`config.rs`）：预置必带旗标且落在 `{prompt}` 之前；
  老配置补旗标一次且仅一次；用户自设旗标绝不覆盖。
  `cli_driver.rs` 加 1 条：拿**真实预置参数**叠续接，钉死 `resume` 仍落在所有全局选项之后。

### 变更

- **codex-cli 预置参数**：`exec --json --skip-git-repo-check {prompt}`
  → `exec --json --skip-git-repo-check --sandbox danger-full-access {prompt}`。
  完全去掉沙箱 ⇒ 命令以当前用户权限运行，**网络与全盘都不再受限**。
  代价：模型生成的 shell 命令失去全部隔离，与"任务文本可来自语音识别"叠加，
  只适用于本机自用、prompt 来源可信的场景。恢复隔离 = 删掉这两个参数，
  或改用 `~/.codex/config.toml` 的 `[sandbox_workspace_write] network_access = true`
  （保留文件系统边界、只放网络）。
- **文档同步**：`docs/任务台设计方案.md` §13 命令串、§14.2 迁移表（世代 `< 3`）、
  §14.3 预置表、**新增 §14.5**（沙箱决策的第一性原理依据 + 实测对照表）；
  `docs/任务台使用说明.md` §3.1 表格 + **新增 §3.9**（症状/根因/处置/代价）。

### 修复

- **Codex CLI 不能联网（"沙箱受限"）**。根因两层叠加（2026-10-14 本机实测复现，
  同一命令两次对照）：
  ① `codex exec` 默认 sandbox 策略 `workspace-write`，该策略下
  `sandbox_workspace_write.network_access` 默认 false；
  ② 本机 `~/.codex/config.toml` 的 `[windows] sandbox = "elevated"` 是 Windows
  原生沙箱（专用低权沙箱用户 + 防火墙规则），联网命令被整体挡掉。
  该文件顶层的 `network_access = "enabled"` **位置错误被静默忽略**——
  codex 只认 `[sandbox_workspace_write]` 表内的同名字段。
  实证：旧预置 curl 退出码 1、`%{http_code}` = `000`；加旗标后退出码 0、`200`。
- **顺带发现（未改，告知用户）**：`~/.codex/config.toml` 顶层的
  `network_access = "enabled"` 是无效写法。任务台已不受影响（旗标绕开沙箱），
  但他**交互式** codex 会话若也想放行网络，需把该键挪进
  `[sandbox_workspace_write]` 表。这是用户的个人全局配置，应用不代改。

### ⚠️ 未完成

- 去沙箱是"全有或全无"：没有"放行网络但保留文件系统隔离"的中间档预置
  （中间档依赖用户全局 config.toml，跨机器不可移植，且 Windows 原生沙箱的
  防火墙是否理会 `network_access = true` 未经本机实证）。需要的话下一步：
  在本机实证该开关，再决定是否加第二档预置。
- 播报排队机制遗留（v0.42.0 起记）：排队只覆盖终态类；合并提醒只报条数不报槽位；
  队首等待无实时进度。

---

## [v0.43.2] — 2026-10-14

> 署名：workbuddy / easyinput-prototype / AI小助手

> patch：**任务台灯语 v2 通过真机验证**，关闭 v0.43.1 的最大遗留项。
> 同时修两处会让"下一位agent / 下一个打包的人"掉坑的配置缺失（见「修复」）。
> **不改协议、不改固件、不改任何既有配置。**

### 新增

- **`device_led` 模块文档新增「Windows HID 描述符缓存」一节**：
  把本轮排障的核心结论沉淀成代码旁的知识——灯完全不响应时**先让设备重新枚举**，
  不要先怀疑代码。同时记下 `0x01`（设备不支持该 Report ID）与 `0x57`
  （支持但参数/长度不对）的语义区别，提醒全矩阵探测必须按错误码分类读。
- **使用说明新增「已完成的硬件联动」小节**：灯语过去被列在「当前未实现的能力」
  表里（虽然那行写了⚠️），结构上会让人以为它还没做完；现已移出并明确标注验证日期。

### 变更

- **代码内灯语状态标注**：v2 语义**待复验 → 已实机验证**（`agent_desk/mod.rs` 模块表、
  `device_led.rs` 模块文档、`docs/任务台设计方案.md` §16.5）。
- **设计方案 §19 已知问题第7 条关闭**：改为记录"已复验通过 + 长期保留的排障口诀"。
- 使用说明 §2 灯语对照补第5 颗的准确行为（**闪完即灭/平时全灭**）——
  原文写"平时极暗"，那是 v0.43.1 之前的旧值，与代码不符。
- 使用说明 §2 灯语对照补一条排错提醒：**灯完全不响应先让设备重新枚举**。

### 修复

- **`src-tauri/Cargo.toml` 缺 `[features] custom-protocol`**（Tauri 官方模板标准项）：
  手工 `cargo build --release` 会产出**dev 模式包**——不内嵌前端资源，
  双击即报 `ERR_CONNECTION_REFUSED`（去 `devUrl` localhost:1420 找页面）。
  平时都用 `tauri build`（CLI 自动带该 feature），所以从未暴露。
  **修法**：补 `[features] custom-protocol = ["tauri/custom-protocol"]`。
  **判据**：exe 体积 17,611,264 → 17,890,816 B（+280 KB ≈ 内嵌的 dist）。
  出包命令现固定为 `cargo build --release --features custom-protocol`。
- **`transport::send_feature` 私有性**（`status_query.rs` 的未提交半成品调用它），
  编译报 `E0603`。已放宽为 `pub(crate)`；**未改动 `status_query.rs` 一个字**
  （多agent 协作纪律：不动他人半成品）。

### ⚠️ 未完成

- 无。灯语四项（四态 / 三档亮度 / 空闲全灭 / S5 按键闪）**均已在真机确认**，
  逻辑侧 `cargo test --lib agent_desk` **273 passed / 0 failed**。
- 说明：**HID 描述符缓存不是 bug，是 Windows 机制的固有约束**，无法"修掉"，
  只能靠"出问题时知道该先干什么"来消化。已写入代码注释与两份文档。

---

## [v0.43.1] — 2026-10-14

> 署名：qinyoux

> patch：v2 灯语实机反馈后**推翻"空闲不灭灯"这条设计**。仅 `device_led.rs` 常量与
> 两条测试，**不改协议、不改固件**。

### 新增

- 无。

### 变更

- **空闲状态改为全灭**（`IDLE_COLOR = Rgb::OFF`、`KEYPRESS_IDLE_COLOR = Rgb::OFF`）。
  灯语语义收敛成一句：**亮 = 有状态，灭 = 没事**。
- S5 保持态同步改全灭（原极暗白 `1,1,1`），静止时不再留一点微光。

### 修复

- **v0.43.0 那条"空闲要留极暗蓝、全灭留给掉线"的设计判断被实机证伪**，
  两条理由都不成立：
  ① WS2812 在 0–30 量级下 `(0,1,2)` **实际可辨**，用户会读成"这槽有微弱状态"，
     与"空闲 = 没事"直接矛盾——这是实机可见的语义噪声，不是理论问题；
  ② 真正的掉线**本来就有独立判据**：`send_feature_report` 返回 `Err`
     + `led_ok` 布尔翻转 + 只在翻转时记的日志。用"全灭"这个视觉信号去表达掉线，
     本来就是多余的间接编码，还制造了歧义。
- 连带修正 `idle_slot_is_dim_but_not_fully_off` → `idle_slot_is_fully_off`、
  `fifth_pixel_without_any_keypress_is_dim` → `..._is_off` 两条测试的断言与命名。

### ⚠️ 未完成

- **v2 灯语仍未实板复验**（空闲改全灭这一版尤其需要）。仅逻辑验证：
  `cargo test --lib agent_desk` **273 passed / 0 failed**。
  下一步：重插设备 / 重启宿主 HID 栈后实测四态 + 三档亮度 + 空闲全灭。
- **"按 S1 后全亮蓝"尚未定位到宿主侧**。已排除的可能（逐条读码确认）：
  `0x16` Report ID 正确、`queue.slots[].running` 快照链路完整、
  `leds_[i] = pixels[i]` 固件渲染严格同序无衰减。
  **最可能是固件回退到 `0x12` 渲染**——`agent_status_color(kRunning)` 返回
  `{0,0,28}` 正是**蓝色**，且 `render_agent_status()` 会把 5 颗全上同色（居中衰减）。
  即"全亮蓝"是 `0x12` 的特征，不是 `0x16` 的表现。
  下一步：看 `led_ok` 是否翻转 / 有无"特征报告下发失败"日志，据此判定是 HID
  描述符缓存（`0x16` 未被枚举）还是别的成因。

---

## [v0.43.0] — 2026-10-14

> 署名：qinyoux

> minor：任务台灯语（`0x16`）**v2 语义重定义**。起因是首次真机烧录后钦友实机报告
> "按 S2 灯还是 S1 亮、第 4/5 颗同时亮绿，与文档不符"。诊断结论见
> `.workbuddy/memory/2026-10-14.md`。**宿主侧单边改动，固件无需重烧**
> （`0x16` 是逐像素通道，语义完全由宿主决定）。

### 新增

- **`SlotLight` 四态枚举**（`agent_desk::device_led`）：`Playing` / `Running` /
  `Pending` / `Idle`，互斥，优先级 **播放 > 执行 > 待播 > 空闲**。
  新增纯函数 `slot_light_for(running, playing, pending)` 与
  `color_for(state, pending)`，把"判状态"与"选颜色"拆成两层各自可测。
- **S5 按键闪烁**：`KeypressFlash`（存 `std::time::Instant`）+ `record_keypress()`。
  按下时**当场下发**一帧全亮（`KEYPRESS_FLASH_COLOR`），120ms 后补一帧灭回去。
  心跳续期**不传**闪烁标记，避免每 5 秒假闪一次。
  `KEYPRESS_FLASH_MS = 120`、`KEYPRESS_FLASH_COLOR`、`KEYPRESS_IDLE_COLOR`
  均为具名常量。
- **三档亮度常量**：`BRIGHTNESS_DENOM = 3` / `BRIGHTNESS_MAX_TIER`，
  档位边界由 `brightness_tier()` 单点定义并直接暴露给测试。

### 变更

- **灯语 v2 布局**（`language_for`）：
  | 像素 | v1 | v2 |
  |---|---|---|
  | 1–4 | 各槽**未读**（绿，3 档亮度） | 各槽**状态四选一**（执行/播放/待播/空闲） |
  | 5 | 全局运行数 0–4 绿/黄/橙/紫/红 | **按键反馈**（按下闪 120ms，其余时间极暗） |
- **状态色**（色值仍为 0–30 量级，与固件既有色板一致）：
  执行 `RUNNING_COLOR` 蓝 `(0,12,30)` / 播放 `PLAYING_COLOR` 青 `(0,20,14)` /
  待播 `PENDING_COLOR` 绿 `(0,24,5)` / 空闲 `IDLE_COLOR` 极暗蓝 `(0,1,2)`。
  播放色由 v1 的**白** `(18,22,22)` 改**青**：白三通道拉满、耗电大，且低亮度下
  易与"待播绿"糊在一起；青色相独立且亮度明显低于蓝。
- **空闲不灭灯**（`IDLE_COLOR` 极暗而非 `OFF`）：全灭必须保留给"设备没连上/
  固件没烧/HID 没枚举"，否则"四槽空闲"与"掉线"长得一模一样、无法诊断。
- **签名变更**：`language_for` / `push_snapshot` 新增 `flash: Option<&KeypressFlash>`。
  闪烁**刻意不进 `RuntimeSnapshot`**——那份结构是 `Serialize + Eq` 的对外契约，
  而 `Instant` 不可序列化，把灯效需求泄漏进公共结构是错的。
- `DeskManager` 新增 `led_keypress` 字段；`handle_reserved_uuid` 在**所有校验之前**
  调 `record_keypress`（它表达"设备收到了这一按"而非"这一按被采纳"，
  被拒的按键也必须有反馈）。

### 修复

- **"按 S2 灯还是 S1 亮"——不是 BUG，是映射没读"哪个槽 running"**。
  v1 的像素 1–4 只读 `unread.slots[i].pending`，`running` 只被聚合成第 5 颗的
  一个数，**槽位维度的执行信息被整个丢掉**。"亮着"表达的是"有未读等你听"，
  不是"在执行"，文档措辞叠加导致了误读。现已由
  `slot_index_selects_its_own_pixel_only` 回归测试钉死槽位隔离。
- 第 5 颗"常亮绿"改为**按键短闪**。v1 的 `running_count == 0 → 绿`（待命）会让
  空闲时长期占用一颗灯表达"什么都没发生"，还会盖掉固件既有 idle 灯效。
- 闪烁计时用 `Instant` 而非墙钟：系统时间被时区同步/NTP 校正跳变会让 120ms 的
  闪烁变成常亮或永不亮。

### ⚠️ 未完成

- **v2 灯语尚未实板复验**：仅逻辑验证（`cargo test --lib agent_desk`
  **273 passed / 0 failed**，新增 7 条）。固件 `0x16` 已于今日烧录并 hash 校验通过
  （app bin `0x190BC0`，SHA256 `D4B3ED62…8AE16`），**本次改动不需要重烧固件**。
  下一步：钦友重插设备 / 重启宿主 HID 栈（让新 Report ID 重新枚举）后，
  对照四态 + 三档亮度实测。
- **`docs/任务台使用说明.md` 与 `docs/任务台设计方案.md` 仍是 v1 描述**
  （含"第 5 颗 = 运行数 0–4 五色"与"固件未烧"两处过期表述）。文档未在本版更新，
  下一步补齐 v2 灯语对照表。

---

## [v0.42.0] — 2026-10-14

> 署名：workbuddy/easyinput-prototype/锐

> minor：任务台播报链路**三个实录 BUG 的根因修复 + 多任务播报人性化**。
> 三个 BUG 均有日志 / rollout 文件 / 源码行号交叉实锤（诊断记录见
> `.workbuddy/memory/2026-10-13.md`），修复各配回归测试钉死。
> 改动集中在 `agent_desk`（Rust）与 `src/types.ts`，**无新增配置字段**。

### 新增

- **终态播报排队 + 合并提醒**（`notice_queue` / `drain_notice_queue`）：
  喇叭被占用时，终态播报不再被丢弃，而是排队（上限 3 条）等当前播放结束后补播；
  排队溢出丢**最旧**一条并计数，队列排空后补一条合并短通知
  （`NoticeKind::ResultsWaiting`，code **11** 追加在闭集末尾 —— `code()` 即缓存
  文件名契约，只能追加不能插队）。排队不打断任何人，包括用户的「播报未读」。
- **事件类播报按 `(槽位, kind)` 记账**（`NoticeGate::should_emit_for`，
  3 秒重复 guard）：S1 的「任务已完成」不再吞掉 S2 的「任务已完成」。
  进度类保持全局账本（跃迁 + 最小间隔）—— 进度本就是由最老任务驱动的全局量。
- **回归测试 10 条**：probe 首见 EOF 登记（`a_file_first_seen_is_registered_at_eof_and_history_is_ignored`）、
  保活截止覆盖整段音频（`keepalive_deadline_covers_the_whole_audio`）、
  双槽完成都能出声（`two_slots_completing_both_get_to_speak`）等。

### 变更

- **`desk_play_notice` 命令签名增加 `AppHandle` 参数**（前端 `invoke` 不变，
  Tauri 自动注入）：试听播放结束后同样触发播报队列排水。
- **`dispatch_task` 中「任务已开始」先于 `emit_state_if_changed` 发出**：
  反序时两条同拍通知都过得了去抖，谁抢到通道全看调度运气；
  正序时起点类先记账，随后的进度类正好被最小间隔挡住 —— 每次派发不再随机
  剪断一半（v0.41.15 实录里那条"0 样本被抢占"日志的来源）。
- **`PlaybackControl` 增加 `duration_ms()`**（设备侧精确值，含重采样与声道展开）：
  两处等待超时（`speak_notice` / `desk_render_result`）改用它，删掉
  `total_samples / 48_000` 的估算（既不除声道数也不顾设备采样率）。
- **前端 `src/types.ts` 的 `NoticeKind` 联合类型补齐**：补上 v0.41.15 起就缺失的
  `running_15min`，并同步新增 `results_waiting`（该类型缺口是 v0.41.15 CHANGELOG
  「未完成」里记下的已知问题，本轮消除）。

### 修复

- **BUG 1「任务下发同时播报任务已完成」= 外部会话监工误报**（`codex_probe.rs`）：
  旧 rollout 跌出 `seen` 回收表后，被任务台自己的 `codex exec resume` 追加写一笔
  ⇒ mtime 变新、重新进入 24h 窗口 ⇒ 被当成"全新文件"**从偏移 0 整读** ⇒
  18 个历史 `task_complete` 全变成新命中，`hits.first()` 播出 23 天前的一条。
  修复：① 首见文件一律按 **EOF 登记、不读内容**（删 `primed` 分支差异）；
  ② `run_probe` 在桌面任务执行中 + 跑完后 10 秒宽限内只消费偏移、不出声
  （`last_desk_finish_ms`，CLI 退出到落盘之间有真实延迟）。
- **BUG 2「只播一小段」= 音频保活截止时间单位错误**（`audio_out.rs`）：
  `total_from_pcm` 返回**毫秒**却被当**样本数**套 `micros = ms*1e6/rate`，
  48kHz 下时长缩 1/48 ⇒ >10.3s 的播报在 `0.02×时长+10s` 被 drop 流掐断
  （实测：13.2s 播到 10.5s 断、35s 播到 10.96s 断）。
  修复：保活截止改 `keepalive_deadline(duration_ms)`（毫秒进、毫秒出），
  时长取**设备侧**精确值。**没有也不该有"播放时长"设置** —— 真旋钮是
  `summary.max_chars`（该音色 ~6.6 字/s，252 字 ≈ 40s 过长，建议 80–120）。
- **BUG 3「S2 完成静默进未读」= NoticeGate 全局一本账**（`notice.rs`）：
  四槽共用"同类 kind 不重复播" ⇒ S1 完成（`last_kind=Completed`）后
  S2 完成被吞，无播报行、无 TTS 请求，只有未读 +1；即使放行，`take_channel`
  后到抢占也会剪断 S1 的长结论。修复即上述「按槽记账 + 终态排队」两项。

### ⚠️ 未完成

- 排队补播目前只在**终态类**上生效；进度类仍是被占用即丢（设计如此：进度可丢弃）。
  队首等待期间的**实时进度**（"正在播第 2/3 条"）未做。
- 合并提醒只报条数、不报"是哪几槽"：动态正文带槽位清单要先定"一句话说清四槽"的措辞。
- 板子喇叭（固件 PCM 下行）仍未接通，本轮排队/播报机制跑在 PC 声卡上；
  固件侧就绪后 `audio_out` 是唯一要替换的点。

---

## [v0.41.15] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> 纯文档版本（patch）：任务台功能已全部实现并稳定运行，本轮把散落在代码注释、
> CHANGELOG 历史与仓库外方案文档里的设计事实**收拢成两份正式文档**。
> **未改任何 Rust / TS / CSS 代码，未改任何配置字段**。

### 新增

- **`docs/任务台设计方案.md`**（工程级设计文档，对着已实现代码写）：
  第一性原理五条事实 → 设计决策映射；`dispatch_task → resolve_driver → dispatch_into → pump_slot`
  全链路；`pump_slot` 七步执行顺序与步间依赖（认领的 Busy 语义、播报早于历史、
  未读先落盘再发事件、会话回写条件）；四槽模型与两层串行守门；
  四档完成判定 + 世界门禁（含"门禁不过 ⇒ 状态翻 Failed"的翻转语义）；
  取消/超时（先杀进程树再回收的硬性顺序）；任务历史 / 产物发现 / 未读账本与租约 /
  状态播报去抖 / 音频输出 / 0x16 灯语 / codex 监工 / 会话续接的原理与边界；
  配置三层迁移机制；**设置项全量清单**（每个参数的用途/类型/默认值/取值范围 + 界面分组 + 代码位置）；
  对外契约（23 个 Tauri 命令 / 3 个事件 / 磁盘格式 / 固件 UUID 映射）；
  安全与 fail-closed 边界 12 条；代码地图；已知问题与未实现清单。
- **`AGENTS.md` §6 文件地图登记新文档**，并更新 `docs/任务台使用说明.md` 的职责描述。

### 变更

- **`docs/任务台使用说明.md` 全文重写**（原内容停留在 v0.35.0 时代）：
  对齐 v0.41.x 已实现状态——新增「播报未读完整链路（取租约 → 合成 → 真播 → 播完回执）」
  「预合成语料与离线播报」「租约语义（取租约不扣未读、释放 ≠ 听完）」
  「丢弃未读两段式确认」「调试工具板块」「运行时三态灯与第 5 颗灯 = 执行槽数」；
  设置速查表改为指向设计方案 §15 的全量清单；
  排错表补充 TTS 未配置 / 语料未预合成 / v0.41.13 修复的配色条目。

### 修复

- （无代码修复）

### ⚠️ 未完成

- 文档记录了实现本身的既有边界（均非本轮引入，实现侧持续跟进）：
  能力路由（Phase 4）、`async_task_poll` 完成判定、运行中实时日志、失败自动重试、
  键盘喇叭 PCM 下行、0x16 灯语未烧板、`stderr` 未进历史（`RunOutcome` 无该字段）。
- **新发现一处类型精度问题**（未修，无运行时影响）：后端 `NoticeKind` 闭集为 **11** 类
  （含 `Running15Min = 10`，`notice.rs:76-119`），而前端 `src/types.ts:533-543` 的
  `NoticeKind` 联合类型只有 **10** 类（缺 `running_15min`）。运行时无碍
  （kind 仅是字符串、语料文本由后端下发），但加新语料时两边需同步，已写入设计方案 §19。

---

## [v0.41.14] — 2026-10-13

> 署名：qinyoux

### 新增

- （无）

### 变更

- （无行为变更；修复仅消除误报错误日志）

### 修复

- **按键触发插件动作时刷「命令未注册：page」**：`pluginHost.ts` `runPlugin` 的幂等分支（插件已激活）直接 `execCommand(pluginId, existing, commandId)`，漏了冷启动分支有的 `__page__` 哨兵守卫。硬件按键触发链路是：Rust `host_action` emit `plugin-action` → 前端先 `plugin_trigger_activate`（Rust 固定用 `__page__` emit `plugin:activate`）→ 已激活插件走进幂等分支 → `commands["__page__"]` 不存在 → 误报。表现：功能正常（命令经 `invokePluginCommand` 照常执行），但每次按键都刷一条 ERR。修复：幂等分支补 `commandId !== "__page__"` 守卫，与冷启动路径对齐。

### ⚠️ 未完成

- 无。前端 `npm run build`（tsc + vite）已通过；生效需重新打包应用（`npm run tauri:build`）或重启 dev。

---

## [v0.41.13] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> ⚠️ 纯 CSS 修复 + 新增自检脚本（patch）。**未改任何 Rust 代码、未改任何组件结构、  
> 未改任何配置字段**。已在真实浏览器（WebView2 + CDP）里**实测确认**修复生效。

### 修复

- **⛔ 设置 → 总结与播报 →「播报哪几类」四个标签文字与背景几乎同色**（钦友截图反馈）。
  - **根因不是配色，是特异度**：那四项是 `<label class="desk-prefer-item">`，  
    外层套着 `.field`，于是 `.field label`（特异度 **0,1,1**）压过了  
    `.desk-prefer-item`（**0,1,0**）—— 它的 `color: var(--text-dim)` 与  
    `display: block` 都实际生效，**组件自己声明的 `color: var(--on-hi)` 根本没赢**。
  - **修法**：给精细规则加祖先前缀 → `.field .desk-prefer-item`（0,2,0），  
    禁用态同理改为 `.field .desk-prefer-item:has(input:disabled)` / `.off`。  
    同时把 `.desk-opt` 与它合并成同一条规则（消除两处定义打架），  
    并补 `margin: 0` 抵消 `.field label` 的 `margin-bottom`。
  - **实测**（CDP 读 `getComputedStyle`，非推断）：  
    浅色主题 `color=rgb(255,255,255)` / `bg=rgb(194,65,12)` = **5.18:1**；  
    深色主题 `color=rgb(26,18,6)` / `bg=rgb(255,107,53)` = **6.54:1**。  
    `display` 也确认由 `block` 变为 `flex`，佐证覆盖关系成立。
- **`.cdp_eval.mjs` 增加 URL 精确匹配**：主窗口 URL 为 `http://localhost:1420/`，  
  其 basename 为空串，原有「按 basename 精确匹配」逻辑定位不到它；  
  补了「URL 全等优先」分支后才能稳定命中主窗口（否则命令会静默打进字幕浮层）。

### 新增

- **`scripts/probe_computed.py`（`npm run a11y:computed`）：运行时配色自检**。  
  遍历**当前页面可见**的文字元素，取 `getComputedStyle` 的实际前景色，  
  逐层上溯找背景色，按 WCAG 算对比度；`< 4.5:1` 非零退出。  
  支持 `--sel "<选择器>"` 定点复查。需应用带 `--remote-debugging-port=9222` 启动。
  - **与 `audit_contrast.py` 的分工**：后者算「规则**声明**的组合」，  
    前者算「浏览器**实际生效**的组合」。本次事故正是后者才能发现。

### ⚠️ 未完成

- **⛔ 特异度冲突的自动检测：静态方案已尝试并放弃，三次都不可靠**（详见  
  `audit_contrast.py` 内的记录）：① 只比元素类型 → 误报 11 组；  
  ② 要求祖先 class 是后代子集 → 永远 False；③ 去 TSX 扫固定字符窗口找祖先 → 仍漏报。  
  根因是**祖先关系属于 DOM 而非 CSS**，选择器文本里不含祖先信息。  
  ⇒ 已从静态脚本里移除该检测，**只保留运行时检查**。  
  ⚠️ 代价：**目前只能定点复查，没法全站自动扫**（需逐页导航，未做）。
- **`probe_computed.py` 的 `bgOf()` 有已知局限**：它优先用元素自身背景，  
  这对「自身是色块」的元素正确，但对「文字色恰好等于父容器色」的情况  
  （如 `.switch-text` 在 `.switch` 内）会报出 1.00:1 假警报。  
  **这是已知未修项**，判断时需人工排除 —— 不能只看它的 ✗ 列表。
- **WebView2 拒绝非浏览器来源的 CDP 连接**（403）。探针用 `suppress_origin=True`  
  绕过；未验证在其它 Chromium 内核上是否同样有效。
- **未做全应用逐页遍历**：探针只覆盖当前可见页。
- **未提交 git**：工作树含其他 agent 在途改动，切分时机由钦友决定。

---

## [v0.41.12] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> ⚠️ 纯样式（patch）。**未改任何 Rust 代码、未改任何组件结构、未改任何配置字段**。  
> 仅新增一个审计脚本 + 改 CSS 变量取值与 6 处 `color` 声明。

### 新增

- **`scripts/audit_contrast.py`：对比度自动审计**，`npm run a11y:contrast` 挂为门禁。  
  解析 `styles.css` 的全部规则，对**每一对「背景色 + 前景色」**&#x5728;**深浅两套主题**下  
  按 WCAG 相对亮度公式算实际对比度，低于 4.5:1 即 FAIL 并以**非零退出码**结束。
  - 渐变背景也纳入：取渐变**两端中对比度更低**的一端算保守值。
  - **为什么必须是脚本而不是人眼扫**：本项目的 `--accent-solid` 在两套主题下亮度  
    **完全相反**（深色=亮橙 `#ff6b35`，亮色=深橙 `#c2410c`）。凡是把"压底色的字色"  
    抽成**一个变量**的规则，就必然在一套主题下失效 —— 这是规律性问题，肉眼只会  
    看到"某处看不清"，看不出它是**哪一类**问题、还有多少处。
  - 解析器用**括号配对**扫描而非正则找 `{...}`（`@media` 嵌套会让正则只抓到最内层），  
    并在求值前**剥离注释**（注释里常含花括号，会把选择器解析坏）。

### 变更

- **亮色主题 `--text-dim`：`#78716c` → `#6f675e`**。原值压 `--panel-2` 只有  
  **4.40:1**（差 0.10 即不合格），影响 **13 个选择器**（次级按钮、`.badge`、  
  `.mode-btn`、`.desk-tag`、`.switch-state`、`.cal-cell` 等）。新值 **5.10:1**，  
  是同一色相族（stone）里的最小改动。深色主题同项实测 5.89:1，**未改动**。

### 修复

- **⛔ 收口 v0.41.11 记下的"同类隐患"：5 处主按钮在深色主题下白字压亮橙（2.84:1）**。  
  `.btn-primary` / `.btn.primary`（含 `:hover`）/ `.chip-btn.chip-active` /  
  `.mic-btn`（录音大按钮）/ `.field button` 全部改用 `--on-hi`。  
  顺带把 `.brand-mark`（侧边栏 logo 渐变块）也改了 —— 白字压那个浅橙渐变  
  只有 **2.08:1**，之前一直读不出来。hover 态一并核查：深色 5.52:1 / 亮色 7.31:1。  
  ⇒ **当前 `styles.css` 中所有「填充 + 文字」组合在两套主题下最低 5.10:1**，  
  全部通过 WCAG AA。
- **未误伤**：`.toast` / `.toast-close` / 品牌渐变底等**不是压 `--accent-solid`** 的规则  
  保持原样（它们的白字分别配 `--err-strong` 等深色底，对比度本就达标）。

### ⚠️ 未完成

- **未在 GUI 人眼点测**：全部结论来自公式计算。**色彩感知与实际渲染仍可能有偏差**  
  （显示器色彩配置、抗锯齿边缘、次像素渲染都会影响观感），两套主题都需你实际过一眼。
- **审计覆盖 88 / 166 条规则**，78 条因含渐变以外的多值（`rgba()`、`var(--x-rgb)`  
  组装、继承）被跳过。若要 100% 覆盖需引入浏览器计算样式（Playwright），本版未做。
- **`--on-accent` 现已无实际使用者**（`toast` 保留），但**未删除该变量** ——  
  它可能仍被插件或未审计到的规则引用，删除有静默失效风险。
- **未量化"暗色底 + 禁用态"的观感**：`.btn-primary:disabled` 等用  
  `opacity` + `inset box-shadow` 表达，脚本无法评价"看起来像不像坏了"，需人眼。
- **未提交 git**：工作树含其他 agent 在途改动，切分时机由钦友决定。

---

## [v0.41.11] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> ⚠️ 纯前端样式 / 文案（patch）。**未改任何 Rust 代码，未改任何配置字段名与磁盘格式**，  
> 无需迁移。本版是 v0.41.10 的实测反馈修正。

### 新增

- **新增颜色变量 `--on-hi`**（深色 `#1a1206` / 亮色 `#fff`），专门给"实心高亮块"做前景色。

### 变更

- **设置页只保留「任务台相关设置」一个分组标题**。原 v0.41.10 在通用设置上方也加了一条  
  「通用设置」标题，钦友反馈**没必要**（页面开头不需要"从这开始"的提示，那条纯属噪音），  
  已删除。`.settings-group-head.first` 变体随之移除（无人使用）。
- **任务台组条目数徽标由 6 改为 5**：v0.41.10 写 6 是我数错了 —— 「终端启动」用的是  
  `CollapsibleNote`（标题不带 `h3`），所以该组实际是  
  文本注入 / 终端启动 / 界面选项 / 总结与播报 / 外部会话监工 = 5 项。  
  徽标是给人看的，数字错了比没有更糟。

### 修复

- **⛔ 实心高亮块在深色主题下"字压底色"读不出来**。`.desk-opt` 与 `.desk-prefer-item`  
  用 `background: var(--accent-solid)` + `color: var(--on-accent)`，但**两套主题的  
  `--accent-solid` 亮度完全相反**：深色是亮橙 `#ff6b35`、亮色是深橙 `#c2410c`。  
  而 `--on-accent` 在两套主题下**都是 `#fff`** ⇒ 深色主题下白字压亮橙，  
  对比度仅约 **2.9:1**（远低于 WCAG AA 的 4.5:1），属于"看着有字、实际读不出"。  
  根因是**复用了 `--on-accent`**：那个变量在亮色主题成立，在深色主题不成立。  
  修法：新增 `--on-hi` 由两套主题各自赋值（深色底配近黑 ≈ 8.6:1，亮色底仍配白 5.2:1），  
  两处实心块改用它。**未改动任何其它使用 `--on-accent` 的既有组件**  
  （按钮、模式按钮等）—— 它们属同一类隐患，但不在本版范围，留作下一版统一处理。

### ⚠️ 未完成

- **未在 GUI 人眼点测**：两套主题下 `--on-hi` 的实际观感、分组吸顶行为、  
  删掉两节后的页面长度变化均未确认，只过了 `tsc` 与 `build`。
- **⚠️ 同类隐患未清干净**：`--on-accent` + `--accent-solid` 的组合在深色主题下  
  另有约 10 处使用（`.btn-primary`、`.btn.primary`、模式按钮等），  
  白字压亮橙的对比度问题依然存在。它们的底色比高亮块更大，**可读性问题更明显**，  
  建议下一版统一收口到 `--on-hi`。
- **未做 WCAG 数值自动化验证**：`--on-hi` 的对比度是按 sRGB 相对亮度公式手算的，  
  没有跑自动化检测工具。
- **未提交 git**：工作树含其他 agent 在途改动，切分时机由钦友决定。

---

## [v0.41.10] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> ⚠️ 纯前端分组 / 文案 / 一个输入框范围（patch）。  
> **未改任何 Rust 代码，未改任何配置字段名与磁盘格式**，  
> 存量 `config.json` / `agent_desk.json` / `cli_terminal.json` 无需迁移。  
> `summary.max_chars` 变了范围（前端夹取到 20–1500），**不改磁盘上已有的值**，  
> 只在用户手动改动时才夹取。

### 新增

- **设置页分组标题条（sticky）**：`.settings-group-head` / `.desk` / `.first` /  
  `.settings-group-title` / `.settings-group-desc` / `.settings-group-count`。  
  左侧品牌色竖条 + 标题 + 一句"这组管什么" + 条目数徽标。  
  **sticky 是关键**：滚到中段时用户仍能看出自己在看哪一组，而不只是在分组起点可见。
- **`summary.prompt_template` 的编辑入口（新增多行文本框）**。该字段此前  
  **后端有真实读取点**（`agent_desk/mod.rs:2508`，替换 `{task}`/`{output}`/`{max_chars}`）  
  但**前端零入口** —— 属于"配置存在但用户改不了"。仅在「总结用执行器」非空时显示  
  （留空 ⇒ 根本不会走二次总结，模板无意义）。
- **「总结用执行器」与「摘要字数上限」的用途说明**（此前完全无说明）。

### 变更

- **设置页按"通用 / 任务台"分成两组**（`GeneralSettings.tsx` 内部插两个标题条）。  
  分界依据不是视觉喜好：`通信配置 / 界面主题 / Everything 搜索` 管的是**应用自身**  
  （换机器、换用户仍成立）；`文本注入 / 终端启动 / 界面选项 / Agent 任务台` 管的是  
  **任务台链路**（文字怎么送出去、Agent 怎么派发、结果怎么回来）。两类设置的变更  
  节奏与影响面不同，此前平铺在同一滚动容器里没有任何标记。
- **`summary.max_chars` 前端夹取到 20–1500**：后端 `VoiceSpec::resolve` 实际生效值是  
  `max(20, 配置值)`，而 TTS 服务端实测接受 1500 字 —— 原界面无 `min`/`max`，  
  可以填 0（会退化到 20）或 99999（必然被 TTS 截断）。
- **「外部会话监工」标题去掉「（C 方案）」**：`C 方案` 是 v0.20 方案文档里的编号，  
  对用户无意义。补上"默认开：没装 codex 时目录不存在 ⇒ 什么都不发生"。
- **「产物目录」说明改为陈述它同时是 `tasks/`（任务历史）的落点** —— 原说明只说  
  "产物落这里"，漏了它还管历史，而这正是用户改错目录后找不到历史的原因。

### 修复

- **⛔ `types.ts` 的粘贴键位说明与实现相反**。原文写「自动：Git Bash → `Shift+Insert`」，  
  而后端 `text_delivery.rs::resolve_for_window` 的实测表里 mintty 那一行是  
  **`Ctrl+V`** —— mintty 读修饰键用泛用 `VK_SHIFT`，**程序模拟发的 `Shift+Insert`  
  一个键都到不了目标**（人手按有效 ≠ 程序模拟有效），而它为注入专门留了 `Ctrl+V` 分支。  
  该错误结论来自早期**人手实测**，v0.30.3 已用程序模拟推翻，但 `types.ts` 的说明  
  一直没跟上。本版同步修正 `PasteStyle` 文档注释、四个选项的 `hint`、以及  
  `GeneralSettings` 里引用同一错误结论的折叠说明。  
  ⚠️ 顺带发现**同源的第二处**：`text_delivery.rs:132` 的 `ShiftInsert` 文档注释  
  仍写"mintty / Git Bash 的约定" —— 本版未改 Rust，留作下一版。
- **⛔ 「总结用执行器」空值的说明是假的**。原文写"复用「模型配置 → ② 语音编辑」的  
  LLM 通道"，但 `summarize_via_driver` 对空 `driver_id` 是  
  `mgr.store.get().driver("")? ` —— **直接返回 None，即不做任何总结**；  
  而选中执行器时走的是该执行器**自己的独立客户端**（`api_driver.rs` 明确"不碰  
  全局语音编辑配置"）。原说明会让用户以为两处配置联动。已改为按真实行为描述。
- **⛔ 「摘要字数上限」的说明引用了一个不存在的限制**。原文"TTS 硬上限 512 Token"  
  来自 `tts.rs` 早期注释，**实测已推翻**（服务端接受 1500 字，无 512 硬限制），  
  `tts.rs:53` 自己也已更正。已同步。

### ⚠️ 未完成

- **未在 GUI 人眼点测**：分组标题条的 sticky 行为、浅色主题下的色条对比、  
  新增的多行文本框布局均未实际确认，只过了 `tsc` 与 `build`。
- **分组未做成"折叠整组"**：两个标题条目前只做视觉分隔，不控制下方卡片的显隐。  
  若日后通用设置要再增项，可考虑整组可折叠 —— 本版未做，避免改变既有交互。
- **`text_delivery.rs:132` 的 `ShiftInsert` 文档注释仍是错的**（应与本版 `types.ts`  
  一并更正为 Ctrl+V 才是 mintty 的程序模拟有效解）。属 Rust 侧注释，本版按  
  "不改 Rust" 的范围约定未动。
- **未做配置项的 WCAG 对比度量化**（沿用 v0.41.9 的口径）。
- **未提交 git**：工作树含其他 agent 的在途改动，切分与提交时机由钦友决定。

---

## [v0.41.9] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

> ⚠️ 纯前端文案 + 样式版（patch）。**未改任何 Rust 代码，未改任何配置格式与命令签名**，  
> 已有 `agent_desk.json` / `config.json` 无需迁移。

### 新增

- **`styles.css` 新增「任务台：选项状态高对比」样式段**（`.desk-opt` / `.desk-opt.off` /  
  `.desk-opt-reason` / `.desk-tag.off` / `.desk-prefer-item` 新形态 / `.btn-ghost:disabled` /  
  `.desk-driver-row.off`）。规则：**可用 = 实心填充 + 高对比前景色 + 正常光标**，  
  **不可用 = 无填充 + 虚线置灰 + `not-allowed`**，禁用原因常驻显示在选项右侧（不是 hover 才看得到）。
- **`.btn-ghost:disabled` 此前完全没有样式** —— 禁用态与可点态视觉无差别，是本版修掉的既有缺陷。
- **`DeskSlotBinding` 新增 `MODE_UNAVAILABLE` 映射**：把「按能力」「自动判定」两种绑定方式  
  在选项文字与提示行上标为**不可用并给出原因**。
- **`DriverEditor` 新增 `COMPLETION_UNAVAILABLE` 映射**：把 `async_task_poll`（异步任务轮询）  
  标为不可用并说明"未实现、选它永远判不出完成"。

### 变更

- **任务台四个组件的说明文字整体精简**（`DeskView` / `DeskSettings` / `DeskSlotBinding` /  
  `DriverEditor`）：每条压到一行以内，删掉口语化铺垫、重复论证与同段内重复的结论。  
  纯文案改动，**不影响任何功能与逻辑**。
- **顺带修正一处过时说明**：`DeskSettings` 顶部原文写"把完整命令行投进你正在用的终端并回车"，  
  那是 v0.26 的「可见执行」路线，**v0.35.0 已整体删除该出口**。原文案属于"文档在骗人"，  
  本版改为按现行行为描述（后台执行 → 播报 → 产物自动打开）。
- **执行器行新增「不可调度」徽标 + 整行虚线置灰**：判据是 `已配置 && 已启用`  
  （与后端 `is_schedulable()` 对齐），任一不满足即标不可用。行本身**不隐藏** ——  
  用户要能看出"它存在、只是现在派不了"。
- **槽位卡新增「不可用」徽标**：`s.notes` 非空时显示（原因移入 `title`，  
  详细文字仍在卡片的 notes 列表里）。

### 修复

- **「按能力」「自动判定」两种绑定方式此前与可用项长得完全一样**。后端  
  `agent_desk::mod.rs::resolve_driver` 对这两种模式**直接返回 Err**（Phase 4 的 Router  
  尚未实现，"失败不静默"硬约束），用户配完按下键才撞到报错。现在下拉项文字带  
  `（不可用）`、选中时提示行给出原因。  
  ⚠️ **刻意不 `disabled` 掉 `<option>`**：那会让老配置里的这两种绑定**读不出来**，  
  反而变成另一种"看不出问题在哪"。
- **预合成语料列表的「未预合成 ⇒ 播不出来」从 `warn` 改为 `off` + 显式「不可用」徽标**。  
  原状态标的是"警告"，但它其实是确定的不可用态。
- **播报区「⚠ 语料未预合成 ⇒ 这一次其实没出声」改为 `desk-tag off` 的「未出声」**，  
  视觉上与其他状态徽标同族，不再是一句漂浮的红色文字。


### ⚠️ 未完成

- **未在 GUI 人眼点测**：本版只过了 `tsc --noEmit` 与 `npm run build`，  
  高对比配色在深/浅两套主题下的实际观感**未逐处确认**。
- **深浅主题对比度未量化**：`.desk-opt.off` 用 `opacity: 0.7` + `--text-dim`，  
  没有用 WCAG 数值验证过是否达到"弱化但仍可读"（建议 ≥ 4.5:1）。
- **`:has(input:disabled)` 依赖 WebView2 版本**：`.desk-prefer-item` 的自动禁用态  
  用了 CSS `:has()`。Chromium 105+ 支持，本项目 WebView2 应无问题，但未在旧版上验证。
- **未提交 git**：按惯例由钦友决定切分与提交时机。

---

## [v0.41.8] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- **`UtteranceTracker`（识别侧分句状态机，`src-tauri/src/asr.rs`）**。把服务端「会乱序、会修订、会合并说话人」的结果帧，  
  收敛成「一句一 uid」的干净事件流，是本次修复的核心机制。
  - `FinalReason`（`ServerDefinite` / `SpeakerSwitch` / `Boundary`）：定稿原因，仅用于日志定位，UI 不依赖。
  - `UttAction`（`Partial` / `Final` / `Correction` / `Ignore`）：一帧可产出多个动作（先封口旧句、再开新句）。
  - 句子身份改用**服务端 `start_ms`**（而非本地自增计数器），定稿时不再自增 uid。
  - `flush()`：会话停止时把仍在流式中的最后半句封口，**不再丢尾巴**。
- **`lt-correction` 事件 + `TranscriptSentence.correction` 标记**。服务端二遍重识别（`enable_nonstream`）回补  
  已定稿句的原文时，只改它该改的那一行原文，**不新建行、不重翻译**（避免重复调用与闪烁）。
- `UtteranceTracker` 新增 **9 个单元测试**（单人连续 / 换人封口 / 句内打断 / 连续短句「好的」不被去重吞 /  
  迟到修订走 Correction / flush 尾巴 / flush 幂等 / 空文本 / 缺 speaker 兜底），锁死本次全部根因路径。

### 变更

- **句子身份从「本地计数器」改为「服务端 `start_ms`」**。这是行为修正：旧计数器在换人/打断时**不会前进**，  
  导致后续帧写到错误的行上（详见下方根因）。已有导出记录的 `utterance_id` 语义不变，仅来源更可靠。
- `run_translate` 主循环与收尾循环的 4 个帧处理点统一改走跟踪器；删除 `let mut last_final = String::new()` 跨句去重。
- 错误判据由 `last_final.is_empty()` 改为 `!tracker.has_finalized()`。
- `live_translate.rs` 翻译消费端**删除重复的 `lt-final` emit**（定稿事件统一由识别侧发出，此前一处发两遍）。
- 前端 `live-translate/index.js`：行状态新增 `finalized` 标志；`onPartial` 不再写已定稿行；  
  `ensureRow` 在定稿后不再改说话人标签；新增 `onCorrection` 订阅 `lt-correction`。
- 插件 `live-translate` 版本未动（本版只加防御，不改插件行为面）。

### 修复

- **【主修复】多人对话/会议场景下，speaker1 末尾一段文本被异常清除或覆盖**。三条独立根因：
  1. **句内打断（主因）**。`utterance_id` 是本地自增计数器，**只在收到 `definite=true` 时才前进**。  
     speaker1 还没被服务端定稿，speaker2 就插话 → 计数器不前进 → speaker2 的中间结果继续打在  
     speaker1 的 `utterance_id` 上；而前端 `onPartial` 是**覆盖式写入** → speaker1 末尾被抹掉。  
     旧代码在服务端 `definite` 缺失时没有任何封口机制，**句子边界完全依赖服务端**。
  2. **同 `start_ms` 内的说话人变化未被识别**。服务端被打断时会把两人内容塞进**同一条** utterance  
     （`start_ms` 相同），此时只有 `speaker_id` 变化能标识换人。修复过程中实测发现：  
     同句判据原本**只看 `start_ms` 相同即判为同句**，等于漏掉了打断场景 —— 已改为  
     **起点相同 且 说话人相同** 才算同句。
  3. **跨句文本去重误杀**。旧代码用 `text != last_final` 跨句去重，speaker1 末句与 speaker2 首句  
     同为「好的 / 对 / OK」这类短句时，第二句被判重复 → **定稿被吞且计数器不前进**  
     → speaker2 文本覆盖 speaker1 的行。会议里「好的」「对」频次极高，这条实际命中率很高。
  - 另修**迟到修订误建新行**：服务端二遍重识别回补已定稿句时，帧若按「新句」处理就会盖到  
    speaker2 正在写的那一行。现按 `start_ms` 命中 `finalized` 表 → 走 `Correction`。
  - 触发条件：说话人分离模式（`mode=conversation`）+ 存在打断/紧接接话/短应答。  
    单人连续说话不触发（旧计数器正常前进）。
- **前序改动遗留的测试编译失败（`cargo test` 此前根本跑不起来）**。`cargo check` 不编译 `#[cfg(test)]`，  
  所以 4 处测试构造体缺字段一直没暴露：`live_translate.rs` 的 `RecordedSentence` 缺 `start_ms`/`end_ms`，  
  `plugin_registry.rs` 3 处 `PluginManifest` 缺 `hidden`。已补齐，`cargo test --lib` 现为  
  **501 passed / 0 failed / 4 ignored**。

### ⚠️ 未完成

- **`live-translate` 旧版「术语 prompt + 热词」入口未重建**（承自 v0.41.6 的覆盖事故）。  
  后端 `lt_start` 无对应参数，需要先定接口（是否走 `config` 透传），再改前端。
- **打断场景的端到端实测未做**：本次修复靠单元测试锁定逻辑，未接真实双人音频跑过。  
  下一步：拿一段真实会议录音（或两路麦克风）复现原场景，确认末尾文本不再丢失。

---

## [v0.41.7] — 2026-10-13

> 署名：workbuddy/easyinput-prototype/锐

### 修复

- **SS 命令面板中文文件名乱码（`ss_everything_search`）**。根因不是显示层，而是**编码层错配**：  
  `everything.rs` 调用 `es.exe ... -csv -size -attributes` 后读 stdout，但 es.exe 是 **ANSI（本地代码页 ACP）程序**，  
  把 UTF-16 文件名按 ACP（本机 936 = GBK）转换后写出——Windows 管道是**裸字节流、没有编码协商**，  
  子进程不会因为父进程想要 UTF-8 就改编码。代码随后用 `String::from_utf8_lossy` 按 **UTF-8** 解这些 **GBK** 字节，  
  4 个字节各自变成一个 U+FFFD，「文档」(`CE C4 B5 B5`) 就成了 `�ĵ�`，且信息不可恢复。
  - `from_utf8_lossy` 的「lossy」只保证**不 panic**，不代表解码正确——这是本 bug 长期未被发现的原因。
  - 原文件头注释「CSV 格式（UTF-8）」是**错的**，很可能就是当初实现偏差的依据；已改正。
  - 修复方式：改用 `-export-csv <临时文件> -utf8-bom`（es.exe **唯一**能产出 UTF-8 的方式，实测首字节 `EF BB BF`），  
    读文件后剥BOM 再按 UTF-8 严格解码。`String::from_utf8` 失败会**显式报错**而非静默乱码。
  - 注意 `-csv` 单独使用**不会**让 stdout 变 UTF-8，别被参数名误导。  
    -临时文件名带 `pid + 原子序号`，并发搜索不会互相覆盖；无论成功/失败/启动失败都清理临时文件。

### 新增

- `everything.rs` 新增 5 个测试（共 12 个，全过）：
  - `test_strip_bom` / `test_bom_stripped_bytes_are_valid_utf8`：BOM 剥离与 UTF-8 合法性。
  - `test_gbk_bytes_would_break_utf8_lossy`：**反向证明**旧写法必然产生替换字符，防止有人改回去。
  - `test_search_real_es_chinese_not_mojibake`：真实 es.exe 端到端搜中文，结果路径不得含 U+FFFD。
  - `test_search_real_es_preserves_non_acp_chars`：GBK 表示不了的字符（Ω/Δ/ß）必须保真。

### ⚠️ 未完成

- ⚠️ 未完成：主工作区当前**编译不过**，但与本次改动无关——`live_translate.rs`（`RecordedSentence` 缺  
  `start_ms`/`end_ms` 初始化）与 `plugin_registry.rs`（`PluginManifest` 新增 `hidden` 字段后测试构造器未跟上）  
  是**其他 agent 未完成的改动**。按共树纪律未代为修改。  
  本次改动已抽出为独立 crate 编译并跑通**12/12 测试通过**，含真实 es.exe 端到端验证。
- ⚠️ 未完成：Super StartUp（`E:\MY_AI\workbuddy\Super_Startup`）存在**同类但较轻**的乱码问题  
  （`_cli_search` 走文本管道 + `errors="replace"`），且其 SDK 快路径因`tools/Everything64.dll`  
  为 1.1.0.18 老版、缺 `Everything_IsQueryRunning` 而**长期失效**，实际 100% 降级到es.exe CLI。  
  **两个项目无任何依赖关系**，待钦友确认是否一并修。

---

## [v0.41.6] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- **live-translate 恢复「清空」按钮（v0.41.5 事故修复）**：v0.41.5 部署内置插件时，用仓库里的旧模板（live-translate v0.1.0，只有开始/停止/导出）覆盖了运行目录里用户实际在用的较新版本，导致「清空」按钮消失。本版在仓库模板上重建该功能集：
  - **清空**：只清屏幕显示，不动后端会话记录——导出仍含本次全部内容（按钮 title 明示）。
  - **导出格式**：Markdown / 纯文本 / JSON 可选（后端 `lt_export` 本就支持，UI 此前硬编码 md），选择持久化。
  - **打开目录**：定位最近一次导出的文件所在目录（`explorer.exe`，反斜杠路径 + `eo.run`）。

### 变更

- **部署脚本加保险**：`deploy-plugins.mjs` 覆盖前自动把运行目录旧版整体备份为 `<id>.bak.<时间戳>`，备份失败直接中止部署——根除「静默覆盖丢功能」这类事故。
- live-translate 插件版本 v0.1.0 → **v0.2.0**；用户配置（lt_config）原样兼容，仅新增 `export_format` 字段（缺省 md）。

### 修复

- 修「实时翻译清除上下文按钮消失」。根因不是功能被删，而是**运行目录副本被仓库旧模板覆盖**（部署只读 `%APPDATA%` 目录、仓库模板未同步更新）。旧版代码无 git 跟踪、无编辑器历史，已无法逐字节恢复，按设计文档（清空只清屏不丢数据等）重建。

### ⚠️ 未完成

- ⚠️ 未完成：旧版的「术语 prompt + 热词」配置入口未重建——后端 `lt_start` 当前只收 mode/源/目标语种，没有对应参数，需先定后端契约再补 UI。钦友确认旧版行为后我再补。

---

## [v0.41.5] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- **「内容台」菜单合并（旋钮 + 命令一个入口）**：原「内容台 · 旋钮」独立类型取消，编码器三个角色（左旋 / 右旋 / 按下）并入「内容台」顶层菜单的下拉里，用「旋钮角色 / 命令」两个分组呈现——现在一个菜单配完内容台的全部 19 种绑定（3 旋钮角色 + 16 命令）。存储不变：选旋钮角色存 `kind=studio`，选命令存 `kind=named`，固件链路零改动。

### 变更

- **内置插件部署到运行目录**：demo 没藏住的根因是 `plugin_registry` 只读 `%APPDATA%\com.easyop.app\plugins\`，上一版只改了仓库 `plugins/builtin/` 模板。本次已跑 `plugins:deploy` 把 4 个插件同步到运行目录（`demo-quick-note` 的 `hidden:true` 已生效），用户数据（storage.json）保留。**需重启 easyop 或在插件管理页点刷新（plugins_rescan）生效。**
- **既有配置不受影响**：已绑「内容台 · 旋钮」的键升级后仍显示在「内容台」菜单下，动作值原样保留。

### 修复

- **demo 菜单未隐藏（v0.41.4 遗留）**：根因是改了仓库模板但没部署到运行目录，且 `deploy-plugins.mjs` 全量部署会被 `plugins/builtin/.workbuddy` 等非插件目录拦死。已修脚本：点名单仍严格报错，全量部署改为警告跳过非插件目录。

### ⚠️ 未完成

- 无（`tsc --noEmit` 通过；旋钮合并为纯前端改造，`cargo check` 上一版已过、本次未动 Rust。真机重启后确认菜单即收尾）。

---

## [v0.41.4] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- **菜单层级修正（顶层并列）**：「实时翻译」「内容台」「风格调色盘」三个插件菜单现在与「功能动作」同等级、并列的顶层菜单，不再嵌套进「功能动作」内部。底层存储仍是 `named`（动作值如 `studio.topics.save` 不变），固件下发与触发链路完全不动——纯 UI 归类，零协议风险。
- **后端 `get_vocabulary` 字段调整**：`action_groups` 只放各可见插件的命令分组；主机内置功能动作单独走新增的 `core_actions` 字段，「功能动作」菜单只渲染 `core_actions`，与插件菜单互不混入。内容台 16 条命令整组出现在「内容台」顶层菜单下，全部可绑到任意物理键。

### 变更

- **隐藏示范插件**：`demo-quick-note`（灵感速记（示范））清单加 `hidden: true`，不再进入按键菜单与 UUID 注册表；`plugin_registry` 与 `get_vocabulary` 均跳过隐藏插件。直接关闭了 v0.41.3 列为「⚠️ 未完成」的「demo 示范插件仍显示一个菜单」问题。
- **用户侧无破坏性变更**：既有按键配置存的是动作 id（如 `live-translate.start`、`copy`），存储范式仍是 `named`，升级后旧配置原样可用；只是编辑时的 UI 多了一层「每插件一个顶层菜单」。

### 修复

- 修掉 v0.41.3 遗留的「⚠️ 未完成」项：demo 示范插件菜单未隐藏。

### ⚠️ 未完成

- 无（本版三项需求均已落地，并通过 `cargo check` + `tsc --noEmit` + studio 回归台 194 项全绿）。

---

## [v0.41.3] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- 「功能动作」类按键的下拉从**单一扁平长列表**改为**按插件 / 功能归类的多个独立下拉菜单**：
  - 每个声明了 `commands` 的插件自动形成一个菜单，标题用插件名（实时翻译 / 内容台 / 风格调色盘 / 灵感速记等）；
  - 未被任何插件认领的主机内置动作归入 `id="core"` 的「功能动作」核心菜单。
  - 后端 `get_vocabulary` 新增 `action_groups` 字段（`ActionGroup { id, label, actions }`），前端 `KeyEditor` 遍历渲染，当前值落在所属组高亮、其余组显示「（未选）」。

### 变更

- 用户侧：**无破坏性变更**。既有按键配置（存的是动作 id，如 `live-translate.start`、`copy`）保持不变；只是编辑时的 UI 从「一个大下拉」变成「每插件一个小下拉」。
- ⚠️ 数据契约：前端消费的 `Vocabulary` 增加了 `action_groups` 字段（新增、非删除），`named_actions` 扁平并集保留以维持校验兼容；旧前端连新后端会忽略新字段，新前端连旧后端会因缺字段报错——属预期的前后向边界。

### 修复

- 间接消解了此前「功能动作下拉过长、难以管理」的体验问题（v0.41.2 把插件命令接入按键后，下拉一度塞进了 ~43 条插件命令）。

### ⚠️ 未完成

- 仅做了「按插件归类」；组内搜索 / 分组折叠仍待后续迭代。~~隐藏不常用插件（demo 示范插件）~~ 已在 v0.41.4 落地（`hidden:true`，不再进按键菜单 / UUID 注册表）。
- 真机未逐键回归：下拉交互为纯前端改造，已通过 `tsc` 类型检查与 `cargo build`；建议 dev 环境实际点开「功能动作」类按键确认各插件菜单正常列出、选中后能保存。

---

## [v0.41.2] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- **插件命令现在可被物理按键直接绑定（通用机制）。** 复用插件 `plugin.json` 已有的  
  `contributes.commands` 作为「可绑按键动作源」，无需插件新增任何字段。easyop 启动时  
  通过 `plugin_registry` 为每个插件命令（`<plugin>.<command>`）确定性生成 uuid v5 保留  
  UUID，自动并入 `get_vocabulary` 的 `named_actions` 词表与 `action_labels` 中文标签。
- 硬件链路打通：按键 → 固件回发保留 UUID → `host_action::handle_host_action` 识别插件动作  
  → emit `plugin-action` 事件（携带 `pluginId` / `commandId`）→ 前端 `pluginHost` 先  
  `plugin_trigger_activate` 确保插件激活，再 `invokePluginCommand` 执行对应命令。
- **实时翻译（live-translate）直接受益**：开始 / 停止 / 导出 三项（`live-translate.start` /  
  `live-translate.stop` / `live-translate.export`）首次出现在按键「功能动作」下拉里，  
  可绑到任意 S1–S8 / 编码器键，实现「按一下开译 / 停译 / 导记录」。内容台 studio、  
  其它插件命令同步自动获得该能力，无需逐个硬编码。

### 变更

- `Vocabulary.named_actions` 由 `Vec<&'static str>` 改为 `Vec<String>`，新增  
  `action_labels: HashMap<String, String>`（插件命令 id → 中文标签），供前端下拉渲染。
- `validate_action` 的具名动作校验放行 `<plugin>.<command>` 形式（新增  
  `plugin_registry::is_plugin_action` 判定），否则配置存不进固件。
- `payload` 的 `reserved_uuid_for_named` / `named_for_reserved_uuid` 增加插件动作分支，  
  复用现有「保留 UUID → 固件」下发/回发通道，不新增固件协议。
- `host_action` 在「普通应用库」分支前新增插件动作路由（3.9 节）。
- `uuid` crate 启用 `v5` feature（`Cargo.toml`：`features = ["v4", "v5"]`），用于确定性  
  派生插件动作 UUID。

### 修复

- 修正此前文档/认知偏差：实时翻译此前**没有**按键功能——其 `plugin.json` 只有  
  `commands`/`tools:[]`/`pages`，无任何按键绑定；"按 S1 启动"实际依赖宿主 host_action  
  配置 + 板子固件个人烧录，并非插件自带。本版让该能力成为真实可配置项。

### ⚠️ 未完成

- 仅覆盖 live-translate 的**核心 3 项**（开始/停止/导出）。清空、切换会议/对话模式、  
  打开页签等命令插件侧尚未声明，暂不能绑键；需要时补 `live-translate` 命令并自然纳入。
- 前端 `pluginHost` 顶层 `listen("plugin-action")` 为 fire-and-forget，未处理监听失败；  
  按键触发前提是对应插件已激活（监听回调会先 `plugin_trigger_activate`，但未被激活时  
  的极端时序未做真机压测）。建议真机验证「未打开实时翻译页签时按 S1 能否拉起并启动」。
- Rust 改动 `cargo build` 已通过；前端未单独跑 `tsc`/`vite build`，请合并前在 dev 环境  
  验证按键下拉出现 live-translate.* 且按一下能触发命令。

---

## [v0.41.1] — 2026-10-12

> 署名：workbuddy/easyinput-prototype/锐

### 新增

- 内容台「平台发布」：新增 `studio.publish.html` 命令 + `studio_io.py write-html-file` 子命令。  
  把整理好的内容交给排版 skill，**只落一个 HTML 文件**（浏览器打开全选复制粘贴到公众号即可），  
  不再生成 .md 稿件 / 投放说明 / 正文.txt / \_manifest.json 等任何附加文件。  
  产物路径记在选题的 `htmlPath` 字段。
- 排版 LLM 不可用时降级到本地零依赖转换器（`markdown-to-html`，样式对齐同一份排版规范），  
  并标注 `degraded` + 说明，保证平台发布不中断。


### 变更

- **移除「配图」阶段**：内容台由六阶段改为五阶段  
  （`inspiration → intel → body → humanized → exported`）。  
  历史数据若停在 `figures` 会被归一化到 `humanized`，不会把阶段回退到第一步。
- **术语统一**：`去AI味` → `内容优化`，`一条龙` → `一键执行`，  
  覆盖阶段名 / 按钮 / 菜单 / 提示文案 / 日志输出 / 浮窗字段 / 使用说明。
- 「打开目录」改为打开**文件实际所在的目录**：优先定位平台发布的 HTML，其次导出的 .md。
- `studio.export`（导出按钮）不再打小红书投放包 / 自动填公众号草稿，只做组稿 .md + `pub.py check`；  
  相关死代码（`checkWechatLogin`、pack/wechat 分支）一并移除。
- 插件命令数 13 → 16（新增 `studio.publish.html`、`studio.open.dir`），`plugin.json` 版本 0.4.0 → 0.4.3。

### 修复

- **「打开目录」先开到上一级、后开到「此电脑」**（v0.41.1 首发即真机失败，根因两轮才找全）：
  1. 第一轮：传给 Windows `explorer` 的路径用了正斜杠（`C:/Users/...`），被当相对路径处理 → 开到上一级。
  2. 第二轮（真因）：改成 `explorer /select,"C:\path"` **单个字符串**参数后，`runner`（`cmd.args()` argv 转义）  
     把含空格的串二次包引号，explorer 拿到畸形命令行 → 解析失败 → 打开「此电脑」。
  3. 最终解法（照 easyop 核心 `open_containing_folder` 的官方写法）：`explorer.exe` + **两个独立参数**  
     `['/select,', '<反斜杠路径>']`，路径本身不带引号；`/select` 失败时退回 `explorer.exe <反斜杠目录>`。
- 新增 `studio.open.dir` 命令（原只绑 UI 按钮，无法被快捷动作路由、也无法回归测试），  
  `plugin.json` 同步声明，插件版本 0.4.2 → **0.4.3**，命令数 15 → 16。
- 回归台新增 ⑤c「打开目录」12 项断言（参数形状 / 反斜杠 / htmlPath 优先 / 无产物拦截），  
  现 **194 项全绿**；「路径不带引号」这条就是本次真机 bug 的回归防线。
- 恢复被误删的「导出」按钮（及帮助条目、旋钮焦点 index 3）。
- 恢复 `cmdTypeset` 的本地兜底分支 —— 此前源码与运行副本都已丢失该分支，  
  LLM 一挂平台发布会直接硬失败。
- 修正灵感采集桥接脚本 `push_to_easyop_storage.py` 的默认阶段：  
  中文「灵感采集」→ 英文键 `inspiration`（内容台用英文阶段键，写中文会导致推进异常）。
- 回归台（studio harness）同步更新：导出改为「组稿+check」两次 `eo.run`，  
  新增平台发布（单文件 / 降级兜底 / 无正文拦截）断言；现 180 项全绿。

### ⚠️ 未完成

- 「打开目录」的真机行为需手点确认（explorer 参数解析无 GUI 自动化验证手段；回归台已锁定参数形状）。
- 平台发布目前只出公众号 HTML；小红书等多平台产物如需恢复，要另行扩展。
- 新选题 schema 仍保留 `figures: []` 惰性数据字段（无流程引用），供旧数据 compose 兼容。

---

## [v0.41.0] — 2026-10-09

> 署名：workbuddy/easyinput-prototype/AI小助手

### 新增

- **协作纪律新增「署名」一条铁律**（`AGENTS.md` §1 / §2 / §3.1）：每个 CHANGELOG 版本节必须写  
  `> 署名：宿主 / 项目 / 会话名`，三段分别标识执行环境、仓库、并行会话。  
  起因是 2026-10-09 的真实覆盖事故 —— 当时只有版本号没有署名，事后无法快速定位责任方。
- `AGENTS.md` §4 新增两条工作纪律：**禁止用旧内容整文件覆盖他人文件**（写前先 `Read` 确认基线，  
  必须整文件重写则先备份到 `.workbuddy/backup/`）、**发现被覆盖先取证再修**（比对 `mtime`、行数、  
  章节数、头部版本号，区分"没写成功"与"写了被覆盖"）。
- 提交信息格式新增 `- 署名：<宿主>/<项目>/<会话名>` 一行。

### 变更

- `scripts/version.mjs`：`bump` 支持 `--author "<署名>"`，缺省回落到环境变量 `EASYOP_AUTHOR`，  
  再回落到 `git config user.name`；骨架自动写入署名行，并新增 `### ⚠️ 未完成` 占位段。
- `scripts/version.mjs check`：**新增署名为强制校验项**，当前版本节缺署名直接输出 `结论：❌` 且退出码 1  
  （历史版本节不追溯，避免补签所有旧条目）。
- 人工改动同样要求署名（写 `human / <项目> / 钦友`），否则版本历史会出现"无人认领"的空白节。

### 修复

- 修复协作纪律的归因缺口：此前版本号只能回答"什么时候改的"，无法回答"谁改的"，  
  多 agent 共树时无法定位覆盖事件的责任方。
- 修复 `version:bump` 骨架缺 `### ⚠️ 未完成` 占位段的问题 —— 骨架里没有这一段，  
  agent 容易忘记写"没做完的功能卡在哪"。

### ⚠️ 未完成

- **历史版本节未补署名**：`check` 只校验当前版本节，v0.40.0 及更早仍无署名。  
  若要补全，需人工逐节回填，建议只在追查具体改动时才回溯，不做全量补签。
- **未验证**：本版只改了 `AGENTS.md` 与 `scripts/version.mjs`（纯文档 + Node 脚本），  
  **未在真机跑 `npm run version:bump`**。已验证的是 `check` 能正确拦下缺失署名（退出码 1）  
  以及 `bump --author` 能正确写入署名行 —— 后者即本版自身的版本节。
- 未回退既有 Markdown 格式化痕迹：README / AGENTS.md / CHANGELOG.md 中存在  
  大段表格对齐与行尾空格改动，来源不明（非本次改动），按 §4 纪律未擅自回退、未批量暂存。

---

## [v0.40.0] — 2026-10-09

> 署名：workbuddy/easyinput-prototype/AI小助手

### 新增

- 将“小智与 easyop 速度差异分析”正式并入 `docs/小智式AI接入easyop实施方案.md`，新增 TTFT/TTFA/完整完成时间定义、模型/上下文/网络/工具/TTS/CLI 收尾等因素拆解，以及分层、可复现的基准测试矩阵。
- 新增统一 `trace_id` 事件建议、P50/P95/P99 统计口径、延迟归因规则和脱敏测试结果要求，作为后续速度优化的数据基线。

### 变更

- 将实施顺序明确为：先补测量，再修 CLI 固定收尾延迟、开放 LLM 增量可见性、落地句子级 TTS、缩短语音播报，最后依据测试结果决定是否调整模型或网络。
- 本版仅更新实施方案、版本说明和版本号，不改变既有运行时行为、固件协议或用户配置格式；方案中的性能指标均为待验证目标。

### 修复

- 修正速度分析中容易把“首音更早”与“任务整体完成更快”混为一谈的问题，要求分别验收 TTFA 与完整完成时间。

### ⚠️ 未完成

- 尚未实现 `trace_id` 埋点、CLI drain 优化、LLM 增量展示、句子级 TTS 或音频流式播放；本版没有进行真机速度对比，下一步按文档第 13～14 节执行基准测试。

---

## [v0.39.7] — 2026-09-30

### 新增

- **开发板固件版本检测：状态条上「开发板检测」右侧多了固件版本徽标**  
  （后端命令 `device_firmware_status`）。  
  它显示的是板子**自己报出**的版本串 —— 实机实测 `0.4.53-idf-v2-ble-store-v1`。  
  链路：USB HID Feature Report `0x13`（17 字节请求）→ Input Report `0x11` / kind `0x04`  
  的分块响应（每块 50 字节 JSON，本次 451 字节共 10 块）→ 按声明长度拼接 →  
  校验 CRC16 与 UTF-8 → 取 JSON 里的 `firmware` 字段。

  **为什么这件事值得单独做**：在此之前，回答"固件到底烧没烧进去"只能靠"我记得烧过"  
  或看灯亮不亮 —— 两个都不算证据。版本串来自板子本身，是这个问题唯一可信的回答；  
  串里的 `idf-v2-ble-store-v1` 这种构建标记还能直接看出板子上跑的是哪一支构建。

  三个触发点：① 板子**离线 → 在线跃迁**时自动读一次；② 应用启动；③ 点击徽标手动重读。  
  **刻意不进 10 秒在线轮询**：这是一次完整的状态回读（实机约 100 ms、约 10 个分块），  
  放进轮询等于每 10 秒打扰一次板子，还会和按键监听器抢同一个 HID 读句柄  
  （抢的结果是偶尔丢一条 host_action，而那种丢失本来就没日志，极难回溯）。
- **`KIND_STATUS_RESPONSE = 0x04` 与 `REPORT_ID_STATUS_REQUEST = 0x13` 落成常量。**  
  此前 `constants.rs` 只把 `0x13` 写在"登记备查"的注释表格里，而且**没有 `0x04`** ——  
  后果是"即使固件回了状态，也会被当未知报文丢掉"，界面上表现为"板子不支持读版本"。  
  这类故障没有任何报错，只是安静地什么也不发生。
- 新增 `src-tauri/src/status_query.rs`：协议纯逻辑（请求帧构造 / 分块解码 / 组装状态机 /  
  JSON 摘要）与真机 IO 分离，配 19 条单测。  
  测试里有一个**按固件算法写的第二编码器**（`test_fixtures::encode_chunk`）：  
  编码走固件的规则、解码走本模块的规则做 round-trip —— 偏移写错一个字节就当场露馅，  
  不必插板子试。另外覆盖了乱序到达、重传、元数据不一致、CRC 不符、非法 UTF-8、  
  request_id 过期块等失败路径。

### 变更

- **`transport::send_feature` 由私有改为 `pub(crate)`。**  
  状态回读必须"同一个句柄先写后读"：`0x13` 的响应只会投给发起请求的那个句柄，  
  "先关再开"是收不到的。

### 修复

- 无（本版是新增能力，未改变任何既有行为）。

### ⚠️ 未完成 / 受限

- 状态 JSON 里还有 `saved` / `bytes` / `crc16` / `capabilities` / `ptt_hotkey` /  
  `hotkey_mode` 等字段，本版只把 `firmware` / `schema` / 电池两项接了出来，  
  其余留在原始 JSON 里没有展示。要做"检查配置是否真的存进板子"时，  
  这几个字段就在同一条链路上，不用再开新通道。
- 电池读数目前只在徽标的**悬停提示**里（`4171 mV / 97%`），没有独立呈现。
- 徽标只在"板子在线且读成功"时显示版本。**没有做**"本地记录的版本 vs 板上实际版本"  
  的比对 —— 本地本来就没有记录固件版本，这一项要等真的有 OTA/烧录流程时再说。

---

## [v0.39.6] — 2026-09-30

### 新增

- **任务台新增「丢弃全部未读」（后端命令 `desk_discard_unread` + 「播报」板块里的按钮）。**  
  位置：`播报 → 播报未读（完整链路）` 下方，用虚线分隔 —— 它与上面的播报流程**不是一回事**。  
  两段式确认（点一下变「确认丢弃 N 条」，再点才执行），不弹原生对话框：  
  原生 `confirm` 会阻塞渲染线程、样式与主题脱节，且在 webview 里行为依平台而异。  
  `slot` 参数省略 = 清全部，传槽号 = 只清该槽（后端已支持，界面目前只用前者）。

### 变更

- **未读的消费不再只有"听完"一条路 —— 但两条路的语义必须分清，界面文案已照此写。**  
  原先未读只能被 `desk_render_result`（取租约 → 真的出声 → 播完回执）一条一条消费，  
  这是**刻意**的："灯亮 = 有一个结果你还没听过"这条承诺必须可信。  
  但它有个真实副作用：调试时反复派发测试任务，未读攒到每槽上限 9 条后一直挂在灯上，  
  把"灯亮 = 有事等你"这个信号本身淹掉了。  
  所以补一个人工的、显式的泄压阀：
  - 「听完了」= `desk_ack_playback` ⇒ 未读减一（**已读**）；
  - 「放弃了」= `desk_discard_unread` ⇒ 未读清零（**删除**）。
  两者不可混用，所以丢弃**不会**被任何自动流程触发，只能由用户点。
- 丢弃时若该槽正持有播放租约，租约**一并作废**（内容已被放弃，那个正在进行的播放  
  不该再有消费未读的资格）；`generation`（历史代际）**不回退**，它是诊断量不是显示值。
- 丢弃会**立即落盘**（`agent_desk_unread.json`），否则重启后这些未读会"复活"。  
  日志记 `warn` 而非 `info` —— 它确实是一次数据销毁，不该被淹没在流水里。

### 修复

- （本版无独立修复项）

### ⚠️ 未完成 / 受限

- **APP 仍然没有「固件版本号检测」能力**（本版没做实现，只把事实查清了）。  
  固件侧其实**早就具备**：`main/app_main.cpp` 有 `kFirmwareVersion`  
  （如 `0.4.40-idf-v2-audio-pool`，产品版为 `0.4.53-idf-v2-ble-store-v1`），  
  并且有现成的读取通道 —— 发 USB HID Feature Report `0x13`（16 字节）→ 固件回  
  `0x11`、kind `0x04`，**分块**（每块 63 字节 = 头 4 + 元数据 9 + JSON 50，上限 512 字节），  
  JSON 第一个字段就是 `"firmware"`。  
  卡在 APP 这一侧：`src-tauri/src/constants.rs` 只把 `0x12/0x13/0x14/0x15` 写在注释里  
  "登记备查"，**没有定义常量、没有任何代码使用**；且 `KIND_CONFIG_ACK = 0x03`、  
  `KIND_HOST_ACTION = 0x05` —— **没有 `0x04`**，所以即便固件回了 kind `0x04`，也会被当未知丢掉。  
  （旁证：`commands.rs` 的 `ble_status` 注释已承认"状态只经 BLE GATT 特征发布……  
  本应用没有 BLE central 能力，所以读不到"。）  
  **下一步**：照 `transport.rs::send_config()` 的模式加一个 `query_status()`  
  （发 `0x13` → 循环 `read_input_report` 收 `0x11`/kind `0x04` 分块拼 JSON → 取 `firmware`）。  
  ⚠️ **必须插 USB 才能验证**（Feature Report 走 USB；查证时机器上的板子只连了 BLE + Wi-Fi，未插线）。

---

## [v0.39.5] — 2026-09-30

### 变更

- **任务台「播报」板块里的「预合成语料」整块收进二级折叠，默认收起。**  
  被折起来的是三段：语料计数器（`11/11 已就绪` / 离线播报 / TTS / 上一条播出的）、  
  「预合成全部语料」按钮及其说明、以及 11 条固定语料的逐条试听列表。  
  理由：这三段合计比一整屏还高 —— 实测展开态该板块高 **1140px**，而视口只有 720px；  
  而日常真正要看的是"能不能播 / 有没有未读"，它们都在标题行上。  
  折起来的是**管理语料**这件事，不是「播报」本身：  
  模块标题行（含状态徽标）、`播报未读（完整链路）`、`最近一次播报` 全部保持常显。  
  折叠状态照旧存 localStorage（键 `easyop.collapse.desk-notice-corpus`），  
  用户手动展开过之后不会被再收回去。  
  ⚠️ 只改了 `DeskView.tsx` 的结构与一条 `.desk-subfold` 样式，**未动后端**、未动其它板块。

### 新增

- **「播报」标题行新增未读徽标**（`未读 N`，仅 `N > 0` 时出现）。  
  播报本身**不产生**未读，但它是**消费**未读的地方 —— 模块折叠之后必须还能一眼看出  
  "有没有结果等你听"。此前该徽标不存在：模块一收起来，未读信息就只剩上面两张卡能读到。

### 修复

- 无（本版只做界面信息量收敛，未改变任何行为语义）。

---

## [v0.39.4] — 2026-09-30

### 修复

- **同时跑两条任务台任务时，进程直接闪退（退出码 `0xc0000409`）。**  
  根因**不在业务代码里**，是上游 Tauri/tao 的一处引用计数 UB：

  tao 的 Windows 事件循环 runner 是 `Rc<EventLoopRunner<T>>`（**计数非原子**），  
  它被 `AppHandle::clone()` 一路深克隆到 ——  
  `tauri-2.11.5/src/app.rs:481` → `WryHandle` → `Context` → `DispatcherMainThreadContext`  
  → tao 的 `window_target`。而 `AppHandle` 是 `Send + Sync + Clone`、  
  官方推荐用法就是"进线程 / 异步块前先 clone 一个"（`Clone` 这条路**没有被主线程守卫**）。  
  于是多个线程同时 clone/drop 就会把这个非原子计数**写坏到 0**；  
  下一次 `Rc::clone` 命中 `assert_unchecked(strong != 0)` 的 UB 检查 ⇒  
  `panic_nounwind` ⇒ **不 unwinding、直接 `abort()`**，进程无声消失、没有任何业务日志。

  **取证与完整回溯**：`.workbuddy/forensics/2026-09-30-rc-abort.md`  
  （异常地址落在 `std/src/panicking.rs:837` 的 `!can_unwind` 分支；  
  崩溃点 `alloc/src/rc.rs:3768` = `RcInnerPtr::inc_strong`；  
  调用方帧 `agent_desk::ensure_pump @ mod.rs:1923`）。

  **修法是堵计数器，不是堵出口**：`src-tauri/Cargo.toml` 新增  
  `[patch.crates-io] tao = { path = "vendor/tao" }`，把 `EventLoopRunnerShared`  
  从 `Rc` 换成 `Arc`（2 处改动，均带 `[easyop patch]` 标记）。  
  自动 trait 不变（`EventLoopRunner<T>` 内部是 `Cell`/`RefCell`，本来就 `!Send + !Sync`），  
  **语义零变化**，只把"并发 clone 会写坏计数"这件事消掉。
  > **为什么不在自己代码里绕**：2026-09-23 崩的是同一个病的**另一个出口**  
  > （`tauri::tray::TrayIcon` 里 tray-icon 的 `Rc`，当时改为全局托盘监听器绕开），  
  > 这次换成了 tao 这条链。全项目还有 **29 处 `AppHandle::clone`** +  
  > **34 处 `get_webview_window`**（后者内部也会 clone 一个 `AppHandle`），  
  > 且将来任何一处新的 `app.clone()` 写进异步任务都会复发 —— "堵出口"是打地鼠。  
  > 完整理由与"何时可以删掉这节"写在 `src-tauri/Cargo.toml` 的注释里。

### 新增

- **`diag_clone_storm`（调试自检命令）**（`src-tauri/src/diag_clone_storm.rs`）。  
  把"多线程同时 clone `AppHandle`"放大成**确定性**压测：N 个线程各做 `iters` 次  
  clone+drop，返回耗时。它是上面那条 tao 补丁的**回归测试** ——  
  **补丁在 ⇒ 16 线程 × 100 万次秒级跑完；补丁丢了 ⇒ 进程立刻 abort**。
  > 为什么必须有它：竞态问题**不能用"跑一遍没崩"来验证**。  
  > ⚠️ **不要在界面上给它接按钮** —— 它的设计目的就是"补丁失效时把进程打崩"。  
  > 只在验证补丁时通过 CDP 手动调用（命令写法见 forensics 文档 §7）。

---

## [v0.39.3] — 2026-09-30

### 新增

- **运行时「三态灯」（红绿灯）**（`agent_desk/state.rs` + `DeskView.tsx`）。  
  把运行时板块原先 7 个平铺计数器换成「一盏灯 + 一句结论 + 明细」。

  三档刻意是**三个不同的东西**，不是"忙 / 闲"两档：
  | 档位        | 灯色            | 含义                |
  | --------- | ------------- | ----------------- |
  | `idle`    | 绿             | 没在跑、也没排队          |
  | `running` | 橙（`--accent`） | 有任务正在执行           |
  | `backlog` | 黄             | **有积压，却没有任何任务在跑** |
  `Backlog` 就是原本"`busy == false` 而 `idle == false`"那个组合。它在平铺计数器里  
  表现为两个各自为真、加起来却自相矛盾的词（"第 5 颗灯：灭" + "空闲：否"），  
  用户得逐条读、还要自己把两条拼起来才知道该不该管。

  **`running` 刻意不用红色**：红灯在中文语境里读作"坏了"，而"有任务在跑"恰恰是  
  系统最正常的时刻 —— 给它点红灯，用户会去排查一个并不存在的问题。

  **判据在后端，前端只做颜色映射**（`LightState::from_queue`）。若让前端从  
  `busy` / `idle` / `total_pending` 自己推，同一套判据就有两份实现，后端改判据时  
  前端不会跟着动，界面会**静默地**显示错误档位 —— 本项目在"字幕浮层显隐"上吃过一次  
  完全一样的亏。三条单测钉住灯语（含"在跑但还有积压 ⇒ 仍算 running"这一边界）。


⚠️ `light` 是 `RuntimeSnapshot` 的**新字段**，且参与 `PartialEq` 比较 ——  
若漏进比较，灯会停在旧档位不动而其它字段看起来一切正常。已由  
`light_change_alone_triggers_an_event` 守住。

### 变更

- **调试相关收进单一模块「调试工具」**（`DeskView.tsx`）。原先标题叫  
  「手动派发（调试）」，**只写了派发、藏住了租约**；现在：
  - 改名为「调试工具」，徽标写明「手动派发 · 播放租约」，默认**收起**，  
    收起态给一行摘要（队列 / 未读 / 喇叭归属）。
  - 内部重组为两个明确子块：**① 手动派发** / **② 播放租约**。
  - **位置不动**。它现在是 Phase 3 接真按键之前唯一能手动压完整条链路的入口，  
    收起已解决"占版面"，挪到页尾只会让它更难找。
- **「取租约」从槽位卡移入「调试工具 → ② 播放租约」**。原因：一次播放演示原先  
  要跨两个板块找（取租约在槽位卡、回执 / 释放在下方的租约面板）。现在生命周期  
  在一处闭合：选槽 → 取租约 → 回执 / 释放。槽位卡只保留**只读状态**  
  （「播放中」「未读 N」徽标），不再放动作。

  「取租约」自带**独立槽位选择器**，与派发槽位、播报槽位三者互不绑定 ——  
  沿用本项目既有判据：不同操作没理由共用同一个选择器。  
  该槽未读为 0 时按钮直接置灰并说明原因（后端本来就会拒），而不是让人点了再吃报错。
- **修正一处旧文案的错误**：原「第 5 颗灯：亮 / 灭」与实际灯语不符 ——  
  `device_led.rs` 里第 5 颗是**正在执行的槽数 0–4 → 绿/黄/橙/紫/红五档**，不是一个开关。  
  已改为折叠说明里如实描述（第 1–4 颗 = 各槽未读亮度三档；第 5 颗 = 执行槽数五档）。
- **说明文字按 v0.39.1 的判据继续收敛**：运行时的两条"为什么"（持久化语义、  
  板端灯语布局）收进折叠；调试模块的导语重写为"它为什么不参与日常使用、价值在哪"。

### 修复

- 无功能缺陷修复。本次是展示层与文案的调整，**无对外契约变更、无配置格式变更**：
  - `light` 是**新增**字段，既有消费者（`busy` / `idle`）全部保留未动。
  - 播放租约的后端命令与语义完全未变，只是入口位置变了。

### 验证

- `cargo check --lib` → exit 0，**54 warnings（与改动前一致，零净增）**
- `cargo test --lib -- --test-threads=1` → **447 passed / 0 failed / 4 ignored**（原 444，净增 3）
- `npm run build`（tsc + vite）→ exit 0
- `node scripts/version.mjs check` → exit 0（五处 0.39.3 一致）

### ⚠️ 未完成 / 已知边界

- **没有在真窗口里目视确认过**：三态灯的呼吸动画、浅色主题下的对比度、  
  折叠按钮与徽标在窄窗口下会不会换行，都还没用眼睛看过。  
  按项目公约，本版只能算"仅编译通过 + 单测通过"。
- **测试跑在隔离 worktree 里**：主工作区当时被并行 agent 的未跟踪新文件  
  `src-tauri/src/live_translate.rs` 编译错误挡住（E0063）。没有去动它，  
  而是 `git worktree add --detach HEAD` 拉干净树、只搬本版改的 2 个 Rust 文件进去取证。
- **未读没有并进那盏灯，是刻意的**：未读是另一条轴（"有结果等你听"），  
  一个任务可以在跑、同时你还有 3 条没听。硬塞进一盏灯只会让两件事互相掩盖 ——  
  所以它单独上色（`未读 > 0` 时转黄），但仍然是独立一项。

---

## [v0.39.2] — 2026-09-30

### 移除

- **任务台移除「执行器」板块**（`src/components/DeskView.tsx`）。  
  该板块只做"**显示 + 检查**"两件事，而这两件在别处**各已有一份**，它是重复的**第三份**：
  1. **配置与「探测」在「模型配置 → ④ 执行器」** —— 调的是同一个 `desk_probe_driver`，  
     报的是同一份错误文本。`docs/任务台使用说明.md` §3.1 指定的排障路径本来就是它。
  2. **不可调度的原因在页首 `blockers` 里已按槽位逐条列出** ——  
     `build_snapshot` 对每个 `SlotBinding::Exact` 都做 `is_schedulable()` 判定并给出  
     `"槽位 S2 的执行器「X」配置有误：{errors}"`。**带槽位号，比原板块更明确**  
     （原板块只说"某执行器不可调度"，不说"哪个槽受影响"）。
  3. 真派发时 `resolve_driver` 还会再报一次（"槽位 S2 的执行器「X」不可调度：{why}"）。
  ⇒ 同一条判据维护两份，迟早漂移成"面板说可用、按下去报错"（`build_snapshot` 的注释  
  记着上一次正是这么踩的）。故删除。**⛔ 不要再加回来。**
- 后端 **`DeskSnapshot.drivers` 字段与 `DriverView` 结构体**一并删除；  
  前端 `src/types.ts` 的 `DriverView` 接口同步移除。  
  ⚠️ **`desk_probe_driver` 命令与 `DriverProbe` 保留** —— 它们属于模型配置页，勿一并删除。

### 变更

- `DriverRegistry::entries()` 降为 **`#[cfg(test)]`**：唯一调用方是被删的那个板块，  
  现仅剩一条单测用它验证"**不可用的执行器也要留在表里**"这个不变量。  
  不降级会多出一条 `never used` 警告。
- 删除 **`Confidence::label()`**：唯一调用方是 `DriverView`。任务历史存的是  
  `high`/`medium` **机器串**（供筛选），不是中文标签；前端「已校验 / 未校验」字样  
  是它自己的文案，不受影响。

### 验证

- `cargo check --lib` → exit 0，**54 warnings**（与改动前一致，**零净增**）
- `cargo test --lib -- --test-threads=1` → **444 passed / 0 failed / 4 ignored**
  - ⚠️ 在主工作区跑**跑不起来**：并行 agent 的未跟踪新文件 `src-tauri/src/live_translate.rs`  
    的测试代码有 `E0063`（`RecordedSentence` 缺 `start_ms`/`end_ms`），与本改动无关。
  - 故改用**隔离 worktree**（`git worktree add --detach <tmp> HEAD` + 只复制本改动涉及的  
    3 个 `agent_desk/**` 文件）取证，结果即上。
- `npm run build`（tsc + vite）→ exit 0，67 modules
- `npx tsc --noEmit` → exit 0
- `node scripts/version.mjs check` → exit 0（五处 0.39.2 一致）

### ⚠️ 未完成 / 待注意

- **未在 easyop 真窗口人眼复核**：任务台现为 5 个板块（槽位 / 运行时 / 手动派发 / 播报 / 任务历史），  
  删块后的**留白变化未经目视确认**。按 `AGENTS.md` 属"仅编译通过"。
- **一处信息缺口，判定为可接受**：**没有绑定到任何槽位**的执行器若不可调度，  
  不会出现在 `blockers` 里（那里只检查被槽位引用的）。但这类执行器**跑不了任何任务**，  
  没有实际后果。若将来要报，应在 `build_snapshot` 里补，**而不是恢复那个板块**。

---

## [v0.39.1] — 2026-09-30

### 新增

- **可复用的折叠（disclosure）原语 `src/components/Collapsible.tsx`。** 一个来源、三种形态，  
  避免每处各写一套：
  - `CollapsibleSection` —— 整块收起（任务台卡片）；
  - `CollapsibleNote` —— 收 § 的导语，触发器挂在 § 标题行右侧；
  - `CollapsibleHint` —— **就地**收字段内的长解释，触发器贴在它要收的那段话旁边  
    （设置页的板块标题与正文常隔十几个字段，把按钮挂到标题上会让人"点了之后内容在屏幕外展开"）。  
    三者共用 `useCollapsedState`，三条实现约束都有具体原因，写在文件头：
  - **默认收起只影响"第一次看到"**：用户手动切过之后按 localStorage 记住，  
    不会每次进来又被收回去（否则每改一项配置都要多点两次，是拿整洁换麻烦）。  
    localStorage 读写全部 `try/catch`，隐私模式下静默降级为"不持久化"，不会因为存不上把界面搞崩。
  - **触发器必须是真 `<button>`**（不是 `div + onClick`）：Tab 可达、Enter/Space 可用、  
    读屏能报展开状态；`aria-expanded` / `aria-controls` 齐全。内容体用 `hidden` 而**不是**  
    条件渲染 —— 条件渲染会让 `aria-controls` 指向一个不存在的元素。
  - **不嵌套 button**：HTML 不允许 button 嵌 button，WebView 会把嵌套拆开。  
    任务历史标题栏里**原本就有**一个「刷新」按钮，所以折叠按钮与它**并列**；点「刷新」不会切换折叠。

### 变更

- **任务台：「播报」与「任务历史」两个板块默认收起。** 这两块合计占了近半屏，而「播报」本身是  
  "锦上添花"（连后端注释都这么写）—— 日常根本用不到，却一直摊在最显眼处。
  - **收起 ≠ 藏起来**：标题旁的徽标照旧常显（「离线可用」/「8/10 已预合成」），  
    「任务历史」另给一行摘要（「最近 N 条」/「还没有记录」/「读取失败」），  
    所以"到底有没有东西"仍然一眼可见 —— 这是"摘要化"，不是"消失"。
  - 其余 4 个板块（槽位 / 队列 / 手动派发 / 执行器）**保持原样**，不连带改动。
- **模型配置页与设置页：长说明默认收起，表单字段与"填写说明"始终可见。**  
  分界判据是**这段字在回答"怎么填"还是"为什么这么设计"**：
  - **常显** —— 字段 label 与输入框；字段级提示（如"资源 ID 与音色必须同版本"，  
    不看到就会填错，它正是"必要的填写说明"）；每个 § 的一句功能摘要。
  - **收起** —— 讲原理 / 传输方式 / 采样率 / TUI raw mode / 历史沿革的长段落。
  - 落点：模型配置页为页首「这一页管什么」+ §1/§2/§3/§4 四节导语 + §3「为什么要有文本上限」；  
    设置页为 §通信配置的 Wi-Fi 自动重连机制与蓝牙历史沿革、§文本注入的「为什么需要这一项」、  
    §终端启动的导语。**就地折叠的按钮文案直接写成用户心里那句疑问**  
    （如"为什么这里没有开关和配对按钮"、"接好之后还要每次手动连吗"），而不是千篇一律的"更多"。
- **折叠 ≠ 删除：所有原文一字未改，只是移进展开区。** 仅三处把段落首句提为按钮文案  
  （如原「**为什么需要这一项**：…」的标题移到按钮上），正文无删减。
- 顺带把 API Key 之外的说明文字也统一了 caret 形态（`▸` 收起 / `▾` 展开），  
  与项目既有的 `desk-history-caret` 观感一致。

### 修复

- 无。本版为展示层调整，未修 bug。

### ⚠️ 未完成 / 已知限制

- **未在 easyop WebView2 真窗口里做人眼复核。** `npm run build`（含 `tsc`）exit 0，  
  但"收起后版式是否真的更清爽、caret 与 hover 反馈是否顺手"必须真窗口目视确认 ——  
  尤其：浅色主题下的对比度、窗口拉窄时标题行里的多个徽标会不会换行。  
  按 `AGENTS.md` §5，这属于"仅编译通过"，**未经真机验证**。
- 折叠状态存 localStorage，键名 `easyop.collapse.<id>`。**改 id 等于重置用户偏好**  
  （已在 `Collapsible.tsx` 注释里钉住）。暂无"一键恢复全部默认收起"的入口。
- 未加展开/收起过渡动画（瞬时切换）。若要加，只改 `.collapsible-*` 样式即可，不动逻辑。

---

## [v0.39.0] — 2026-09-28

### 新增

- **StyleCam 集中配置 v2（插件 0.4.0 → 0.5.0）。** 设置面板新增「图像参数 / API 配置」两分区，统一管理 provider、接口地址、模型、1K/2K/4K、宽高、输出格式、JPEG 质量、生成数量、HTTP 超时、重试和输出目录；新增 `config.get/set/validate/reset`、`provider.list`、`key.status/set`，清单扩为 21 条命令、6 条 tools。
- **provider 能力与安全执行策略。** EasySU 宽高按 3% 容差映射到五种比例，Ark 仅方形，OpenAI 兼容/智谱使用像素尺寸；动态计算 60–1260 秒的 `eo.run` 超时。仅对 408/425/429/500/502/503/504 和发送前连接失败重试，POST 发送后的不确定超时不重试，避免重复计费。
- **API Key 临时 secret 通道。** UI 密码框不预填；Key 经 nonce + `stylecam.secret` 交给 Python，消费后立即清空，并用临时文件 + `os.replace()` 原子更新 `.env`。Key 不进入 config、job、`eo.run` 参数、日志或命令信封，状态接口只返回掩码与长度。

### 变更

- **配置存储升级至 v2，向后兼容。** 旧 `size=1K/2K/4K` 自动迁移为 `resolution`，旧 `size=<宽>x<高>` 自动迁移为 `width/height`；新字段优先，迁移时清除历史 Key 字段。非敏感配置优先级为 CLI > job/单次覆盖 > `stylecam.config` > `tools/stylecam_io.env` > provider 默认值。
- 根 `plugins:test` 现在串行执行 studio harness、StyleCam harness、`clamp-extra.mjs` 和 Python unittest。StyleCam 当前验证数为 287 + 29 + 18，全部不联网。
- **StyleCam 工作台 UI 按 DeskMate 的空间方法重构。** 页面改为顶部素材架、中部创作机器、右侧强度控制、底部作品带，设置入口退居右上角；保留 10 种风格轨道、生成出片动效、创意显示、作品集和集中配置能力。900px 以下强度区下移，640px 以下单列，素材与作品轨道局部横滚。
- StyleCam README / DESIGN 已同步最终配置清单、范围、provider 限制、Key 安全、生效方式和工作台 UI 结构。
- **内容台（studio）「+公众号」重构：默认产出可复制粘贴的 HTML。** 原实现里 HTML 仅是登录态失效时的降级产物，登录态有效则走 `pub.py publish wechat --save-draft` 自动填草稿，与「复制粘贴到后台」的预期相反。现改为：默认把去AI味后的正文（+配图）按 wechat-writer-pro 的 `layout-guide.md` 规范排成全内联样式 HTML，写成本地投放包；自动填草稿降级为需显式 `autoDraft` 开关。排版规范运行时动态读取真实 skill 文件，读不到才用内置兜底提示词（已对齐 正文#333 / 系统字体 / 移动端字号，不再违反 layout-guide 铁律）；原硬编码提示词声称源自、但本机并不存在的 `wechat-typesetting-trae-main` skill 已废弃。

### 修复

- 补齐配置边界、provider capability matrix、重试状态码、连接失败重试、动态 timeout 夹取、3% 比例边界、Key 清理和设置面板 DOM/CSS 断言，防止集中配置回归。
- 内容台「+公众号」HTML 路径原先只排正文文本、丢弃 `figures` 配图数组，与 `compose` 草稿路径不一致；现 `cmdTypeset` 把 humanized + figures 一并送入排版，两份产物配图一致。

### ⚠️ 未完成 / 已知风险

- 已用 Chrome headless 直接加载 StyleCam 真实 `src/main.js` 生成浅色工作台截图，并测得 1180px 与 547px 面板宽度下均无整页横向溢出；尚未在 easyop WebView2 真窗口中做人工目视与滚动手感复核。
- 前端配置校验尚未完全对齐 Python 的 URL/长度与 provider capability matrix；UI provider 联动未完整禁用不支持的 `format/n`；“自检连接”按钮当前使用 `probe:false`。本版由 Python 执行前校验兜底。

---

## [v0.38.0] — 2026-09-25

### 新增

- **内容台旋钮导航（选中→确认两步模式）。** 编码器旋转在 5 个功能间切换焦点  
  （新建选题 / 一条龙 / 推进阶段 / 导出 / +公众号），按下执行高亮项。  
  3 个具名动作 `studio_prev` / `studio_next` / `studio_confirm`，对应保留 UUID  
  `…000e` / `…000f` / `…0010`。Rust 端 `STUDIO_FOCUS: AtomicU8` 维护焦点，  
  emit `studio-focus` / `studio-action` 事件 → 前端 `App.tsx` 监听 →  
  `invokePluginCommand` 调插件 `studio.quick_action` 命令路由。  
  **属对外契约变更（新增 UUID 映射），但向后兼容**（旧固件不触发这些 UUID 即不受影响）。
- **`eo.clipboard_read` 宿主 API。** 新增 Tauri 命令 `eo_clipboard_read`，  
  供插件读取系统剪贴板文本。起因：`eo.get_selection()` 模拟 Ctrl+C 时 easyop 已切到前台，  
  按键发给 easyop 而非原应用，取不到外部选区。`clipboard_read` 作为 fallback。  
  Rust 端 `clipboard_util::read_clipboard()` 公开函数，前端 `pluginHost.ts` 的 `eo` 对象挂载。
- **内容台自动生成情报卡 + 一条龙流水线。** `gen.intel` 命令用 LLM 从选题标题生成情报卡  
  （受众 / 平台 / 风格 / 关键信息点）；`gen.all` 命令顺序执行情报卡→标题→正文→口语化，  
  中途失败可断点续跑（跳过已落库步骤）。
- **正文字数可配置。** `BODY_SYSTEM` 从常量改为函数，默认 500 字，面板加字数输入框，  
  值存 `eo.storage` 的 `config.wordCount`。
- **内容台双主题支持（深色 / 浅色）。** CSS 改为插件私有变量驱动（30+ 变量），  
  `[data-theme="light"] .studio-panel` 覆盖浅色值。跟随 `<html data-theme>` 属性切换，  
  不硬编码颜色。深色：深蓝渐变 + 白色星点；浅色：淡蓝白渐变 + 淡蓝星点。
- **内容台 UI 优化。** 卡片网格布局（`.sp-grid` + `.sp-card`），星空/银河系主题，  
  帮助按钮（「?」）+ 帮助弹窗（使用说明）。
- **《内容台插件架构设计》文档**（`docs/内容台插件架构设计.md`）：6 章完整架构设计，  
  涵盖插件集成机制 / 按键映射 / 主题兼容 / 测试验证 / 使用说明 / 版本更新。
- **内置插件「风格调色盘」改 provider 注册表架构（插件版本 0.2.0 → 0.3.0）。**  
  `tools/stylecam_io.py` 引入声明式注册表 `PROVIDERS`，内置 `ark` / `openai_compat` /  
  `zhipu` 三家：端点、模型链、鉴权、图生图传法（`image` 数组 / 不支持）、  
  `size` 传等级还是像素、响应解析（`b64_json` / 下载 `url`）、探测策略全部进表，  
  **加新 provider 只加一项，主流程零改动**。  
  起因：easyop 配置里的 `llm_api_key` 实测是方舟 Agent Plan 网关 Key，只对 `/api/plan/`  
  有效且套餐白名单不含任何生图模型（4 个 Seedream 全部 `UnsupportedModel`），  
  生图必须换一把 Key；用户不想每次换 Key 都改代码。  
  **换 API 只改 `tools/stylecam_io.env` 四行**：`PROVIDER` / `IMAGE_API_KEY` /  
  `IMAGE_BASE_URL` / `IMAGE_MODEL`（优先级：命令行 > `.env` > easyop 的 `llm_api_key`，  
  后者仅 `PROVIDER=ark` 兜底）。  
  不支持图生图的 provider **不静默丢素材**：退化为纯文生图并 `MSG=` 明确告知，  
  脚本新增 `I2I=1/0` 让前端知道要不要提示用户；`doctor` 输出 `PROVIDER=` / `ENDPOINT=` /  
  `MODEL=` / `I2I=`，`--probe` 按该 provider 的策略探测，`ark` 直连 401 判 Key 种类的  
  `KEY_KIND=agent_plan` 逻辑保留。  
  **stdout 契约未变**（`OK=` / `CHUNK=i/n:` / `META=` 等语义原样，只加字段）。
- **（2026-09-28 补记）内置插件「风格调色盘」新增 `easysu` provider，图生图真机打通  
  （插件版本 0.3.0 → 0.4.0）。** `PROVIDERS["easysu"]`（EasySU 中转，  
  `https://aigc.easysu.cn/v1`）成为 `.env` 默认档：走 `size_mode=res_ratio`  
  （发 `resolution` + `ratio`，**不发 `size`**，默认 `1K` / `1:1`），探测用零成本的  
  `GET /v1/models`。起因仍是「easyop 的 `llm_api_key` 是方舟 Agent Plan 网关 Key、  
  白名单不含任何生图模型」那条环境限制 —— 所以**不再要求用户去换方舟原生 Key**，改走中转。
- **图生图（I2I）实测生效。** `/v1/images/edits` 吃 **JSON body**，`image` 是 base64  
  **字符串**（不是数组，裸 b64 与 `data:` 前缀都收），注册表新增 `i2i="edits_json"`；  
  此前 `ark` / `openai_compat` / `zhipu` 三档要么不支持 I2I、要么只能退化成纯文生图。  
  真机出图验证：构图 1:1 保留，图生图真实生效（不是退化）。实测体积：1K PNG 约  
  **931 KB**（base64 后约 1.24 MB），`CHUNK` 64 KB 分块可承载 —— 这也是默认取 `1K`  
  而非 `2K` / `4K` 的原因：再往上体积成倍增长，容易顶到脚本 `MAX_B64_BYTES = 12MB` 上限。
- **隐私告知文案动态化。** `STAGE_HINTS.privacy` 由写死的「上传到火山方舟」改为模板  
  「这张图会上传到`{provider}`用于生成……」，`{provider}` 用 `doctor` 回传的  
  `PROVIDER_NAME=` 填充（未自检时回落「当前生图服务」），换 provider 不再出现  
  「文案说的服务商」与「实际上传的服务商」不一致。该文案不在 `CODE` 表里，  
  因此 PRD 5.2 逐字比对不受影响，`test/harness.mjs` 未改，241 条断言仍全绿。


### 变更

- **内容台插件退出 ⌘SS 搜索，统一走侧边栏页签。** `plugin.json` 删除 `studio.toggle`  
  命令、清空 `contributes.tools`、清理 `activationEvents`。`index.js` 删除 overlay 相关  
  逻辑（`cmdToggle` / `mountPanel` / `destroyPanel`），统一为页签模式。  
  **影响**：用户打开内容台的入口从 ⌘SS 搜索改为侧边栏「内容台」页签，更直接。
- **`search_hits` 逻辑修复。** `plugin_registry.rs` 原逻辑"tools 为空 = 全部命令进搜索"，  
  清空 tools 反而让所有命令出现在 ⌘SS。改为"只有被 tools 显式引用的命令才进搜索"。
- **`PluginPageInfo` 序列化修复。** Rust 端 snake_case 前端 camelCase，缺  
  `#[serde(rename_all = "camelCase")]`，导致页签点击后 `pluginId=undefined`。已补上。
- 内容台插件版本 0.3.0 → 0.4.0，命令数 11 → 13（新增 `gen.intel` / `gen.all` /  
  `quick_action`），tools 清零，pages 保持 1。
- 验证台 harness.mjs 从 111 项扩充到 181 项，新增 `clipboard_read` stub、  
  `chatReplies` 队列、旋钮 quick_action 路由等断言。

### 修复

- **「新建选题」报"没有选中文本"**：根因是 `eo.get_selection()` 的 Ctrl+C 时序陷阱  
  （easyop 切前台后按键发给 easyop 而非原应用）。改为三段式 fallback：  
  输入框有值 → 直接用；`get_selection` 成功 → 用；否则 → `clipboard_read` fallback。  
  错误提示也改为引导用户先复制再点。

---

## [v0.37.1] — 2026-09-24

### 修复

- **开发板 Wi-Fi 心跳判据过严 ⇒ "Wi-Fi 不稳定、偶尔断开"（真机反馈）。**  
  心跳周期 2 s，而超时只有 5 s ⇒ 只容忍**连丢 1 个包**（连丢 2 个 = 6 s 即踩线）。  
  实测日志里出现过 **7748 ms** 的空档（连丢 3 个），一小时出现 9 次  
  "心跳中断 → 恢复"，且成对间隔只有 1～16 秒：
  ```
  15:56:55 [warn] 板子心跳中断（超过 5 秒未收到）
  15:56:56 [info] 收到板子心跳，链路在线     ← 只隔 1 秒
  ```
  **真正的伤害不是界面显示，是丢音**：`resolve_mic` 正是按这个判据选录音源的，  
  翻转的那一瞬间会把录音源退回**电脑麦克风** ⇒ 用户对着板子说话却录到静音，  
  提示只写"空结果"。三处改动：
  - `HEARTBEAT_TIMEOUT` 5 s → **15 s**（容忍连丢 6 个包）。上界同样受限：  
    它同时决定"对着板子说话却没反应"要多久才提示用户。
  - `now_ms()` 改用**单调时钟**（`Instant`）。判据问的是"过了多久"，与"现在几点"  
    无关；用墙钟时 NTP 校时会让差值跳变 —— 往前跳 = 凭空一次离线，  
    **往后跳 = `saturating_sub` 归零 ⇒ 真掉线也不报**（这一侧是静默失效，更危险）。  
    需要真实时刻的地方（历史落盘的 `last_success_unix`）仍用墙钟，两者不混用。
  - 抖动**不再计入连接成功**（`RESUMED_FLAP_MS = 30 s`）。原先每次"离线→在线"都  
    `mark_success`，而 `success_count` / `last_success_unix` 正是历史表的**排序依据**  
    ⇒ 抖得最凶的那张网反而被越推越高。判据抽成纯函数 `counts_as_success()`，  
    **日志与历史共用同一个调用**，不允许出现两份证据互相打架。
- **首次连接被误判成"抖动" ⇒ 第一次连上永远不写进历史。**  
  上面那条修复里，我用"离线时长为 0"表示"从未在线过"，但离线起表**刻意**在  
  "第一次评估到非在线状态"时就设上（否则会漏掉"开机时板子本来就没连上"，  
  T4 重下发永不触发）⇒ 应用启动时板子还没发包，第一个 tick 就把它设上了，  
  首包到达时它已是"启动到首包的时长"而非 0。真机日志：
  ```
  16:27:25 keyboard_audio: 收到首次心跳
  16:27:26 Wi-Fi 链路：心跳恢复（离线 1.0 秒，判为短暂抖动，不计入成功次数）
  ```
  判据改为显式传"曾经在线过"：`counts_as_success(offline_ms_before, ever_online_before)`。  
  **"从未在线过"与"刚离线了 0 毫秒"是两件事，不能用一个数值兼任。**
- **板子比上位机快时，成功记录永远写不进去。** "确认 + 写历史"原先在**选网之前**  
  执行，而目标名要靠 `target_ssid` 兜底 —— 那个字段由**下发分支**写入 ⇒ 心跳在启动  
  延迟（3 s）走完之前就到达时目标恒为 `None`：
  ```
  16:33:35 keyboard_audio: 收到首次心跳
  16:33:36 Wi-Fi 链路：收到板子心跳，链路在线
  （没有"已确认连接" ⇒ 历史没记）
  ```
  板子常开就意味着历史**永远积累不起来**，而历史表正是自动选网的依据。  
  ⇒ 该段挪到选网**之后**，用本轮选网结果兜底（`candidate.ssid`）。
- **任务台「可接任务」卡片显示错误（且是个从没被发现的字段误用）。**  
  卡片读的是 `cfg.tts.model`，而 `model` 是**百炼专用**字段 —— 豆包 provider 下  
  它被忽略、界面上也根本不显示。于是界面里明明填着 `seed-tts-2.0`，  
  卡片却一直显示陈旧的 `seed-tts-1.0`。根因是"哪个字段生效"这件事没有单一出处，  
  修法是新增 `TtsConfig::active_model()` 按 provider 分流，三处展示点  
  （任务台卡片 / 启动日志 / 快照）统一用它。  
  ⛔ 判据**不能**用"哪个字段非空取哪个"兜底：切换过 provider 的配置里两个字段  
  都非空，猜错就把不生效的那个显示出来 —— 那正是这次要修的毛病。

### 变更

- 「可接任务」里的 TTS 行多一层判断：API Key 已填但**模型标识为空**时给出明确提示  
  （原来会把空字符串渲染成一个绿色的"好消息"，而它同样发不出请求）。
- 日志/界面文案里的"超过 5 秒"改为引用 `heartbeat_timeout_secs()` ——  
  不再手写数字，避免再次出现"改了阈值忘了改文案"（排查的人会照着错的前提去查）。

### 验证

- `cargo test --lib -- --test-threads=1`：**443 passed / 0 failed / 4 ignored**（净增 6 个）
- `npm run build`：exit 0；`version:check`：exit 0（五处 0.37.1 一致）
- 真机（任务台显示）：`desk_snapshot.tts_model` 由 `seed-tts-1.0` → **`seed-tts-2.0`**
- 真机（重启一轮，开发板在线）：`success_count` 17 → 18，且日志走的是修正后的路径：
  ```
  [info] keyboard_audio: 收到首次心跳 addr=10.x.y.z:61601
  [info] Wi-Fi 链路：收到板子心跳，链路在线            ← 不再误判成"抖动"
  [info] Wi-Fi 链路：已确认连接（SSID=vin**st，心跳到达）  ← 历史写进去了
  ```
- 超时文案已随常量更新：真断线时日志为「板子心跳中断（超过 **15** 秒未收到）」，  
  并在离线 ≥ 20 s 后触发 T4 重下发。真机观察到一次完整闭环：  
  16:29:41 中断 → 16:30:03 下发（`通道=usb-hid，板子已存 NVS=true`）→ 16:30:04 心跳回来。
- ⚠️ **"抖动不计成功"未跑出完整闭环**：它需要板子真离线 ≥ 45 秒再回来，  
  而当天板子一直稳定在线。判据已由单测 `flapping_does_not_count_as_a_success` 钉住，  
  且"抖动"这条日志路径**已被真实触发过一次**（见上面 16:27:26 那条）。  
  下次板子在场且方便断电时可复验。
- ⚠️ 注入脚本 `.workbuddy/inject_heartbeat.py` 首次使用**未达成目的**，原因是前提判断错了：  
  当时把"开发板 USB 不在位"当成"没有心跳源"，实际上板子 Wi-Fi 那头一直在发心跳 ——  
  脚本注入的包被真流量淹没（两条路刷的是同一个时间戳，连"离线"都造不出来）。  
  适用前提已写进该脚本文件头。

---

## [v0.37.0] — 2026-09-24

### 新增

- **Wi-Fi 链路四环节自动化（自动检测 / 自动连接 / 历史记录 / 快速重连）。**  
  新增独立模块 `src-tauri/src/wifi_link.rs`（约 1500 行 + 23 个单测）与设计文档  
  `docs/wi-fi链路自动连接设计.md`。四件事的判据与触发条件：
  | 环节                                       | 做什么                                                                                                  | 触发条件                                                           |
  | ---------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
  | 自动检测                                     | 读「当前所连 SSID + 本机首选 IPv4 + 可见 AP 列表」，组成环境指纹 `epoch = 当前SSID\|首选IPv4`                                  | 每秒 tick；在线时 30 s 重探一次，离线时 5 s；USB 插入立即重探                       |
  | 自动连接                                     | 把**带 Wi-Fi 端点的完整配置载荷**写进板子 NVS（先 USB HID，后 UDP 直投）                                                   | T1 开机首次（3 s 启动宽限后且板子未在线）；T2 检测到 USB 配置通道出现；T4 离线超过 20 s 仍未收到心跳 |
  | 历史记录                                     | `output_dir()/wifi_known.json`，最多 16 条，按成功证据排序；启动时从 `config.json` 的 `wifi_ssid`/`wifi_password` 迁移一次 | 只在「离线 → 在线」跃迁时记一次成功（以**收到心跳**为准，不以"下发成功"为准）                    |
  | 快速重连                                     | 端点已在 NVS ⇒ 板子上电**自行**连接（固件本职）；上位机侧只做兜底                                                               | T3 环境指纹变化（换网 / 本机 IP 变了）⇒ 立即重下发；重试退避 0 / 5 / 15 / 30 / 60 秒    |
  | 「连上了」的唯一判据是收到板子的 EIHB 心跳（超时 5 秒），不是下发回执。 |                                                                                                      |                                                                |
  | 板子的自我重连约每 10 秒一轮；上位机只在环境变化或长期离线时介入。      |                                                                                                      |                                                                |
- 新增命令 `wifi_link_status`（前端 2 秒轮询）。
- 新增 `keyboard_audio::send_config_json`（UDP 直投配置，上限 2048 字节，与固件 `kConfigMaxJsonLen` 对齐）。

### 变更

- **「Wi-Fi 已连接」徽标不再是前端硬写的。** 以前点一下「连接」就  
  `setWifiConnected(true)`，与板子实际状态无关，于是「点了成功但麦克风不出声」可以一直挂着；  
  现在徽标只显示后端的 `board_online`（= 收到板子心跳）。
- **`payload::build_firmware_payload(profile)` 已删除**，改为  
  `build_firmware_payload_with_wifi(profile, Option<WifiEndpoint>)` —— 强制每个调用方  
  显式表态「这一次要不要带 Wi-Fi 端点」，从类型上消灭"忘了带端点 ⇒ 板子被清网"这一整类问题。
- `wifi_connect` 返回值改为 `{ssid, channel, saved_to_board, lan_ipv4, hint}`。  
  设置页「Wi-Fi 配置」新增「刷新状态」按钮与链路状态明细（链路状态 / 上位机所处网络 /  
  本轮目标 / 最近失败 / 历史网络按成功证据排序）。
- 配置 JSON 下发成功不再被表述为"已连接"——下发成功只说明端点进了 NVS。

### 修复

- **`wifi_connect` / `wifi_disconnect` 从来没有生效过。**  
  旧实现下发的是 `{"cmd":"wifi_connect",…}`，而开发板固件的配置协议**没有 `cmd` 字段**  
  且要求 `schema` 必填 ⇒ 整包被判 `InvalidSchema` 静默丢弃；  
  且旧实现不等回执就写日志"已下发"，所以故障对用户表现为"点了没反应、日志还说成功"。  
  现在改为下发完整配置载荷并等待 USB 回执。
- **每次「下发到开发板」或「切换方案」都会清空板子的 Wi-Fi 端点。**  
  旧载荷只含 `schema` / `target_platform` / `profiles`，而固件  
  `ConfigState::apply_json` 是**整体替换**语义（未提及的字段一律回默认值），  
  于是 `wifi_ssid` / `audio_host` 被清成空串 ⇒ 固件的  
  `audio_ready() = enabled && !ssid.empty() && !host.empty() && port != 0` 为假  
  ⇒ 板子主动释放 Wi-Fi，麦克风静默失效。  
  已在 `push_config` 与 `host_action::handle_profile_switch` 改为带端点下发，  
  并在两处加注释钉住"别改回不带端点"。
- **版本脚本漏掉了第 5 处版本号：`src-tauri/Cargo.lock`。**  
  `scripts/version.mjs` 原先只校验/同步四处（package.json / tauri.conf.json / Cargo.toml /  
  package-lock.json），而 `Cargo.lock` 里本包那条 `[[package]]` 也带版本号，  
  且是**唯一会被 `cargo build` 自动改写**的一处 —— 升级后不构建就停在旧号，  
  下次构建又被"改成"新号，于是版本文件在"一致 / 又不一致"之间来回摆，  
  先后取决于有没有跑过构建。现已纳入 `check` 与 `sync`（只认 name 为 `easyop` 的那一条，  
  不碰依赖版本），校验口径从"四处"改为"**五处**"。

### ⚠️ 未完成 / 已知边界

- **首次配网必须插 USB**：端点还没进 NVS 时板子不在网上，UDP 直投这条路  
  （其服务端判据是 `from == dest`）必然打不通。
- 板子端**没有**主动扫描并上报周围 AP 的能力，环境判断全部在上位机侧完成  
  （没有为此改固件，也没有新增 Report ID）。
- 麦克风来源不是「Wi-Fi 键盘麦克风」时，整条链路**主动停摆**（状态 `disabled`）——  
  这是故意的：用户选了别的来源，后台不该还在改板子的网络配置。
- 历史文件 `wifi_known.json` 里**密码是明文**（与既有的 `config.json` 同级别，未新增暴露面，  
  但也未加密）。迁移是"只增不清"：`config.json` 里的旧字段保留，表单预填仍可用。
- 「零下发、直接命中已在线心跳」的那一轮（例如重启上位机、板子本来就连着），  
  `last_channel` 会是空 —— 语义正确（本轮没走任何通道），但 UI 上略显空。
- 既有未完成项照旧：`AsyncTaskPoll`、运行中逐行实时日志、失败自动重试、  
  键盘喇叭出声、板端灯语 `0x16`。

---

## [v0.36.0] — 2026-09-24

### 变更

- **内容台（studio）插件退出 ⌘SS 搜索，统一走侧边栏页签。**  
  移除 `studio.toggle` 命令和 overlay 浮层模式，清空 `contributes.tools`。  
  插件不再出现在 ⌘SS 搜索结果里。⌘SS 命令面板回归搜索/启动文件本职。  
  `contributes.tools` 机制本身保留，其他插件仍可按需声明。  
  影响：老用户以前能 ⌘SS 搜「内容台」打开，现在需改走侧边栏点「内容台」页签。  
  文档同步更新：AGENTS.md 冒烟路径、`路径A-插件页签接口契约.md`、`内容台规范.md`、  
  `easyop 插件系统设计文档.md`、`plugins/README.md`、`studio/README.md`。  
  验证台 145 项全过（原 156 项中 11 项 toggle/overlay 断言已移除/替换）。

### 修复

- （无）

---

## [v0.35.4] — 2026-09-23

> ⚠️ **本节为回顾补记**：原条目在 2026-09-24 13:24 与 v0.35.3 一起被**一次外部操作**  
> 从工作树清掉（文件顶部回退到 v0.35.2），按本次改动的实际内容、commit 记录与  
> `.workbuddy/memory/2026-09-23.md` 重建，**不是逐字还原**。

### 新增

- **豆包 TTS 支持「WebSocket 双向流式 V3」端点。**  
  新增 `tts.rs::synthesize_volc_bidirection_ws`，按官方双向流式的上行帧形状  
  `[头 4B][event 4B][sid_len 4B][sid][size 4B][payload]`（头 flags `0b0100`）  
  与握手序列 `1→50→100→150→200→35x→102→152→2→52` 实现合成。  
  `TtsConfig::is_bidirection()` 的判据 = 去掉 `?`/`#`、去尾 `/` 后**最后一个路径段  
  严格等于 `"bidirection"`**（`bidirection-proxy`、`bidirection/stream2` **不算**）。  
  **刻意不提供"双向端点"常量** —— 一个带错误前缀的默认值比没有更糟。
- `scripts/` 新增三个零依赖协议探针：`probe_tts_bidirection.py`（权威探针，  
  `--repro-old` 可复现"0 帧"）、`_probe_bidir_variants.py`（四种帧形状对照）、  
  `_capture_bidir_frames.py`（抓真实 hex 供单测）。

### 变更

- `ServerFrame` 新增 `session_id: Option<String>`（连接类事件存 `connection_id`、  
  会话类存 `session_id`，两者布局相同）；`parse_server_frame` 由"只跳过 id 段"改为**捕获**。  
  **单向路径行为不变**（实测字节级回归一致）。
- 新增 `(Volc, Http) if is_bidirection()` 分支 → 返回 `InvalidInput`，明确提示该端点需 `wss://`。
- 抽公共 `volc_ws_connect` / `bidir_send` / `bidir_recv` / `bidir_expect`，  
  单向 `synthesize_volc_ws` 改为复用（其内联连接块删除）。

### 修复

- **修「双向流式端点跑不起来」——现场表现为 `等待音频超时(10 秒无新帧，已收 0 帧)`。**  
  真根因是**同一个服务、上行帧形状有两套**：单向  
  `[头][sequence 4B][size 4B][payload]`（flags `0b0001`）vs 双向  
  `[头][event 4B][sid_len 4B][sid][size 4B][payload]`（flags `0b0100`）。  
  **把单向帧发给双向端点，服务器不报错、只是静默等待** ⇒ 客户端收 0 帧。  
  ⚠️ **这个错误形状没有任何错误码**，靠读文档看不出来，只能实测。  
  **判据记牢：「0 帧」先怀疑帧形状，别先怀疑 key / 网络 / 端点。**
- **会话类事件必须带非空 `session_id`**：`sid_len == 0` → 服务器回  
  `session ID length is zero`；填了 sid 但帧里没写出 sid 段 →  
  `parse payload size failed: body too short`。⇒ 已把这条规则**编码进类型**：  
  `FrameScope::Session(&str)` 传空串直接返回 `Err`，想绕开得先改类型（这是故意的）。

### 实测钉死的事实（别再重测）

- 连接类帧 `StartConnection` 真身 14 字节：`11 14 10 00 00 00 00 01 00 00 00 02 7b 7d`（有单测锁死）。
- **只有 `152`（SessionFinished）是终止符**：`351`（TTSSentenceEnd）在 TTS1.0 是**逐句**触发，  
  拿它当终止会早停。
- **文本只放 `TaskRequest`**：放 `StartSession` 也能通（实测 3.58 s vs 3.81 s，无双份合成），  
  但按文档语义更干净。
- `FinishConnection` 失败**忽略**（音频已到手，不因此判失败）。
- 单次成功合成样本：16 帧 / `48000 Hz` / 171778 samples / 3578 ms 音频 / 墙钟 1513 ms。
- `/plan/` 网关前缀在本账号的 key 上是**硬要求**，官方裸路径返回 **401 `Invalid X-Api-Key`**。


### 验证

- `cargo test --lib -- --test-threads=1` → **418 passed / 0 failed / 4 ignored**。  
  `tts::tests` 覆盖：端点判定、连接帧字节表、会话帧带 sid、真实 hex 回包解析、  
  协议错误不误判为鉴权失败、`https` 端点被拒且有方向提示。
- 两个 `#[ignore]` 实时用例手动跑通：双向合成（`logid=20260923180004E5669E24BDE7F50799B9`）、  
  单向回归（字节级不变）。
- `cargo check` → exit 0（52 个既有 warning）；`npm run build` → exit 0；`npm run version:check` → exit 0。

### ⚠️ 未完成

- **`cargo build` 链接期 `LNK1104`**：`easyop.exe` 被运行中的 App 实例占用，需关掉 App  
  才能完整链接；`cargo check` 与全部测试不受影响。
- **ASR 只做结构审查、未改一行代码**。唯一发现的潜在隐患：`asr.rs` 700/729/778/809 行  
  `final_text = text.clone();` **无条件覆盖**，而空文本恰是正常 `last=true` 帧带回的  
  （`emit_partial` 已对空文本有守卫，两处不对称）。实测 1094 次返回**0 次**触发 ⇒  
  属潜在地雷而非现症。修法：`if !text.trim().is_empty() { final_text = text; }`。  
  另两个候选（`result_type=chunk` 降延迟、跨次复用 WS 连接省约 231 ms）**均未验证、均未动**。
- 双向流式已端到端跑通，但**未做长时间稳定性压测**。

---

## [v0.35.3] — 2026-09-23

> ⚠️ **本节为回顾补记**：原条目在 2026-09-24 13:24 被同一次外部操作清掉，  
> 按 `.workbuddy/memory/2026-09-23.md` 的工作记录重建，**不是逐字还原**。

### 变更

- **字幕浮层的显隐决策权整体收归后端（前端 0 处控制）。**  
  `VoicePanel.tsx` 里那段"跟随录音状态"的浮层显隐效果，连同 `overlayEnabled` 状态  
  与 `get_overlay_enabled` 回填一并删除；`lib.rs::set_overlay_enabled` 接管  
  "开关被关掉"这个只在后端可见的时机（关 → `hide()`；开 → 若 `is_listening()` 补一次 `show()`，  
  后端只在"录音开始"那一刻读过开关，不补这手用户要等下一轮才看得见）；  
  `session.rs::show_overlay` 提升为 `pub(crate)`，让上一步复用**同一个**定位逻辑。

### 修复

- **修「任务台执行/说话时小浮窗出来了、但没有文字」**（钦友报，现象精确到"有窗无字"）。  
  结论：**后端一直是对的，字也一直上去了 —— 是被前端收掉的。**  
  `asr-partial` / `asr-final` 正常送达、DOM 文本正确，**但同一时刻浮层窗口  
  `isVisible()` 是 `false`** —— 前端用**一个** `status`（由 `asr-state` 与 `edit-state`  
  **两个通道轮流写入**）的**子串**推导"是不是还在录"，而 v0.35.2 新增的状态行  
  「识别完成，正在派给 S{slot}」含 `识别完成` ⇒ 命中编辑态判定 ⇒ 前端立刻  
  `hide_overlay()`，**正文只亮约 200 ms**。  
  ⇒ 即 v0.35.2 那版"字幕已修好"并未真正生效。
- **判别实验（很省事，记下来）**：正文不变、只改收尾状态行措辞 ——  
  「识别完成，正在派给 S1」→ 被收掉；「正在听，请说话…」→ 保持可见。  
  可见性随**文案**变化 ⇒ 凶手就是"由文案驱动的显隐逻辑"。
- 这个坑早有化石：`session.rs` 通话链路里那条「通话中：已派给 S{slot}」是**故意多补的**，  
  为的是把状态拉回通话态、"骗过"前端那套推断。  
  ⇒ **看到"为骗过某判断而补文案"，该拆的是那个判断。**

### ⚠️ 踩坑记录（第一版修法是错的，只删了一半）

第一版只删 `hide_overlay`、保留 `show_overlay`，**看起来通过了第一轮验证**  
（任务台序列里正文可见 3.2 s）。但真机跑一轮**空录音**暴露了它：后端 `hide` 之后  
浮层**仍可见 4.7 s**。原因是同文件里 `status → phase` 那条 `useEffect` 声明在显隐效果  
**之后**，status 变化那一帧显隐效果带**上一轮的 `phase`（listening）** 先跑一次，  
于是在后端 hide 之后又把浮层叫回来。  
**⇒ 互补动作（show/hide）只删一条之前，必须先查时序/声明顺序；改完两个方向都要验。**

### 验证（全部实测，非推断）

| 用例                                                     | 结果                                                    |
| ------------------------------------------------------ | ----------------------------------------------------- |
| A 任务台派发序列（`show→asr-final→识别完成，正在派给 S1→desk-slot-key`） | 正文可见 **3.2 s**，唯一一次 hide 是脚本显式调的 ✓                    |
| B 真机跑一轮空录音（`start_recording` ×2，静音）                    | 后端 show → 后端 hide，**没有再自己回来** ✓（修复前此例挂 4.7 s）         |
| C 开关五连：显示→关→录音中(关)→录音中途开→本轮结束                          | `true,false,false,true,false` **全对** ✓；收尾已把开关恢复为**开** |

`npm run version:check` → exit 0；`tsc --noEmit` → 0 错误；`cargo build` → 成功（仅既有 warning）。

### 新增（开发侧观测工具）

- `.workbuddy/` 下新增零依赖 CDP 观测链：`cdp_eval.mjs`（`list` / `eval` / `evalfile`，  
  `--url` 支持**精确 basename** 与 `~正则`）、`cdp_inject.js`（注入事件 + 可见性记录器），  
  以及 `emit_desk_seq.js` / `v1v3_test.js` / `ab_test.js` / `real_round.js` / `switch_test.js`。
- ⚠️ **工具自身三个静默坑**（已记入 `silent-failure-forensics` 与 `windows-dpi-window-capture`  
  两个 skill）：目标选择用 `includes` 会误命中 `edit_overlay.html`；  
  `__TAURI_INTERNALS__.invoke` 是 non-writable，挂钩**静默失败**；  
  页面 HMR/重载会**清掉注入的记录器**。

---

## [v0.35.2] — 2026-09-23

### 变更

- **任务台语音派发后，字幕浮层多留 1.8 秒再收起**（`session.rs::DESK_SUBTITLE_LINGER`）。  
  理由：注入路径的结果紧接着就出现在输入框里，浮层马上收掉不影响用户"看见自己说了什么"；  
  而任务台的结果进的是后台队列，**屏幕上不留任何痕迹** —— 浮层是唯一能看见它的地方，  
  立刻收掉等于没显示。停留期间若用户又开始新一轮录音，则**不再收**（浮层已归新一轮接管）。
- 浮层收尾权改为按路径分流：`deliver_hold_result` 的返回值语义化 ——  
  任务台路径返回 `true`（本函数已在延时后自己收尾），注入路径返回 `false`（维持原样，  
  由调用方兜底）。调用方据此决定是否再调一次 `hide_overlay`。

### 修复

- **任务台语音派发时，字幕一个字都不显示**（钦友反馈："字幕没显示，但语音肯定识别了，  
  因为我在日志中看到识别后的文本，请修复 S1 字幕的显示"）。两个原因叠在一起：
  1. **任务台路径收尾时只发 `asr-state`（状态行）、从不发 `asr-final`（识别正文）** ——  
     而注入路径的第一件事就是 `emit("asr-final", …)`；
  2. 本机 ASR 走 `.../plan/sauc/bigmodel_**nostream**` 端点，**识别过程中服务端回的每一帧  
     文本都是空的**（实测 `#1`~`#22` 全部 `文本=""`），真正的内容只在收尾那一下给出来。  
     于是"工作期间本来就是空的" + "收尾又只发状态" ⇒ 屏幕上一片空白，  
     而后端日志里「语音：识别成功 N 字」一切正常 —— **成功却静默**，比报错更难查。  
     修法：新增纯函数 `desk_subtitle_events(slot, text)`（**先发正文、后发状态行**，  
     顺序反了浮层会停在"识别完成"上、看不出东西去了哪儿），  
     Hold 与 Call（连续通话逐句派发）两条路都接上。
- **空识别帧不再上屏**（`asr.rs::emit_partial`）。过去 `asr-partial` 会把空字符串也转发，  
  下游收到就 `setText("")` —— 字幕浮层、语音面板、编辑浮层会被这些空包**一遍遍擦掉**  
  已有内容。判断收到一处：`run_call` 当初自己加了非空判断、`run_recognition` 漏了，  
  这正是这两条路表现不一致的来源。浮层侧（`overlay.html`）另加一道防御：空 payload 直接返回。

### 验证

- 单元测试 **411 passed / 0 failed**（新增 `session::tests::desk_dispatch_puts_the_text_on_the_subtitle`）。
- **真机契约验证**（CDP 直读浮层 DOM，不靠截图猜）：
  - 连发 3 个空/空白 `asr-partial` ⇒ 浮层正文与占位态**均未被擦除**；
  - 复现修复前行为（开始录音 → 只发 `asr-state`）：正文 **空**、状态行有字 ⇒ **用户现象精确复现**；
  - 修复后的真实事件序列（`asr-final` + `asr-state`）：正文显示"把今天的日志整理一下"，状态行"识别完成，正在派给 S1"；
  - 再补一个空 `asr-partial` ⇒ **正文仍保留**。
- 浮层窗口显示链路单独验过：`show_overlay` → `isVisible()=true`；`hide_overlay` → `false`。

---

> ⚠️ **以下至 v0.33.0 的条目为「回顾补记」**
>
> 2026-09-23 14:57，`CHANGELOG.md` 与 `README.md` 被一次外部修改同时改写，  
> 版本历史顶部只剩 `v0.32.0` —— **v0.33.0 ~ v0.35.1 的条目全部丢失**  
> （而 `package.json` 当时已经是 0.35.1，所以 `npm run version:check` 长期处于失败态）。  
> 以下内容按 `.workbuddy/memory/2026-09-22.md` 与 `2026-09-23.md` 的实际交付记录重建，  
> **不是逐字还原**。若与当时的实际改动有出入，以代码与 memory 为准。

---

## [v0.35.1] — 2026-09-23

> **回顾补记**（说明见上）。

### 变更

- 任务台运行中的任务卡上直接加 `×`：按下即变「取消中…」并 disabled，  
  该槽 `running` 变空后自动撤掉标记。此前取消按钮只在下方操作区，且点下去没有任何即时反馈。

### 修复

- **「取消执行」按下去毫无反应，只能等 CLI 自己跑完**（钦友反馈）。  
  取消的信号链路本身是完整的（`desk_cancel_running` → token → 150ms 轮询 → kill → 终态映射），  
  病在**杀掉之后收尾等不到头**：
  - `Child::kill()` 只杀**直接子进程**。Windows 上 `codex.cmd` 的真实结构是  
    `cmd.exe`（Rust 标准库对 `.cmd`/`.bat` 自动包的一层）→ `node.exe`；
  - 杀了 cmd，`node` 还活着，而且**继承着 stdout 管道的写端** ⇒ 管道永不 EOF  
    ⇒ `runner.rs` 的收尾循环 `while let Some(ev) = line_rx.recv().await` **永远返回不了**。
  - 实测复现：`cmd /c start /b ping -n 12 127.0.0.1` —— 父进程 `exit=0` 之后，  
    `run()` 仍被拖住 **11.14 秒**，正好等于孙进程的寿命。  
    修法：新增 `terminate_tree()`（`taskkill /T /F /PID`，**必须在 `child.kill()` 之前**调用，  
    否则 `ParentProcessId` 链已断）+ 收尾循环加**硬超时**（正常 2s / 被终止后 300ms）。  
    真机实测：从取消到进程树全部消失 **510ms**（修复前要等到 CLI 自己跑完）。
- **豆包 API 执行器的取消被完全忽略**：`api_driver.rs` 的取消参数写作 `_cancel`，从未被读取。  
  改用 `tokio::select!` 让请求 future 与取消信号竞速，取消分支赢 ⇒ future 被 drop ⇒  
  reqwest 关连接 ⇒ **请求真的中断**。刻意不给 `llm_client` 加取消参数（会连带改动语音编辑链路）。
- 顺带修 `windows-dpi-window-capture` skill 里 `cdp_eval.mjs` 的 30 秒黑洞：  
  `rpc()` 的 `setTimeout` 没有 `clearTimeout`，promise 早已 resolve 但 node 还挂着 timer  
  满 30 秒才退出，把真实耗时完全盖住。

---

## [v0.35.0] — 2026-09-23

> **回顾补记**（说明见上）。

### 新增

- **任务台「后台执行 + 完成后播报」**：按下槽位键 → 执行器后台跑 → 完成后语音播报 →  
  有产物则自动打开文件/文件夹。按键（S1–S4）与页面按钮**走同一条路**  
  （都进 `agent_desk::dispatch_task` → 队列 → `pump_slot`），  
  队列 / 同槽串行 / 四档完成判定 / 世界门禁 / 未读 / 播报 / 会话续接全部共用。
- `agent_desk/history.rs`：任务历史落盘（`index.jsonl` + 每条一个 `.log`，留 200 条，  
  裁剪时连日志一起删）。新增命令 `desk_task_history` / `desk_task_log`；  
  任务台页面加历史面板，用 10 秒低频轮询取数 —— 后端是"先发播报事件、后写历史"，  
  在事件回调里立刻拉会稳定漏掉刚跑完那一条。
- `agent_desk/artifact.rs`：产物发现 = **时间窗扫描（mtime ≥ 任务开始时刻）+ 可选 glob**；  
  新增执行器字段 `artifact_glob`（默认空）/ `artifact_open`（**默认开**）。

### 变更

- **删除「出口 A」**（按键把命令行投进用户正在用的终端）。它唯一的卖点是"过程可见"，  
  与任务台"说完就走、结果回来找你"的价值正好相反，代价却是绕开整套机制 ——  
  于是"按键盘"与"点按钮"能力不同，而用户以为它们一致。  
  同步删除 `slot_command_line` / `deliver_command_line` / `desk_deliver_foreground` /  
  `text_delivery::deliver_to_window*` 等。  
  ⚠️ `agent_desk/delivery.rs` **没有删**：它被收窄成"等外部终端到前台"，`cli_terminal.rs` 仍在用。
- 「是否自动回车」（`submit_after_paste`）现在只影响「启动终端」那条路。
- `SummaryConfig` 从**死配置**（零读取点）真正接线：播报正文支持  
  `Corpus`（固定语料）/ `Text`（现成文本）/ `Summarize`（后台跑一次总结执行器再念）。

### 修复

- **产物 glob 拿绝对路径比对** ⇒ `*.md` 一条也匹配不上，且完全静默 ——  
  改为用"相对工作目录的路径 + 文件名"比对。
- **播报「0 样本」被误判成"设备没出声"**：同一刻触发的两条播报，旧的那条会被  
  `take_channel` 调 `stop()` 抢占。改为按 `PlaybackControl::stop_requested()` 分流 ——  
  为真 = 被抢占（正常），为假才是设备没出声。修之前每次派发都产出一条假警报。
- **产物排序不可复现**：Windows 文件时间戳粒度约 1ms，同一毫秒写的两个文件  
  mtime **完全相同**，"谁更新"在文件系统层面就无解。改为 `mtime 降序 + 路径升序`兜底 ——  
  不承诺更准，但**可复现**。

---

## [v0.34.1] — 2026-09-22

> **回顾补记**（说明见上；以下仅为要点）。

- **新增 `docs/任务台使用说明.md`**（纯文档 = patch）。把"任务台怎么用、哪些能力还没落地"  
  写成面向使用者的说明，并把「⚠️ 未完成 + 卡在哪 + 下一步」逐条显式标注，  
  避免下一个 agent 误以为能用。

---

## [v0.34.0] — 2026-09-22

> **回顾补记**（说明见上；以下仅为要点）。

### 变更

- **执行器配置合并进统一的「模型配置」入口**：不再有独立的执行器页签，  
  任务台执行器与语音编辑 LLM 共用同一处配置界面。

---

## [v0.33.1] — 2026-09-22

> **回顾补记**（说明见上；以下仅为要点）。

### 修复

- **`target/` 目录 6 天积到 54 GB**：每轮 `cargo build` 都会新增 `.rcgu.o` 与旧版 `deps`  
  产物，而 Cargo 不做回收。给出定向回收办法（`cargo clean -p <包名>` 只重编本包、秒级回收），  
  README 补「磁盘占用与定期回收」，AGENTS.md §6/§8 同步。

---

## [v0.33.0] — 2026-09-22

> **回顾补记**（说明见上；以下仅为要点）。

### 新增

- **独立的「API 配置」页签**：把密钥 / 端点 / 模型这些"填错会静默失败"的设置  
  从各处收拢到一页，便于集中核对。

---

## [v0.32.0] — 2026-09-22

### 新增

- **TTS 支持 WebSocket 传输（豆包 v3 单向流式）**。`tts.rs` 现在**按端点 scheme 自动选传输**：  
  `https://` 走原有的 HTTP chunked + NDJSON，`wss://` 走火山二进制帧  
  （`头 4B + event 4B + session_id + payload_size + payload`）。两条路由**同一份配置**描述 ——  
  服务商、密钥、音色、采样率完全一致，只有"这段 JSON 怎么送出去"不同。做成"同配置双传输"而不是  
  新开一个 provider，是因为多开一个下拉框只会多出"选了 HTTP 却填了 wss 端点"这种错配机会，  
  而它报出来的错（连接失败 / 400）指不到真正的原因。  
  新增：`TtsConfig::transport()`、`Transport`、`build_client_frame` / `parse_server_frame` /  
  `read_u32` / `read_payload` / `explain_volc_code`、`synthesize_volc_ws`。
- **`scripts/probe_models.py`：模型配置连通性探测**。一次把「语音编辑 LLM / 流式 ASR /  
  任务台 TTS / 每个 Api driver」**逐条真打一次**，并把结论分三档：  
  `OK`（拿到业务结果）/ `AUTH`（连上了但被拒 → 改账号密钥）/ `FAIL`（连不上或协议不匹配 → 改代码网络）。  
  分档的理由：这两类问题的修改方向相反，混成一句"不可用"会把排查方向带偏。
- **TTS WS 的真机断言测试** `tts::tests::live_volc_ws_synthesis_produces_playable_48k_pcm`  
  （`#[ignore]`，需要 `EASYOP_TTS_KEY` 与网络）。本版**已用它实测通过**：  
  `48000 Hz / 181608 采样 / 3783 ms / 12 帧 10 句 / 耗时 2137 ms`，  
  服务端回读文本 19 字与请求 19 字一致。

### 变更

- `synthesize_volc` 的前置校验抽成 `TtsConfig::volc_preflight`，**WS 与 HTTP 共用**。  
  分开实现最容易出现的漏是"HTTP 拦了、WS 没拦"，而它在真机上只表现为一次必然失败的网络往返。
- 任务台设置页（`DeskSettings.tsx`）三处提示更新：端点提示写明"按前缀自动选传输"（https / wss 两种），  
  音色提示给出**同版本配对示例**（1.0 配 `zh_female_cancan_mars_bigtts`；2.0 配  
  `zh_female_vv_uranus_bigtts` 等实测可用音色），resource 提示补上"账号未授权的 resource_id  
  会在 WS 握手阶段就 403"。百炼提示注明只有 HTTP。
- `probe_models.py` 的探针请求 `max_tokens` 从 16 提到 256，并识别"上限给小了"的假失败  
  （见下方修复第 3 条）。`driver` 与 `llm` 两条探测共用同一个 `probe_chat_endpoint`，  
  避免一处修了判读、另一处没修而给出相反结论。

### 修复

- **`cargo check` 整个项目编译不过（依赖回归，非本模块引入）**。  
  链：`tauri-build` / `tauri-utils` → `schemars 0.8.22` → `indexmap 1.9.3`。  
  indexmap 1.9 的 `IndexMap<K, V, S = RandomState>` **只在 `has_std` 生效时**才给 `S` 默认值，  
  而 `has_std` 由它自己的 build.rs 用 autocfg 探测 sysroot 得出；在 **rustc 1.98** 上这个探测探不到  
  （证据：`target/debug/build/indexmap-*/output` 里只有 `rustc-check-cfg`、没有 `rustc-cfg=has_std`），  
  于是 schemars 的 `pub type Map<K, V> = indexmap::IndexMap<K, V>` 报  
  `struct takes 3 generic arguments but 2 were supplied`。  
  修法：在 **`[build-dependencies]`** 加一条 `indexmap = { version = "1.9", features = ["std"] }`，  
  触发它 build.rs 第一句的"显式给了 std 就别探测"分支。  
  ⚠️ **必须挂 `[build-dependencies]`**：schemars 处在构建依赖图（host）里，而 Cargo resolver v2  
  （edition 2021 默认）**不跨宿主图与目标图合并特性** —— 挂 `[dependencies]` 实测无效，报同一个错。
- **错误码不再笼统报"上游报错"**：`45000010`（鉴权，key 不属于语音合成服务）与  
  `55000000`（音色与 resource 版本不匹配，附实测可用音色）现在给出**修改方向相反**的两段指引；  
  WS 握手阶段的 **403** 单独报"账号 / 套餐 / 密钥"，与建连之后才出现的 55000000 分开。
- **"接口是通的，但回复是空的"这类假失败不再被读成接口故障**：`deepseek-flash` 这类带思维链的模型  
  会把 `max_tokens` 全部花在 `reasoning_content` 上（16 个 token 时 `content=""`、  
  `finish_reason=length`）。探测脚本现在用 256 的上限，并在正文为空且 `reasoning_tokens > 0` 时  
  直接点明"是上限给小了，不是接口不通"。

### ⚠️ 未完成

- **WS 路径只支持"单向流式"**（整段文本一次发完）。火山另有**双向流式**端点  
  `/api/v3/tts/bidirection`，需要客户端先发 StartSession / FinishSession 事件帧 —— **本版不支持**。  
  误填那个端点会报"回帧解析失败"并附头部十六进制。任务台"念一段总结"用不到双向流式。
- **TTS 的下发链路（PC 喇叭播放 / 下发固件）未验证**。本版验证到的边界是  
  "合成出 48 kHz 单声道 PCM 并落盘成可听的 WAV"，再往后（Opus 编码、I2S 播放）没跑过。

---

## [v0.31.0] — 2026-09-21

### 新增

- **内容台公众号导出降级为本地 HTML 投放包**：`studio.export` 新增 `publishWechat` 参数。导出时先检测公众号登录态  
  （`pub.py status`）——有效则走 `pub.py publish wechat --save-draft` 自动填草稿；失效则不阻塞，调  
  `eo.chat` 用排版提示词（提炼自 wechat-typesetting-trae-main skill 的 blue-minimal 模板规范）生成  
  全内联样式 HTML，写入本地投放包（正文.html + 投放说明.md + \_manifest.json），后续手动粘贴进公众号  
  编辑器。与小红书 `pack xhs` 的"本地交付 + 人工最后一公里"模式对齐。
- **内容台面板新增「+公众号」按钮**：一键触发上述导出分支。
- **`studio_io.py` 新增 `write-html` 子命令**：把排版好的 HTML 写成公众号投放包目录。
- **未复核的终端粘贴键会在日志里被点名**（`UNVERIFIED_PASTE_KEYS` +  
  `paste_key_verified`，本条由处理「S1 命令没粘进终端」那条线并入）：  
  焦点窗口命中一条**没被程序模拟验证过**的表项（当前是 Windows Terminal 的  
  `Ctrl+Shift+V`）时，日志打一行"这一条还没用程序模拟实测过…若终端里没反应，请把  
  「粘贴方式」显式改成 `ctrl_v` 再试"。  
  为什么值得做：这类失败的现场是"**日志写投递成功、终端里一个字都没有**"，  
  不留这句话，用户会先怀疑板子 / 识别 / 自己，**不会怀疑那张表**。  
  ⛔ 行为本身**没有**变化 —— 表项没改，只是不再沉默。

### 变更

- `studio.export` 的返回信封新增 `wechatMode` / `wechatOk` / `wechatTail` 三个字段（向后兼容，原字段不变）。
- 排版提示词（`TYPESETTING_SYSTEM`）提炼自 wechat-typesetting skill 的 SKILL.md + references/blue-minimal.md
  - guidelines.md，用 `eo.chat` 复刻其 AI 排版流程（skill 本身是 AI 驱动的，没有 CLI）。
- 验证台从 144 项增至 156 项，新增公众号导出两条路径（登录态有效走 publish / 失效走 HTML）的断言。


### 修复

- **槽位按键投进终端后输出是一屏"乱码 + 字符串"**：根因是预置执行器 `codex-cli` 的  
  `args` 带 `--json`，而 `slot_command_line` 把这条命令**原样粘进终端给人看**——  
  `--json` 让 codex 把 stdout 打成 JSONL 事件流、并在交互终端叠一层 ANSI 转义/进度 TUI，  
  裸投进 mintty 就成了一屏乱码（"字符串"是 JSON 里的 prompt/回复文本）。  
  修法：**终端投递这条路自动摘掉 `--json`**（这条路由人看，没有任何机器解析 JSONL，  
  留着纯有害）；后台「派发」按钮走 `build_args`→`run_once`，**仍保留 `--json`** 做完成判定  
  与 TTS 总结。两条路径各自独立、不漂移：终端版 = 后台版去掉 `--json`  
  （`cli_driver::command_line` + 测试 `command_line_drops_json_for_terminal_readability`）。  
  验证：同一份 `codex.cmd exec --json …` 之前粘进去跑出来是 JSONL，改后粘进去跑出来是  
  codex 干净的可读流式输出。⚠️ 你那边需**重新编译 easyop** 才能让修复生效（Rust 侧改动）。

### ⚠️ 未完成

- **配图生成（`studio.gen.figures`）仍未实现**：公众号 HTML 投放包当前只含文字，不含配图。用户将另行补图。
- **HTML 粘贴进公众号编辑器的样式保真度未经 E2E 验证**：需真粘一次确认排版不塌（规范§十五待验证前提第 1 条）。
- **排版质量依赖 LLM**：`eo.chat` 返回的 HTML 可能不严格遵守内联样式规则，生产环境需关注稳定性。

---

## [v0.30.4] — 2026-09-21

> ⚠️ **本版没有单独发布**：bump 到 0.30.4 的同时，同一棵工作树上另有改动把真源  
> 顶到了 **v0.31.0**，本节保留为**历史账**（改动内容已并入 v0.31.0，见上）。  
> 多 agent 共树时看不到彼此的 diff，版本说明是唯一账本 ⇒ 留着比删掉安全。

> 把 v0.30.3 剩下的那个"**没把握却什么都不说**"的地方补上：终端粘贴键表里  
> `CASCADIA`（Windows Terminal）那一条**还没用程序模拟复核过**，而现在它一旦失效，  
> 日志里会**点名**说出来，并给出退路。行为本身没变。

### 新增

- **未复核的粘贴键会在日志里被点名**（`UNVERIFIED_PASTE_KEYS` + `paste_key_verified`）：  
  焦点窗口命中一条没被程序模拟验证过的表项时，日志打一行  
  `粘贴键：焦点窗口类是「…」，这一条还没用程序模拟实测过…若终端里没反应，请把
  「粘贴方式」显式改成 ctrl_v 再试`。  
  ⇒ 为什么值得单独做：这类失败的现场是"**日志写投递成功、终端里一个字都没有**"，  
  不留这句话，用户会先怀疑板子 / 识别 / 自己，**不会怀疑这张表**。
- 单测 `unverified_terminals_are_flagged_and_the_measured_one_is_not`，  
  同时钉住**反方向**的错：把已实测的项标成未复核 ⇒ 每次投递都刷假警告，  
  警告一旦成为常态就没人看了，那和没有警告一样。

### 变更

- 无行为变更。`CASCADIA_HOSTING_WINDOW_CLASS` 仍是 `Ctrl+Shift+V`（**不猜、不改**）；  
  `mintty` 仍是 `CtrlV`（v0.30.3 实测结论）。

### 修复

- 无。

### ⚠️ 未完成

- **v0.30.3 那三节"未完成"全部仍未完成**，本版没有推进其中任何一条：
  - **没按过真板子 S1 走完整链路**（需要用户在手上操作，我做不了）。
  - **`CASCADIA` 依旧未复核**：本机**没有安装 Windows Terminal**  
    （`wt.exe` / Appx 包 / 进程三项都不存在）⇒ **在本机无法实测**，  
    所以只能把它标成未复核，不能改表。复核办法：装 WT 后跑  
    `scripts/e2e_terminal_paste_key.py`（脚本按类名找窗口，换个终端即可复用）。
  - "被 UIPI 拦下"没有真实复现（需要以管理员身份开一个终端）。

---

## [v0.30.3] — 2026-09-21

> 修「S1–S4 槽位按键说出任务后，命令**没有**粘进终端」。  
> 根因是**终端粘贴键表里 mintty 那一项记错了基准**：表里存的是"**人手动按**能用的键"  
> （`Shift+Insert`），而程序模拟按键的条件更严 —— 实测结果是**一个键都到不了目标**，  
> 于是日志写着"已把命令粘到「…mintty」"、终端里一个字都没有。

### 新增

- **槽位按键投递前的"目标必须是终端"闸门**（`agent_desk::delivery::TargetPolicy`）：  
  槽位按键（`TerminalOnly`）在目标不是已实测终端时**当场拒绝**，并在日志/界面上说清  
  "命令没有投出去、请先点一下终端"。界面上的「投到我正在用的窗口」按钮保持宽松  
  （`Any`）——它的目标只能靠后台记录**推测**，没把握就不该加硬约束。  
  ⇒ 顺带消灭了一类**静默错投**：09-21 现场日志里，前台是飞书时命令被粘进了**飞书的  
  聊天输入框**，而日志写的是"S1 已把命令粘到「飞书[Chrome_WidgetWin_1]」"并判成功。
- **`SendInput` 返回值核对**（`text_delivery::send_one`）：`SendInput` 会返回"实际插入  
  输入流的事件数"，少于请求数就是失败（最常见是被 UIPI 拦下 —— 目标以管理员身份运行）。  
  以前这个返回值被丢掉，于是**一个键都没送达也报"投递成功"**；现在会明确报错。
- **两个实测脚本**（`scripts/`，可复用）：
  - `probe_key_events.py`：装一个吞掉自己注入事件的低层键盘钩子，看 **OS 把一条  
    `SendInput` 变成了什么**（vk / 扫描码 / 扩展位 / 上下）—— 零副作用，不碰任何窗口。
  - `e2e_terminal_paste_key.py`：在真终端里逐个试粘贴组合、**截屏读回**，判定"哪个组合  
    真的能粘贴"。会弹出一个测试终端窗口几秒。

### 变更

- **`mintty`（Git Bash / MSYS2）的粘贴键：`Shift+Insert` → `Ctrl+V`**（实测结论，见下）。  
  影响所有走"终端粘贴"的注入：语音输入、槽位按键投递、命令前缀预置。  
  ⚠️ 这是**改了一张表**，不是"改了个偏好"——理由与证据写在 `text_delivery.rs` 文件头。  
  Windows Terminal（`CASCADIA`）那一项**没动**（未用程序模拟复核过，不猜）。
- 三处失败不再静默（都只记警告，不错判成"整次注入失败"）：等修饰键松开超时、  
  原剪贴板还原失败、粘贴后的回车没送出去。

### 修复

- **S1–S4 槽位按键说出任务后命令没粘进终端。**  
  根因：`TERMINAL_PASTE_KEYS` 里 `mintty → ShiftInsert`，而**程序模拟** `Shift+Insert`  
  在 mintty 里**什么都不做**。源码证据：mintty 的 `Shift+Ins` 走通用分支  
  `if (clip_shortcuts && key == VK_INSERT && mods && !alt) { if (shift) win_paste(); }`，  
  其中 `shift` 来自 `get_mods()` → **`GetKeyState(VK_SHIFT)`（泛用 VK）**；  
  而注入的 shift 到达目标时是 `VK_LSHIFT`，**泛用位不会被置上** ⇒ `shift == false`。  
  （Ctrl 那条不中招：`get_mods()` 查的是**具体**的 `VK_LCONTROL`。）  
  ⇒ 改成 `Ctrl+V`：mintty 有一条**专为"被注入的 Ctrl+V"写的**兼容分支  
  （`// Workaround for Windows clipboard history pasting simply injecting Ctrl+V
  (mintty/wsltty#139)` → `if (key == 'V' && mods == MDK_CTRL && !scancode) win_paste();`）。  
  本机 mintty 实测：`Ctrl+V`（`wScan = 0`）**粘贴成功**；`Shift+Insert`（无论补不补 E0  
  扩展位）均无反应。  
  ⛔ 因此 `key_input` 的 `wScan` **必须留 0**：那条兼容分支的判据正是 `!scancode`，  
  补上真实扫描码会把它关掉（实测确认过：补了之后 `Ctrl+V` 立刻失效）。

### ⚠️ 未完成

- **只在"程序模拟"这一层实测过**（`scripts/e2e_terminal_paste_key.py`，目标是本机  
  `C:\Program Files\Git\usr\bin\mintty.exe`）。**没有按过真板子的 S1 键**走完整链路  
  （按键 → 说话 → 识别 → 拼命令行 → 投递）。
- **Windows Terminal（`CASCADIA`）那一项仍待复核**：`Ctrl+Shift+V` 依赖 SHIFT 修饰键，  
  与 mintty 踩的是同一类坑，但它是否真受影响**没有实测**（本机没有该终端在场时不敢猜）。  
  临时办法：设置里把「粘贴方式」显式改成 `ctrl_v`（表只影响 `auto`）。
- 判定失败的兜底：**未做过"注入被 UIPI 拦下"的真实复现**（需要以管理员身份开一个终端）。

---

## [v0.30.2] — 2026-09-21

> 两个**静默失效**的修复：**普通键盘 `e` 键被吞**、**语音编辑字幕只出最前面一两个字**。  
> 两个都不报错、日志里也没有线索，现象分别是"键盘坏了"和"识别不准" ——  
> 排查方向从一开始就是错的。另附蓝牙音频通路的结论（见"未完成"节，结论是**不必做**）。

### 修复

- **打开本应用后，普通键盘的 `e` 键打不出来**（其余键全部正常）。  
  根因：键盘钩子（`ptt_hook.rs`）的**编辑键**分支把"抑制这一下按键"写在了"修饰键匹配"  
  **之外** —— 只要 VK 对上编辑键的主键就整个吞掉。而默认编辑键是 `Ctrl+Shift+E`，  
  于是**单按 `e` 也被吞**（连系统自动重复的 keydown 一起）。这是"全局热键"最经典的事故面：  
  **吞键必须与"匹配成功"成对出现**，两者一旦分离，那一下按键就在系统里凭空消失。  
  修法不是就地补一个 `if`，而是把判据抽成纯函数 `keydown_verdict`：
  - **主键与编辑键共用同一个判据**。此前两处各写一遍 `mods_state & mods == mods`，  
    等于给"两把键行为分叉"留口子（那样用户就得记两套按法）。
  - 新增单测 `keydown_verdict_matrix` 钉死它。其中「只按 `e`（无修饰键）**必须放行**」  
    就是这次的回归点；同时钉住另一条容易被"优化"错的语义：单键 PTT（`mods == 0`）  
    在任何修饰键状态下都成立（这是「按住右 Ctrl 说话」这类用法的基础）。
- **语音编辑时字幕只显示最前面一两个字，后面的全不显示**。  
  根因：`edit_overlay.html` 处理 `asr-partial` 时把 `setText` 也塞进了状态判断  
  （`if (currentState === "listening")`），而它紧接着就把状态改成 `recognizing`  
  ⇒ **从第二段 partial 起全部被自己挡住**。识别本身一直是好的 —— 这正是"第二次按下  
  编辑键后完整文本立刻出现"的原因，也是容易把它误判成"识别不准"的原因。  
  修法：`setText` **无条件**执行，只有"状态跃迁"才做判断；唯一该拦的是"已进入改写阶段"  
  之后才姗姗来迟的那一段结果（那是上一轮的残响，不能盖掉正在流式输出的改写文本）。

### 变更

- **删掉从未被发射过的 `edit-partial` 事件监听**（`edit_overlay.html` / `VoicePanel.tsx` 各一处）：  
  编辑链路复用 `asr::run_recognition`，识别文本本来就走全局 `asr-partial`。  
  ⛔ **死通道比缺通道更危险** —— 它看起来像有一条路。上面那个字幕 bug 正是因为  
  "以为编辑模式另有一条通道、于是再写一条兜底分支"才引入的。现在只有一条通道。
- **两个字幕浮层补 `.panel.recognizing` / `.status.recognizing` 样式**：  
  此前 `setState("recognizing")` 会把面板打回 idle 的半透明底 —— 正在往外滚字幕时  
  反而"看暗了"，像是已经停下了。

### ⚠️ 未完成 / 未验证

- **本次改动没有真机验证**，两条修复各需要一次真机确认：  
  ① 在真键盘上打一段含 `e` 的文字（以及 `Ctrl+Shift+E` 仍能触发编辑键）；  
  ② 走一遍「选中文本 → 按编辑键 → 说话」，字幕应**持续增长**而不是停在头两个字。
- **`cargo test --lib` 并行执行偶发 `STATUS_HEAP_CORRUPTION` (0xc0000374)**。  
  与本次改动无关（本版只增加了一个纯逻辑单测、没碰任何 FFI）。判据：  
  **`--test-threads=1` 串行跑必过**（386 passed / 0 failed / 2 ignored）。  
  怀疑是若干 FFI 用例（Everything SDK / HID 枚举 / 剪贴板 / cpal）并行不安全，  
  **具体是哪一个尚未定位**。在定位之前跑测试请加 `--test-threads=1`，  
  否则可能拿到"整份崩掉"的假信号、误判成自己改坏了。
- **蓝牙音频通路的结论（本次核实，答案是"不必做"）**：ESP32-S3 的蓝牙是 **BLE-only**  
  （Espressif 官方：`Classic Bluetooth is not supported`），而所有成品"蓝牙麦克风/耳机"  
  走的是 **HFP/HSP（经典蓝牙）** ⇒ 这条路在 SoC 上就不存在；LE Audio 同样不行  
  （ESP-IDF 支持表里 ESP32-S3 的 `ESP-BLE-ISO` / `ESP-BLE-AUDIO` 均为不支持）。  
  ⇒ 它&#x662F;**"路径不存在"，不是"技术难点"，也不是"等着排期的需求"**。不需要再做任何事。

---

## [v0.30.1] — 2026-09-21

> 内容台插件（studio）的可移植性修复与一处自检笔误。仅改 `plugins/builtin/studio/`，不动宿主契约。

### 新增

- **内容台外部工具路径可配置**：`studio.export` / `studio.doctor` 调用的 Python 解释器与  
  `content-publisher` 目录现在优先从 `eo.storage` 的 `config` 键读 `{ cpDir, pyExe }`，  
  没有才回退到内置默认值。换机器只需往 storage 写一次配置，不必改源码。  
  生产环境推荐 `pyExe` 设为 `python`（走 PATH）。

### 变更

- `studio_io.py` 的默认输出目录从硬编码 `E:\MY_AI\tools\content-publisher\drafts` 改为  
  读环境变量 `EASYOP_DRAFTS_DIR`，回退到 `~/Documents/easyop-drafts`。对既有调用无影响  
  （`studio.export` 的 `--out-dir` 仍可显式覆盖）。

### 修复

- **`studio.doctor` 不再误报「插件目录」检查失败**：原代码 `!!helperPath().startsWith('__') === false`  
  因运算优先级恒为 `false`（先 `!!` 得 `true`，再 `=== false` 得 `false`），导致自检在正常环境下  
  也把「插件目录」标为不通过。改为 `!helperPath().startsWith('__')`，与下一行「自带脚本」逻辑一致。

---

## [v0.30.0] — 2026-09-21

> **本版撤销了 v0.29.0 的「两段式确认」**，把语音按键的语义统一成**短按**：  
> 按一下开始说话 → 再按一次注入；录音期间按**回车** = 注入并发送。  
> 同日还短暂实现过「按住说话、松开结束」（`ptt_up` / `EditUp`），也一并撤掉了 ——  
> 那条路是**把硬件能力当成了产品意图**（固件确实发 keydown/keyup 一对，但产品规定是短按）。

### 新增

- **录音期间按回车 = 一次按键完成「结束录音 + 注入输入框 + 发送」**，省掉"先按语音键结束、  
  再按回车提交"那一步。
  - 判据是纯函数 `enter_grabs`，**只在"这一轮的话会进输入框"时才接管回车**  
    （`PttOrigin::Voice` 且没有任务台归属）。任务台派发 / 语音编辑 / 连续通话**都不接管** ——  
    在那些场景里吞掉回车就是静默吃用户一次按键，而现象与"键盘坏了"一模一样。
  - 只认**不带修饰键**的回车：`Ctrl+Enter` / `Shift+Enter` 在聊天窗和编辑器里各有含义，  
    不替用户决定。
  - 钩子层通过回调向会话状态机查判据（`set_enter_gate`），**不存副本**：副本会老化，  
    老化后会把用户**所有**回车都吞掉。
  - **识别在途时按回车同样认**（先按了语音键、又顺手补一下回车）：把这次交付升级成  
    "注入并发送"，而不是让那一按落空。

### 变更

- **语音按键统一为「短按」语义**（`Hold` 模式）：按下开始录音，**再按一次**结束并注入  
  （不发送）。"结束"那一按就该把话交出去，不再需要第三下。
- **面板 ● 按钮与语音键（KEY1）恢复为完全等价**（都走 `ptt_down`）。v0.29.0 引入的  
  `ptt_down_manual` 分叉已删除 —— 那套理由依赖"待确认"这个中间态，中间态没了，分歧也该没。  
  ⛔ 别再为它们造第二条路径：两个入口行为不一致，正是这一版要消灭的那类矛盾。
- **删除 `ptt_up` / `edit_ptt_up` 与"松开"事件**：钩子只把 keydown 转成事件，keyup 只抑制、  
  不再上报。额外好处是整条链路**不再依赖 keyup 送达** —— 钩子被临时挂起、按键被别的进程  
  吃掉，都不会让录音卡住（对比"按住"语义下，丢一次松开就是一段永远收不了尾的录音）。
- **删除 `get_pending_inject` / `inject_pending` / `cancel_pending_inject` 命令与  
  `voice-pending` 事件**；新增 `get_failed_inject` / `retry_failed_inject` /  
  `clear_failed_inject` 命令与 `voice-failed-inject` 事件。
- **删除 `stop_recording` 命令**（`force_stop`）：它绕过按键归属校验，  
  而面板按钮现在走 `ptt_down`（会正确拒绝"去结束别人正在录的那段"）。
- 前端不再自己记"我是不是在录"（`recRef`）：那份镜像会与后端真实状态漂移，而漂移的表现  
  正是"按钮写着停止、按下去却开始了一段新录音"。按钮标签本来就来自后端事件。

### 修复

- **「注入失败」既不再丢内容，也不再需要用户多按一次键。** 注入失败的内容留在后端  
  （`failed_inject`），面板出现一条**失败条**（用错误色，与正常流程区分）可「重新注入」/「丢弃」。  
  重试失败时内容会**再放回去** —— 试了一次没成不会让内容消失。  
  ⛔ 它**只出现在异常路径**，正常流程永远碰不到，所以不会像 v0.29.0 那样给正常操作加按键。
- 语音键在"录制已结束、结果还在路上"那一秒被再按时不再**静默**：会提示  
  "上一句还在识别中，请稍候…"（此前那一按确实什么都不做，用户只能以为按键失灵）。
- 设置里「粘贴后自动回车」的说明改为如实：它**不管语音输入**（语音按"你怎么结束录音"决定），  
  只管命令注入等其它路径。
- 浮层不再对 `asr-final` 下"已经进输入框"的结论 —— 它是在**注入之前**发的，而注入可能失败；  
  真正该显示的状态由随后的 `asr-state`（"已注入输入框…"）给出。

### ⚠️ 未完成 / 未验证

- 本次改动**只过了单测（385 passed）+ 编译 + 类型检查 + 构建**，**没有在真机板子上按过**。  
  需要在真机上按这几条走一遍：
  1. 按一下语音键 → 说话 → 再按一下 ⇒ 内容进输入框，**不发送**；
  2. 按一下语音键 → 说话 → **按回车** ⇒ 内容进输入框**并发送**（一次按键完成）；
  3. 录音期间按 `Ctrl+Enter` / `Shift+Enter` ⇒ **不该**被接管，应原样传给前台程序；
  4. **不录音时按回车** ⇒ 完全不受影响（这是最关键的一条：接管过宽会让用户以为键盘坏了）；
  5. 任务台槽位键录音期间按回车 ⇒ 不参与（结果仍只派发，不进输入框）。
- **蓝牙三条限制与 v0.29.0 相同**（无蓝牙音频通路 / 连接状态读不到 / 板子 Wi-Fi 音频通道  
  从未上线），本次未动，说明见下一条。

---

## [v0.29.0] — 2026-09-21

> ⚠️ **本版有两项行为已被 [v0.30.0] 撤销**：①「两段式确认」——用户明确要求去掉中间那一次  
> 按键；②同日短暂实现过的「按住说话、松开结束」（`ptt_up`）——**把硬件能力当成了产品意图**，  
> 已回退为短按。读下面内容时请以 v0.30.0 为准。

### 新增

- **语音输入改成「两段式确认」**：按住/按下语音键说完一句之后，识别结果**停在字幕上  
  等你确认**，再按一次语音键才注入当前输入框。在此之前，"结束录音"这一按同时承担了  
  "停止 + 识别 + 提交"三件事，而**第三件是用户的决定**，不是宿主能替他做的。
  - 只作用于**单次按键（Hold）**&#x6A21;式。连续通话与语音编辑保持原样：通话是多句流式产出、  
    编辑是改写选中文本，在它们中间插一步确认只会打断"一口气说完"。
  - **任务台派发刻意不走这条路**：派发带着"未读"这个承诺（结果出现在你看不见的地方，  
    所以留灯等你回来），中间插人工确认会让这个承诺变成"还得再按一下才算数"。
  - 面板上新增「注入 / 取消」按钮，并支持"再按一次语音键"。取消是显式动作 ——  
    否则想放弃一句就只能靠再录一段把它顶掉，那不叫取消。
  - 注入失败时内容会**放回**待确认状态并可重试，不会凭空消失。
  - **面板上的 ● 按钮与语音键刻意分叉**：语音键只有一颗，语义必须在「开始 / 结束 /  
    确认」之间复用；而面板上已经摆着显式的「注入 / 取消」按钮，所以 ● 按钮**永远按  
    自己的标签行事**（开始录音），不会变成"注入"。否则会出现"按钮写着『开始录音』、  
    按下去却在注入"这种界面说谎。物理按键走 `ptt_down_from`，界面按钮走新增的  
    `ptt_down_manual`（内部 `allow_confirm_pending = false`）。此时若确实停着一条待  
    确认内容，新一段识别结果会把它顶掉 —— 后端会**出声提示**，不会静默丢字。
- 新增命令 `get_pending_inject` / `inject_pending` / `cancel_pending_inject`；  
  新增事件 `voice-pending`（等待确认）、`voice-injected`（**真的进了输入框**）；  
  字幕浮层在等待确认期间**保持显示**（那是你唯一能核对的依据）。

### 变更

- **「粘贴后自动回车」默认值由开改为关**（世代标记 `injection_defaults_version` 2 → 3，  
  老配置会自动迁移）。理由：回车是**不可撤销**的提交，而"该不该现在提交"只有你知道；  
  语音内容常常是接在一条还没看完的命令后面的，此时回车等于直接执行它。  
  需要提交时按 **KEY2**（默认绑 Return）。设置项仍在，想要"说完就发"可以打开。  
  ⚠️ 这次迁移会覆盖在上一世代里手动打开过该开关的配置（世代标记无法区分  
  "用户主动打开"与"旧默认值写盘"），受影响的话去设置里再打开一次即可。
- **蓝牙（BLE HID）不再是"开关 + 配对 + 状态"三件套，改为一说明块。** 原三样都不成立：  
  开关下发的 `{"cmd":"ble_enable"}` 固件并不认识（它的配置协议只接受固定的配置字段，  
  **没有指令字段**），而且走的是不等回执的下发通道，所以按了等于没按且从不报错；  
  状态标签读的是 **Wi-Fi UDP 音频通道的心跳**，与蓝牙无关；开发板的蓝牙本来**就没有开关**  
  （BLE HID 在固件里常开，配对由 Windows 侧完成）。`ble_enable` / `ble_disable` /  
  `ble_pair` / `ble_status` 四个命令已改为**如实返回**（不再假装成功）。
- 麦克风来源里的「蓝牙键盘麦克风」明确标注**不生效**：开发板只有 **Wi-Fi UDP** 一条音频  
  通路（固件 `AudioTransport` 只有 `Unavailable` / `WifiUdp`），蓝牙在这台设备上只传按键、  
  从不传音频。配置里的 `mic_source: "bluetooth"` 会**纠错迁移**为 `auto` 并留日志  
  （判据是"值等于已知错值"，`pc` / `wifi` 这类真能生效的选择一律不动）。

### 修复

- **字幕浮层不再谎报"已上屏"**：`asr-final` 的处理里曾硬编码 `setState("done", "已上屏")`，  
  而识别完成之后有两种命运（直接注入 / 挂起等待确认），浮层看不到这个区别 ——  
  在等待确认时那句话也是这么写的，用户据此以为已经进去了。现在只贴文本并写中性的  
  "识别完成"，真正的状态由随后的 `asr-state` 给出。
- 语音面板新增「待确认」状态，与「最近上屏」**分开存放**：前者是"还没送出去，等你点头"  
  （可注入、可取消），后者是"已经送出去了"（历史，只读）。两者共用一个状态时，  
  界面就再也说不清这句话到底进没进输入框。
- **「最近上屏」那一行不再提前报喜**：它此前跟着 `asr-final` 走，而那个事件在"识别完成、  
  停下等确认"时也会发 —— 于是用户还没点头，界面就已经写着"最近上屏：xxx"。现在它只认  
  新增的 `voice-injected`（仅在注入成功后发出）。同一件事在这一版里被修了两次（浮层一次、  
  面板一次），根因是**拿"识别完成了"当"送出成功了"** —— 这两个是不同的事实。
- **注入任务异常终止时不再丢内容**：`inject_pending` 里有三种结局，而第三种（执行线程  
  panic / 运行时提前关闭，拿不到 `Result`）此前既不放回内容、也不打日志 —— 那句话就被  
  净丢了，且日志上一条线索都没有。现在它与"注入报错"走完全一样的处理：放回 + 出声。

### ⚠️ 未完成 / 未验证

- 本次改动**全部只过了单测（385 passed）+ 编译 + 类型检查 + 构建**，  
  **没有在真机板子上按过**。需要实测的路径：① 按一次语音键说话 → ② 再按一次 →  
  字幕应停在结果上且输入框**没有**任何变化 → ③ 再按一次 → 内容进输入框但**不回车** →  
  ④ 按 KEY2 才提交。以及「取消」按钮、以及在待确认状态下**点面板 ● 按钮重新录音**  
  （应重新开始录音、不得变成注入，旧那句被顶掉时要出声提示）。
- **蓝牙麦克风不可修复**（不是本次没做，是设备没有这个能力）：要脱离电脑麦克风只能走  
  Wi-Fi UDP 通路；而该通路目前**从未收到过开发板心跳**（日志证据：只有  
  `UDP 监听已启动`、没有任何首次心跳记录），所以"按住板子说话"仍会走电脑麦克风。  
  这一步在环境侧：确认板子连上同一 Wi-Fi 且固件在推 `EIHB` 心跳与 `EIAU` 音频。
- **蓝牙连接状态读不到**（宿主缺 BLE central 能力）。开发板本身有完整状态  
  （`{"ble":{"connected":..,"valid":..,"itvl":..,"latency":..,"timeout":..}}`），  
  但只经 BLE GATT 特征发布，不走 USB HID 的 `0x11` 回执。要显示真实状态需要新增  
  Windows 侧 BLE 读取实现（WinRT `Windows.Devices.Bluetooth`），本次没做 ——  
  与其拿 Wi-Fi 心跳顶替，不如如实说"读不到"。

---

## [v0.28.1] — 2026-09-21

### 修复

- **「语音识别」全部失败 —— 根因是板子音频监听线程被误杀。**  
  `KeyboardAudioManager` 是 `Clone` 类型（内部 `Arc<Inner>`），却实现了 `Drop`  
  并无条件 `shutdown()`；而录音路径拿到的只是 `keyboard_audio_clone()` 产生的  
  **局部克隆**，函数一返回就被 drop ⇒ **第一次录音结束就把全局唯一的 UDP 监听  
  线程带走** ⇒ `last_heartbeat_ms` 再不更新 ⇒ `is_online()` 恒为 false ⇒  
  板子麦克风永久失效、所有录音静默退回电脑麦克风。  
  用户按着板子说话，声音根本没进这个设备。  
  **正确模型：监听线程的生命周期 = 进程生命周期**，退出时由 `RunEvent` 显式停。  
  现场证据（当日日志）：`14:04:58.976 开始录音 → 14:04:59.184 监听线程已退出`  
  （206 ms，恰为一个 `RECV_TIMEOUT` 轮询周期）；且当天 **9 次语音输入全部  
  "服务端回包 2 个、零文本"，4 次语音编辑却有 3 次识别成功** —— 编辑时人本来  
  就对着电脑说，所以反而正常。
- **语音编辑的字幕串行**（现象："字幕显示逻辑混乱，显示了其他字符"）。  
  编辑流程有两个阶段（识别 → LLM 改写），但前端把两者写进**同一个 `liveText`**：  
  识别阶段是**覆盖**语义（服务端回累积全文），改写阶段是**追加**语义 ⇒ ASR 原话  
  还没清掉，LLM 增量就接在后面，两段来源与语义都不同的文本被拼成一段。  
  现拆为 `liveText` / `editText` 两个独立状态；后端在改写流开始时新增  
  `edit-llm-start` 事件，前端据此切换到改写字幕。

### 新增

- **音频电平判据（峰值 / RMS，dBFS）。**  
  "服务端回了包但里面没有文字"有两种**完全相反**的原因：用户没说话，或声音根本  
  没进这个设备。过去这两者在日志里完全同形，只能靠用户回忆"我当时到底说了没"。  
  现在每次识别结束都记一行实测电平（正常说话约 -30 ~ -10 dBFS，静音在 -60 以下），  
  "空结果"提示也会直接点明"这轮几乎是静音（峰值 -56 dBFS），声音没进来"。
  > 该判据已用对照实验验证：全零静音 WAV ⇒ 服务端**恰好回 2 个包、零文本**；  
  > 真实语音 WAV ⇒ 回 10 个包并给出文本。与故障现场日志的指纹逐字一致。

### 变更

- **录音源决策必须说清理由。** 日志与提示不再只写"用了电脑麦克风"，而是带上原因：  
  "从未收到开发板心跳（板子没连同一 Wi-Fi，或固件没在推流）" / "刚掉线，最近一次  
  心跳在 N ms 前" / "配置指定"。  
  ⛔ 不合并成一句"离线"——**"从没连上过"和"刚刚掉线"是两种病**，处置方向相反。
- **回包日志补全两处盲区**：① 收尾阶段"既非文本帧、也非错误帧"的回包过去被  
  **静默丢弃**（连一行日志都不留），现补上；② 认不出来的回包不再只记 16 字节 HEX，  
  而是附上 payload 正文预览 —— 一眼就能分开 `{"result":{"text":""}}`（听清了、  
  但这轮没内容）与完全另一套结构（对端不是我以为的那个服务）。

### ⚠️ 未完成

- **板子 Wi-Fi 音频通道从未上线**（当日日志 0 次心跳记录）。这是**环境侧**问题，  
  代码修不了：需确认板子已连同一 Wi-Fi，且固件在推 `EIHB` 心跳与 `EIAU` 音频。  
  在此之前，"按住板子说话"仍会退回电脑麦克风 —— 区别是现在**失败会明确说出来**，  
  而不是让用户对着"未识别到内容"猜。
- 以上改动只过了单测（378 passed）与端到端探针对照实验，**尚未在板子上按过**。

---

## [v0.28.0] — 2026-09-21

### 新增

- **「启动终端」动作（`cli_terminal`）—— 任务台改走这条路线的第一步。**  
  按一下绑了这个动作的键 → 开一个终端窗口，并把配置好的命令（默认  
  `codex exec --skip-git-repo-check`）**打进输入框但不回车** → 再按语音键说话，  
  识别到的内容接在命令后面 → 回车即执行。  
  这样 **「启动环境」与「输入内容」是两个各自独立的步骤**，不再要求宿主替用户  
  决定"什么时候算说完、什么时候该提交"。为什么必须拆开见下（变更第 1 条）。
  - **⛔ 不能"把命令当参数喂给终端"**：`cmd /k <命令>` 是**直接执行**，  
    而我们需要的是"只打进去、等你把话说完"。所以只能**先开窗口、再往窗口里打键**  
    —— 而"往窗口里打键"正是 `text_delivery` 已有的能力，直接复用，  
    而不是另写一套 `SendInput`。
  - **末尾那个空格由代码补**（`prefix_payload`）。命令是前缀、语音也不带前导空格，  
    少了它粘出来就是 `--skip-git-repo-check打开计算器` —— 被 shell 当成**一个**参数，  
    报错长得像"codex 参数写错了"。为此给注入层加了"**保留首尾空白**"的口子  
    （`clipboard_content`，默认仍 trim），并由单测钉死。
  - **焦点已经在终端上就不再开新窗口**，直接往那个终端里预置。否则"上次没等到窗口"  
    的重试路径会变成"每按一次多一个窗口、永远打不进去"。  
    复用与"等窗口"用的是**同一个判据**（`wait_for_terminal_foreground`，超时给 0 = 只查一次），  
    所以这不是一个新的风险方向。
  - **注入串行化**（`LAUNCH_LOCK`）：并发两次注入会互相踩剪贴板 —— 先结束的那次会把  
    后一次刚写进去的内容当成"用户原来的剪贴板"恢复回去，表现出"粘出莫名其妙的内容"  
    且**全程不报错**。
  - 配置存独立文件 `cli_terminal.json`（command / cwd / terminal / auto_run），  
    不动 `config.json`。设置页新增「终端启动」一节，&#x542B;**「试一下（立刻启动终端）」**&#x6309;钮：  
    配错了不必去板子上按一下才知道。
  - 结果通过 `cli-terminal` 事件回报（带 `ok` 字段）。必须单开一条事件，因为  
    `launch()` 要等窗口（上限 6 秒），后端把它 **spawn 到独立线程**了 ——  
    直接调会把这一个 HID 监听线程钉住 6 秒，表现为"板子上按键全失灵"。
- **「启动终端」并入动作体系的 10 处接线**：`constants.rs`（保留 UUID  
  `...000d` + 词表 + 碰撞单测）、`payload.rs`（双向映射）、`validate.rs`、  
  `src/constants.ts`、`src/types.ts`、`src/actionLabel.ts`（新增「启动终端」分组）、  
  `src/components/KeyEditor.tsx`（选中时给出两步走说明）。

### 变更

- **任务台（`agent_desk`）路线的取舍**：原先设计的"按第二下 → 把该槽执行器的  
  完整命令行投进你正开着的终端并回车"（v0.26.0）**保留不动**，但**不再作为主路线**。  
  原因不是它坏了，而是**它要求宿主替用户判断"这一段话什么时候说完"** ——  
  而按住说话的物理按键在固件里只回发一条报文（没有按下/松开），  
  这个判断只能靠宿主猜。改成"先开终端、再语音输入"以后，  
  **两件事各自都有明确的物理起点**，不再需要那个猜测。
- 设置页「终端启动」一节：输入框类字段点「保存」才落盘（与「Everything 搜索」同一约定），  
  开关则立即保存（保存时带上同页未保存的输入框内容，避免把刚打的命令覆盖回旧值）。
- `text_delivery` 内部：`deliver_inner` 增加 `keep_edges` 参数；新增  
  `deliver_to_window_prefix`（不 trim、不回车）与 `deliver_to_window_submit`  
  （强制回车）。原 `deliver_to_window_typed` 由前者取代（两者都未被发布过）。

### 修复

- 本轮未修新缺陷。上一版（v0.27.0）的语音识别修复**仍待真机验证**，  
  本次未触碰 `asr.rs`。

### ⚠️ 未完成 / 待验证

- **「启动终端」完全没在真机上按过。** 只过了 `cargo test --lib`（371 passed）、  
  `tsc --noEmit`、`vite build`。请按顺序核验：
  1. 设置 → 终端启动 → 「试一下（立刻启动终端）」：应出现终端窗口且输入框里有命令、**未回车**；
  2. 给某个键绑「启动终端」，按一下：同上；
  3. 接着按语音键说"打开计算器"：内容应**接在命令后面**（中间有空格），回车后执行。
  - 若第 1 步报"没等到它成为前台窗口"：说明新窗口没拿到焦点。  
    **不要连按**（会开多个窗口）；按提示点一下那个终端，再按一次键即可（会走复用分支）。
- **`auto_run`（打完直接回车）默认关，且未实测**。开了之后语义就退化成"一键跑固定命令"，  
  与前缀用法相反 —— 保留这个开关只是为了不把可能性焊死。
- 任务台旧链路（S1–S4 派发 / 未读 / 播报）**未验收、未改动**；其自身缺口见 v0.27.0 一节。

---

## [v0.27.0] — 2026-09-21

### 修复

- **语音识别"说了话却一个字都不上屏"** —— 根因是**收尾负包的 sequence 写法**，  
  与麦克风、按键、配置都无关：
  - 负包原先写死 `sequence = -1`。火山 `sauc/bigmodel_async` 现在会回  
    `autoAssignedSequence (-N) mismatch sequence in request (-1)`，**每次录音都在收尾那一刻被判失败**。
  - 更糟的是代码收到错误帧就 `return Err`，把**已经识别出来的整段文本丢掉**；  
    用户看到的是"识别失败"，而音频其实一路都被逐字识别正常。
  - 本机实测四种写法的服务端反应（`scripts/probe_asr_e2e.py --compare` 可复现）：
    | 负包写法                            | 服务端        |
    | ------------------------------- | ---------- |
    | `sequence = -1`（旧写法）            | ❌ mismatch |
    | `sequence = -(下一序号)`            | ✅ 通过       |
    | **不带 sequence，仅 LAST 标志**（本次采用） | ✅ 通过       |
    | `sequence = 下一序号`（正数）           | ❌ mismatch |
  - 采用"不带 sequence"：不依赖"数得准第几帧"，不会因为某帧发送失败而错位。  
    单测 `final_frame_carries_no_sequence_only_the_last_flag` 钉死，防止被"顺手补回去"。
  - 通话链路（Call 模式）同一处负包一并修正。
  - **已识别文本不再因错误帧作废**：只有"一个字都没有**且**服务端报错"才判失败。  
    收尾报错但文本完整时照常交付，错误另写日志。

### 新增

- **日志落盘**（此前只存内存，关掉应用全失 —— 上一次故障排查就是卡在这里）：
  - 路径：`%APPDATA%\com.easyop.app\logs\easyop-YYYY-MM-DD.log`，按天分文件；
  - 每条 flush（崩溃时最后几行恰恰是原因）；保留 14 天，启动时自动清理旧文件；
  - 内存缓冲容量 1000 → 4000；
  - 「日志」面板新增「**打开日志目录**」按钮，排障时一键取到当天那份。
- **ASR 链路日志补全**（请求/响应关键字段 + 状态变化 + 错误原文）：
  - 建连：端点、resource_id、request_id、耗时、**服务端 `X-Tt-Logid`**（提工单的唯一凭据）；
  - 首包：完整请求 JSON；
  - 上行：每 25 帧汇总帧数与字节数（不被逐帧刷屏）；
  - 回包：每条的序号 / `last` / 文本预览；非文本回包打印十六进制头；
  - 错误帧：**原文**（含 mismatch 这类协议错误）；
  - 收尾：总帧数 / 字节数 / 回包数 / 最终文本字数 / 错误。
- **LLM 链路日志补全**：请求体预览（截断 600 字符）、参数（model / temperature / max_tokens）、  
  响应状态与 `X-Request-Id` 一类追查 ID、失败时的端点与耗时。
- **诊断脚本 `scripts/probe_asr_e2e.py`**：用真实音频按同一套二进制协议走完整链路  
  （建连 → 首包 → 上行 → 负包 → 收文本），把"服务端问题"与"本机采集问题"一刀切开。  
  配置从应用 `config.json` 读取，不写死密钥。

### 变更

- 语音状态提示补记日志：录音源、每一次入口按下（哪个键、开始还是结束、模式），  
  识别成功/空结果/失败的结果摘要。事后复盘"谁把谁的录音截断了"不用再靠猜。

### ⚠️ 未完成 / 待验证

- **本次修复的端到端验证在 Python 探针上完成，尚未在板子上按过**。  
  请按 KEY1 说一句话核对：应能正常上屏，且日志里能看到"识别成功 N 字"。
- 任务台按键链路（v0.25–v0.26 的 S1–S4 派发/投递）保留不动，但**已决定改走另一条路线**  
  （按键启动终端 + 语音输入拆成两步），该新动作尚未实现。

---

## [v0.26.2] — 2026-09-21

### 修复

- **⛔ 语音功能会整体变砖：识别任务可能永远不返回，导致"所有语音入口一起失效"。**  
  现象是：所有录音入口（KEY1 语音输入、任务台 S1、编辑键、界面上的开始录音）  
  按下后都不再有任何反应，界面上一个字都不报 —— 只能重启应用。

  根因是一条**结构性**的缺陷，不是某一处的疏漏：
  - 宿主的"当前有没有在录音"是**唯一的会话槽**，而它的清空点在识别任务  
    **返回之后**（`session.finish(gen)`）；
  - 识别链路里有三个 `await` 点都是网络 IO，且**全都没有超时**：  
    建连（TCP/TLS）、发包、以及收尾等"服务端的最后一包"。  
    其中收尾那段等的是"服务端回最后一包或断开连接"—— 服务端**完全可以两者都不做**；
  - 于是任务卡死不返回 ⇒ 会话槽永远被认为是"有人在录" ⇒  
    同一个键再按只会去"结束"一段永远不会结束的会话（**按了没反应**），  
    别的键还会被判成归属冲突而拒绝 ⇒ **四个入口一起死**。
- **`asr.rs` 全链路补上超时**（新增「防砖超时」一节）：  
  建连 10 秒、单包发送 5 秒、收尾等待 8 秒。宁可拿已经收到的半截文本，  
  也不能把整个语音功能锁死。`run_recognition` 与 `run_call`（通话）两条链路都补了。
- **会话层的最后一道兜底：卡死自愈。** `ptt_down_from` 在按下时先做一次  
  「这段会话是不是已经卡死」的检查（Hold 上限 = 正常上限 3 分钟 + 2 分钟余量；  
  通话 = 30 分钟 + 3 分钟），超时则**强行释放**并明确告知用户  
  （"上一段录音…已强制释放，本次按键按「开始」处理"）。  
  即便将来又冒出一个没想到的等待点，用户最多等几分钟，而不是重启应用。

### 新增

- **"没识别到字"现在说得出病在哪一段链路**（`AsrOutcome` + `explain_empty`）。  
  过去三种完全不同的故障 ——「麦克风一个字节都没采到」「音频上去了但服务端一声不吭」  
  「服务端回了但内容是空的」—— 在界面上都显示同一句"未识别到内容"，  
  排查只能靠猜。现在分别给出：录音源是谁、上行了几帧多少 KB、服务端回了几个包。
- **新增 `voice-alert` 事件通道**：语音链路的失败提示过去**只**发到字幕浮层，  
  而浮层只有录音那几秒才显示 —— 恰恰是故障大多发生的时候（按下之前/之后）。  
  现在同步发到主窗口提示条，**没有浮层的时候失败也一定有地方看得见**。
- **`scripts/probe_asr.py`**：5 秒判断"语音服务这一环通不通"。  
  只做一次 WebSocket 握手，区分「网络/鉴权问题」与「本机录音侧问题」；  
  配置从应用自己的 `config.json` 读，**不写死任何密钥**。

### 变更

- 录音开始时把**录音源**（开发板麦克风 / 电脑麦克风）写进日志 ——  
  排查"没收到音频"时第一个要回答的就是这一问。
- 归属冲突的拒绝提示改走统一告警出口（浮层 + 主窗口），不再只发浮层。
- ⚠️ **未完成**：`voice-alert` 只在主窗口以提示条显示 5 秒，**没有历史面板**，  
  关掉窗口就看不到了；`log_buffer` 仍然不落盘（关掉应用日志全失，事故无法复盘）——  
  这次排查就吃了这个亏，建议作为下一个优先项。

---

## [v0.26.1] — 2026-09-21

### 新增

- 无对外新增能力。会话层新增 `PttOrigin`（录音**发起者**）与纯判据 `press_conflict()`，  
  是修复本版按键冲突所必需的结构，不是新功能。

### 变更

- **任务台槽位键的归属校验变严**：以前按**另一个槽**的键会「结束当前那段、并按原槽派发  
  （附一句提示）」；现在直接**拒绝**，并提示先按原槽收尾。理由：按 S2 的意图是"给 S2 录"，  
  把它当"结束"用是反直觉的，而那句提示多半没人读完。
- `handle_reserved_uuid` 不再用 `current_desk_slot()` 猜去向，改为走 `ptt_down_from()`；  
  后者返回"这一按是否真的起了作用"，被拒时**不再播报"开始录音"**（播了用户会以为在录）。
- 界面文案（按键重映射 / 任务台设置页）补上"录音期间别的键按下会被拒绝"这条规则。

### 修复

- **⛔ 按键冲突：按 KEY1（语音输入）会把任务台正在录的那段结束掉，并当作任务派出去执行命令。**

  默认方案里 `KEY1 = voice_ptt_hold`（走全局热键 `Ctrl+Shift+Space`）、`KEY6 = task_desk S1`。  
  两者看似无关，实则驱动**同一个录音会话**：`lib.rs` 的钩子线程 → `ptt_down()`；  
  任务台按键 → `handle_reserved_uuid()` → `ptt_down()`。而 `ptt_down` 只会 toggle  
  （"有录音就停、没有就开"），这句判断**不认得当前这段录音是谁开的** ⇒ 任意入口的按下  
  都能"结束"别人正在录的那段：
  | 操作           | 用户以为     | 实际发生                         |
  | ------------ | -------- | ---------------------------- |
  | S1 录音中按 KEY1 | 开始一段语音输入 | **结束 S1 那段，当作任务派给 S1 并执行命令** |
  | KEY1 录音中按 S1 | 开始给 S1 录 | **结束 KEY1 那段，识别结果上屏到编辑器**    |
  两次都**完全静默**：不报错、日志无异常，只能靠"结果不对"自己发现。
- **根因**：录音会话是**互斥资源**（一个麦克风、一路 ASR、一个注入出口），却被四个入口  
  （全局 PTT 热键 / 编辑键 / 任务台槽位键 / 界面按钮）共享，而"这段录音是谁发起的"  
  **从不被记录**。
- **修法**：让录音会话**自己记住发起者**（`PttOrigin::{Voice, Edit, DeskSlot(n)}`），  
  入口按下时先比对，不一致就**明确拒绝**：不改归属、不 toggle、说清是谁在录、该按哪个键收尾。
  - `origin` 存进 `Active` 结构、与 `current` **同生共死** ⇒ `recording_origin()` 与  
    `is_listening()` 读同一个锁同一个 `Option`，"正在录音"和"知道是谁开的"在结构上  
    不可能漂移。这与 v0.25.0 的归属修复是同一条原则：**属于某段会话的东西，不要放成全局字段** ——  
    放成全局就必须回答"谁负责清它"，而清的时机必然夹在异步回调之间，早一步晚一步都错。
  - 判据抽成**纯函数** `press_conflict()`：拒绝动作要 `AppHandle`（得 emit 提示）⇒ 测试里  
    构造不出来；不抽的话这条防线只剩"看代码好像是对的"。

### 未完成

- ⚠️ 本次修复**只过了单测（350 passed）+ 编译 + 前端构建，未真机验证**。
- 界面上的「开始录音」按钮（`start_recording`）与 KEY1 同属 `Voice` 入口，可以互相收尾 ——  
  这是有意的（两者都是"普通语音输入"），但**未实测**。
- `force_stop()`（`stop_recording` 命令 / 手动停止）**刻意不做归属校验**：它是"强制结束"  
  语义，是用户被卡住时的出口。这一点未在界面上写明。

---

## [v0.26.0] — 2026-09-21

### 变更

- **⛔ 任务台按键的第二次按下，从「后台派发」改为「投到你正在用的终端里执行」**  
  （钦友定：要的就是过程可见 —— 命令在你眼前跑，输出你自己看）
  - 第二次按下现在做的事：取该槽绑定的执行器 → 按它的模板展开成**完整命令行** →  
    粘进你正在用的终端 → 按全局「粘贴后回车」设置决定是否回车提交。  
    例（钦友本机 S1 绑定 `codex-cli`）：  
    `codex.cmd exec --json --skip-git-repo-check 帮我看看这个目录有什么`
  - **不再是**"进队列 → 后台跑 → 完成后播报 + 留未读"。那条后台路径**保留在页面的  
    「派发」按钮上**，两条入口并存，只是按键默认走可见的那条。
  - 与语音输入的分工：**第一次**按下 = 开始录音（出字幕，和语音输入一样）；  
    **第二次**按下 = 上面这一步。固件按键只回发一条报文、没有按下/松开之分，  
    所以本来就是"按一下开、再按一下关"，**松开不会停止识别**（这一点没有改动）。

### 新增

- `cli_driver::command_line()`：**把执行器配置渲染成一条可以粘进终端的命令行**。
  - ⛔ 关键不变量：**必须复用后台那份 `build_args`**，不许另写一份。理由：用户会  
    交替使用"页面派发（后台）"和"按键投递（前台）"，两边拼出来的东西但凡有一次  
    不一样，就没有任何日志会报错，只会表现为"两次跑得不一样"。已用单测逐项对照 argv。
  - **保守的引号策略**：参与的参数里若含 `"`、`$`、`` ` ``、`\`、换行 —— 一律报错终止，  
    不硬拼。原因：粘贴进去的是**别人的解释器**（cmd / PowerShell / bash 三种规则各不相同，  
    不存在一种写法在三处都对），而用户有充分理由相信这是 easyop 替他拼好的。  
    中文无空格则完全不加引号（少一层引号就少一处差异），已单测钉住。
  - 不带 `cd`：交互式终端有自己的当前目录，硬塞 `cd` 会覆盖掉用户刚切好的位置，  
    且 `cd /d` 与 bash 写法不同。driver 的 `cwd` 只影响后台执行。
- **执行器是 Api 形态时明确失败**，不静默降级：  
  Api 是 HTTP 直连、**没有命令行这种东西**。若悄悄降级成"粘原话"，用户会看到自己的话  
  被粘进终端却什么都没发生 —— 比直接报错难懂得多。
- **`--json` 提示**：命令行里带 `--json` 时，投递成功的提示会额外说明"输出是 JSONL 事件流、  
  不好读，想看得清楚就把该槽绑到不带 `--json` 的执行器"。  
  依据：本机 `codex exec --help` 实测原文 *--json  Print events to stdout as JSONL*。  
  这类"能跑但看着像坏了"的情况最容易被误诊成功能故障，所以明确说出来（**不偷偷改参数** ——  
  改了就不是他配的那条命令了）。检测会跳过引号内部，避免"请用 --json 输出"这种正常句子误报。
- 抽取 `deliver_blocking()`：页面按钮与按键**共用同一份投递实现**。两处各写一份必然漂移，  
  表现为"按钮能用、按键不能（或反之）"，而这种差异没有任何日志会报错。

### ⚠️ 未完成

- **未真机验证**：需要钦友按一次 S1 走完整链路（期待看到：终端里出现一条完整命令并回车执行）。
- **建议先做的一步**：S1 现在绑的 `codex-cli` 带 `--json`，粘到终端会输出一屏 JSONL。  
  若要"过程看得清楚"，在设置页另建一个 **不带 `--json`** 的 CLI 执行器（其余参数相同）  
  并把 S1 绑到它；带 `--json` 那个留给页面上的后台派发（完成判据需要它）。
- 播报只从 PC 喇叭出声（板子喇叭需固件新增 PCM 通道，钦友已定不做）；`0x16` 灯语未烧板。
- `evidence_cmd`（世界门禁）仍未给 codex-cli 配默认命令。
- 「阶段配置与说明不一致」仍未定位（缺截图）。

---

## [v0.25.0] — 2026-09-21

### 新增

- **⛔ 硬件按键 S1–S4 现在真的能派任务了**（此前 `handle_reserved_uuid` 只认领、  
  报"未实现"，按了没反应）
  - **按键只决定"这段录音派给谁"**，录音与识别复用既有 PTT + ASR 链路，  
    不发明第二套录音。固件按键是白名单制的 `host_action:<uuid>`，回发**只有一条  
    报文、没有按下/松开之分** ⇒ 只能"按一下开、再按一下关"，这正好与  
    `ptt_down` 的 toggle 语义一致（Hold 模式下 `ptt_up` 本来就是空的）。
  - 识别结果按归属分叉：`Some(slot)` ⇒ 派进任务台；`None` ⇒ 照旧上屏。  
    两条出口**互斥**，同一次录音只走一条 —— 混着走会让用户既在终端看到一份文本、  
    又收到一条任务，而他以为自己只说了一次。
  - **第二次按下不改归属**：一段录音只能属于一个槽。允许"说到一半改主意"会产出  
    半句派 S1、半句派 S2 这种谁都解释不清的结果。想换槽就先结束再重按。
  - **派发失败绝不退回上屏**：那会把一句话粘进用户当前窗口，而他以为自己说的是  
    给 Agent 的任务 —— 文本去了错的地方还报成功。失败时明确告诉他没派成 +  
    把原话还给他（`dispatched: false`，界面按失败着色）。
  - 归属模型：`desk_intent`（**认领即消失**）+ `recording_desk_slot`（仅供提示）。  
    见下面"修复"里那两条 —— 这是本次设计上真正花力气的地方。

### 变更

- 界面文案按"按一下开、再按一下关"重写（原来是"按住 S1–S4 说话"，与固件实际  
  能力不符）：`DeskSettings` 顶部说明、按键重映射里"任务台"动作的说明。  
  设置页原来那句"只落地了配置层与 TTS 能力"已过期，改为如实说明剩余缺口  
  （板子喇叭路未做、`0x16` 灯语未烧板）。
- 不影响既有配置：本次没有新增配置项、没有改配置格式。

### 修复

- **⛔ 归属被"停止键"清掉 ⇒ Hold 模式永远派不出去（自查发现，尚未上机）**
  - 原实现在 `stop_active()` 里清空归属。而流程是：第二次按键 → `stop_active`  
    先清归属 → 识别结果回来 → 已无处可派 → 话被上屏。任务台永远收不到一句话，  
    而界面显示"已上屏"，看起来一切正常。
  - 修法：**归属不是全局可变状态，是"这一次录音会话"的属性** —— 在 `start_hold` /  
    `start_call` 开头认领一次（`take_desk_intent`，认领即消失），之后作为局部值  
    沿调用链传给 `inject_one` / 注入 worker，不再回头读全局。去向在按下"开始"  
    那一刻就定了，用户什么时候按停止、什么时候超时都影响不了它。
- **⛔ 上一次录音的归属残留 ⇒ 下一次普通语音输入被错派（自查发现）**
  - Hold 超时结束只走 `finish()`（不清归属），残留值会被下一次录音捡走 ⇒  
    用户明明没按槽位键，话却被派进了任务台。
  - 修法同上："认领即消失"让残留**从机制上不可能存在**，不依赖任何一处记得去清。  
    提示用的 `recording_desk_slot` 在 `finish()` 里作废，且只清**当前**会话的  
    （迟到的旧会话不会动新会话的）。
- **归属必须在 `ptt_down` 之前读**：结束那一按会走到会话收尾并清掉提示归属，  
  先按键再读则永远读到 `None` ⇒ "你按的是 S2、但这段是给 S1 的"这句关键提示  
  永远不会出现，用户会以为自己改派成功了。
- 顺带：删掉 `runner.rs` 里一条多余的 `use std::os::windows::process::CommandExt`  
  （`Command` 是 `tokio::process::Command`，它自带 `creation_flags`，trait 没被用到）。

### ⚠️ 未完成

- **未真机验证**：按键链路的正确性目前只有 4 条单测（归属一次性 / 停止不清归属 /  
  会话结束作废 / 旧会话不越界清）+ 编译 + 类型检查。需要钦友在板子上按一次  
  S1 走完整链路：按一下 → 说话 → 再按一下 → 看任务台是否多了一条任务。
- **播报只从 PC 喇叭出声**（板子喇叭需固件新增 PCM 通道，钦友已定不做）。
- `0x16` 灯语两侧已通但**未烧板**。
- `evidence_cmd`（世界门禁）仍未给 codex-cli 配默认命令。
- 「阶段配置与说明不一致」仍未定位（缺截图）。

---

## [v0.24.2] — 2026-09-21

### 新增

- **投递时可指定"这一次用哪组粘贴键"**（任务台派发区下拉，值记在 localStorage）
  - **为什么必须能覆盖全局设置**：codex / claude 这类 TUI 把 **Ctrl+V 绑成了  
    "粘贴图片"** —— 键事件一到它们手里，就先用图片格式读剪贴板，读不到就报  
    `Failed to paste image: no image on clipboard`，**并且不回落到文本粘贴**  
    （openai/codex #17103 的原文：*Ctrl+V in TUI is image-only when it reaches  
    Codex as a key event*；#19698 是同一个现象的上游报告）。
  - 推论（也是这次的真正教训）：**Ctrl+V 只有在被终端自己吃掉时才是"文本粘贴"**。  
    而"哪个键会被终端吃掉"是**按终端**来的（Windows Terminal 吃 Ctrl+Shift+V、  
    mintty 吃 Shift+Insert），不是按目标程序来的 —— 程序名在这里派不上用场。  
    所以它只能由用户当场定，且**不能写回全局**（全局那一项被语音注入共用）。
  - 界面把这条坑直接写在按钮下面：投给 TUI 别用 Ctrl+V；WT 选 Ctrl+Shift+V、  
    Git Bash 选 Shift+Insert，只有普通编辑器才用 Ctrl+V。

### 修复

- **⛔ 投递到 codex TUI 报 `Failed to paste image`（钦友实测反馈）**
  - 定性：**这不是 easyop 的错，是 codex 自己的行为**（本仓库搜不到该字符串）。  
    easyop 这一步做的是"写文本进剪贴板 + 发粘贴键"；文本确实进了剪贴板，  
    codex 收到键后按图片去读、读不到就报错且不回落。
  - 所以修法不是改注入逻辑，而是**让"用哪个键"变成这次投递可显式指定的**  
    （见上）—— 选终端自己的粘贴键，键根本不会到达 codex，它就无从去读图片。
  - 日志里现在能看出用的是哪个键：`文本注入：粘贴方式 = X（配置 Y，本次由投递指定）`。

---

## [v0.24.1] — 2026-09-21

### 修复

- **⛔ A″ 投递：焦点没跳回目标窗口（钦友实测反馈）**
  - 现象：点「投到我正在用的窗口」，焦点没回终端（或回去了又被抢回来），  
    内容最终没进终端。
  - 根因：**"激活"与"按键"之间隔了整整一秒**。原实现在点按钮后先还焦点，  
    再调 `text_delivery::deliver` —— 而它第一步是 `wait_keyboard_neutral`（最长  
    **600ms** 等修饰键松开），之后才是写剪贴板 + 按粘贴键，全程 1.3s 以上。  
    这 1.3s 里 Windows 完全可能把前台改回去（点击收尾的**延迟激活**、系统挑  
    "下一个前台窗口"），于是按下 Ctrl+V 那一刻前台已经不是终端 ⇒  
    **内容粘进了 easyop 自己的界面，而日志一路写着"投递成功"**。
  - 修法：新增 `text_delivery::deliver_to_window(text, hwnd)` —— 把  
    **激活 → 判定粘贴键 → 按下** 压到零间隔：等修饰键松开**之后**、按键**之前**  
    再激活一次并回读校验，校验不过就直接失败（宁可什么都不做，也不粘错地方）。  
    `deliver()` 的既有行为与 7 个调用点**完全不变**（纯增量）。
  - 顺带补上三行诊断日志，让"焦点到底去哪了"变成可查的：  
    ① **选中谁立刻记一条**（含句柄/类名/标题/来源 —— 投递失败时最需要知道的就是这个）；  
    ② 归还失败时记"归还后前台实际是谁"；③ 投递结束后前台若不是目标 ⇒ warn。
  - 界面提示分三种结局说清楚：投进已知终端 / **投进不在终端名单里的窗口**  
    （粘贴键退化为 Ctrl+V，TUI 会把它当快捷键吃掉 —— 正是最早踩的那个坑）/  
    **投完焦点被改走**（回目标窗口确认内容）。

### 变更

- **世界门禁现在跑在任务的工作目录里**（原来继承 easyop 自己的进程目录）。
  - 后果：填相对路径的门禁（如 `if exist out.png`）会"明明生成了却判失败"，  
    而失败原因只写"门禁未通过" ⇒ 排查成本全转给用户。
  - 现在 `evidence_cmd` 用执行器的「工作目录」字段（为空时才继承）。

---

## [v0.24.0] — 2026-09-20

### 新增

- **A″ · 把任务打进你开着的终端**（不再只有"后台跑"这一条路）
  - 由来：钦友拍板的路线是 **A″ + C** —— 过程看得见（A″）+ 走开时有人喊（C）。  
    C 上一版已在跑（5 秒一次扫 codex rollout），**A″ 这一半从来没接上**，  
    于是"看得见思考过程"这件事只能靠自己手敲。本次补的就是这一半。
  - 新增命令 `desk_deliver_foreground` + 任务台按&#x94AE;**「投到我正在用的窗口（可见执行）」**：  
    把这句话粘进你正在用的那个终端（Windows Terminal 的 codex TUI / Git Bash），  
    过程就在你眼前；**不入队、不起子进程、不判完成**（交互 TUI 拿不到 stdout 终态）。
  - ⛔ **不产生未读、不点灯**：结果就在你正开着的终端里、永久驻留。  
    "跑完了"由 C（外部会话监工）播一句。再计一份未读是重复记账。
  - **目标窗口怎么选**（`delivery::pick_target`，纯逻辑、6 条单测钉死）：  
    ① 派发那一刻的前台窗口不是 easyop ⇒ 就投它（语音/按键路径焦点往往还在终端）；  
    ② 前台是 easyop 自己（点按钮必然如此）⇒ 退回后台跟踪记录的"上一个外部窗口"；  
    ③ 两个都不是外部窗口 ⇒ **报错请用户先点一下终端**。  
    ⚠️ 绝不退化成"投给自己" —— 那是一个没有受害者、也没有意义的成功。
  - **先还焦点，再注入，注入前再确认一次焦点**：注入走的是模拟按键，前台是谁就粘进谁。  
    焦点没真的还回去就粘，内容会落进别的窗口而日志仍写"投递成功"——  
    这类"看起来成功"的失败最难查，所以校验失败当场报错、本次不投。
  - 投递目标（类名 + 标题）进日志与界面提示：**投到哪儿去了必须让用户知道**。

### 修复

- **⛔ 子进程会闪一个控制台黑框**（`runner.rs` 未设 `CREATE_NO_WINDOW`）
  - Windows 上给命令行程序起进程默认会分配控制台 ⇒ 任务一跑就闪黑框、还可能抢一下焦点，  
    而它的输出我们**全从管道里读**，那个窗口零信息量。
  - 修法：Windows 下 `creation_flags(0x0800_0000)`。非 Windows 分支不受影响。

### ⚠️ 未完成 / 需要钦友实测

- **A″ 只做了"从任务台页面点按钮"这一条触发路径**；更自然的"按住键说话 → 投进终端"  
  仍是**语音输入那条既有链路**（它本来就是把识别结果粘进焦点窗口），  
  两者没有打通 —— 语音说完是否也要记进任务台，等你用一轮再定。
- **按键链路（S1–S4 硬按键）仍未接**：`handle_reserved_uuid` 依旧只认领、不派发。  
  它依赖固件录音下行一起验，未实测前不固化"哪个键说给哪个槽"。
- 本次改动**未经真机点击验证**：单测覆盖的只是目标选择逻辑（纯逻辑），  
  真实投递需要你开着终端点一下按钮（会粘进去 + 按粘贴配置决定是否回车）。
- `evidence_cmd`（世界门禁）仍未给 codex-cli 配 ⇒ "完成"仍只是"上游说完了"。

---

## [v0.23.0] — 2026-09-20

### 新增

- **多轮对话：Codex 会话续接**（原来每问一次都重开一个会话）
  - 现象：第一次问完，第二次问"那刚才那个呢"，codex 从没见过上一轮 ⇒ 答不上来。
  - 根因：`RunRequest` 里根本没有"上一次会话"这一项。每次都是  
    `codex exec "<prompt>"` ⇒ 每次都是全新 thread。
  - 实测依据（本机两步验证）：  
    `exec --json --skip-git-repo-check "记住 7391"` → thread_id `01a0bf1e-…`；  
    再 `exec --json --skip-git-repo-check resume 01a0bf1e-… "那个数字是多少？"`  
    → **同一个 thread_id**，回答 `7391`。
  - 做法（**数据驱动，不是给 codex 单写一个 driver**）：  
    `ContinuationSpec { id_event_type, id_field, args }` ——  
    ① 会话 id 从哪一行输出里来 ② 那行里哪个键 ③ 带回去要加哪些 argv。  
    `RunRequest.session_id` / `RunOutcome.session_id` 贯穿，`DeskManager` 按槽位持久化  
    （文件 `agent_desk_out/slot_sessions.json`，**跨重启存活**）。
  - `resume <id>` 的位置是实测定死的：**插在全局选项之后、prompt 之前**  
    → `exec --json --skip-git-repo-check resume <id> "<prompt>"`。
  - 换执行器 ⇒ 旧会话**自动作废**（codex 的 thread id 交给别的工具毫无意义）。
  - **失败降级**：续接没成立（输出为空且未完成）⇒ 丢掉 id 用新会话重跑一次，  
    warn 记一条。会话 id 是上游给的，它的生命周期我们管不着，不兜这下会变成  
    "每次都失败且无线索"。
  - **「新起一轮对话」按钮**（任务台手动派发区）：换话题的唯一出口。  
    没有它，一个槽挂上会话就再也下不来。
- **迁移层 `migrate_preset_features` + 世代标记 `preset_features_version`（当前 = 1）**：  
  老配置里没有新字段 ⇒ `#[serde(default)]` 静默变成"不支持"，升级了却没变化。  
  世代标记保证**只补一次**，且用户在设置页主动取消勾选的**不会被补回来**。

### 修复

- **⛔ 进度档位：「留空/填 0」会把后面的档整体前移**（说明与实现不一致）
  - 界面写「第 2 档 5 分钟」，实际却在 1 分钟的位置播"已运行五分钟" ——  
    `normalize()` 把 0 用 `filter` 压实了，位置语义丢失。
  - 修法：**0 留在原位**（= 这一档不用），后面的档不得因它前移。
  - 连带修一个更狠的：`progress_notice` 之前不跳过 0，而 `elapsed >= 0` 恒真 ⇒  
    **留空最后一档时，任务刚起步就会宣称"已运行十五分钟"**。这类谎话比不播更伤。
  - 三个输入框与三句语料的对应关系现在是契约：第 N 个框永远对应第 N 句。
- **⛔ 状态播报：一条样本都没播出去时没有任何提示**（"设置了却听不到"的排查口）
  - `speak_notice()` 现在检查 `played_samples == 0` ⇒ warn，并指明去看  
    **Windows 默认输出设备**和音量。
  - `audio_out` 每次播放记一行日志：**设备名 / 设备采样率 / 声道 / 源采样率 / 样本数**。  
    最大一类"没声音"是声音去了另一台设备（耳机 / 虚拟声卡）——  
    那种情况代码每一步都成功，只有这行日志能把它指出来。

### 变更

- `RunOutcome` 新增 `session_id`（driver 回传本轮会话），`RunRequest` 新增 `session_id`。
- 新增命令：`desk_session_info`（各槽会话摘要）、`desk_reset_session`（新起一轮）。
- 设置页执行器区新增「会话续接」三格（仅 Cli 形态显示）；任务台派发区显示  
  当前在续接的会话短 id + 「新起一轮对话」按钮。
- 进度档位说明改写：写清实际是 **4 句话**（"正在推进"不在配置表里，  
  任务一跑起来就说；跨越第一档后按时刻升档），以及留空 = 该档不用且留在原位。

### ⚠️ 未完成 / 需要钦友提供信息

- **第 3 项「阶段配置与说明不一致」没能完全定位**：全仓库搜索后，**没有找到  
  名为「阶段」的配置项**，也没有任何界面文案里出现 `phase` 字样。  
  最贴近的是「进度档位（分钟）」（本次已修两处不一致）。  
  若你指的是别处（例如固件下发回执里的 `phase=`，或按键重映射里的某个下拉），  
  **请给一张截图或告诉我它在哪个页面哪一行** —— 我按"阶段数量 / 名称 / 顺序 /  
  触发条件"四项去核对。
- 会话续接**尚未在真实任务里跑过一轮**：本次只验证了 argv 组装（单测）与  
  codex 的 resume 行为（实跑脚本）。第一次真实多轮 = 你下一轮提问。
- A 方案仍是 headless（不开终端窗口），按键链路未接 ⇒ A″ 仍不可用。
- `evidence_cmd`（世界门禁）未给 codex-cli 配 ⇒ "完成"仍只是"上游说完了"。

---

## [v0.22.1] — 2026-09-20

### 修复

- **⛔ 状态播报从来没出过声**（根因：播报只发事件，喇叭这一环没人接）
  - 现象：日志里写着「任务台播报: S1 「任务已开始」」，界面字幕也跳了，  
    **但喇叭一声没响**。任务开始 / 完成 / 失败全都只是"看起来播了"。
  - 根因：`emit_notice_with` 只做两件事 —— 打日志 + `emit` 事件；播放 wav 的代码  
    只存在于 `desk_play_notice`（设置页试听按钮）里，**自动播报从未调用它**。  
    前端收到事件后也只 `setLastNotice`（显示字幕），不出声。
  - 修法：把播放逻辑抽成 `speak_notice(&DeskManager, kind)`，命令与自动播报共用  
    **同一个出口**；`emit_notice_with` 在有可用语料时 spawn 一次出声。
  - ⚠️ 为什么必须落在宿主（Rust）：喇叭是**设备级的承诺** —— 用户听到"已开始"  
    才会走开。交给前端 ⇒ 任务台页面没打开 / 正切页的那一瞬间，这一句就静默消失，  
    而日志仍然记着"已播报"。这是"看起来在工作、实际没兑现"的典型形态。
  - 失败分级（不静默，也不刷屏）：`NoAsset` / `Busy` 用 info（前者日志已标注，  
    后者是刻意不打断未读播放），只有 `Failed` 用 warn。
- **⛔ 完成播报念的是 JSON 噪音，不是结论**
  - 现象：任务跑完，播报念的是 `{"type":"turn.completed","usage":{…}}` 那一串。
  - 根因：老逻辑"取输出尾部 150 字"。而 `codex exec --json` 的 stdout 是**事件流**，  
    最后一行永远是 usage 统计；用户要听的那句在**倒数第二行**  
    （`item.completed` 里的 `agent_message.text`）。
  - 修法：新增纯函数 `final_agent_text()` —— 逐行解析 JSONL，取最后一条  
    `agent_message`；解析不出来的行跳过（**半行不能废掉整份输出**）。  
    取不到才退回"取尾部"（非 JSONL 的执行器仍走老路）。
  - 同时把结论作为 `detail` 送进字幕与日志 ⇒ 界面上能看见 Agent 到底说了什么  
    （"微信已启动。"），用户据此判断"到底做没做成"。

### 变更

- `spoken_result` 拆出 `spoken_body`（正文），终态播报与未读播报**共用同一份正文** ——  
  分两处算会出现"一处念 JSON、一处念结论"的自相矛盾。
- 新增 4 条单测，其中一条把**本机实跑抓到的 codex 真实 stdout** 钉成断言，  
  把"该从哪种事件里取结论"写死在测试里（不再有第二次猜错）。

### ⚠️ 未完成

- **A 方案（槽位后台 `codex exec`）是 headless 的**：不会打开任何终端窗口，  
  过程不可见 —— 这是设计如此，不是故障。想看过程需要 A″（语音打进可见终端）。
- **A″ 目前不能用**：`handle_reserved_uuid` 仍是"按键已触发，但派发链路未实现"，  
  按 S1–S4 只会打一条 warn。**按键链路未接**。
- **完成 ≠ 达成**：`turn.completed` 只表示"上游这一轮说完了"，不表示"世界变了"。  
  本次实测「打开微信」：codex 用 PowerShell `Start-Process 'weixin://'` 返回  
  `started-uri` 就自述"微信已启动"，而实际 `Weixin` 进程在跑、**主窗口标题为空**  
  （没弹出可见窗口）。要验证世界状态须给执行器填 `evidence_cmd`（世界门禁），  
  当前 codex-cli 的该项为空。

---

## [v0.22.0] — 2026-09-20

### 修复

- **⛔ Codex CLI 任务"全部做完却报失败"**（根因：完成标记取错了流）
  - 实测证据（本机实跑）：`codex.cmd exec --json --skip-git-repo-check "<prompt>"`  
    的 **stdout 全文**只有 4 行 exec JSONL ——  
    `thread.started` / `turn.started` / `item.completed` / **`turn.completed`**。  
    **`task_complete` 从头到尾不在 stdout 上** —— 它是 app-server 事件，只落在  
    `~/.codex/sessions/**/rollout-*.jsonl`（C 方案扫的那个）。
  - 于是预置里那条 `pattern = "task_complete"` **永远命中不了**：进程 exit=0、Agent 也真的  
    回答了，任务台却统一报"失败，输出里没出现完成标记"。用户日志与之完全吻合  
    （`stdout_len=2456`、同时 rollout 里有 `task_complete` 且带 `last_agent_message`）。
  - 判据改为 `turn.completed;task_complete`（多候选，任一命中）。
- **⛔ 旧值不会因为改了预置而自动变**（`apply_preset_patches`）
  - `normalize()` 刻意**不动已存在的条目**（否则用户在设置页改的东西会被每次启动覆盖），  
    ⇒ 预置里的错值会跟着用户的 `agent_desk.json` 一直错下去。已确认用户磁盘上就是  
    `"pattern": "task_complete"`，**只改代码默认值救不了**。
  - 新增一层**纠错表**（`(driver id, 已知错值, 正确值)`）：只有磁盘值**正好等于已知错值**  
    才改，改完立刻写回并记一条日志（"已修正预置执行器的过时配置：codex-cli"）。  
    **用户自己设的任何值一律保留** —— 这是纠错，不是把预置重新灌给用户。
  - ⚠️ 表里每加一条，都必须有实测结论支撑，并在注释里写清错在哪。
- **runner 给子进程显式关掉 stdin**（`Stdio::null()`）
  - 此前不设 ⇒ 继承宿主的 stdin。不少 CLI（codex 就在 stderr 打  
    `Reading additional input from stdin...`）发现 stdin 可读会**真的等输入**，  
    一路挂到超时为止 —— 本次诊断时复现过：同一条命令换个宿主方式就从 17 秒变成永不返回。

### 新增

- **完成标记支持多候选串**：`pattern` 可用 `;` 分隔多个串，**任一命中即完成**  
  （不含分隔符就是单串，与旧配置完全兼容）。  
  上游工具的日志格式会随版本变；绑死一个串 = 上游一改名，全部任务集体判失败。
- **判定失败时必须带证据**（`driver::tail`）：失败原因里附**退出码 + stdout 字数 + 输出尾部 600 字**，  
  并同步记一条 warn 日志。
  - 起因：这次的报错只说"没出现某某标记"，光看这句**永远看不出该改成哪个串**，  
    非得把输出抓出来数一遍。决定任务终态的事件几乎总在输出末尾 ⇒ 取**尾部**  
    （既有 `truncate` 取头部，故另加 `tail`，两者用途不同，不合并）。

### ⚠️ 未完成

- **`codex exec` 完成后没取回 `last_agent_message`**：它在 rollout 文件的  
  `task_complete` 事件里，stdout 上只有 `agent_message` 的 text。  
  目前任务播报仍只有"成功了/失败了"，没有结果摘要 —— 要做就得把 A 方案也能像 C 探针那样  
  按启动时刻去关联 rollout。**未做**。
- **stdout 不适用于别的执行器**：本次只实测了 codex-cli 这一个；其余 CLI 执行器的  
  `log_pattern` 判据**未经实测验证**（多数用的是 `process_exit_zero`，风险较低）。
- 仍未 GUI 实测：本修复的第一次真实任务就是下一次运行；建议核对日志里是否出现  
  「已修正预置执行器的过时配置」与「任务完成」。

---

## [v0.21.0] — 2026-09-20

### 新增

- **`PasteStyle` 增加 `Ctrl+Shift+V`**（`text_delivery.rs`）
  - 起因（本机实测 09-20）：不同终端的粘贴键**并不统一** ——  
    `Git Bash`（mintty）用 `Shift+Insert` ✓，而 `Windows Terminal` 用  
    **`Ctrl+Shift+V`**（或鼠标右键），`Shift+Insert` 在那里不保证可用。  
    v0.20.0 的"终端 ⇒ 一律 Shift+Insert"因此是**错的**。
  - 判定表从"终端名单"改成**类名 → 键位的映射表**（`TERMINAL_PASTE_KEYS`）：  
    `CASCADIA_HOSTING_WINDOW_CLASS → CtrlShiftV`、`mintty → ShiftInsert`。  
    ⚠️ 若将来把它退化回"所有终端同一组键"，Windows Terminal 会重新粘不进去。
  - **未实测的终端（conhost / ConEmu）不进表**，拿不到实测结论就回落 `Ctrl+V`  
    （改造前的既有行为）—— **不猜**。猜错的表现是"换了个终端就粘不进去"。
  - 三种组合 + 裸回车**共用同一份按键序列**（`send_combo(&[修饰键], 键)`，  
    抬修饰键逆序）。分几份写会出现"某一组偶发丢键"这种只在特定窗口复现的问题。
  - 设置页「粘贴方式」下拉新增 `Ctrl+Shift+V` 一项；`Auto` 仍是默认（推荐）。

### 变更

- **「粘贴后自动回车」默认改为开**（`DEFAULT_SUBMIT_AFTER_PASTE = true`）
  - 起因（实测）：TUI 里粘进去**只落地到输入框、不会自动提交**，不回车的话  
    "说完一段话"这条链就没闭合。原先"默认关"的理由（怕误提交没改完的提示）  
    在终端场景不成立 —— 那里粘进去的就是刚说完的整段话，提交正是预期动作。
  - ⚠️ **默认值变更必须显式迁移**，否则老用户看不到新默认：他们的 `config.json`  
    里写着旧默认值写盘留下的 `false`，而"磁盘里有值" ≠ "用户选过"。  
    新增世代标记 `injection_defaults_version`（当前 = 2）：  
    标记不是当前世代 ⇒ 忽略磁盘值、套用新默认并记一条日志；  
    用户手动改过一次之后即以用户的选择为准。  
    **今后每次改文本注入的默认值，都要把这个常量 +1。**
- 设置页开关初值改为 `true`，与后端默认一致（避免"先显示关、再跳成开"像是被改了两次）。

### 修复

- **Windows Terminal 里粘贴失败**（v0.20.0 引入的实测回归）：`Auto` 判为终端后一律发  
  `Shift+Insert`，而 Windows Terminal 实测要 `Ctrl+Shift+V`。
- 设置页/后端报错文案补上 `ctrl_shift_v`（此前漏列新值，用户照报错抄会抄不到）。

### ⚠️ 未完成

- **仅在 Windows Terminal 与 Git Bash 上实测过**；conhost（cmd / PowerShell 窗口）、  
  ConEmu、WezTerm、Alacritty 等**未实测**，当前一律回落 `Ctrl+V`（可能有 TUI 粘不进去的问题）。  
  要用就先实测，再把「类名 → 实测可用的键」写进 `TERMINAL_PASTE_KEYS`。
- 语音打进终端这条链**仍无端到端自动化验证**（需要真实焦点窗口 + 真实 TUI 进程）：  
  本次改动只钉住了纯逻辑（类名解析 / 键位映射 / 默认值），实际按键效果靠人工实测。

---

## [v0.20.0] — 2026-09-20

### 新增

- **A″ · 文本注入支持 `Shift+Insert`**（`text_delivery.rs` 新增 `PasteStyle{Auto,CtrlV,ShiftInsert}`）
  - 起因：`Ctrl+V` 在终端里的 **TUI 程序（codex / vim / htop）中粘不进去** —— TUI 把终端置于  
    raw mode 直接读按键字节，`Ctrl+V` 在那里是字面量 `0x16`，会被 TUI 当成自己的快捷键吃掉。  
    终端级粘贴由**终端模拟器**处理，惯例是 `Shift+Insert`（Windows Terminal、Git Bash 均支持）。
  - `Auto`（默认）按**焦点窗口类名**判定终端：`CASCADIA_HOSTING_WINDOW_CLASS` /  
    `mintty` / `ConsoleWindowClass` / `VirtualConsoleClass` ⇒ 走 `Shift+Insert`，其余走 `Ctrl+V`。  
    判定失败的兜底一律是 `Ctrl+V`（**= 改造前的既有行为**，最坏情况退化，不会变成新的坏行为）。
  - **两种粘贴方式共用同一份按键序列逻辑**（`send_combo`）。分成两份写，将来只改一处时  
    会变成"Ctrl+V 正常、Shift+Insert 偶发丢键"这种只在特定窗口复现的问题。
  - 新增 `config.json` 的 `paste_style` 与 `submit_after_paste`：设置页「文本注入」段可改。  
    真源只放一份（`text_delivery` 的进程内全局），`SessionInner` 不再另存，避免两份状态漂移。
  - `submit_after_paste`（粘贴后回车）**默认关**：误回车会提交一个还没改完的提示，  
    代价高于多按一次。
- **C · 外部会话监工**（新增 `agent_desk/codex_probe.rs`，11 条单测）
  - 场景：用户**自己开终端跑 codex**，任务台不参与执行，只在完成时告诉他一声 ——  
    补的正是"用户走开的那些时刻"这个缺口。
  - 增量扫描 `~/.codex/sessions/**/rollout-*.jsonl`，识别 `payload.type == "task_complete"`  
    并提取 `last_agent_message`。
  - **挂在既有的 5 秒灯语心跳上**，不另起线程（两个循环做同一件事时，  
    "只改了其中一个的间隔"是最容易发生的漂移）。
  - 配置 `DeskConfig.probe{enabled,root}`，`root` 留空 = `%USERPROFILE%\.codex\sessions`。  
    默认**开**：没装 codex 时根目录不存在 ⇒ 零副作用。

### 变更

- **`NoticeEvent` 新增 `detail` 字段**（可选）：C 方案的会话摘要。**只上屏、不进语音** ——  
  语音播的仍是固定语料"任务已完成"，动态文本要现调 TTS，那会毁掉"预合成一次、断网也能播"。
- **C 用独立的去抖账本**（`DeskManager::probe_gate`），不与 `notice_gate` 共用。  
  共用一本账会让"槽位任务刚播过 Completed"把"外部会话也完成了"判成"与上一条相同 ⇒ 沉默"，  
  于是**一条真实的结果被无声吞掉**。

### 修复

- 无。

### ⚠️ 未完成 / 未验证

- **A″ 与 C 均未在本机实测**（无 GUI 手点、无真机）。已验证的只有：  
  `cargo test --lib` 相关用例全绿、`cargo check --lib` 零新增警告、`npx tsc --noEmit` 与  
  `npx vite build` 通过。
- **A″ 有两点必须钦友实测，此处不替他宣称**：  
  ① `Shift+Insert` 能否真的把文本粘进 Windows Terminal / Git Bash 里的 codex TUI；  
  ② 粘进去后是否需要再按回车才提交（需要的话打开「粘贴后自动回车」）。
- **C 未验证**：首轮静默（重启不连播历史）只有单测证据；扫描命中后的实际播报链路未手点过。
- C **刻意不产生未读**：结果就在终端里、永久驻留，再点一颗"有结果没听"的灯属于重复记账。  
  若后续希望"走开很久回来也能发现有结果"，应当走**桌面通知**（系统通知中心可翻看），  
  而不是把它塞进槽位未读。

---

## [v0.19.0] — 2026-09-20

### 新增

- **任务台播报配置第一次真正生效。** `NoticeConfig` 此前是**死配置** —— 在  
  `agent_desk/` 下零引用，代码用的是硬编码「8 秒间隔 + 1/5 分钟档」。现在引入  
  `NoticePolicy`（`notice.rs`），配置的每一项都落到 `NoticeGate` 上：
  - 新增 `on_progress` 开关。进度是最吵的一类，**可以单独关掉，而不必为了安静一点  
    连"任务已完成"一起关**。
  - `enabled` / `on_start` / `on_complete` / `on_fail` 四组开关按 `match` **穷尽**分组  
    —— 将来加 `NoticeKind` 变体却忘了归组会**编译不过**，而不是让那条播报静默消失。
  - `min_interval_secs` / `progress_marks_secs` 真正生效。档位表做归一化：过滤 0 →  
    升序 → 去重 → 按语料闭集容量截断（多写的在日志里明说，不静默）。
- **新增第 3 档进度语料 `NoticeKind::Running15Min`（"已运行十五分钟"）**，让配置里  
  `[60, 300, 900]` 三档全部可表达。刻意**追加在 `code()` 末尾**而不是插在  
  `Running5Min` 之后：`code()` 同时是缓存文件名的一部分，插在中间会把磁盘上已有  
  缓存**整体错位**（读到另一句音频，且不报错）。
- **`DriverConfig.timeout_ms`——单次执行超时终于可配**（0 / 留空 = 用默认 180 秒）。  
  出厂预置的 CLI 驱动给 15 分钟。
- **按键重映射页内直接保存。** `RemapEditor` 顶部新增保存栏，复用「方案」页**同一个  
  handler**（同一条写盘路径，不存在"两处保存行为不一致"）。不再需要为了保存一个  
  刚改的键而跳到另一页。
- 设置页新增：「播报哪几类」四个开关、「进度档位（分钟）」可编辑（3 格 + 容量说明）、  
  「单次执行超时（秒）」输入框（带"没有不超时这个选项"的说明）。

### 变更

- **Codex CLI 预置换成高置信度完成判据。** 参数加 `--json` / `--skip-git-repo-check`，  
  完成判定由 `process_exit_zero`（未校验）改为 `log_pattern = task_complete`（已校验）。  
  原因：`task_complete` 是 codex 的 **JSONL 事件名**，只有 `--json` 才会把它打到 stdout  
  （实测确认源码事件为 `{"type":"event_msg","payload":{"type":"task_complete",…}}`）。
- **API 驱动的超时改读 `driver.timeout_ms`。** 此前 `driver_default_timeout_secs`  
  直接 `_driver` 忽略配置、恒返回全局默认，而 `driver_timeout_secs` 取  
  `min(req, cfg)` ⇒ **用户配多长时间都会被压回 180 秒**，且完全不报错。
- `notice.rs` 模块头修正一处过时描述（写着"PC 侧是前端 Web Audio"；实际播放早已由  
  后端 `audio_out` 用 cpal 直接写系统输出设备）。

### 修复

- **`NoticeConfig` 是死配置** —— 设置页里那一整段播报项从来不生效（v0.18.0 的欠账）。
- **驱动超时不可配，且 API 驱动会覆盖它** —— `DEFAULT_TIMEOUT_MS = 180_000` 是硬编码，  
  拿它卡 CLI Agent 会让**每个真实任务都在 3 分钟处被判超时**，而那一刻恰好落在"它干得  
  好好的"区间里。
- **修正两处与实现不符的注释**（`mod.rs` 的"任务跑完不会自动播报"、`notice.rs` 的  
  "PC 侧是前端 Web Audio"）。这类注释的危害不是不好看，而是**会让下一个 agent  
  照着错的模型去排查**。

### ⚠️ 未完成

- **板子喇叭路未做**（钦友已定：只做 PC 喇叭）。
- **两个 ⛔ Gate 仍只在代码级成立**：3.5「播到一半中断 ⇒ 未读保留」、3.6「Notice 不污染  
  未读」，**都没在 GUI 手点验证过**。
- **按键链路仍未接**：`handle_reserved_uuid` 只报"未实现"，按 S1–S4 说话进不了队列。
- **预置驱动不会迁移**：`DeskConfig::normalize()` 只补"从未出现过的"预置 driver，  
  磁盘上**已存在**的旧驱动值不会随代码更新。本次 codex 预置的改动是**手工同步**到  
  本机 `%APPDATA%\com.easyop.app\agent_desk.json` 的。
- **本次新增的 UI 与两条配置路径均未在 GUI 手点过**：重映射页保存栏、播报分类开关、  
  进度档位编辑、超时输入框 —— 都只有类型检查与构建证据。

### 验证

- `cargo test --lib agent_desk::` → **190 passed / 0 failed**（原 180；新增 10 条：  
  分类开关真的静音、总开关静音全部、静音时**不记账**以便开关打开后还能播、  
  配置的间隔真的生效、配置的档位真的搬动梯子、策略变更不清空去抖账本、  
  档位表归一化（乱序/含 0/超长）、容量与语料闭集对齐、档位数随配置表长短变化、  
  出厂默认未被悄悄改掉）。
- `npx tsc --noEmit` exit 0；`npx vite build` 通过。
- `cargo check --lib`：`agent_desk/*` **零警告**（全库 50 条全在既有位置，  
  分布与改动前逐项一致：pinyin 25 / capability 16 / ei_protocol 3 / runner 2 /  
  plugin_registry 2 / llm_client 1 / everything 1）。

---

## [v0.18.2] — 2026-09-20

> 修一条**让任务台完全无法执行任何任务**的缺陷。症状是"派发后任务一直卡在执行中，  
> 没有任何反馈、没有窗口弹出、也不会超时"。

### 修复

- **★ 执行泵把 `StartOutcome::Busy` 误当成"队列空了"，导致任务永久卡死**（`agent_desk/mod.rs::pump_slot`）。
  - **根因**：`state::dispatch_into` 是"入队 + 启动"的**复合操作** —— 它在**派发那一刻**  
    就把任务放进运行位。而 `pump_slot` 循环第一步调 `start_next` 时，运行位已被占，  
    于是拿到 `Busy`；旧代码写的是 `Ok(_) => None`，把 `Busy` 和 `Idle` 一并当成  
    "没事可做"⇒ **立即 `break`，泵退出，任务从未被执行**。
  - **后果是完全静默的**：任务永久停在"正在执行"，没有子进程、没有终态、没有未读、  
    没有播报，**连超时都不会触发**（超时在 `runner` 内部，而 `runner` 从未被调用）。  
    只有"刚派发即卡住"这一个可观察现象。
  - **修法**：新增 `state::claim_current()` —— 执行侧的**认领**语义：  
    `Started` → 用它；`Busy` → **认领当前运行位上的那一条**；`Idle` → 才是真没事可做。  
    把它放在**纯数据层**（`state.rs`）而不是泵里，是为了能被单测钉死 ——  
    `pump_slot` / `ensure_pump` 需要 `tauri::AppHandle`，此前**零测试覆盖**。
  - 新增 `queue::running_task()`：只读查询"当前占用运行位的任务"。
  - 新增 **防御**：泵记住上一条已处理过的任务号；若再次认领到同一条，说明 `complete_task`  
    没能清出运行位，此时**停机并报错**而不是重跑 —— 否则会变成"同一条任务被无限重复执行"  
    的热循环（真的会反复拉起 Agent，既烧钱又刷屏）。
  - **新增 5 条回归测试**（`state.rs`）：`claim_after_dispatch_takes_the_task_dispatch_already_marked_running`、  
    `claim_on_idle_slot_starts_the_next_pending_one`、`claim_on_empty_slot_reports_nothing_to_do`、  
    `claim_never_reports_busy_as_nothing_to_do`、`claim_propagates_slot_range_errors`。  
    `cargo test --lib agent_desk::` → **180 passed / 0 failed**（此前 175，其中没有任何一条  
    覆盖"派发之后泵能不能真取到任务"）。
  - ⚠️ **修正一处此前的错误记录**：Phase 2 的"GUI 手点已验证通过"**不成立** ——  
    当时验证的是「手动取租约 / 回执 / 释放」这条账本链路，**派发执行链路从未跑通过**。  
    这也说明"页面看起来在正常工作"不能当作执行链路的证据。
- **预置驱动 `codex-cli` 有两处必然失败的配置**（`agent_desk/config.rs` 预置 + 本机既有配置）：
  1. `command: "codex"` → `"codex.cmd"`。**Windows 上 CreateProcess 在程序名没有扩展名时  
     只会自动补 `.exe`**（这与 PATHEXT 无关，那是 cmd.exe 的行为），而 npm 全局安装留下的是  
     `codex.cmd` / `codex.ps1` ⇒ 裸名**必定启动失败**。本机实测：`codex` 找不到、  
     `codex.cmd` 正常返回 `codex-cli 0.152.0`。已把这条坑写进预置注释（`bailian-cli` 早有同类提醒，  
     但 `codex-cli` 漏了）。
  2. 完成信号 `log_pattern: "task_complete"` → `process_exit_zero`。`task_complete` 是 codex 写在  
     `~/.codex/sessions/**/rollout-*.jsonl` 里的**事件名**，**不出现在 stdout** 上  
     （本机实测：今天 0 个 rollout 文件，历史 37 个文件命中该字符串）⇒ 用 `log_pattern` 匹配 stdout  
     **永远匹配不上**，**跑得再成功也会被判失败**。改用进程退出码后，置信度如实标为「未校验」；  
     要真校验请配「世界门禁」（`evidence_cmd`）。

### ⚠️ 未完成 / 已知缺口（本版**未**修，按优先级）

1. **`NoticeConfig` 是死配置** —— `agent_desk.json` 里的 `notice` 段  
   （`enabled` / `on_start` / `on_complete` / `on_fail` / `min_interval_secs` / `progress_marks_secs`）  
   **在 `agent_desk/` 下零引用**：`notice.rs` 用的是硬编码常量（8 秒间隔、只有 1min/5min 两档）。  
   ⇒ **在设置里改这些项目前没有任何效果**。这是"配置写了却不读"的静默失效，属 v0.18.0 的欠账。
2. **超时不可配**：`DriverConfig` **没有超时字段**，实际生效的是硬编码  
   `DEFAULT_TIMEOUT_MS = 180_000`（3 分钟）。对真实 Agent 任务偏短。  
   加字段本身是纯增量（`DriverConfig` 带 `#[serde(default)]`），但**必须同时补前端设置界面** ——  
   否则前端回存配置时会静默把它重置回默认值（正是本项目明令禁止的"配置被静默清掉"）。
3. **`runner.rs` 未抑制控制台窗口**：起子进程时没有 `CREATE_NO_WINDOW` ⇒  
   **任务真正开始执行时会闪出一个控制台窗口**。此前"没有窗口弹出"恰恰是"子进程从未创建"的旁证。
4. **`log_buffer` 只在内存、不落盘** ⇒ 关掉 easyop 后日志全失，事故无法事后复盘。

---

## [v0.18.1] — 2026-09-20

### 新增

- **《easyop 插件系统设计文档》**（`docs/easyop 插件系统设计文档.md`）：合并原 5 份有效文档  
  （扩展机制规范 / 插件集成指南 / 内容台规范 / 路径A 接口契约 / ARCHITECTURE）为一份  
  综合设计文档，涵盖系统总体架构、插件接口定义、生命周期管理、通信与依赖、安全模型、  
  内容台规范、调试与错误处理、已知问题与优化方向。已更新为反映 v0.17.0 状态  
  （`registerPage` 已实现、`pages` 不再丢弃、侧栏页签 + ⌘SS 双入口并存）。

### 变更

- **删除过时文档**：`docs/ss-palette-deep-dive.md`（P0-P2.5 全部完成，仅 P3 未做，不在 AGENTS.md  
  文件地图中）、`docs/ROADMAP.md`（引用 v0.7/v0.8/v0.9 里程碑但当前已 v0.18.x，"内容台集成"  
  列为远期但已实现）。3 份调研类文档（灵感来源 / 桌面宠物 / 企业助手方案v3）遵循 AGENTS.md  
  "调研类，仅追加"公约保留不动。对既有配置无影响。

### 修复

- （无）

---

## [v0.18.0] — 2026-09-20

> 主任务：Agent 任务台 Phase 3 第二段「后声」（方案文档 §788–799 的 3.4–3.8）。  
> ⚠️ 本版**不含板子喇叭路**（固件无"播任意 PCM"通道，规模评估见方案文档 Phase 3 §七）。

### 新增

- **状态播报语义层 `agent_desk/notice.rs`**：把「播报」与「未读」**彻底分成两条通道**。  
  `Notice(状态播报)` 不产生未读、不进账本、不 `grant`；只有 `Result(任务终态)` 才产生未读。  
  **为什么必须分开**：5 颗灯是多路、零成本、不打断的**状态信道**；1 个喇叭是单路、有成本、  
  会打断的**事件信道**。状态变化的正确信道是灯，不是喇叭。少了这条 ⇒ 用户看到未读灯亮、  
  按下去只听到"正在推进" ⇒ **灯失去可信度**，而"灯可信"是整台设备的根。
  - 10 条固定语料（`NoticeKind`），`#[serde(rename_all = "snake_case")]`；`code()` 同时是  
    缓存文件名的一部分 ⇒ **只能末尾追加、不能重排**。
  - **两条驱动源刻意分开、收敛到同一个 `emit_notice`**（去抖账本只一份）：  
    **事件驱动** = 终态 / 起点（"刚刚完成了"这个事实在状态快照里根本看不到）；  
    **状态驱动** = 进度档位（`RuntimeSnapshot` → `DeskPulse`）。
  - **去抖两条规则刻意不同**：① 跃迁才播（同 `kind` 一律沉默）；② `NOTICE_MIN_INTERVAL_MS`  
    = 8000ms 的间隔**只管进度类**（`Progressing / Running1Min / Running5Min`），  
    起点与全部终态**不受约束** —— 把"任务已完成"因为"离上条太近"而吞掉，是这套系统最不能犯的错。
  - **被拦下时不更新 `last_kind`**（下次轮询还会再试）；**重复送达不刷新 `last_spoken_at_ms`**  
    （否则持续轮询会把时间戳一直往前推 ⇒ 下一条真通知永远等不到间隔，这正是  
    "连续 10 次轮询只产生 ≤1 次播报"能成立的原因）。
  - **进度只做固定档位**（1 / 5 分钟），不做连续值 —— 固定语料是有限闭集，连续值会让语料集  
    无界，从而毁掉"预合成一次、长期复用、可离线"这个前提。取**最早开始**的任务驱动进度。
- **语料预合成缓存 `agent_desk/notice_assets.rs`**：离线可用性的硬前提，不是优化 ——  
  "任务失联"这条告警恰恰出现在网络出问题的时候，现调 TTS 等于"最重要的通知在最需要它的  
  时候发不出来"。
  - 判有效用 **`MIN_VALID_WAV_BYTES = 46`**：只看"文件存在"会把中断写入的 0 字节残骸当成  
    可用缓存 ⇒ 播报无声且极难排查。
  - **先写 `.tmp` 再 rename**（原子），否则写到一半被打断会留下半个恰好越过阈值的文件。
- **PC 侧播放载体 `agent_desk/audio_out.rs`**（cpal）：`PlaybackControl::completed_fully()`  
  = `is_finished() && !stop_requested()`，这是**消费未读的唯一凭据** ⇒ "播到一半拔电未读保留"  
  自动成立（未读账本落盘、租约刻意不落盘 ⇒ 重启后没有悬空消费）。
  - 三种采样格式（F32 / I16 / U16）**共用同一份泛型游标逻辑** `fill_channels<T>` ——  
    避免"i16 机器正常、f32 机器停不下来"这种只在特定声卡复现的问题。
  - 被中断则**静音且不推进游标**（否则"播了多少"变成假账）；硬上限 = 时长 + 10s  
    （设备被拔掉时声卡回调可能再也不被调用）。
  - **⚠️ 清场不调 `control.stop()`** —— 那会污染 `completed_fully()`；真正需要打断的场景已在  
    `take_channel` 里停过。
- **4 条 Tauri 命令**：`desk_notice_status` / `desk_prepare_notices` / `desk_play_notice` /  
  `desk_render_result`。
  - `desk_prepare_notices`：单条失败不中断、**逐条汇报**（10 句里 9 句成功就该留下那 9 句）。
  - `desk_render_result`：取租约 → 合成 → 播放 → **只有 `completed_fully()` 才 `consume`**；  
    未播完则 `release` 保留未读；内容缺失时明确报"内容已随重启丢失，未读计数仍保留"。
  - **播放在后端（Rust / cpal），不在前端**：不是实现偏好，是**承诺的可靠性** ——  
    "听完才消费未读"的判定点放 webview 里，刷页面 / 渲染进程崩溃都会让承诺失效；  
    进程内播放则"播完了"是**本地事实**。这也让板子路（固件喇叭）将来可平替。
- **前端播报区**（`DeskView.tsx`）：语料缓存状态 + 逐条试听 + 「播报未读」+ 播报字幕条。  
  字幕条同时是**第一诊断入口**：没听到声音时，先看这里是「没播」还是「播了但没出声」  
  （`audio_path` 为空即"语料未预合成"）。
- 新事件 `agent-desk-notice`（payload 含 `slot / kind / text / terminal / audio_path`）。

### 变更

- `agent_desk/mod.rs`：`DeskManager` 新增 `notice_gate` / `pending_results` / `playing` 三个字段；  
  `emit_state_if_changed` 里增加状态驱动的进度播报；`pump_slot` 增加终态播报与结果文本留存；  
  `dispatch_task` 在**真的开始执行**（非排队）时才发 `TaskStarted`（排队那一刻这句话是假的）。
- `agent_desk/mod.rs` 新增 `SlotResult` / `NoticeEvent` / `PlayingState` 与  
  `CHANNEL_ID: AtomicU64`（单调递增）+ `clear_channel_if_mine`：不做这个判断会出现  
  "A 播完清场 → 但 B 刚接管 → A 把 B 清了 ⇒ 喇叭被永久标记占用中"。
- `lib.rs` 注册表新增上述 4 条命令。
- `types.ts` / `api.ts`：补播报相关类型与 4 个封装。
- `DeskView.tsx`：「播报未读」用**独立于派发槽位**的选择器（两件事没理由绑在同一个选择器上）。

### 修复

- **`plugin_registry.rs` 编译阻塞**（非本版功能，属路径 A 的未完成尾巴）：`Contributes` 加了  
  `pages: Vec<PageDecl>` 后，两处测试构造未补该字段 ⇒ `cargo test --lib` 整个编译不过。  
  已补 `pages: vec![],`，语义零影响。
- **`styles.css` 死引用**：`.desk-slot-key` 用了从未定义的 `var(--font-mono)`（全项目仅此一处），  
  已改为既有的 `var(--mono)`。

### ⚠️ 未完成

- **板子喇叭路未做** —— 固件**没有**"播一段 PC 来的实时 PCM"的通道  
  （`SpeakerOutput::RequestKind` 只有 `None / Diagnostic / Asset`；产品路径只有  
  `request_asset`（已落 Flash 的音库资源）与 `request_embedded_asset`（编译进镜像的常量资源））。  
  这与方案文档 §599 的 **G2 结论完全一致**（"动态摘要必然落 Flash"）。要走这条路需要：  
  ① PC 侧 EIAD 编解码器（Symphonia 0.5.5 + Rubato 0.15.0 的等价实现，**目前零实现**）；  
  ② 音库下发客户端（`0x14`/`0x15` 的 opcode 序列 + session cookie + 640B plan 重组器 +  
  4KiB block + SHA256 原子提交，**目前零实现**）；③ 固件新增播放入口（属协议扩展）。  
  **规模是已交付内容的数倍，且需硬件。**
- **`desk_render_result` / `desk_play_notice` / `desk_prepare_notices` 无单测** —— 需  
  `tauri::State` + 真实 TTS + 声卡，属集成层。
- **未在 GUI 手点验证**：PC 侧播报与"半途中断保留未读"目前只有类型检查 + 编译证据。
- **按键链路仍未接**：`handle_reserved_uuid` 只报"未实现"。

---

## [v0.17.0] — 2026-09-20

### 新增

- **插件页签常驻 tab**：插件 `plugin.json` 声明的 `contributes.pages` 不再被 Rust 静默丢弃；启动时 `plugin_pages` Tauri command 返回页签列表，前端侧栏动态追加导航项（如「内容台」）
- **`registerPage` 宿主能力**：`pluginHost.ts` context 新增 `registerPage(pageId, renderFn)`，插件侧 `typeof === 'function'` 判断已就位，本次补上宿主侧实现
- **`PluginPageContainer.tsx`**：新建组件，激活插件 → 调 `renderPage(container)` → 挂载插件 DOM；unmount 时 `lifecycle.dispose()` 摘干净
- **侧栏宽度调整**：220px → 176px，padding 14px → 10px

### 变更

- `plugin_registry.rs`：`Contributes` 加 `pages: Vec<PageDecl>` 字段，新增 `all_pages()` 函数
- `Sidebar.tsx`：`Tab` 类型改为 `string`，新增 `pluginPages` prop，内置导航与插件页签间加分隔线
- `App.tsx`：启动时拉取 `plugin_pages` → 传入 Sidebar → 主区按 tab 渲染 `PluginPageContainer`
- `pluginHost.ts`：激活后跳过 `__page__` sentinel 的 execCommand；新增 `pageRenderers` / `pendingActivations` Map
- ⌘SS 里的 `studio.toggle` 保留为快捷入口（双入口并存）

### ⚠️ 未完成

- 页签模式**未在真机验证**：需 `npm run tauri dev` 启动后侧栏点「内容台」确认插件 DOM 正确挂载

---

## [v0.16.0] — 2026-09-20

### 新增

- **Agent 任务台 · 设备灯语通道（Phase 3 第一段「先灯」）**：新增 `0x16` 逐像素帧，把运行时  
  快照映射成 5 颗 WS2812 的颜色下发开发板 —— 第 1–4 颗显示各槽未读数（绿色分 3 档亮度；  
  正在播的槽显示**白色**，与"有未读"一眼可分），第 5 颗显示正在执行的**槽数**  
  （0–4 → 绿/黄/橙/紫/红）。  
  **为什么新增而不是复用固件既有的 `0x12`**（`agent_status.h`）：`0x12` 的渲染是  
  `render_agent_status()` 用一个 `state` 查表得到**一个**颜色、再按到中心的距离衰减铺满 5 颗，  
  即"整条灯带一个状态"，装不下任务台要的**五路独立**信息。扩 `0x12` 的 payload 等于  
  **修改**一条已在用的协议（固件 `AGENTS.md` 明令禁止），故走纯增量。  
  帧头偏移**刻意与 `0x12` 完全一致**（`seq` 在 4、`ttl` 在 8），固件侧可直接复用  
  `read_agent_status_u32_le()` 与 `agent_status_sequence_is_newer()`。
- `transport::send_feature_report()`：通用 Feature Report 下发入口。与既有的"配置分块"  
  (`chunk_reports`) 分开 —— 那个带 Magic/CRC/分块头，这个只负责"把字节送到支持 Feature Report  
  的集合上"，协议含义由调用方负责。
- **灯语心跳泵**：每 5 秒续期一次。帧自带 15 秒 `ttl_ms`，所以**上位机崩溃/退出后灯会自动  
  回落到固件既有状态机**，不会留下一个永远亮着的"幽灵未读灯"（与未读账本的"拔电保留未读"  
  是两条独立的正确性）。

### 变更

- `constants.rs` 补 `REPORT_ID_DESK_LED = 0x16`，并把固件已占用的 `0x10`–`0x15` 登记成  
  **注释表**（只写注释、不定义没人用的常量 —— 定义死常量只会变成一批 dead code 警告，  
  对下一个人是噪音而非文档）。
- 灯语**只在"有未读 或 有任务在跑"时点亮**；全空闲时发一帧 clear，把灯让回固件既有的  
  idle 灯效。  
  ⚠️ **与方案文档原案不同（有意为之）**：放弃了"运行数为 0 时第 5 颗常亮绿"。那会永久占用  
  5 颗灯去表达"什么都没发生"，且覆盖用户已经习惯的 idle 灯效。第 5 颗的"0 → 绿"仍会在  
  **有未读但没在跑**时出现 —— 那才是它真正有信息量的时刻。
- **不影响任何既有配置**：`0x16` 是新增 Report ID，既有 `0x10` / `0x11` / `0x12` 的语义一字未改。

### 修复

- 无（本版为纯新增）。

### ⚠️ 未完成（别误以为能用）

- **固件侧尚未改动**：`0x16` 的解析与渲染需要固件配合，改动清单见  
  `Agent任务台-落地方案与实施清单.md` Phase 3 §六（含可照抄的代码骨架与验收方式）。  
  **在固件支持之前，灯语下发会失败** —— 按"失败不静默"，日志里会留一条 warn  
  （同类失败后续静默，避免每 5 秒刷屏）。
- **未在真机验证灯语**：单测只证明了"算出来的颜色对"，没证明"板上真的亮成那样"。
- **按键链路仍未接**：按 S1–S4 说话进不了队列（`handle_reserved_uuid` 只报"未实现"）。
- **音频下行（3.4–3.8）未开始**：`device_audio.rs` / `notice.rs` 仍是空位。

---

## [v0.15.0] — 2026-09-20

> **主题**：Agent 任务台 **Phase 2 —— 宿主侧编排层**落地（队列 / 未读 / 执行器 / 状态机）。  
> 这六项（2.2–2.7）**全程无硬件依赖、纯单测驱动**，因此可以在等板子之前先把逻辑做扎实。  
> 整体仍是**纯增量**：不动固件协议、不动既有分流、不动既有配置格式。

### 新增

- **槽位队列 `agent_desk/queue.rs`**：每槽一条 FIFO，**同槽严格串行、跨槽可并行**
  - 这不是策略而是**物理约束**：一个槽位 = 一个按键 + 一颗灯 + 一段播放，三样都是单份的。  
    同槽并行会出现"两段总结只能听一段"（幽灵未读）与"两个任务抢一颗灯"
  - 每槽上限 `MAX_DEPTH_PER_SLOT = 8`，**到顶明确报错，绝不静默丢弃**
  - **去重只在 pending 层**：排队中同文本拒绝（几乎必然是同一按键重复触发）；  
    已在跑的再入队则允许（语义是"再跑一轮"）
  - `QueueError` 六个变体各自带 `message()`，病因分开报；`StartOutcome` 把  
    `Busy { running_id }` 与 `Idle` **分成两种状态**（用一个 `None` 表达会让调用方只能猜）
- **未读账本 `agent_desk/unread.rs`**：未读代际 + **播放租约**（`grant` / `release` / `consume` / `expire`）
  - 未读的本质是一条承诺"有结果等你听"，三条硬属性：跨进程重启存活 / 只能被"完整听完"消费 /  
    一次播放只消费一条
  - 租约 = `(slot, token)`，`token` 全局单调递增、**永不复用**。播放最长 150 秒，  
    没有租约就无法知道回执对应哪一次播放 ⇒ **迟到回执不会消费新产生的未读**
  - 喇叭是全局单通道 ⇒ 租约全局唯一，抢占必须先 `release`，而 **`release` 不消费未读**
  - `consume` 只在 token 精确匹配时扣减；`expire` 返回**槽位列表**而非数量  
    （"静默失效的租约是最难查的故障"）
  - **未读落盘（独立文件 `agent_desk_unread.json`），租约刻意不落盘**——  
    租约描述的是进程内的物理播放状态，不落盘 ⇒ "播到一半拔电，未读保留"自动成立，无需恢复逻辑
  - 也**不把未读并进主配置**：两者写入频率差几个数量级，合并会让配置文件被高频重写
- **执行器注册表 `agent_desk/driver/mod.rs`**：`validate_driver` + `DriverRegistry` + `probe()`
  - 用**枚举分派而非 `dyn trait`**：`async fn in trait` 不 dyn-safe，引入 `async_trait` 会加依赖。  
    **形态是封闭集（Api / Cli 两种），Agent 才是开放集** ⇒ 形态用枚举（编译期穷尽），  
    Agent 用配置（"加新 Agent = 加一行 JSON，不写 Rust 代码"）
  - **errors 与 warnings 的分界 = 「自相矛盾」vs「还没填完」**：Api 配 `ProcessExitZero`、  
    `LogPattern` 没填 pattern、Cli 缺 `{prompt}` 是 error；缺 key / 缺 model / 缺 HOME  
    是 warning。**缺 model 不能算 error**——预置里有几个是"等你填"的占位，  
    标成红字会让界面长期挂一片红，而人对长期红字会免疫
  - **世界门禁 `evidence_cmd` 统一施加**：非空时在 driver 自认完成后**再跑一次外部证据**，  
    只有退出码 0 才算真完成并升为 `High`；失败时**绝不标 Completed**。  
    门禁不写进各自 driver（"信号从哪来"与"要不要过门禁"是正交的两件事），  
    且**只在基础成功时才跑**（否则会掩盖真正的失败原因）
- **执行器实现**：`driver/api_driver.rs`（OpenAI 兼容 HTTP，**零改动复用 `llm_client::LlmClient`**，  
  每个 driver 一个独立客户端——共用 `DeskManager` 里那个会出现"配置串了但没有任何报错"）  
  与 `driver/cli_driver.rs`（参数模板 `{prompt}` + `runner::run_stream` 直连事件汇）。  
  **取消 / 超时优先于一切完成判定**，且"启动失败"与"跑失败"分开表述
- **状态机 `agent_desk/state.rs`**：`DeskRuntime` + `RuntimeSnapshot` + `complete_task()`
  - **终态 → 未读的规则集中在 `complete_task()` 一处**，避免分散到调用点后漏掉"失败"那一格。  
    四种终态规则**刻意不同**：成功 ✅ / **失败 ✅**（失败也必须被听到，否则用户以为它还在跑）/  
    超时 ✅ / 取消 ❌（用户自己按的）
  - 队列与未读**同一次调用一起改**——拆开会留下自相矛盾的窗口
- **编排器**（`mod.rs`）：`dispatch_task()` / `ensure_pump()` / `pump_slot()`
  - 入队与启动在**同一把锁**内（拆开会有"用户看到排队中、实际已在跑"的窗口）
  - **执行泵单飞**（`AtomicBool::compare_exchange`）：否则每次派发都 spawn 一个泵，  
    会得到"取不到任务就退出"的空转任务，且让"谁在驱动这个槽"不可追踪。  
    票据归还后**必须再检查一次**（泵运行期间可能有新任务入队）
- **8 条新命令**：`desk_runtime_state` / `desk_dispatch_task` / `desk_cancel_pending` /  
  `desk_cancel_running` / `desk_clear_pending` / `desk_grant_playback` / `desk_ack_playback` /  
  `desk_release_playback`。参数名用单字（`slot` / `task` / `token`）以消除 JS 侧 camelCase 隐形转换
- **新事件 `agent-desk-state`**：只在状态**真变化时**发（靠 `RuntimeSnapshot` 逐字段  
  `PartialEq` 做指纹，准确且便宜），不刷屏

### 变更

- **`build_snapshot()` / `desk_probe_driver()` 改用 `DriverRegistry` 作为唯一校验源**：  
  此前这里各有一份手写的检查逻辑，与 `validate_driver` 会得出不同结论（三处会漂移）
- `DeskView.tsx` 重写：**「进度」占位段换成真实运行时数据**，新增槽位队列 / 未读 / 灯语展示、  
  手动派发调试面板、以及取租约 / 回执 / 释放三个演示按钮
  - 数据源是**事件 + 轮询**双通道：后端只在真变化时发事件，因此首次进入必须靠轮询拿初始值，  
    之后事件负责即时性、轮询负责兜底（事件通道断了界面不会僵住）
- `DriverView` 新增 `schedulable` / `errors` / `warnings`（`schedulable` = 校验无错 + 已启用 +  
  配置完整，比原来的 `configured` 严格）；`SlotView` 新增 `notes`
- 能力候选改用**真实可调度数**（`registry.supporting(cap).len()`）而非 `prefer.len()`——  
  后者会显示"候选 3 个"而实际一个都跑不了

### 修复

- **校验分层把"还没填"误判成"自相矛盾"**：Api 形态缺 endpoint / model 被打成 error，  
  会让界面长期挂一片红字。降为 warning（"还没填完"归 `is_configured()` 管，不归 `validate_driver`）。  
  这一条以及下面两条都是**测试先暴露出来的**，不是事后想到的
- **能力路由的空候选判定失真**（见上）
- **`build_snapshot` / `desk_probe_driver` 双份检查漂移**（见上）

### ⚠️ 未完成

- **按键链路未接**：`handle_reserved_uuid` 目前只认领槽位按键并报"未实现"，  
  **不会派发任务**。按 S1–S4 说话现在仍然进不了队列
- **音频下行未接**：任务跑完**不会自动播报**，只会产生未读；TTS 只到"合成 → 落盘 wav"，  
  下发到键盘播放属 Phase 3；Opus 编码同样推迟到 Phase 3
- **进度出口用的是 `NullSink`**（假进度条比不显示更危险）
- **`ensure_pump` / `pump_slot` 没有单测**：它们需要 `tauri::AppHandle` 才能跑，属集成层
- **未在 GUI 手点验证**：8 条命令已全部接进 `DeskView`、`tsc` 与 `vite build` 均通过、  
  三处命令名交叉核对一致，但**没有真人点过一遍**
- **能力路由（`Capability` / `Auto`）未实现**（计划 Phase 4）：`resolve_driver` 对这两种模式  
  **明确报错并告诉用户怎么办，不回落**——回落会变成"静默走错路"

---

## [v0.14.0] — 2026-09-20

> **主题**：豆包 TTS 链路打通 + 任务台槽位绑定归位到「按键重映射」。  
> 两件事都是**纯增量**：不动固件协议、不动既有分流、不动既有配置格式。

### 新增

- **新增动作类型「任务台」**（`ActionSpec::TaskDesk(String)`，避免与 `HostAction` 混用）
  - 按键重映射的类型下拉多一项「任务台」，选中后可指定该键属于 **S1–S4** 哪个槽位。  
    新增常量 `DESK_SLOT_OPTIONS`（`"1"`–`"4"`）——**不做 0/1 基转换**，槽位号即字符串本身
  - **槽位的执行器绑定（指定执行器 / 按能力 / 自动判定）搬到了该键的面板里**  
    （新组件 `src/components/DeskSlotBinding.tsx`）。设置页不再重复维护一份槽位配置
  - 新增 4 个保留 UUID `UUID_DESK_SLOT1..4`（`…0009`–`…000c`），配套  
    `desk_slot_uuid()` / `desk_slot_of_uuid()` / `desk_slot_count()`（表为唯一真源，  
    `agent_desk::config::SLOT_COUNT` 由它推导，不再写死 4）
  - `agent_desk::handle_reserved_uuid()`：拦截槽位按键的保留 UUID 并 emit  
    `desk-slot-key` 事件。**认领时明说 `dispatched: false`（Phase 2/3 未实现），  
    不静默吞掉**——否则报文会掉进「按 UUID 开程序」分支，报一句误导性的「打开失败」
- **豆包语音合成大模型（火山引擎 v3 单向流式）作为 TTS 默认 provider**  
  （`TtsProvider::Volc`，`seed-tts-1.0`）
  - `POST /api/v3/tts/unidirectional`，HTTP chunked + **NDJSON**，逐行解 base64 拼 PCM
  - **直接向服务端要 48 kHz** ⇒ 豆包路径**零重采样**（百炼那条路仍走 24→48 kHz 转换）
  - **两种错误形状都认**：鉴权错是 HTTP 401 + `header.code`；音色与 resource 不匹配是  
    **HTTP 200 + `code=55000000`** —— 只看 HTTP 状态码会把后者当成成功
  - 缺结束哨兵（`code=20000000`）或 0 采样一律**报错**，不当成功

### 变更

- `DeskSettings.tsx` 的 TTS 段改为**按 provider 条件渲染**（豆包：`resource_id` /  
  `speaker` / `explicit_language`；百炼：`model` / `voice` / `language_type`）；  
  换 provider 时**一并切换端点与音色**，避免留下"豆包端点 + 百炼音色"这种必然失败的组合
- `DeskSettings.tsx` 的「槽位绑定」段改为一段**指向按键重映射的说明**（不再可编辑）
- `App.tsx` 新增 `desk-slot-key` 监听：把槽位号反查成具体键位点亮，与既有 `host-action`  
  高亮行为对齐；提示色调由 `dispatched` 决定（未派发 = 明确的失败，不是成功）
- `src-tauri/Cargo.toml` 新增直接依赖 `base64 = "0.22"`（依赖树里本就存在，零下载成本）

### 修复

- **豆包 `req_params.additions` 必须编码成 JSON 字符串，不是嵌套对象**
  - 根因：火山侧 Go 结构体里该字段类型是 `string`，传对象会返回  
    `json: cannot unmarshal object into Go struct field TTSReqParams.req_params.additions of type string`
  - 阴险之处：这个错误裹在 **HTTP 200 + `code=55000000`** 里返回，只看状态码会当成成功、  
    然后拿到 0 采样。而默认配置带 `explicit_language`，**必踩**
  - 已抽出纯函数 `volc_request_body()` 并加两条**离线回归测试**钉死  
    （`volc_additions_is_encoded_as_json_string` / `volc_audio_params_pin_pcm_and_target_rate`）
- `validation.ts` 补 `task_desk` 分支：此前会落进 `default` 报「未知动作类型」，  
  让一个合法动作在 UI 上被判成非法

### ⚠️ 未完成

- **任务台派发链路仍是 Phase 2/3**：按 S1–S4 说话后，槽位队列、未读租约、灯语、  
  音频下行**都未实现**。按键现在能被识别并高亮，但 `dispatched` 恒为 `false`
- TTS 只到「合成 → 落盘 wav」；**下发到键盘播放未接**（固件侧 G2 结论：动态摘要必然落 Flash）
- 未在 GUI 里手点验证：按键重映射 →「任务台」→ 槽位下拉 → 执行器绑定 → 保存 → 下发的全链路
- Opus 编码仍推迟到 Phase 3

---

## [v0.13.0] — 2026-09-19

> **主题**：Agent 任务台落地（Phase 1）—— 新增 TTS 通用能力 + 任务台配置层 + 界面。  
> 方案文档：`../Agent任务台-落地方案与实施清单.md`（v3）。

### 新增

- **TTS 通用能力**（`src-tauri/src/tts.rs`，Capability Core 新增 `CAP_TTS = "tts"`）
  - `TtsClient::synthesize(text) -> TtsAudio`，对接阿里云百炼 DashScope **原生**协议（`qwen3-tts-flash`）
  - **内置 24 kHz → 48 kHz 重采样**：百炼原生输出 24 kHz，而固件  
    `speaker_audio_contract.h` 的 `max98357a_supports_lrclk()` 只接受  
    8k/16k/32k/44.1k/48k/88.2k/96k（**不含 24 kHz**），`kSpeakerPlaybackSampleRate`  
    又是编译期 `static_assert` 钉死 48000。把转换放在能力层，等于从机制上消灭  
    「采样率不匹配导致播不出声」这个故障
  - 零依赖手写 WAV 解析/封装：按 chunk 遍历而非硬编码 44 字节偏移（百炼返回的 wav  
    可能带 `LIST`/`fact` 附加块，写死偏移会把元数据当音频）
  - 带 `ErrorCode` 的结构化错误：未配置 Key / 入参非法 / 网络失败 / 上游错误 各自可辨
- **Agent 任务台配置层**（`src-tauri/src/agent_desk/`，新增 `CAP_AGENT_DESK = "agent-desk"`）
  - `DeskConfig`：总开关、产物目录、TTS、摘要、播报、执行器列表、4 个槽位绑定
  - **独立配置文件 `agent_desk.json`**（位于 `app_config_dir()`），不改既有  
    `config.json` 的 `PersistConfig` —— 多 agent 共树下，改动面即风险面
  - **预置 7 个执行器**，全部对应真实可用资产：`bailian-api` / `doubao-api` /  
    `deepseek-api` / `dsh`（DeepSeek Harness）/ `bailian-cli` / `qwen-code` / `codex-cli`。  
    **「加新 Agent = 加一行配置，不写 Rust 代码」**
  - `CompletionSignal { kind, pattern, evidence_cmd }` + `Confidence{High,Medium}`：  
    把「进程退出 ≠ 任务完成」变成**显式配置**。填了世界门禁（`evidence_cmd`）  
    才升为「已校验」；只靠 `process_exit_zero` 的只能是「未校验」
  - `SlotBinding { Empty | Exact | Capability | Auto }`：把「人选执行器」与  
    「模型选能力」统一成同一个枚举的四种取值，显式优先、自动兜底
- **6 个 Tauri 命令**（纯新增，未改任何既有命令签名）  
  `desk_get_config` / `desk_set_config` / `desk_set_tts_config` / `desk_snapshot`  
  / `desk_probe_driver` / `desk_tts_test`
- **侧栏新增「任务台」页签**（`DeskView.tsx`）：就绪状态 + 阻塞项逐条列出 +  
  4 槽位卡 + 执行器清单（含完成信号与可信度徽章）。**只读视图，不含任何密钥**
- **设置页新增「Agent 任务台」配置区**（`DeskSettings.tsx`）：总开关 / 产物目录 /  
  TTS 全量配置（含试合成落盘试听）/ 槽位绑定 / 执行器展开编辑 / 摘要字数与播报去抖

### 变更

- `capability.rs`：`ALL_CAPABILITIES` 由 10 项扩为 12 项（新增 `tts`、`agent-desk`）
- `lib.rs`：注册 2 个新模块 + 6 个新命令；setup 中初始化任务台管理器
- `styles.css`：新增 `.desk-*` 与 `.settings-save-bar` 样式，**全部走既有主题变量**，  
  深/浅色自动适配，未引入新色值
- `types.ts` / `api.ts`：新增任务台类型与 6 个命令封装

### 修复

- （无）

### ⚠️ 未完成（下一版继续，勿当成可用功能）

- **槽位队列 / 未读代际 / 播放租约**（Phase 2，`agent_desk/{slots,queue,unread}.rs`）——  
  所以任务台页签的「进度」区**没有实时数据**，只有一行说明
- **设备下行**（Phase 3）：灯语 `Report 0x16` 逐像素帧（需改固件）、音频 Opus 下发、  
  Notice 播报通道 —— **固件未改，键盘灯与喇叭目前收不到任何任务台数据**
- **Driver 运行时探测与真派发**（Phase 4）：当前 `desk_probe_driver` **只做静态配置检查**  
  （`scope: "static"`），不会真跑子进程或 HTTP
- **Harness 接入**：`bailian-api` 已能配好但**尚未接入执行链路**——现在没有"按下键 → 派发任务"的代码
- **`volc` TTS 服务商**：只留了枚举位置，选中会**明确报错**（不静默回退百炼）
- **`bailian-cli` 的 npm 包名未在本机核实**：`bailian-cli` / 命令 `bl` 来自文档检索，  
  首次启用前请先手工跑一次 `bl --help` 确认二进制真实存在

---

## [v0.12.1] — 2026-09-19

### 变更

- **侧栏导航上移**：移除 `.side-nav` 的 `margin-top: auto`，导航项紧贴侧栏顶部，消除原动作类型筛选块移除后留下的顶部空白
- **移除工作台「按键配置」模块**：使用率低，磁贴网格不再包含该项；同时移除 `group` / `counts` / `onGroupChange` 状态及 `handleGroupChange` 函数、`ACTION_GROUPS` 筛选条等关联代码
- **使用记录直接展示**：不再作为磁贴，`UsageLogView`（日历热力图 + 时间线）始终可见于工作台顶部
- **工作台磁贴网格精简为 2 项**：提示词库 + 词库管理，2 列布局；上方加「管理工具」分区标题
- `Workbench.tsx` Props 精简：移除 `vocabulary` / `apps` / `expanded` / `onExpand` / `onActionChange` / `flash` / `group` / `counts` / `onGroupChange`，仅保留 `profile` + `onPromptsChange`
- `App.tsx` 清理死代码：移除 `expanded` 状态、`useMemo` 导入

### 修复

- （无）

---

## [v0.12.0] — 2026-09-19

### 变更

- **动作类型筛选从侧栏移至工作台**：侧栏上半部分的「动作类型」筛选块已移除，降级为工作台「按键配置」详情内的横向 chip 筛选条；侧栏仅保留主导航
- **工作台主界面改为 2×2 磁贴网格**：原水平子 tab 栏（按键配置 / 使用记录 / 提示词库 / 词库管理）改为 2×2 磁贴卡片，点击进入详情，顶部「← 返回」回到主界面
- `Sidebar.tsx` Props 精简：移除 `group` / `onGroupChange` / `counts`，仅保留 `tab` / `onTabChange`
- `Workbench.tsx` Props 扩展：新增 `group` / `counts` / `onGroupChange`，承接原侧栏的筛选职责
- `styles.css`：移除 `.workbench-subtabs` / `.workbench-subtab` / `.workbench-content` 样式，新增 `.module-grid` / `.module-tile` / `.module-back` / `.action-filter-bar` / `.filter-chip` 样式

### 修复

- （无）

---

## [v0.11.0] — 2026-09-19

### 新增

- **工作台改造为「日常使用记录中心」**：在原按键配置磁贴布局之上新增三个子 tab——使用记录、提示词库、词库管理，保留原有磁铁式按键配置不变
- **使用记录存储层**（`usage_log.rs`）：JSONL 追加写 + ISO 8601 时间戳，ASR 上屏和 ⌘SS 命令执行成功后自动埋点；提供 `usage_log_query` / `usage_log_daily_counts` Tauri commands 查询
- **日历热力图 + 时间线**（`UsageLogView.tsx`）：按月展示每日记录数量热力图，点击日期展开时间线列表，按类型着色（语音=珊瑚橙、命令=靛蓝、提示词=绿、按键=黄）
- **提示词库管理视图**（`PromptLibraryView.tsx`）：复用已有 `workbench_prompt_*` CRUD API，在工作台内直接增删改提示词，无需打开浮窗
- **ASR 自定义词库**（`asr_vocab.rs` + `VocabView.tsx`）：管理语音识别热词（文本 + 权重 1-10），持久化到 `asr_vocab.json`；提供 `asr_vocab_list/add/remove` Tauri commands
- **ASR 热词注入**：火山引擎 ASR 请求（按住说话 + 通话模式）自动注入 `asr_vocab` 热词到 `request.hotwords` 字段，提升专业术语识别准确率
- 新增 `dayjs` 依赖（~2KB）用于日历日期计算

### 变更

- `Workbench.tsx` 重写为子 tab 框架，新增 `onPromptsChange` prop；原有磁贴布局完整保留为「按键配置」子 tab
- `App.tsx` 新增 `handlePromptsChange` 并传入 Workbench，提示词修改即时标记 dirty
- `styles.css` 新增子 tab 栏、日历、时间线、提示词库、词库管理样式，沿用珊瑚橙主题

### ⚠️ 未完成

- ASR 热词注入仅编译通过，**未在真机验证**火山引擎是否正确接受 `request.hotwords` 字段格式
- 使用记录埋点仅覆盖 ASR 上屏和 ⌘SS 执行两条路径，按键触发和提示词使用尚未埋点

---

## [v0.10.1] — 2026-09-19

### 变更

- 应用图标颜色从蓝色 `(43,95,217)` 改为珊瑚橙 `#ff6b35`，与界面主题 `--accent` 一致

---

## [v0.10.0] — 2026-09-19

> **本版为「路径 A · 插件页签」的插件侧落地。宿主侧未动**——两侧按冻结契约并行开发。  
> 契约见 [docs/路径A-插件页签接口契约.md](docs/路径A-插件页签接口契约.md)。

### 新增

- **冻结接口契约** [docs/路径A-插件页签接口契约.md](docs/路径A-插件页签接口契约.md)（v1.0）：  
  双 agent 并行开发时唯一的对齐物。定义清单字段规则、插件侧 `registerPage` 五条铁律、  
  宿主侧 H1–H5 任务清单、安全铁律、**向前兼容铁律**、验收口径
- **插件清单新增 `contributes.pages` 贡献点**（内容台已声明 `studio.board`）：  
  让插件能把常驻界面挂成宿主主窗的一个页签，而不是只能从 ⌘SS 里搜命令打开
- **实例新增 `context.registerPage(pageId, renderFn)`**（内容台已调用）：  
  宿主给容器、插件画内容。**不走字符串注入**——本项目 `csp: null` + `withGlobalTauri: true`，  
  任何 HTML/SVG 字符串注入都等价于 API Key 泄漏面，故 `icon` 只接受**图标名**
- 新增页签模式渲染验证页 `plugins/builtin/studio/test/panel-preview-page.html`：  
  亮色主题 + 模拟主窗（侧边栏 + 内容区），含 16 项**页面内**自断言（尺寸、position、内容计数）
- 验证台新增 33 项页签契约断言（111 → **144 项**）：含"旧宿主无 `registerPage` 时命令仍全注册"  
  这条向后兼容回归，以及"`position:fixed` 只允许挂在浮层包装类上"的静态扫描

### 变更

- **内容台面板重构为「视图」抽象**（`plugins/builtin/studio/index.js`）：  
  原来只有"主窗浮层"一种形态，现在 `overlay`（⌘SS 浮层）与 `page`（宿主页签）  
  **共用同一份 DOM 与 CSS**，差别只在外层定位。新增视图注册表，`refreshPanel()` /  
  `setStatus()` 改为遍历所有视图——否则会出现"在浮层里点了按钮，页签里不更新"
- 插件版本 `0.1.0` → `0.2.0`
- `plugins/builtin/studio/README.md` 补充页签模式与两种模式的分工

### 修复

- 面板 CSS 原先把 `position:fixed` 写死在 `.studio-panel` 上——**这种 UI 塞进页签会盖住整个窗口**。  
  现拆为：`.studio-overlay`（浮层定位包装）负责定位，`.studio-panel` 只负责填充。  
  验证台加了静态扫描守住这条边界

### ⚠️ 未完成

- **宿主侧 H1–H5 全部未实现**（应由另一位 agent 承接，清单见契约 §四）：  
  H1 `plugin_registry.rs` 解析/校验/排序 `pages` · H2 新增 Tauri 命令 `plugins_pages` ·  
  H3 新增 `plugin_activate_page`（**`commandId` 传空串，且 `pluginHost.ts` 必须加  
  `if (!commandId) return;` 守卫**，漏了会在日志里刷"命令未注册："） ·  
  H4 `context.registerPage` + 导出 `getPluginPages()` · H5 `Sidebar.tsx` / `App.tsx` 接线
- 因此**页签目前在真机上还看不见**——插件已声明且已注册，等宿主侧就位后自动生效，  
  插件侧不需要再改一行（这是契约 §六 向前兼容设计的用意）
- 内容台旧缺口仍在：`studio.gen.figures` 未做（**小红书投放包必失败**，缺图 → `pack xhs` 退出码 3）；  
  **飞书采集未做**；⌘SS → 面板全链路仍未在 GUI 手点验证
- **本版非破坏性说明**：新增 `pages` 属**增量**字段，老宿主未加 `deny_unknown_fields`，  
  会静默忽略 → 老用户可直接升级。**但若宿主侧将来给 `Contributes` 加 `deny_unknown_fields`，  
  本条即转为破坏性变更**（会拒绝加载带 `pages` 的新清单）

---

## [v0.9.3] — 2026-09-19

### 变更

- 设置页：将"LLM 编辑配置"和"语音编辑键"两个独立区块合并为统一的"语音编辑配置"区块。  
  LLM 参数（后端 / 端点 / 模型 / Key / 超时）在前，语音编辑键在后，逻辑不变、  
  配置读写不受影响。合并理由：该 LLM 本质上是语音编辑键的配套轻量云端小模型  
  （提示词优化 / 文本拓展 / 快速响应），后续计划为更强的大模型单独开配置区域

---

## [v0.9.2] — 2026-09-19

### 新增

- 插件性能 profiling：`pluginHost.ts` 加入 `performance.now()` 计时，覆盖：
  - `activate()` 耗时统计（记入 `PluginStats.activateMs`）
  - 命令执行耗时与调用次数（`PluginStats.commandCalls[cmd] = { count, totalMs }`）
  - `eo.*` API 调用计数（`PluginStats.apiCalls[api] = count`）
  - 超过 5s 阈值的操作自动记 warn 日志（`[plugin-profile]` 前缀）
- 导出 `getPluginStats()` 供调试 / 监控查询运行时统计
- 更新《插件集成指南》§5.5 调试与监控：补充 profiling 说明

---

## [v0.9.1] — 2026-09-19

### 新增

- 新增 [docs/插件集成指南.md](docs/插件集成指南.md)：面向插件开发者的完整指南，  
  涵盖架构设计（三层分离 + 清单 + 生命周期）、通信机制（Tauri invoke / HTTP 桥 / 方式对比）、  
  宿主 API（`eo.*` 7 项可用 + 7 项待实现）、集成步骤（6 步从零开发）、  
  实际应用考量（版本兼容 / 错误处理 / 性能 / 安全隔离 / 调试监控）、完整示例

---

## [v0.9.0] — 2026-09-18

### 新增

**第一个真正的业务插件：内容台（`plugins/builtin/studio/`）**

- 新增 11 个命令：`studio.toggle` / `state` / `topics.list|save|remove` / `stage.advance` /  
  `gen.title|body|humanize` / `export` / `doctor`；六阶段流转  
  （inspiration → intel → body → humanized → figures → exported）带准入门禁
- 新增 `plugins/builtin/studio/index.js`：单文件自包含 ESM（宿主用 `blob:` import，  
  blob 模块不能再 import 相对路径）
- 新增 `plugins/builtin/studio/tools/studio_io.py`：组稿落盘窄腰（读 `storage.json` → 写 `.md`  
  → 打印 `POST=<路径>`）。**纯标准库**，不依赖 playwright
- 新增 `scripts/deploy-plugins.mjs` + `npm run plugins:list|deploy`：把内置插件源装到  
  `%APPDATA%\com.easyop.app\plugins\`（`scan_dir` 只扫一层，中间差一次拷贝）
- 新增 `npm run plugins:test`：内容台验证台（桩 `eo.*`，111 项断言）

**宿主侧小幅补齐**

- `plugin_host.rs`：`plugin:activate` 的 payload 增加 `dir`（插件目录绝对路径）。  
  插件常自带脚本/资源，而 `eo.*` 没有"取自身路径"的能力——由 host 直接给，省得插件去猜
- `pluginHost.ts`：读 `dir` 并把 `pluginDir` 放进 `context`

### 变更

- `plugins/README.md` 重写：目录约定、命名空间铁律、`eo.*` 实测状态表、  
  **已有哪些能力 / 缺哪些能力**、进度表（激活层已从 ⬜ 改 ✅ 并给出真机证据）
- 示范插件改名：`plugins/studio-quick-note/` → `plugins/builtin/demo-quick-note/`，  
  命令 `studio.quick_note` → `demo.quick_note`（**避让 `studio.*` 命名空间**——那是内容台的；  
  两个插件同前缀会让 `find_plugin_by_command` 指错插件）

### 修复

- 内容台面板：`mountPanel()` 忘记把根节点赋给 `panelEl`，导致面板挂出来但列表/详情永不刷新，  
  且重复 `toggle` 会挂出第二个面板（`mountPanel` 开头的幂等判断形同虚设）。**截图验证抓到的**

### ⚠️ 未完成

- **`eo.fs.write` 缺失**：插件无法自己写文件，内容台只能靠自带 Python 脚本绕行。  
  补上后 `studio_io.py` 的"组稿"就能搬回 JS
- **`eo.panel.open` 缺失**：内容台开不了独立窗口（规范要 1080×720、`decorations(true)`），  
  现在只能挂主窗内浮层（340px 宽，空间局促）
- **`studio.gen.figures` 未做** → 小红书投放包**必然失败**（`pub.py pack xhs` 缺图退出码 3，已实测）。  
  这是当前最该补的一环
- **`contributes.keyActions` 未写**：`PluginManifest.contributes` 只有 `commands`+`tools`，  
  规范里出现的 `keyActions`/`panels`/`contextActions` 会被 serde **静默丢弃**（写了不报错也不生效）
- **`runner` 无动作白名单**：《内容台规范》§九承诺的"动作白名单枚举"未落地，`eo.run` 可跑任意程序
- **未在 easyop GUI 里手点验证**：⌘SS 搜"内容台"→回车 → 面板弹出这条完整链路需要人点一次  
  （插件已被真机扫描到，`activate` 事件链路在 v0.8.2 已修）

---

## [v0.8.3] — 2026-09-18

### 变更

- 开发板检测间隔从 3 秒加长到 10 秒（`App.tsx` deviceTimer）
- 设备检测日志改为只在**状态变化**时输出：离线→在线写"设备连接成功"，  
  在线→离线写"设备断开"。状态不变时不再写日志，避免刷屏。  
  原限频方案（每 5 秒最多一条）已移除，改为状态边沿触发

---

## [v0.8.2] — 2026-09-18

### 新增

- 新增 `src/pluginHost.ts`：前端插件宿主——监听 Rust emit 的 `plugin:activate` 事件，  
  在主窗执行插件 JS（定义 `eo.*` API → `blob:` URL 动态 `import()` 插件 `index.js` →  
  调 `activate(context)` → 执行初始命令）。幂等：已激活插件直接执行命令不重新 activate

### 变更

- `src/main.tsx`：启动时调 `initPluginHost()` 挂事件监听
- `plugin_host.rs`：`activate()` 改为 `app.emit("plugin:activate", payload)` 事件方案，  
  不再用 `WebviewWindow::eval` / `initialization_script`（Tauri 2 动态 webview 的 eval  
  存在时序问题：JS 可能在页面加载前被丢弃）。模块文档注释同步更新
- 删除 `plugin_host.rs` 中已废弃的 `build_host_js` 函数及其测试（逻辑已搬到前端 TS）
- 清理 `plugin_host.rs` 无用 import（`Serialize` / `tauri::Manager`）和未用变量 `label`

### 修复

- 修复 v0.8.0 插件激活层无法真机执行的问题：`eval` 和 `initialization_script` 两种方案  
  均未执行 host JS（日志到"已激活"后断链）。根因：Tauri 2 的 `eval` 可能在页面加载前  
  调用导致 JS 丢失；`initialization_script` 在动态创建的 webview 里可能不执行。  
  改用事件方案（Rust emit → 前端 listener）绕过时序问题

### ⚠️ 未完成

- 真机验证未做：需 `npm run tauri dev` 启动后搜"灵感"回车，确认 `storage.json` 被创建
- `npm run build` 因本机 esbuild 0.21.5 二进制损坏（Windows + Node.js v24 兼容问题，  
  二进制崩溃码 0xC0000409）未通过；`tsc --noEmit` + `cargo check` 均通过
- `eo_chat_stream` 尚未暴露（本版只有非流式 `eo_chat`）

---

## [v0.8.1] — 2026-09-18

### 新增

**《企业助手方案 v3》架构符合性评估**

- 新增 [docs/企业助手方案v3-架构符合性评估.md](docs/企业助手方案v3-架构符合性评估.md)：  
  对 `easyinput-english/详细实现方案v3-按优先级版.md`（企业私有化语音助手方案）做  
  **源码级**核验，结论为「问题诊断可用，架构叙事不可用」——
  - v3 §三「6 层架构（保留）」的 10 个目录 **0 个存在**（实测 `src-tauri/src` **零子目录**、  
    **36 个扁平 `.rs`**）；v3 把自家 P3 的"目录重构"产物写成了 P0 之前的既成事实
  - v3 的 P0 断言 5 项中 3 项准确、1 项已过时（`launcher.rs:72` 早已是 `.arg()` 形式）、  
    1 项行号与函数名错（`optimize()` 不存在；真实发送链 `voice_edit.rs:91→126→llm_client.rs:207-217`）
  - v3 全程未提 easyop 已定案的**能力内核（Capability Core）+ 插件体系**，导致 8 个新增模块  
    有 6 个落点与 canon 相左（TTS / 知识库 / 脱敏按架构应作为**能力**并入 Core，而非新建私有目录）
  - 两处硬技术错误：①Rust **无"模块级禁止 `use std::fs`"的 lint**，单 crate 扁平结构下做不到  
    "编译期隔离"；②删 `get_api_key` 会破坏现有设置页（Key 回显/保存依赖它），应改**掩码返回**
  - 附「可采纳部分」与「优先级重排」（收口 → 能力化 → 业务插件 → 体验 → 分发）

### 变更

- 无代码行为变更。本版仅新增一份评估文档 + 版本号递增（patch：纯文档）

---

## [v0.8.0] — 2026-09-18

### 新增

**阶段 0 · 框架级三项前置（plugin 体系基础设施）**

- 新增 `runner.rs`：可观测子进程执行器（`tokio::process` + 行读 + UTF-8 环境注入 +  
  超时 + 取消 + 退出码），供内容台调 Python 脚本用。含 6 个单元测试
- 新增 `capability.rs`：能力内核骨架——13 档统一错误码表（`ErrorCode`）+  
  统一返回信封（`ApiEnvelope<T>`）+ 10 个能力名常量（`CAP_*`）。含 6 个单元测试
- `llm_client.rs` 的 `chat` / `chat_stream` 已在 v0.7.0 加入，本版确认其对齐 Capability Core
- `Cargo.toml`：tokio 加 `process` + `io-util` feature

*阶段 1 · 插件激活层 + eo. 宿主 API**

- 新增 `plugin_host.rs`：激活层——建隐藏 webview / 主窗 eval + 注入 `eo.*` API +  
  `blob:` URL 动态 `import()` 插件 `index.js` + 调 `activate(context)` + 执行命令
- 新增 9 个 `eo.*` Tauri commands：`eo_get_selection` / `eo_replace_selection` /  
  `eo_inject` / `eo_storage_get` / `eo_storage_set`（原子写）/ `eo_log_info` /  
  `eo_log_warn` / `eo_log_error` / `eo_chat` / `eo_run`
- `plugin_registry.rs`：加 `find_loaded(plugin_id)` 供 plugin_host 读 manifest
- `ss_execute` 改 async，拦截 `kind="plugin"` 直接调 `plugin_host::activate`
- 新增 `public/plugin-host.html`：隐藏 webview 宿主页面

**规范文档更新**

- 《easyop 扩展机制规范》§六.3 / §九：内容台从"编译进 easyop 的 component"改为  
  "随 easyop 发布的内置插件"——不做沙箱时两者运行时权限相同，插件路线把后端逻辑  
  推到 JS，改 UI 不编译 Rust。落地顺序调整为 5 阶段
- 《内容台规范》全篇适配：§一定案 / §二继承（eo.* API 表）/ §三架构图（JS 模块）/  
  §四入口命令 / §五 UI 组织（插件目录）/ §十一实施步骤（P0/P1 前置 + M0-M5）

### ⚠️ 未完成

- **plugin_host 真机验证未通过**：`main_win.eval()` 注入的 host JS 未执行（日志到  
  "插件已激活"后无后续），原因待查（可能是 Tauri 2 eval 时序问题）。替代方案：  
  改用 Tauri 事件让前端主动执行插件 JS（Rust emit → 前端 listener → import/activate）
- **隐藏 webview 方案未验证**：`initialization_script` 注入的 host JS 同样未执行，  
  暂改用主窗 eval 作为 fallback
- **eo_chat_stream 未暴露**：`chat_stream` 需要 Tauri 事件推送增量，本版只暴露了  
  非流式 `eo_chat`

---

## [v0.7.1] — 2026-09-18

### 新增

**桌面宠物方案评估**

- 新增 [docs/桌面宠物-可行性与优化评估.md](docs/桌面宠物-可行性与优化评估.md)：  
  对 `easyinput-english/虚拟宠物方案-规划与可行性评估.md`（v1.1）的评估，  
  结论&#x4E3A;**「技术选型可采用，但集成方案须按 easyop 现状重写」**
- 核心发现（均为代码实测，非推测）：
  - 原方案写给一个**尚不存在的"企业助手"架构**——`orchestrator` / `tts/speaker.rs` /  
    `config/default.yaml` / P0–P5 路线图在 easyop 中**全部不存在**，集成点章节整段作废
  - 原方案最亮的功能「口型同步」缺数据源：easyop **没有 TTS**，  
    却把 SAPI5 写作"已有"，实际只有"表情 + 文本"两通道
  - 原方案**未提点击穿透**——本项目 `set_ignore_cursor_events` 零使用，  
    不穿透则透明区域会吞掉鼠标事件，属桌宠的生死线
  - 原方案把"企业下发改 SVG 字符串"当优势，实为**新增注入面**：  
    本项目为 `csp: null` + `withGlobalTauri: true`，注入的 SVG 可携带脚本并读取 API Key
  - 性能数字需重估：「内存 < 15MB」混淆了**渲染开销**与**WebView2 进程开销**，  
    真实增量待实测（已给出实测方法，作为开工前 Spike）
- 优化建议要点：状态机由"先画 9 态"改为**先列真实事件再推状态**（v1 收敛到 6 态）；  
  切分为「宿主机制 + 插件形象」；形象走「v1 手写几何 → v2 生成立绘」两段式；  
  并将原方案内部的 4 处自相矛盾一并列出

### 变更

- 无代码改动。本版仅为文档新增。

---

## [v0.7.0] — 2026-09-18

### 新增

**插件体系 · 发现层**

- 新增 `plugin_registry` 模块：启动时扫描 `%APPDATA%\com.easyop.app\plugins\<id>\plugin.json`，  
  解析为 `PluginManifest`（`id` / `name` / `version` / `main` / `activationEvents` /  
  `contributes{commands,tools}` / `capabilities`），校验必填字段并建内存注册表；  
  搜索路径只读内存，不碰磁盘（与 `mini_apps` 同范式：`static Mutex<Vec<T>>` + `init(dir)`）
- 新增三条命令：`plugins_list`（列出已发现插件）/ `plugins_rescan`（重扫目录）/  
  `plugins_dir`（返回插件根目录，便于定位与投放）
- **LLM 通用调用**：`llm_client` 新增 `ChatMessage` / `ChatOptions` 与  
  `chat()` / `chat_stream()`；`chat_stream` 走 `reqwest::bytes_stream()` +  
  `futures-util` 实现**真流式**（原有 `stream_edit` 是先取全量再切片吐出的**假流式**，仅为语音编辑写死 SYSTEM_PROMPT 的场景保留）
- **示范插件 `studio-quick-note`（灵感速记）**：`plugins/` 下的参考实现，  
  声明命令 `studio.quickNote` 与同名 tool，供后续插件照抄清单格式
- 新增 `plugins/README.md`：记录插件体系四层进度（发现 ✅ / 接入 ✅ / 激活 ⬜ / 沙箱 ⬜）  
  与清单字段说明

**⌘SS 面板接入插件结果**

- 搜索结果并入 `kind = "plugin"` 条目，图标区新增 🧩 青色专属样式，与「小程序」「内置工具」区分
- 前端 `executeCurrent()` 补齐 `plugin` 分支：**未实现的执行路径明确报错，不静默返回成功**  
  （原 `switch` 落到 `default: break`，点了没反应，是最难排查的失败态）

### 变更

- 本版起建立版本纪律：`package.json` 为版本号**唯一真源**，  
  `tauri.conf.json` / `Cargo.toml` / `package-lock.json` 由 `scripts/version.mjs` 同步；  
  新增 `npm run version:check` 作为收工自检项（详见 [AGENTS.md](AGENTS.md)）
- 顺带修复历史版本漂移：`package-lock.json` 自 v0.4.0 起未跟随版本号（0.4.0 → 0.6.1）

### ⚠️ 未完成

- **插件激活层（`plugin_host`）尚未实现**：插件目前**能被发现、能被列出、能被搜索到，  
  但不能被执行**。激活需要隐藏 webview 加载插件 `main` 入口、注入 `context.api`（`eo.*`）  
  并承接 `registerCommand` 回调；难点是 Tauri 的 `WebviewUrl` 只接受静态 URL，  
  插件目录不在前端 `dist` 内 → 需走「内置 shell 页面 + invoke 取 JS 后 eval」或自定义 protocol
- 规范文件 `docs/easyop 扩展机制规范.md` 仍写「插件体系推迟到阶段 3」，与代码现状矛盾，待同步

---

## [v0.6.1] — 2026-09-18

### 变更

**提示词库命名统一（界面文案收尾）**

- 主窗口「工作台」页的浮窗开关按钮由「浮窗工作台 / 收起浮窗」改名为 **「提示词库 / 收起提示词库」**，  
  与浮窗实际承载的功能一致，避免「界面是提示词库、按钮却叫浮窗工作台」造成的认知偏差
- KEY4 及编码器具名动作的界面标签统一为「提示词库 · 上一项 / 下一项 / 确认」；  
  侧栏筛选分组、`api.ts` 注释、浮窗窗口标题（`easyop 工作台` → `easyop 提示词库`）一并订正
- 仅改**用户可见文案**，底层动作 ID（`workbench` / `workbench_prev` / `workbench_next` / `workbench_confirm`）  
  与固件保留 UUID 映射**保持不变**，不影响已下发配置与硬件联动

**使用说明补充**

- README 明确提示词库的两个入口：顶栏「提示词库」按钮（无开发板时也可验证）与硬件 KEY4
- 新增 [docs/ROADMAP.md](docs/ROADMAP.md)：近期 / 中期 / 远期三个层级的后续优化方向

---

## [v0.6.0] — 2026-09-18

### 新增

**浮窗工作台 → 方案提示词库**

- KEY4 浮窗从「按键动作列表（只读速览）」改为**提示词库**：内容随当前方案切换，  
  支持在浮窗内直接**新增 / 编辑 / 删除**，选中回车即把该条提示词注入到光标处
- 提示词挂在**方案**上（写入 `profiles.json` 的 `prompts` 字段），  
  切换方案时浮窗内容随之整体更换
- 浮窗行内交互：顶栏「＋ 新增」开行内输入行，每条行内「编辑 / 删除」，  
  乐观更新 + 以后端返回的**权威快照**刷列表，避免多窗口并发时的旧数据回填
- 后端新增 `Prompt { id, text }` 模型与三条命令  
  `workbench_prompt_add` / `workbench_prompt_update` / `workbench_prompt_remove`，  
  统一走 `mutate_prompts`：改内存 → 校验 → **先落盘、后广播**，  
  杜绝「只改内存不落盘」与「只落盘不广播」两类静默分叉
- 校验：提示词正文上限 4096 字节（`PROMPT_MAX_UTF8_BYTES`）、  
  每方案上限 60 条（`PROMPT_MAX_COUNT`），空内容直接拒绝
- 浮窗尺寸固定 480 × 360（逻辑像素），条目超出走**内部滚动**，不再随条数变高

**⌘SS 面板：搜索框下方小程序图标区**

- 面板**搜索框正下方**新增小程序图标网格，形态对齐微信小程序入口：  
  已集成的小程序摊成一格一格的图标，**点一下即启动**
- **鼠标悬停显示名称**：名称默认透明，`hover` 时淡入（占位高度照留，悬停不会挤动布局），  
  同时用 `aria-label` 保证读屏也能拿到名字
- 图标按集成类型区分底色：`app`（外部应用）/ `url`（Web 项目）/ `folder`（项目目录）
- **一输入搜索词，图标区整块隐藏**，不与搜索结果抢注意力、不干扰正常搜索；  
  关键词清空后自动摊回
- 图标区数据每次唤出面板都重新拉取（`mini_apps_list`），  
  在小程序管理页增删后回到面板即可看到最新列表
- 条目较多时图标区内部滚动（上限 156px），面板高度仍由内容自适应

### 变更

- `workbench::BoardInputEvent` 新增 `profile_id` 字段：浮窗据此判断  
  「收到的快照是不是当前方案的」，避免切方案瞬间显示上一个方案的词条
- 浮窗键盘优先级：输入态（新增 / 编辑）下 `Esc` 先取消编辑，再退到收起浮窗；  
  `↑↓` 移动高亮、`回车` 注入当前项

---

## [v0.5.0] — 2026-09-18

### 新增

**小程序：把外部应用集成进 easyop**

- 语义纠正 —— 「小程序」指**外部** App / Web 项目 / 项目目录被集成进 easyop，  
  在面板里列出、点击即启动；面板不渲染它们的内容，只做「列出 / 启动 / 增删」
- 新增 `mini_apps` 模块与注册表（`%APPDATA%\com.easyop.app\mini_apps.json`），  
  内存缓存 + 启动预热，搜索路径不碰磁盘
- 三种集成类型：`app`（本机 exe，可带启动参数）/ `url`（Web 项目，缺协议自动补 `https://`）/  
  `folder`（本地项目目录）
- 面板内「小程序」管理页：搜索「小程序」或「集成」进入，  
  表单支持名字 / 目标 / 参数，填目标时复用已预热的应用索引直接给出本机应用建议（120ms 防抖）
- 名字留空自动推导（网址取域名、路径取文件名），同类型同目标自动去重

### 变更

**「小程序」→「内置工具」正名**

- v0.4.0 里被叫作「小程序」的计算器 / 待办 / 截图 / 翻译，实为 easyop **自带的内置工具**，  
  与「集成外部应用」是两件事 —— 本次把两者彻底拆成两个 kind：  
  `"tool"`（切面板视图，不启动进程，不收面板）/ `"miniapp"`（真启动外部目标，启动后收起面板）
- 搜索通道拆分：内置工具走 `BUILTIN_TOOLS` 关键词表，小程序走 `mini_apps::search_hits()`
- 前端 `MINI_APP_RENDERERS` 保留为内置工具的渲染器集合，新增 `miniapps` 一档做集成管理页
- 使用频次历史键自动迁移：`miniapp:<tool>` → `tool:<tool>`（幂等，启动时一次）

### 修复

- **首次搜索明显卡顿**：每次搜索都重扫开始菜单 / 注册表 / 安装目录，并对 800+ 项  
  逐条重算 `to_lowercase` / 拼音首字母 / 缩写  
  → 改为 `IndexedApp` 预计算 + `Arc` 共享，启动预热（`warm_up()`），  
  并用 `BUILD_LOCK` 串行化扫描，避免预热与首次搜索并发重复扫盘；  
  前端本地搜索与 Everything 探测改为双通道 + 请求序号，不再相互阻塞
- **按 Esc 收起面板会把 easyop 整个退出**：面板窗口关闭触发了 Tauri 默认的  
  「所有窗口关闭即退出应用」→ 新增显式退出意图（`EXIT_INTENT`）白名单，  
  非用户主动退出（托盘「退出」/ 主窗口退出）一律 `api.prevent_exit()`，  
  面板常驻不销毁，Esc 只收起

---

## [v0.4.0] — 2026-09-18

### 新增

**⌘SS 命令面板（Super StartUp）**

- 独立浮窗面板：屏幕居中、无边框、置顶、按结果条数动态调整高度（160–520px）
- 三层评分搜索：精确前缀 95 / 包含匹配 70 / 拼音首字母 45 / 单词缩写 40，  
  应用库 98/75/50/45，系统命令 92/72/48，小程序 88/62/88/55
- 拼音匹配：全拼（`weixin` → 微信）、简拼（`wx` → 微信）、单词缩写（`vscode` → Visual Studio Code）
- 频次排序：本地持久化使用次数，加权 `min(count * 0.5, 15)`，排序按 得分 ↓ → 频次 ↓ → 标题 ↑
- 应用索引增强：注册表 `App Paths`、All Users 开始菜单、常见安装目录、  
  应用类型分类、扫描结果本地缓存
- 结果项高亮匹配片段，支持中英文混排

**小程序框架 + 首批四个小程序**

- 注册协议：Rust 端只决定「何时露出」小程序，前端按 `kind = "miniapp"` 渲染 UI；  
  `MiniAppDef.id` 同时是结果项 `target` 与前端 `MINI_APP_RENDERERS` 的键，两边严格一致
- 两个入口：关键词命中（「打开 计算器」）/ `=` 前缀直接带算式（`= 12*34` 得 100 分）
- 进入小程序后面板主体整体切换视图，搜索框 / 结果列表 / 状态栏让位；  
  Esc 优先级为 小程序 → 精简右键菜单 → 收起面板
- **计算器**：自写递归下降求值器，支持 `+ - * / % ^` 与括号，  
  刻意不使用 `eval` / `new Function`（规避面板内执行任意代码的风险）
- **待办清单**：`localStorage` 持久化，回车快速添加、点击切换完成、删除
- **截图**：GDI `BitBlt` 抓取整个虚拟桌面，手写 PNG 编码（复用既有 `flate2`，  
  零新依赖），保存到 `图片\easyop` 目录
- **翻译**：选择语言方向后打开在线翻译页面（不引入 API key 与新依赖）

### 修复

- **精简右键菜单不生效**：后端 `ss_palette::show()` 从未 emit `ss-palette-show`  
  → 补齐事件发送
- **精简右键菜单不生效（第二处）**：`ss_palette` 窗口缺少 capabilities 声明，  
  导致 `tauri.event.listen` 静默失败 → 新增 `capabilities/ss_palette.json`
- **面板被系统命令列表裁剪**：改为调用 `ss_resize` 撑高窗口（上限 520px）
- **设置页开关状态难以辨识**：轨道内加「开 / 关」文字、开启态强调色、状态徽标
- **开关滑块位移偏差**：`translateX(22px)` → `translateX(24px)`
- **`.compact-menu` 背景透明**：引用了从未定义的 CSS 变量 `--panel-1` → 改为 `--panel-2`
- **设置文案失实**：「常用的 8 项」订正为「常用的 6 项」

---

## [v0.3.0] — 补录（原日期未记录）

### 新增

- **语音编辑**：选中文本 → 说出修改要求 → LLM 流式编辑 → 自动替换
- **方案切换**：按键触发方案切换，主机端接管（借 Host Action 通道绕过固件不支持多方案的限制）
- **中文固定文本**：含非 ASCII 字符的文本由主机端用剪贴板 + Ctrl+V 注入

### 修复

- **LLM SSE 解码**：`resp.bytes()` + `from_utf8_lossy` 绕过 reqwest `text()` 的解码问题
- **编辑键初始化顺序**：`set_edit_ptt_key` 移到 `PttEngine::start` 之后
- **去重时间窗口**：Call 模式改为 2 秒窗口去重，允许用户有意重复说相同内容
- **浮层状态残留**：每次编辑 / 识别开始时清除上次遗留字幕

---

## [v0.2.0] — 补录（原日期未记录）

### 新增

- **三通道通信架构**：EIAU / EIHB / EICC / EICA 协议编解码 + UDP 17333 音频监听
- **通信配置命令**：Wi-Fi 连接 / 蓝牙控制 / 麦克风源选择
- **可视化键盘布局**：4×2 按键网格 + 圆形编码器

### 修复

- **HID 冲突**：仅打开 app-command 集合，跳过键盘集合

> 说明：v0.2.0 与 v0.3.0 的功能当时已落地，但未同步更新版本号与变更日志，  
> 上述条目按 README 的版本历史补录，原始发布日期无据可考。

---

## [v0.1.1] — 2026-09-09

### 变更内容

**构建配置完善**

- `package.json`：新增 `tauri:build` 脚本命令，方便一键打包桌面应用
- `tauri.conf.json`：启用 Tauri 打包配置（`bundle.active = true`）
  - 设置应用图标 `icons/icon.ico`
  - 版权声明：`Copyright (c) 2026 easyop`
  - 应用分类：`Utility`（工具类）
  - 简短描述：easyop · ESP32 按键配置与语音输入工具
  - 详细描述：easyop 是一款配合 ESP32_S3_BOT 使用的桌面配置工具，支持按键方案管理、语音输入（ASR）与 LLM 文本编辑

### 影响

- 现在可以通过 `npm run tauri:build` 生成 Windows 安装包
- 打包产物会包含正确的图标、描述与元信息

---

## [v0.1.0] — 2026-09-08

### 新增

- 初始版本发布
- 前端：React 18 + Vite + TypeScript，三页签界面（按键映射 / 本机应用 / 语音输入）
- 后端：Rust + Tauri 2，Win32 FFI（HID / 键盘钩子 / SendInput / ShellExecuteW）
- 三通道通信：HID 配置下发 + Wi-Fi UDP 音频流 + 云端 ASR 流式识别
- 语音编辑：选中文本 → ASR → LLM 流式编辑 → 自动替换
- Host Action：方案切换、固定文本注入、打开应用
