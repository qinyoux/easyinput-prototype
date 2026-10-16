# easyop 项目长期记忆

> 仅保留跨会话有效事实；逐次过程写每日工作日志。

## 核心架构与任务台
- 硬件按键/页面按钮统一进入 `agent_desk::dispatch_task → resolve_driver → dispatch_into → pump_slot`；队列、完成判定、门禁、历史、播报、续接集中在此链路。
- 同槽严格串行、跨槽并行，默认超时 180 s，CLI 预置可用 900 s，最大深度 8。完成信号分 High/Medium，`evidence_cmd` 可升 High；世界门禁仅在基础执行成功后运行。
- `pump_slot` 顺序依赖：认领→执行→终态→播报→结果留存→历史/产物→会话回写。播报早于历史落盘，前端历史用低频轮询。
- 历史在 `output_dir()/tasks/`，`index.jsonl` + 单任务日志，保留 200 条；产物按任务开始时间和 glob 判定。
- 运行时灯态必须使用后端纯函数 `LightState::from_queue`，前端只做颜色映射；running 用橙色，backlog 表示有积压但无人运行。
- 未读语义不可混用：`desk_ack_playback` 表示听完并递减；`desk_discard_unread` 表示用户主动放弃并清零。任何自动流程不得丢弃未读。

## 音频、网络与固件
- 当前已有 ESP32-S3 Wi-Fi 音频输入、UDP/EIAU、PTT、流式 ASR、豆包/百炼 TTS、PC 播放和任务播报；板载扬声器下行尚未形成闭环。
- **Codex CLI 沙箱与联网（v0.43.3）**：任务台 codex-cli 预置带 `--sandbox danger-full-access`
  （完全去沙箱，网络+全盘不设限；代价=模型命令零隔离，仅适用本机自用）。
  根因两层：`codex exec` 默认 workspace-write 策略禁网 + 本机
  `~/.codex/config.toml` 的 `[windows] sandbox = "elevated"`（Windows 原生沙箱，防火墙挡网）。
  ⚠️ 该 config.toml **顶层** `network_access = "enabled"` 是无效写法——codex 只认
  `[sandbox_workspace_write]` 表内同名字段（钦友个人全局配置，应用不代改）。
  老配置靠 `migrate_presets` 世代 3 方向三补旗标（只补一个沙箱旗标都没有的；
  `SANDBOX_FLAG_CANDIDATES` 三种形态任一出现即视为用户已表过态）。
  实测对照：无旗标 curl exit 1/`000`；有旗标 exit 0/`200`。
  `codex sandbox windows` 自身起不来（CreateProcessAsUserW error 2），不能当沙箱测试工具。
- TTS 配置按 provider 分流，展示/日志必须使用 `active_model()`，不能简单取非空字段。
- Wi-Fi 在线唯一可信判据是 EIHB 心跳，当前超时 15 s；每次配置下发必须带完整 Wi-Fi 端点。首次配网必须插 USB。
- 固件状态回读：Feature Report `0x13` 请求，`0x11` kind `0x04` 分块响应；必须同一 HID 句柄先写后读，畸形请求会 fail-closed。固件源码在独立仓库 `E:\MY_AI\trae\ESP32_S3_BOT\skills\easy-input-maker\`。
- TTS 双向流协议事件/帧形状严格，双向要求 WSS；已有探针脚本。当前实测双向 TTS 可用，但应用侧仍主要完整合成后播放。

## 稳定性与安全约束
- `AppHandle` 跨线程 Rc abort 已通过 `vendor/tao` Arc 补丁根治，勿删除补丁；`diag_clone_storm` 仅用于诊断，不接 UI。
- 取消需先终止整棵进程树，再回收子进程；管道收尾必须有硬超时。当前执行中逐行反馈仍不足，容易造成等待感。
- 密钥/配置安全仍需继续收口；插件和未来 MCP 工具应使用能力白名单，不开放任意程序路径执行。

## 版本与协作纪律
- `package.json` 是唯一版本真源；用 `scripts/version.mjs` 同步 tauri/Cargo/lock 文件。改动必须同步 `CHANGELOG.md`、`README.md`，收工执行版本一致性检查。
- **打包必须走 `npm run tauri:build`**：手 `cargo build --release` 缺 `custom-protocol` 特性 ⇒ dev 模式包（前端没内嵌，双击报 ERR_CONNECTION_REFUSED）。产物是便携 exe（无 installer bundle），按惯例归档到 `release/easyop-v<版本>.exe`。
- 多 agent 共树禁止回退他人改动、顺手格式化、盲目全量暂存；Rust 混合行尾修改前需保护行尾。验证失败要区分主工作区他人半成品与本次改动。
- 工作区记忆每日追加到 `.workbuddy/memory/YYYY-MM-DD.md`；本项目不删除 `.workbuddy`。

## 已知未实现
- MCP Runtime、统一 Conversation Orchestrator、运行中实时日志、失败自动重试、键盘喇叭播放/ESP32 音频下行、离线唤醒词、声纹和视觉能力尚未完整实现。
- 播报排队机制遗留（v0.42.0）：排队只覆盖终态类；`ResultsWaiting` 只报条数不报槽位；队首等待无实时进度。
- 当前重点：把小智式速度分析纳入方案，先补齐 trace 测量，再优化 LLM 增量可见、句子级 TTS、短播报和 CLI 管道收尾。

## SS 命令面板与Everything（2026-10-13 定位）
- easyop 的 SS 命令面板是**自带独立实现**，与 `E:\MY_AI\workbuddy\Super_Startup\` **无任何依赖**（全仓0 处引用）。
  相关文件全在 easyop 内：`src-tauri/src/everything.rs`、`public/ss_palette.html`、`src/components/GeneralSettings.tsx`。
- ⚠️ **已修复（v0.41.7）**：`everything.rs` 曾用 `es.exe -csv` 读 stdout（实为 ACP/GBK 字节）
  却调 `String::from_utf8_lossy` 按 UTF-8 解 → 中文变 `�`。已改为 `-export-csv <tmp> -utf8-bom`
  +剥BOM + `String::from_utf8`（严格）。**教训：`from_utf8_lossy` 只保证不 panic，不代表解码正确。**
- **跨进程只有字节，没有编码契约**：es.exe 是 ANSI 程序，管道是裸字节流，不做编码协商。
  「调用关系」≠「编码一致」。`-csv` 不等于 UTF-8，唯一 UTF-8 出口是 `-export-csv`+`-utf8-bom`。
- easyop 全仓**0 处引用 Everything SDK**（无 libloading/LoadLibrary），只用 es.exe CLI。
- 教训：项目名相似（easyop「SS 面板」vs Super_Startup「SS」）极易误判，跨项目诊断前先验依赖边界。

## 任务台播报三 BUG 根因（2026-10-13 诊断 → v0.42.0 已修复，各配回归测试）
- **BUG 1「下发即播任务已完成」= 外部会话监工误报**：`codex_probe` 的 24h mtime 窗口 + `seen` 回收，
  使任务台自己 `codex exec resume` 追加写的旧 rollout 被当「全新文件」从偏移 0 整读，
  全历史 task_complete 变新 hit，`hits.first()` 播出最旧一条（23 天前）。
  已修复：首见一律按 EOF 登记（删 primed 分支，消灭整类误报）+ 桌面任务有槽在跑或完成后
  10s 宽限内 probe 只消费偏移不出声（`last_desk_finish_ms` + `PROBE_GRACE_MS`）。
- **BUG 2「只播一小段」= 音频保活截止时间单位错**：`audio_out.rs` 把 `total_from_pcm` 的**毫秒**
  当样本数套 `micros = ms*1e6/rate`，48kHz 下时长缩 1/48 ⇒ >10.3s 的播报在 `0.02×时长+10s` 被 drop 流掐断。
  已修复：`PlaybackControl::duration_ms()` 是设备侧权威值（含重采样/声道展开），`keepalive_deadline(毫秒进毫秒出)`；
  调用方等播完必须用 `duration_ms()`，禁止自己估算。**没有也不该有播放时长设置**；
  真旋钮是 `summary.max_chars`（该音色 ~6.6 字/s，252 字≈40s 过长，建议 80–120）。
- **BUG 3「S2 完成静默进未读」= NoticeGate 全局一本账 + 后到抢占**：「同类 kind 不重复播」四槽共用
  ⇒ S1 完成后 S2 同名终态被吞。已修复为**两套账本**：进度类走全局账本（跃迁+最小间隔，原逻辑）；
  事件类（起点/终态）按 `(slot, kind)` 记账 + `EVENT_REPEAT_GUARD_MS=3_000`（只挡同拍双送达）。
  **终态播报排队**：`notice_queue` cap 3、满丢最旧+计数、单飞排水、排空后补 `ResultsWaiting` 合并提醒；
  `may_start_playing`：结果可接管一切、进度类后到接管、终态绝不抢占。
  `dispatch_task` 的 `emit_notice(TaskStarted)` 必须在 `emit_state_if_changed` **之前**（同拍双 notice 否则互抢）。
- `NoticeKind` 闭集 12 类（`ResultsWaiting=11` 只能末尾追加，`code()` 是缓存文件名契约）；闭集变更后需重跑「预合成语料」补齐。

## 播报排队三条遗留（2026-10-15 → v0.44.0 全关）
- **① 进度类不再"被占即丢"**：纯函数 `enqueue_decide` 返回 `Append / Replace / Drop{reason}`；
  终态必排队；进度类**队里有终态则过期丢弃**（FIFO 下永远排在终态后，终态播完时任务多已结束），
  队里有同类则 Replace 只留最新。判据是**价值的时间衰减曲线**（终态不衰减/进度衰减/事件前端实时可见）。
- **② 合并提醒记明细而非计数**：`notice_dropped` 从 `AtomicUsize` 改为
  `Mutex<Vec<DroppedNotice { kind, slot }>>`；正文"还有 2 条（S1 任务已完成、S3 任务失败）"，
  >3 条退化纯条数（TTS 按字数计费）。进度类被丢**不记**明细（过期进度报进提醒是噪音）。
- **③ 队首等待实时可见**：新事件 `agent-desk-channel`（`playing` + `notice_queue`）+ 前端喇叭实况区
  （在播什么 / 排几条 / 队首单独点出"下一个轮到谁"）。

## 播报通道机制（易复发的坑）
- **触发模型不同必须分事件**：`agent-desk-state` 由任务状态驱动；
  `agent-desk-channel` 由**播放动作**驱动（播完/抢占/入队都发生在任务状态没变的时刻，
  等状态事件再变时等待期已经结束）。
- **同一份投影只能算一遍**：`project_channel` 是喇叭实况唯一投影，快照（5s）与实时事件**同源取值**；
  两处各算一遍会漂移成"喇叭响着 A、界面写着在播 B"，这类 bug 排查时最难重现。
- 事件"真变化才发"用**内容比对**（derive `PartialEq` + `last_channel_state`），
  **不用版本号** —— 并发播放下版本号必然错序。
- ⚠️ 持 `notice_queue` 锁时**禁止**调会重入该锁的函数（`Mutex` 重入即死锁）；
  `enqueue_notice` 把决策结果记成 `queued` 布尔带出作用域后再推事件。
- `speak_notice` 带了 `app` 参数：播放起止都在它内部，调用方在外面拿不到准确时刻。
- 前端"事件 + 轮询"混合时，轮询只作**初值兜底**（`channelSeenRef` 门控）——
  无条件覆盖会因请求往返延迟让界面在"在播/空闲"之间来回跳，比慢 5 秒更困惑。
- 排队规则的失败形态是**静默的**（听不到、也无报错），只能靠纯函数单测钉死。
- 枚举变体带字段时，测试里想忽略字段必须 `matches!(.., Variant { .. })`；
  写 `{ reason: _ }` 编译失败（`_` 只能做赋值左值，E0599）。

## 主工作区当前编译状态（2026-10-13）
- ⚠️ `cargo build` **失败**，原因是**其他 agent 的半成品**，非本项目改动：
  `live_translate.rs`（`RecordedSentence` 缺 `start_ms`/`end_ms` 初始化）、
  `plugin_registry.rs`（`PluginManifest` 新增 `hidden` 字段后测试构造器未跟上）。
- 验证手法：把目标模块抽成独立 crate 编译测试，可绕开他人半成品拿到干净结论（本次12/12 通过）。
## 内置插件部署纪律（2026-10-12 事故后立规）
- 插件运行时只读 `%APPDATA%\com.easyop.app\plugins\`；仓库 `plugins/builtin/` 只是模板，**改仓库不生效，必须 `node scripts/deploy-plugins.mjs <id>`**。
- **部署前先 diff 运行目录 vs 仓库**：运行目录那份可能是用户/agent 直接改过的唯一副本（live-translate 曾因此丢功能，无 git 无历史不可恢复）。
- deploy-plugins.mjs 已内置覆盖前自动备份（`<id>.bak.<时间戳>`），但「先看再部署」仍是第一纪律。
- live-translate 插件**未入 git**；studio/stylecam/demo 已入 git 与否以 `git log -- plugins/builtin/<id>` 为准。
