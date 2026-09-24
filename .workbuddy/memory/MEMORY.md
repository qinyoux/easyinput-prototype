# easyop 项目长期记忆（curated）

> 只在**跨会话仍然成立**的事实写这里；逐次工作记在 `YYYY-MM-DD.md`。

## 任务台 / 执行器（v0.35.0 核过）

**按键与页面按钮走同一条路（v0.35.0 起）** —— 都进后台队列：

- `session.rs::dispatch_to_desk`（按键 S1–S4）与 `desk_dispatch_task`（页面按钮）
  **都调 `agent_desk::dispatch_task`** → `resolve_driver` → `dispatch_into` → `pump_slot`。
  队列 / 同槽串行 / 四档完成判定 / 世界门禁 / 未读 / 播报 / 会话续接**全在这条**。
- ⛔ **原「出口 A」（按键把命令行投进用户正在用的终端）已在 v0.35.0 整体删除**。
  别把它当"还存在的另一条路"：`slot_command_line` / `deliver_command_line` /
  `desk_deliver_foreground` 命令 / `text_delivery::deliver_to_window*` 全没了。
  删除理由（写进 CHANGELOG v0.35.0）：它唯一的卖点「过程可见」与任务台的价值正好相反，
  代价却是绕开整套机制，于是"按键盘"和"点按钮"能力不同，而用户以为它们一致。
- ⚠️ **`agent_desk/delivery.rs` 没有被删**：它被收窄成"等外部终端到前台"，
  仍被 `cli_terminal.rs`（「启动终端」那条路）使用。别当遗留物删掉。
- 「是否自动回车」（`text_delivery::submit_after_paste`）现在**只影响「启动终端」那条路**。

**`pump_slot` 的五步顺序有依赖，改前先读那一段注释**（mod.rs）：
认领 → 执行 → 收终态 → **5.5 播报** → 5.6 留存结果 → **5.7 落历史 + 扫产物** → 会话回写。
关键事实：**后端是「先发播报事件、后写历史」** ⇒ 前端**不能**在播报事件回调里立刻拉历史
（会稳定漏掉刚跑完那条），所以历史面板用 10 秒低频轮询。

**关键常量/事实：**

- `DEFAULT_TIMEOUT_MS = 180_000`（照 HTTP 调用定的）⇒ **CLI 执行器必须自己调大**
  （预置给 900_000）。0 或留空 = 用这个默认值。
- `MAX_DEPTH_PER_SLOT = 8`；同槽严格串行（一个按键+一颗灯+一段播放都是单份），跨槽并行。
- 完成信号：`HttpStreamEnd`/`LogPattern` → High；`ProcessExitZero`/`AsyncTaskPoll` → Medium；
  **填了 `evidence_cmd` 就升 High**。世界门禁只在基础执行已成功时才跑，
  未通过会把 `Completed` **翻成** `Failed`。
- 续接靠 **argv `resume <id>`**（不是文件）。codex 从 `thread.started` 取 `thread_id`。
- Windows 上 codex 命令**必须写 `codex.cmd`**（`CreateProcess` 对无扩展名只补 `.exe`，
  npm 装的是 `.cmd`；与 PATHEXT 无关）。
- **任务历史**：`output_dir()/tasks/`（即「产物目录」下，**不是** `%APPDATA%`；产物目录留空才退回
  `%APPDATA%\com.easyop.app\agent_desk_out`）。`index.jsonl` + 每条一个 `.log`，
  留 200 条，裁剪时**连日志一起删**；**先写日志、后写索引**（反过来会留下"索引有、日志无"）。
  新命令 `desk_task_history(limit)` / `desk_task_log(task)`。
- **播报抢占**：`take_channel` 会对前一条播报调 `stop()` ⇒ 同一刻触发的两条播报，
  旧的那条 `played_samples = 0`。**"0 样本"要分两种**：`stop_requested()` 为真 = 被抢占（正常），
  为假才是设备没出声。（修之前一律按"设备没出声"报，每次派发都产出一条假警报。）
- **产物发现**（`artifact.rs`）：判据 = **mtime ≥ 任务开始时刻**（时间窗）+ 可选 `artifact_glob`。
  ⚠️ **glob 必须拿"相对工作目录"的路径去比** —— 拿绝对路径比的话 `*.md` 一条也匹配不上，
  且完全静默。`*` 不跨 `/`，`**` 跨且 `**/` 可匹配零层目录。
  执行器字段 `artifact_glob`（默认空）/ `artifact_open`（**默认开**，必须用
  `#[serde(default = "default_true")]`，用 `#[serde(default)]` 老配置会拿到 `false`）。
  跳过名单**不含** `dist`/`build`/`out`（它们可能是用户的产物目录）；`Found::skipped` 进日志。
  ⚠️ **排序并列是常态**：Windows 文件时间戳粒度**约 1ms**，连着写的两个文件会拿到
  **完全相同**的 mtime。所以"同一毫秒内谁更新"**文件系统层面就无解**（试过毫秒截断、
  全精度两种方案，都无效）。最终 = `mtime 降序 + 路径升序`兜底：不承诺更准，但**可复现**。
  写产物相关测试时**别断言"最新的一定排第一"**，除非中间隔了一个时间片。
- **播报正文**（`VoiceSpec`）：`Corpus`（只念固定语料）/ `Text`（念现成文本）/
  `Summarize`（后台跑一次总结执行器再念）。
  **二次总结必须放在 `emit_notice_with` 里 spawn 的后台任务里算**，不能在 `pump_slot` 里
  `await` —— 那会让整个槽位陪着等几十秒。

**预置三条执行器各自的形态**（`config.rs::default_drivers`，出厂只有 `doubao-api` 启用）：
`doubao-api`(Api/HttpStreamEnd) · `codex-cli`(Cli/LogPattern `turn.completed;task_complete`/有续接)
· `dsh`(Cli/ProcessExitZero/**无续接**，headless 不支持跨调用续跑)。

## 取消 / 终止（v0.35.1 起，动 `runner.rs` 前必读）

- **取消 = 终止整棵进程树，不是杀那个进程。** `runner.rs::terminate_tree`：
  先 `taskkill /T /F /PID`，**再** `child.kill()` + `wait()` 回收。
  ⚠️ **顺序是硬性的**：taskkill 靠 `ParentProcessId` 链找子孙，父进程一死链就断了，
  只剩再也找不到从属关系的孤儿 —— 反过来写，它会安静地只杀一个已经死掉的 pid。
  用绝对路径 `%SystemRoot%\System32\taskkill.exe`（`PATH` 被用户改坏时不能失效）。
- **`Child::kill()` 只杀直接子进程。** Windows 上 `codex.cmd` 的真实结构是
  `cmd.exe`（Rust 标准库对 `.cmd`/`.bat` 自动包的一层）→ `node.exe`。
  杀 cmd 之后 node **照常活着**：继续改文件、继续烧 API 额度。
- **收尾循环（drain）必须有硬超时。** 行读任务只在**管道 EOF** 时结束，而管道 EOF
  要求**所有持有写端的进程**都退出 —— 包括孙进程。node 活着 ⇒ 管道永不 EOF
  ⇒ `while let Some(ev) = line_rx.recv().await` **永远返回不了**。
  实测被拖 **11.14s**（`cmd /c start /b ping -n 12`，父进程 exit=0 之后），
  正好等于孙进程寿命。现在：正常退出宽限 2s、**被终止后 300ms**，到点记 warn 后返回。
  这条是底线：**无论树有没有杀干净，`run()` 一定会返回**。
- **`api_driver` 的取消靠 `tokio::select!` 与 `chat_stream` 的 future 竞速**
  （取消分支赢 ⇒ future 被 drop ⇒ reqwest 关连接 ⇒ 真正中断请求）。
  **不给 `llm_client` 加取消参数是刻意的** —— 那会连带改动语音编辑那条链路。
  取消时回传**空 `output`**：半截正文被当成任务结果留存（历史/未读/播报）只会误导。
- 前端：`DeskView` 用 `cancelling: number[]` 标记"已按取消、等收尾"，
  该槽 `running` 变空后由 `useEffect` 自动撤掉。「取消执行」有两个入口
  （运行中任务卡右侧的 `×` + 槽位下方的按钮），**同一个处理器**。
- ⚠️ **任务跑的过程中仍然零反馈**（`pump_slot` 里出口是 `NullSink`，Phase 3）。
  这是"感觉下发到出结果很慢"的**感知主因**，不是性能问题 ——
  别跑去优化调度：pump 是**事件驱动**的（派发即 `ensure_pump` → spawn），
  调度延迟可忽略。
- 量"取消要多久"别用 CDP 脚本裸读：见 skill `windows-dpi-window-capture`
  里 `cdp_eval.mjs` 那个"超时 timer 没 clear ⇒ node 挂满 30 秒"的坑。

## 字幕浮层（v0.35.3 起，动语音链路前必读）

### ⛔ 铁律：浮层显隐**只有一个决策者 —— 后端**（v0.35.3）

**前端 0 处** `show_overlay` / `hide_overlay`（`VoicePanel.tsx` 里那段"跟随录音状态"的
效果已整体删除，连 `overlayEnabled` 状态一起）。后端在**录音开始** show，
**每条终态路径** hide：注入成功 / 空结果 / 识别失败 / 任务台停留结束 / 通话挂断 / 转编辑；
开关关掉也由 `lib.rs::set_overlay_enabled` 收（开的时候若 `is_listening()` 补一次 show）。
**别在前端再加"我觉得该显示了/该收了"的判断** —— 加一个就多一个抢方向盘的。

理由是这里连着坏过两次（详见 `.workbuddy/memory/2026-09-23.md` 的 v0.35.3 段）：

- 前端曾用一个 `status` 字符串的**子串**推导"是不是还在录"，而那个 `status` 由
  `asr-state` 与 `edit-state` **两个通道轮流写入** ⇒ 任何一句新文案都可能把它判成"干完了"。
  v0.35.2 新加的「识别完成，正在派给 S{slot}」正好命中 ⇒ 正文只亮 **~200 ms**。
- **只删"收"、留着"显示"是假修复**：`status → phase` 那条 `useEffect` 声明在显隐效果**之后**，
  status 变化那一帧显隐效果带着**上一轮的 `phase`** 先跑一次 ⇒ 在后端 hide 之后又把浮层叫回来
  （实测：空录音结束后浮层仍可见 **4.7 s**）。**互补动作删一条前必查声明顺序，且两个方向都要验。**

### 三件事一起才成立，缺一个就是"识别了但屏幕上没字"

1. **识别正文只认 `asr-final`**。任务台派发路径过去**只发 `asr-state`（状态行）**、
   从不发正文，于是屏幕空白而日志正常。已修（`session::desk_subtitle_events`
   纯函数，**先正文、后状态行** —— 顺序反了浮层会停在"识别完成"）。
   ⛔ 新增任何"派发/交付"路径时，别忘了发正文。
2. **空 `asr-partial` 不上屏**。判断收在 `asr::emit_partial` **一处**
   （`run_call` 原本自己判了非空、`run_recognition` 漏了）；空帧转发 = 下游被一遍遍擦掉。
   ⚠️ **但别再把"这个端点回包文本永远是空的"当前提**：2026-09-23 晚的实测日志里，
   `.../plan/sauc/bigmodel_**nostream**` 在流式阶段（`last=false`）**逐字累积出了正常文本**
   （旧注释记的 `#1`~`#22` 全空只是那一次的观测；而且用户实际就是靠这些 `asr-partial`
   在浮层上看到实时文字的）。**当轮结论只认当轮日志。**
3. **任务台派发后浮层多留 1.8 秒**（`session::DESK_SUBTITLE_LINGER`）——
   任务台不注入输入框，浮层是唯一凭据。收尾权按路径分流：
   `deliver_hold_result` 返回 `bool`，`true` = 它自己收，调用方**别再收一次**。

**现场取证套路**（不用麦克风也能验）：CDP 从主窗口
`window.__TAURI__.event.emit('asr-final'|'asr-partial'|'asr-state', …)`
（`core:default` 含 emit 权限，事件全局广播），再直读浮层 `#text` / `#status-text`。
⚠️ **探针必须同时采三样**：`window.__TAURI__.window.getCurrentWindow().isVisible()`、
DOM 文本、状态行 —— **DOM 有值 ≠ 用户看得见**（v0.35.3 的故障正是"有字但窗口隐藏着"）。
工具在 `.workbuddy/cdp_eval.mjs`；它自身踩过的三个静默坑（目标选择用 `includes` 会误命中
`edit_overlay.html`、`__TAURI_INTERNALS__.invoke` 不可写导致挂钩静默失败、
HMR 会清掉注入的记录器）见 `silent-failure-forensics` skill 的「工具的自我验证」。

## TTS 双向流式（v0.35.4 起，动 `tts.rs` 前必读）

### 根因：同一个服务，**上行帧形状有两套**，发错**不报错、只静默 0 帧**

| 方向 | 上行帧 | 头 flags |
|---|---|---|
| 单向 | `[头 4B][sequence 4B][size 4B][payload]` | `0b0001` |
| **双向** | `[头 4B][event 4B][sid_len 4B][sid][size 4B][payload]` | `0b0100` |

把单向帧发给双向端点 ⇒ 服务器不报错、静默等 ⇒ 现场表现为
**`等待音频超时(10 秒无新帧，已收 0 帧)`**。这是本次唯一真根因，靠读文档看不出来。

**"0 帧"是双向路径最典型的失败签名**：先怀疑帧形状，别先怀疑 key/网络/端点。

### 三个已实测钉死的事实（别再重测）

1. **会话类事件必须带非空 sid**。`sid_len == 0` → 服务器回 `session ID length is zero`；
   填了 sid 但**帧里没写出 sid 段** → `parse payload size failed: body too short`。
   Rust 侧已把这条规则**编码进类型**（`FrameScope::Session(&str)`，空串直接 `Err`）——
   想绕开它得先改类型，这是故意的。
2. **只有 `152` 是终止符**。`351` 在 TTS1.0 是**逐句**触发（对应 `TTSSentenceStart`），
   拿它当终止会早停。事件真名别记混：`350=TTSSentenceStart` / `352=TTSResponse`(音频) /
   `351=TTSSentenceEnd`(读回全文) / `150=SessionStarted` / `152=SessionFinished`。
3. **文本只放 `TaskRequest`**。放 `StartSession` 也能通（实测 3.58 s vs 3.81 s，无双份合成），
   但按文档只放 `TaskRequest`，语义更干净。

- 连接类帧 `StartConnection` 真身 14 字节：`11 14 10 00 | 00 00 00 01 | 00 00 00 02 | 7b 7d`
  （`{}` 的 payload 恰好 2 字节）。有单测锁死这串。
- 握手顺序：`1 → 50 → 100 → 150 → 200 → 35x → 102 → 152 → 2 → 52`；
  `FinishConnection` 失败**忽略**（音频已到手，不因此判失败）。
- 单次成功合成样本：16 帧 / `48000 Hz` / 171778 samples / 3578 ms / 墙钟 1513 ms。
- `is_bidirection()` 判据 = 去 `?`/`#`、去尾 `/` 后**最后一段严格等于 `"bidirection"`**
  （`bidirection-proxy`、`bidirection/stream2` **不算**）。**故意不提供"双向端点"常量** ——
  一个带 `/plan/` 误导的默认值比没有更糟。
- 探针三件套在 `scripts/`：`probe_tts_bidirection.py`（`--repro-old` 复现 0 帧）、
  `_probe_bidir_variants.py`（四种帧形状对照）、`_capture_bidir_frames.py`（抓 hex 喂单测）。
- ⚠️ **`cargo build` 会 `LNK1104` 锁在 `easyop.exe` 上**，只要钦友的应用开着就链不上；
  `cargo check` 与测试不受影响。**别去 taskkill 钦友的实例**。

### ASR：只增不改的原则（2026-09-23 审查结论）

审查过 `asr.rs` 全部函数，**一行没改**。唯一找到的潜在隐患：`final_text = text.clone();`
在 700/729/778/809 行**无条件覆盖**，而空文本恰是正常 `last=true` 帧带回来的
（`emit_partial` 已有非空守卫，两处不对称）。实测 1094 次返回**0 次**中途空文本 ⇒
是潜在地雷不是现症。要修就是 `if !text.trim().is_empty() { ... }`。
其余两个候选（`result_type=chunk`、跨次复用 WS 连接省 231 ms）**均无验证、均未动**。

## ⚠️ 版本文件被外部清空 —— **已发生两次**（2026-09-23 / 09-24）

`CHANGELOG.md` / `README.md` 的**版本历史顶部会被外部进程整段删掉**，回退到较早版本：

| 时间 | 被清内容 | 回退到 |
|---|---|---|
| 2026-09-23 14:57 | v0.33.0 ~ v0.35.1 | v0.32.0 |
| 2026-09-24 13:24:28 | v0.35.3 + v0.35.4 | v0.35.2 |

两次共同特征：**两个文件 mtime 精确到同一纳秒** ⇒ 同一进程同时重写。
已排除本仓库工具：`.git/hooks/` 为空、无 `.gitattributes`、
`scripts/version.mjs` 的 insert 逻辑**只插不删**（`raw.slice(0,at)+block+raw.slice(at)`）。
⇒ 疑并行 agent 会话。

### ⛔ 三条铁律

1. **`version:check` 失败时，先看这两个文件的顶部还在不在**，别先查版本号同步。
   它是本仓库**唯一**能暴露这类破坏的报警器。
2. **提交后必须复跑 `version:check`。** 2026-09-24 那次：`git add` 与 `commit` 之间文件被外部
   改写 ⇒ `6768e93` 提交的是坏版本，而事后 `git status` 显示"干净"，**干净掩盖了问题**；
   只有复跑校验才发现。**多 agent 并行时 `add`→`commit` 存在被他人中途改写的窗口。**
3. **这两个文件没有能回滚的 git 基线**（好版本从未进 object store —— 写入发生在 `add`
   读取之前；实测 `git cat-file --batch-all-objects` 里 CHANGELOG 只有一个坏 blob）
   ⇒ **提交前先单独拷一份**（如 `/tmp`），被覆盖只能重建。
   重建时**必须标注「⚠️ 回顾补记，不是逐字还原」**（两次都是这么处理的）。

详细时间线与处置见 `.workbuddy/memory/2026-09-24.md`。

## ⚠️ 已知的"UI 看得到、运行时不可用"（不要按说明书去用）

1. **槽位绑定的「按能力」/「自动判定」未实现（Phase 4）**：`resolve_driver` 直接报错。
   `DriverRegistry::supporting()` 只被 UI 用来显示候选数，不参与派发。
2. **`RunOutcome` 没有 `stderr` 字段** ⇒ 历史日志里那段 `----- stderr -----` **永远是空的**
   （`write_log` 支持它，但调用点只能传 `""`）。失败原因只在 `reason` 里且已被截断。
3. **任务历史面板没有"打开产物"入口**：自动打开只在任务成功那一刻发生；
   从历史点开旧产物做不到，后端也没开放"打开任意路径"的命令。
4. 其他未落地：`AsyncTaskPoll` 完成信号、**跑的过程中的**逐行实时日志（跑完后完整落盘有）、
   失败自动重试、键盘喇叭出声（只 PC 喇叭）、板子灯语 `0x16`（固件未烧）。
5. **外部会话监工（C）播报仍是固定语料**（`VoiceSpec::Corpus`），不念 `hit.summary` ——
   不是遗漏，它同时是"预合成语料退路仍然可用"的持续验证点。

## 协作纪律里容易踩的

- 收工必须 `npm run version:check` 到 exit 0；版本号真源只有 `package.json`。
- **`npm run version:bump` 会改 `package.json`/`Cargo.toml`/`tauri.conf.json`**，
  有活实例时会触发前端重刷（甚至整轮重编译重启）——收工前留意。
- 真机 GUI 点测前先确认**没人在用该实例**（详见 `windows-dpi-window-capture` skill）。
- `cargo test --lib` 默认并行会假死，要加 `--test-threads=1`。
- **⚠️ 行尾：`git config core.autocrlf = true`，工作树里 `src-tauri/src/**/*.rs` 的行尾是
  **混合**的（2026-09-23 实测：CRLF 11 个 / LF 44 个，无 `.gitattributes`）。
  所以别信"这批文件是 CRLF"这种一概而论的说法 —— **改之前先量**。
  `agent_desk/` 里除 `mod.rs`（CRLF）外全是 LF。**
  铁律不变：**一次外科手术只能改该改的那几行，绝不能顺带换掉整个文件的行尾**。
  用 Python 时，**绝不能用 `io.open(p, encoding="utf-8")` + `newline=""` 写回** ——
  读时会把 `\r\n` 归一成 `\n`，写回就把**整个文件**变成另一种行尾，
  于是 `git diff` 变成"全文重写"（AGENTS.md §4.6 明令禁止的假 diff）。
  正确做法：先量 → `open(p,"rb")` 读字节 →
  `replace(b"\r\n", b"\n")` + `replace(b"\n", 换行字节)` 按**原样**写回。
  改完用 `tr -cd '\n' | wc -c` 与 `tr -cd '\r' | wc -c` 对比确认没混合。
  （Edit 工具本身会保留原有行尾，不会污染。）
- `git add -A` 会连临时备份一起提交 —— `.gitignore` 已堵住三类噪音：
  `*.bak` / `*.bak-*`（v0.35.0）、`.workbuddy/shots|watch`（一次性取证产物）、
  `.tmp-lark/` / `draft_*_folder/`（飞书内容流水线的中间态）（v0.35.4）。
- **大提交别用 `git add -A` 一把梭**：多 agent 并行时它会吞掉别人**未完成**的中途状态。
  日常改动走精确路径。无论哪种，入库前先三件套：
  ① `git add -A -n` 干跑 + 逐文件 `stat` 排序（看体积与意外文件）
  ② 密钥扫描（`sk-…` / `api_key=…` 正则遍历待提交文件）
  ③ 提交后**复跑 `version:check`**（见上「版本文件被外部清空」铁律 2）。
