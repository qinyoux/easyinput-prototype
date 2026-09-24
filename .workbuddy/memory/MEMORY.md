# easyop 项目长期记忆（curated）

> 只写**跨会话仍然成立**的事实；逐次工作与故障叙事记 `YYYY-MM-DD.md`。
> ⚠️ 注入上限实测 ≈17.4 KB（超了静默截断）；精简只压措辞、不删事实。

## 本机启动 / 真机联调（2026-09-24 实测）

- **两步都要 `run_in_background`**（否则进程随 shell 死）：① `npm run dev`（vite，strictPort 1420）；
  ② `cd src-tauri && WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" ./target/debug/easyop.exe`。
  debug 版前端来自 `devUrl: http://localhost:1420` —— **没 vite 就是白屏**。
- ⚠️ **vite 会被 node safe-delete shim 拦死**（要清 `node_modules/.vite/deps` 51 文件 > 阈值 50）。
  放行：命令前加 `CODEBUDDY_SAFE_DELETE_BULK_STATE_DIR= CODEBUDDY_TOOL_CALL_ID=`（`checkBulkDeleteGuard`
  缺这两者任一即 return）。目标是 vite 自己的缓存，不是用户数据。
- CDP 挑**主窗口**用 `~正则`：`node .workbuddy/cdp_eval.mjs eval "<js>" --url '~^http://localhost:1420/$'`。
- **TTS 配置在 `%APPDATA%\com.easyop.app\agent_desk.json` 的 `tts` 段**（命令 `desk_set_tts_config`）；
  `config.json` 只有 `asr_url`、**没有 tts**。`X-Api-Resource-Id` 取自 **`resource_id`**；`model` 是
  **百炼专用**、volc 忽略 —— 别被启动日志的 `model=seed-tts-1.0` 误导。
- `enumerate_device_paths()` **不按 VID/PID 过滤**（返回全部 HID），校验在 `open_matching` ⇒ 两种报错含义不同：
  **「未找到开发板…请确认已通过 USB 连接」= 列表为空**；**「所有候选 HID 设备均无法下发…」= 列表非空但全失败**。
- ⚠️ **板子只有蓝牙在线时配置通道全废**（09-24 实测）：PnP 里 `BTHLEDEVICE\…VID&02303A_PID&1006…` = **OK**、
  `USB\VID_303A&PID_1006\EASY-INPUT-V2` = **Unknown（不在位）**，启动日志报「所有候选 HID 设备均无法下发命令」
  +「任务台灯语下发失败」。**推断（未验证）**：Feature Report 配置通道只在 USB HID 上可用。
- 真机实测（09-24 13:58，debug 版 + 用户真实配置，走 `desk_tts_test`）：**48000 Hz / 229404 采样 / 4779 ms /
  16 帧 13 句 / 墙钟 1389 ms**，事件 50→150 齐全 ⇒ **双向流式在应用里已可用**。
- `desk_tts_test`（`agent_desk/mod.rs`）**只合成 + 落 `tts_preview_<ms>.wav`，不播声音** —— 静默端到端自检用它。
- ⚠️ `cargo build` 有活实例时 `LNK1104`（`cargo check`/测试不受影响）。**别去 taskkill 钦友的实例。**

## 任务台 / 执行器（v0.35.0）

**按键与页面按钮同一条路**：`session.rs::dispatch_to_desk`（S1–S4）与 `desk_dispatch_task`（页面按钮）
**都调 `agent_desk::dispatch_task`** → `resolve_driver` → `dispatch_into` → `pump_slot`。
队列 / 同槽串行 / 四档完成判定 / 世界门禁 / 未读 / 播报 / 会话续接**全在这条**。

- ⛔ **原「出口 A」（按键把命令行投进用户正在用的终端）v0.35.0 已整体删除**，别当还有另一条路：
  `slot_command_line`/`deliver_command_line`/`desk_deliver_foreground`/`text_delivery::deliver_to_window*` 全没了
  （删因见 CHANGELOG v0.35.0：它绕开整套机制 ⇒ 两个入口能力不同而用户以为一致）。
- ⚠️ **`agent_desk/delivery.rs` 没被删**（收窄成"等外部终端到前台"），仍被 `cli_terminal.rs`（「启动终端」）用。
- `submit_after_paste`（是否自动回车）现在**只影响「启动终端」那条路**。
- **`pump_slot` 顺序有依赖**：认领 → 执行 → 收终态 → **5.5 播报** → 5.6 留存结果 → **5.7 落历史 + 扫产物** → 会话回写。
  **后端「先发播报事件、后写历史」** ⇒ 前端**不能**在播报回调里立刻拉历史（会稳定漏掉刚跑完那条），
  历史面板改用 10 秒低频轮询。
- 常量：`DEFAULT_TIMEOUT_MS = 180_000`（照 HTTP 定的）⇒ **CLI 执行器必须自己调大**（预置 900_000）；0/留空=用默认。
  `MAX_DEPTH_PER_SLOT = 8`；同槽严格串行（一个按键+一颗灯+一段播放都单份），跨槽并行。
- 完成信号：`HttpStreamEnd`/`LogPattern` → High；`ProcessExitZero`/`AsyncTaskPoll` → Medium；
  **填了 `evidence_cmd` 升 High**。世界门禁只在基础执行已成功后才跑，未通过把 `Completed` **翻成** `Failed`。
- 续接靠 **argv `resume <id>`**（不是文件）；codex 从 `thread.started` 取 `thread_id`。
- Windows 上 codex 必须写 **`codex.cmd`**（`CreateProcess` 对无扩展名只补 `.exe`，npm 装的是 `.cmd`）。
- **任务历史**：`output_dir()/tasks/`（「产物目录」下，**不是** `%APPDATA%`；产物目录留空才退回
  `%APPDATA%\com.easyop.app\agent_desk_out`）。`index.jsonl` + 每条一个 `.log`，留 200 条、裁剪时**连日志一起删**；
  **先写日志、后写索引**。命令 `desk_task_history(limit)` / `desk_task_log(task)`。
- **播报抢占**：`take_channel` 会对前一条调 `stop()` ⇒ 同一刻两条播报，旧的那条 `played_samples = 0`。
  **"0 样本"分两种**：`stop_requested()` 为真 = 被抢占（正常），为假才是设备没出声。
- **产物发现**（`artifact.rs`）：判据 = **mtime ≥ 任务开始时刻** + 可选 `artifact_glob`。
  ⚠️ **glob 拿"相对工作目录"的路径去比**（绝对路径比则 `*.md` 一条也匹配不上，且完全静默）；`*` 不跨 `/`、
  `**` 跨且 `**/` 可匹配零层。`artifact_glob` 默认空；`artifact_open` **默认开**（必须
  `#[serde(default = "default_true")]`，用 `#[serde(default)]` 老配置会拿到 `false`）。跳过名单**不含**
  `dist`/`build`/`out`；`Found::skipped` 进日志。⚠️ **排序并列是常态**（时间戳粒度 ~1ms，连写的两文件 mtime
  可能完全相同）⇒ `mtime 降序 + 路径升序`（不承诺更准但**可复现**）；测试**别断言"最新的一定排第一"**。
- **播报正文**（`VoiceSpec`）：`Corpus`/`Text`/`Summarize`。**二次总结必须放在 `emit_notice_with` 里 spawn 的
  后台任务里算**，不能在 `pump_slot` 里 `await`（那会让整个槽位陪等几十秒）。

**预置三条执行器**（`config.rs::default_drivers`，出厂只有 `doubao-api` 启用）：`doubao-api`(Api/HttpStreamEnd) ·
`codex-cli`(Cli/LogPattern `turn.completed;task_complete`/有续接) · `dsh`(Cli/ProcessExitZero/**无续接**)。

## 取消 / 终止（v0.35.1，动 `runner.rs` 前必读）

- **取消 = 终止整棵进程树**。`runner.rs::terminate_tree`：先 `taskkill /T /F /PID`，**再** `child.kill()`+`wait()`。
  ⚠️ **顺序硬性**：链靠 `ParentProcessId` 找子孙，父进程一死链就断 ⇒ 反过来只会杀一个已死的 pid。
  用绝对路径 `%SystemRoot%\System32\taskkill.exe`。
- **`Child::kill()` 只杀直接子进程**：`codex.cmd` = `cmd.exe` → `node.exe`，杀 cmd 后 node 照常活着（继续烧额度）。
- **收尾 drain 必须有硬超时**：行读只在**管道 EOF** 结束，而 EOF 要求**所有持有写端者**（含孙进程）退出
  ⇒ node 活着则 `line_rx.recv()` **永不返回**（实测被拖 **11.14 s**）。现在：正常退出宽限 2s、**被终止后 300ms**，
  到点记 warn 后返回 —— 底线是 **`run()` 一定返回**。
- **`api_driver` 取消靠 `tokio::select!` 与 `chat_stream` 的 future 竞速**。**不给 `llm_client` 加取消参数是刻意的**
  （会连带改动语音编辑那条链路）。取消时回传**空 `output`**（半截正文被当成任务结果留存只会误导）。
- 前端：`DeskView` 用 `cancelling: number[]` 标记"已按取消、等收尾"，槽 `running` 变空后 `useEffect` 撤掉；
  两个入口（任务卡右侧 `×` + 槽位下方按钮）**同一个处理器**。
- ⚠️ **任务跑的过程中仍零反馈**（`pump_slot` 出口是 `NullSink`，Phase 3）—— 这是"下发到出结果很慢"的
  **感知主因**，不是性能问题（pump 事件驱动，派发即 spawn，调度延迟可忽略）。

## 字幕浮层（v0.35.3，动语音链路前必读）

### ⛔ 显隐**只有一个决策者 —— 后端**

前端 **0 处** `show_overlay`/`hide_overlay`（`VoicePanel.tsx` 那段"跟随录音状态"的效果连 `overlayEnabled`
状态一起整体删除）。后端**录音开始** show，**每条终态路径** hide（注入成功/空结果/识别失败/任务台停留结束/
通话挂断/转编辑）；开关关掉也由 `lib.rs::set_overlay_enabled` 收。**别在前端再加"我觉得该显示/该收了"**。
（连坏过两次，教训记在 `2026-09-23.md` v0.35.3 段：前端拿 `status` 子串推导"是否还在录"必出错；
**只删"收"、留着"显示"是假修复**，且 `useEffect` 声明顺序会让旧值先跑一次 —— **互补动作删一条前必查声明顺序，
两个方向都要验**。）

### 三件事一起才成立，缺一个就是"识别了但屏幕上没字"

1. **识别正文只认 `asr-final`**。任务台派发路径过去**只发 `asr-state`（状态行）**、从不发正文 ⇒ 屏幕空白而日志
   正常。已修（`session::desk_subtitle_events`，**先正文、后状态行** —— 反了浮层会停在"识别完成"）。
   ⛔ 新增任何"派发/交付"路径都别忘了发正文。
2. **空 `asr-partial` 不上屏**，判断收在 `asr::emit_partial` **一处**（`run_call` 原本自己判了、`run_recognition` 漏了）。
   ⚠️ **别再把"这个端点回包文本永远是空的"当前提**：09-23 晚实测 `.../bigmodel_**nostream**` 在流式阶段
   （`last=false`）**逐字累积出了正常文本**（旧注释的 `#1`~`#22` 全空只是那一次的观测）。**当轮结论只认当轮日志。**
3. **任务台派发后浮层多留 1.8 s**（`session::DESK_SUBTITLE_LINGER`）—— 任务台不注入输入框，浮层是唯一凭据。
   收尾权按路径分流：`deliver_hold_result` 返回 `bool`，`true` = 它自己收，调用方**别再收一次**。

**现场取证套路**（不用麦克风也能验）：CDP 从主窗口 `window.__TAURI__.event.emit('asr-final'|'asr-partial'|'asr-state', …)`
（`core:default` 含 emit 权限），再直读浮层 `#text`/`#status-text`。⚠️ **必须同时采三样**：`isVisible()`、
DOM 文本、状态行 —— **DOM 有值 ≠ 用户看得见**。工具 `.workbuddy/cdp_eval.mjs`（它自身踩过的三个静默坑见
`silent-failure-forensics` skill 的「工具的自我验证」）。

## TTS 双向流式（v0.35.4，动 `tts.rs` 前必读）

### 根因：同一个服务，**上行帧形状有两套**，发错**不报错、只静默 0 帧**

| 方向 | 上行帧 | 头 flags |
|---|---|---|
| 单向 | `[头 4B][sequence 4B][size 4B][payload]` | `0b0001` |
| **双向** | `[头 4B][event 4B][sid_len 4B][sid][size 4B][payload]` | `0b0100` |

把单向帧发给双向端点 ⇒ 服务器不报错、静默等 ⇒ 现场 = **`等待音频超时(10 秒无新帧，已收 0 帧)`**。
**「0 帧」是双向路径最典型的失败签名**：先怀疑帧形状，别先怀疑 key/网络/端点。

三个已实测钉死的事实（别再重测）：
1. **会话类事件必须带非空 sid**。`sid_len == 0` → 服务器回 `session ID length is zero`；填了 sid 但
   **帧里没写出 sid 段** → `parse payload size failed: body too short`。Rust 侧已把规则**编码进类型**
   （`FrameScope::Session(&str)`，空串直接 `Err`）—— 想绕开得先改类型，这是故意的。
2. **只有 `152` 是终止符**。`351` 在 TTS1.0 是**逐句**触发，拿它当终止会早停。事件真名别记混：
   `350=TTSSentenceStart` / `352=TTSResponse`(音频) / `351=TTSSentenceEnd`(读回全文) / `150=SessionStarted` /
   `152=SessionFinished`。
3. **文本只放 `TaskRequest`**（放 `StartSession` 也能通，3.58 s vs 3.81 s、无双份合成，但按文档语义更干净）。

- `StartConnection` 真身 14 字节：`11 14 10 00 | 00 00 00 01 | 00 00 00 02 | 7b 7d`（有单测锁死）。
- 握手：`1 → 50 → 100 → 150 → 200 → 35x → 102 → 152 → 2 → 52`；`FinishConnection` 失败**忽略**。
- `is_bidirection()` 判据 = 去 `?`/`#`、去尾 `/` 后**最后一段严格等于 `"bidirection"`**（`bidirection-proxy`、
  `bidirection/stream2` **不算**）。**故意不提供"双向端点"常量**（带 `/plan/` 误导的默认值比没有更糟）。
  `(Volc, Http) + is_bidirection()` → 明确报错提示需 `wss://`。
- 探针三件套在 `scripts/`：`probe_tts_bidirection.py`（`--repro-old` 复现 0 帧）、
  `_probe_bidir_variants.py`（四种帧形状对照）、`_capture_bidir_frames.py`（抓 hex 喂单测）。

### ASR：只增不改的原则（2026-09-23 审查结论）

审查过 `asr.rs` 全部函数，**一行没改**。唯一潜在隐患：`final_text = text.clone();` 在 700/729/778/809 行
**无条件覆盖**，而空文本恰是正常 `last=true` 帧带回来的（`emit_partial` 已有非空守卫，两处不对称）。
实测 1094 次返回**0 次**中途空文本 ⇒ 潜在地雷不是现症。要修就是 `if !text.trim().is_empty() { … }`。
另两个候选（`result_type=chunk`、跨次复用 WS 连接省 231 ms）**均无验证、均未动**。

## ⚠️ 版本文件被外部清空 —— **已发生两次**（2026-09-23 / 09-24）

`CHANGELOG.md`/`README.md` 的**版本历史顶部会被外部进程整段删掉**（09-23 14:57 退回 v0.32.0；09-24 13:24:28
退回 v0.35.2）。共同特征：**两文件 mtime 精确到同一纳秒** ⇒ 同一进程同时重写。已排除本仓库工具（`.git/hooks/`
空、无 `.gitattributes`、`version.mjs` 的 insert **只插不删**）⇒ 疑并行 agent 会话。时间线见 `2026-09-24.md`。

### ⛔ 三条铁律

1. **`version:check` 失败时先看这两个文件顶部还在不在**，别先查版本号同步 —— 它是本仓库**唯一**报警器。
2. **提交后必须复跑 `version:check`**。09-24 那次 `add`→`commit` 之间被外部改写 ⇒ `6768e93` 提交的是坏版本，
   而事后 `git status` 显示"干净"，**干净掩盖了问题**。（多 agent 并行时 `add`→`commit` 存在被改写的窗口。）
3. **这两个文件没有能回滚的 git 基线**（好版本从未进 object store）⇒ **提交前先单独拷一份**；
   重建时**必须标注「⚠️ 回顾补记，不是逐字还原」**。

## ⚠️ 已知的"UI 看得到、运行时不可用"（不要按说明书去用）

1. **槽位绑定「按能力」/「自动判定」未实现（Phase 4）**：`resolve_driver` 直接报错；
   `DriverRegistry::supporting()` 只被 UI 用来显示候选数，不参与派发。
2. **`RunOutcome` 没有 `stderr` 字段** ⇒ 历史日志里 `----- stderr -----` **永远是空的**；失败原因只在
   已被截断的 `reason` 里。
3. **任务历史面板没有"打开产物"入口**；后端没开放"打开任意路径"的命令。
4. 其余未落地（详见 CHANGELOG 各版「⚠️ 未完成」）：`AsyncTaskPoll` 完成信号、**跑的过程中的**逐行实时日志
   （跑完有完整落盘）、失败自动重试、键盘喇叭出声（只 PC 喇叭）、板子灯语 `0x16`（固件未烧）。
5. **外部会话监工（C）播报仍是固定语料**（`VoiceSpec::Corpus`）—— 不是遗漏，它同时是"预合成语料退路
   仍然可用"的持续验证点。

## 协作纪律里容易踩的

- 收工必须 `npm run version:check` 到 exit 0；**版本号真源只有 `package.json`**。
- `npm run version:bump` 会改 `package.json`/`Cargo.toml`/`tauri.conf.json`，有活实例时会触发前端重刷
  （甚至整轮重编译重启）——收工前留意。
- 真机 GUI 点测前先确认**没人在用该实例**（见 `windows-dpi-window-capture` skill）；**别 taskkill 钦友的实例**。
- `cargo test --lib` 默认并行会假死，要加 `--test-threads=1`。
- **⚠️ 行尾**：`core.autocrlf = true`，且 `src-tauri/src/**/*.rs` 是**混合**的（09-23 实测 CRLF 11 / LF 44，
  无 `.gitattributes`）⇒ **别一概而论，改之前先量**。铁律：**一次外科手术只改该改的那几行，绝不顺带换掉
  整个文件的行尾**。用 Python **绝不能 `io.open(p, encoding="utf-8")` + `newline=""` 写回**（读时归一
  `\r\n`→`\n`，写回就换掉整个文件的行尾 ⇒ 假 diff，AGENTS.md §4.6 禁止）。正确：先量 → `open(p,"rb")` 读字节
  → `replace(b"\r\n", b"\n")` + `replace(b"\n", 换行字节)` **按原样**写回；改完 `tr -cd '\n'`/`'\r'` 对比。
- **大提交别 `git add -A` 一把梭**：多 agent 并行时它会吞掉别人**未完成**的中途状态；日常走精确路径。
  入库前三件套：① `git add -A -n` 干跑 + 逐文件 `stat` 排序；② 密钥扫描（`sk-…`/`api_key=…` 遍历待提交文件）；
  ③ 提交后**复跑 `version:check`**（见上铁律 2）。
- `.gitignore` 已堵住三类噪音：`*.bak`/`*.bak-*`（v0.35.0）、`.workbuddy/shots|watch`（一次性取证产物）、
  `.tmp-lark/`/`draft_*_folder/`（飞书内容流水线中间态）（v0.35.4）。
