# easyop 项目长期记忆（精简版）

> 仅保留跨会话有效的事实；逐次过程写每日日志。

## 启动与真机联调
- 调试需同时后台运行 Vite（1420）和 `src-tauri/target/debug/easyop.exe`；debug 前端依赖 `devUrl`，无 Vite 会白屏。
- Vite 清缓存可能触发批量删除保护；目标仅限 `node_modules/.vite/deps`。
- CDP 选择主窗口使用 URL 正则 `~^http://localhost:1420/$`。
- TTS 配置在 `%APPDATA%\com.easyop.app\agent_desk.json` 的 `tts` 段；`config.json` 不含 TTS。Volc 的资源头取 `resource_id`，忽略百炼专用 `model`（展示/日志一律走 `active_model()`，见下文「配置字段的按分支才生效陷阱」）。
- HID 枚举返回全部设备，VID/PID 校验在打开阶段；蓝牙在线不等于 USB Feature Report 配置通道可用。
- `desk_tts_test` 只合成并保存 WAV，不播放。2026-09-24 真机验证双向流式 TTS 可用：48 kHz、229404 采样、4779 ms 音频、16 帧/13 句、墙钟 1389 ms。
- 有运行实例时 `cargo build` 可能 LNK1104；不要强杀用户实例。

## 任务台核心架构
- 硬件按键与页面按钮统一进入 `agent_desk::dispatch_task → resolve_driver → dispatch_into → pump_slot`；队列、完成判定、门禁、历史、播报、续接均在此链路。
- 原“向当前终端投命令”的出口已于 v0.35.0 删除；`delivery.rs` 仅供“启动终端”等待前台使用，`submit_after_paste` 也只影响该路径。
- `pump_slot` 顺序有依赖：认领→执行→终态→播报→留存结果→历史/产物→会话回写。播报早于历史落盘，前端历史用低频轮询，不能在播报回调立即拉取。
- 默认超时 180 s，CLI 预置应自设更长超时（当前 900 s）；同槽严格串行、跨槽并行，最大深度 8。
- 完成信号：HTTP 流结束/日志模式为 High；进程 0 退出/异步轮询为 Medium；`evidence_cmd` 可升 High。世界门禁只在基础执行成功后运行，可把成功翻为失败。
- Codex 续接使用 argv `resume <id>`，Windows 可执行名必须是 `codex.cmd`。
- 历史位于 `output_dir()/tasks/`：`index.jsonl` + 单任务日志，保留 200 条，先写日志再写索引。
- 产物判据：mtime 不早于任务开始 + 可选 glob；glob 匹配相对工作目录，`*` 不跨目录、`**` 可跨。`artifact_open` 默认 true；排序为 mtime 降序、路径升序以保证可复现。
- 播报支持 `Corpus/Text/Summarize`；二次总结必须在通知后台任务中执行，不能阻塞槽位。
- 默认执行器：doubao-api（启用）、codex-cli、dsh。

## 取消与执行反馈
- 取消必须终止整棵进程树：先 `taskkill /T /F /PID`，再 `child.kill()+wait()`；反序会丢失父子链。
- 管道收尾必须有硬超时：正常 2 s、终止后 300 ms，保证 `run()` 返回。
- API 取消以 future 竞速实现，取消时结果正文为空，避免半截输出被留存。
- 当前执行过程中仍无逐行反馈（`NullSink`），这是“等待感”主因；运行结束后才有完整日志。

## 字幕浮层
- 显隐只由后端决定：录音开始 show，各终态 hide；前端不得再根据状态推导显隐。
- 任务台字幕必须先发 `asr-final` 正文、再发状态；空 `asr-partial` 在统一入口过滤；派发后保留 1.8 s。
- 验证要同时检查窗口可见性、正文 DOM、状态 DOM；DOM 有字不等于用户可见。

## TTS 双向协议
- 单向帧：头+sequence+size+payload（flags `0b0001`）；双向帧：头+event+sid_len+sid+size+payload（flags `0b0100`）。帧形状错会静默 0 帧。
- 会话事件必须带非空 sid；仅事件 152 表示会话结束，351 是逐句结束；文本只放 `TaskRequest`。
- 双向握手：`1→50→100→150→200→35x→102→152→2→52`；FinishConnection 失败可忽略。
- `is_bidirection()` 仅在 URL 最后一段严格为 `bidirection` 时成立；Volc 双向要求 WSS。
- 探针：`scripts/probe_tts_bidirection.py`、`_probe_bidir_variants.py`、`_capture_bidir_frames.py`。
- ASR 潜在风险：最终空帧可能覆盖已有 `final_text`；当前非现症，若修应只在非空时覆盖。

## Wi-Fi 链路自动化（v0.37.0 起，动 `wifi_link.rs` 前必读）

- **⛔ 「连上了」的唯一可信判据 = 收到板子的 EIHB 心跳**（`is_online()`，**15 s** 超时，UDP 17333）。
  ⚠️ 这个超时值被改过（v0.37.1：5 s → 15 s），**别照旧资料写 5 s**；要引用就调
  `keyboard_audio::heartbeat_timeout_secs()`，文案里也别手写数字。
  **下发成功 ≠ 已连接** —— 只说明端点进了 NVS。前端「Wi-Fi 已连接」徽标只读后端的 `board_online`，
  不许再本地硬写。历史上正是"点一下连接就本地置真"导致"点了成功但麦克风不出声"可以一直挂着。
- **⛔ 每一次配置下发都必须带 Wi-Fi 端点**（`payload::build_firmware_payload_with_wifi`）。
  固件 `ConfigState::apply_json` 是**整体替换**语义：未提及的字段一律回默认值 ⇒
  漏带端点会把 `wifi_ssid`/`audio_host` 清成空串 ⇒ `audio_ready()` 为假 ⇒ 板子**主动释放 Wi-Fi、
  麦克风静默失效**。（旧 `build_firmware_payload(profile)` 已删除，就是为了从类型上消灭这类漏带。）
- **四环节**：检测（每秒 tick，指纹 `epoch = 当前SSID|首选IPv4`）/ 自动连接
  （T1 开机 3 s 后 · T2 USB 通道出现 · T4 离线 > 20 s；退避 0/5/15/30/60 s）/
  历史（`output_dir()/wifi_known.json`，上限 16，只在**离线→在线跃迁**记一次）/
  快速重连（端点已在 NVS ⇒ **板子自己连**，约 10 s 一轮；T3 环境指纹变了才重下发）。
- **首次配网必须插 USB**：端点没进 NVS 时板子不在网上，UDP 直投这条路（服务端判 `from == dest`）必然不通。
- 麦克风来源 ≠ Wi-Fi 时整条链路**主动停摆**（`state=disabled`），这是故意的。
- 板子端**没有**扫描环境的能力（未改固件、未加 Report ID）；判断全在上位机。
- 历史文件里**密码明文**（与 `config.json` 同级，未新增暴露面）；迁移是"只增不清"。

### 心跳判据与"假离线"（v0.37.1，用户报"Wi-Fi 偶尔断开"的真根因）

- **心跳周期 2 s vs 超时 5 s ⇒ 只容忍连丢 1 个包。** 实测空档 **7748 ms**（连丢 3 个），
  一小时冒 9 次「中断 → 恢复」且成对间隔只有 1～16 秒 ⇒ 那是判据太紧，**不是 Wi-Fi 不稳**。
- **真正的伤害是丢音，不是图标闪。** `resolve_mic` 按 `is_online()` 选录音源 ⇒
  翻转瞬间退回**电脑麦克风** ⇒ 用户对着板子说话却录到静音，提示只写"空结果"。
  用户说的"不稳定"绝大多数是**录到静音**。排查这类报障先查录音源切换，别只看徽标。
- **两处 `now_ms()` 必须用单调时钟**（`OnceLock<Instant>`）：判据问的是"过了多久"，
  与"现在几点"无关。用墙钟时 NTP 校时会让差值跳变 —— 往前跳 = 凭空一次离线；
  **往后跳 = `saturating_sub` 归零 ⇒ 真掉线也不报**（静默失效，比误报危险）。
  真实时刻（`last_success_unix`）才用墙钟，两者不混用。
- **`+1` 不是笔误**：`0` 在本仓库是"未开始 / 从未发生"的哨兵值，业务时间戳取到 0 会被读成"还没开始"。
- **抖动不计入成功**（`RESUMED_FLAP_MS = 30_000`）。原先每次"离线→在线"都 `mark_success`，
  而 `success_count`/`last_success_unix` 正是历史表的**排序依据** ⇒ 抖得最凶的网反而被推得最高。
  实测某网 `success_count` 已到 17，而它真实只连过 2 次。
- **判据抽成纯函数 `counts_as_success(offline_ms_before, ever_online_before)`，日志与历史共用同一调用** ——
  不允许出现两份证据互相打架。⚠️ 别用 `offline_ms_before == 0` 表示"首次连接"：
  `offline_since_ms` **刻意**在"第一次评估到非在线"就起表（否则 T4 永不触发）⇒ 启动时会假命中最先那个 tick。
  **"从未在线过"与"刚离线 0 毫秒"是两件事，不能用一个数值兼任。**
- **"确认 + 写历史"必须在选网之后。** 它要 `target_ssid` 兜底，而那个字段由**下发分支**写入 ⇒
  心跳先于启动延迟（3 s）到达时目标恒为 `None` ⇒ 板子常开则历史**永远积累不起来**（而历史表正是选网依据）。
- ⚠️ 未跑完的闭环：「抖动不计成功」需板子真离线 ≥ 45 秒再回来。判据已由单测钉住，
  且该日志路径已真机触发过一次。**下次板子在场且方便断电时补验。**
- ⚠️ 待查风险：`netsh wlan show networks` 同步阻塞 tick，可能让 `offline_since_ms` 时间测量偏斜。
- ⚠️ **注入式验证前必须先确认真信号源不在场**：`.workbuddy/inject_heartbeat.py`（往 17333 注入 EIHB 帧）
  首次跑无效 —— 我把"USB 不在位"当成了"无心跳源"，实际板子 Wi-Fi 那头一直在发 ⇒
  注入被真流量淹没，两条路刷同一时间戳，连"离线"都造不出来。前提已写进脚本文件头。

### 配置字段的"按分支才生效"陷阱（v0.37.1）

- `TtsConfig` 里 `resource_id`（豆包 / `X-Api-Resource-Id`）与 `model`（百炼）**按 provider 才生效**。
  ⇒ 任务台卡片原先读 `cfg.tts.model` 就一直显示陈旧的 `seed-tts-1.0`，而界面填的是 `seed-tts-2.0`。
- **展示与日志一律走 `TtsConfig::active_model()`**（按 provider 分流），三处展示点已统一。
  ⛔ **不能**用"哪个字段非空取哪个"兜底：切换过 provider 的配置两个字段都非空，猜错就把不生效的显示出来。
- **通用自查口径**：结构体里有"按分支才生效的字段"（provider / 模式 / 协议）时，
  任何 `cfg.xxx.field` 的**展示与日志**都可能是错的 —— 值得全局 grep 一遍 `\.field`，确认每个都经过分流函数。

## 已知未实现/受限能力
- 槽位“按能力/自动判定”未实现。
- `RunOutcome` 无独立 stderr，历史 stderr 段为空；失败原因在截断后的 reason。
- 历史面板没有打开产物入口。
- 未实现：AsyncTaskPoll、运行中实时日志、失败自动重试、键盘喇叭播放、板端灯语 0x16。
- 外部会话监工播报仍用固定语料。

## 协作与版本纪律
- 版本唯一真源为 `package.json`；同步**五处**：tauri.conf.json / Cargo.toml / package-lock.json /
  **src-tauri/Cargo.lock**（第 5 处曾被漏掉 —— 它是唯一会被 `cargo build` 自动改写的一处，
  漏同步会让版本号在"一致 / 又不一致"间来回摆）。任何改动同步 CHANGELOG、README，
  并以 `npm run version:check` exit 0 收工。
- CDP 工具 `.workbuddy/cdp_eval.mjs`：`list` / `eval` / `evalfile` / `shot <png>`。
  取证顺序：先 eval 查 `isVisible()` + `getBoundingClientRect()`（DOM 有字 ≠ 看得见），再 `shot` 看版式。
- `cargo test --lib` 必须加 `--test-threads=1`。
- 多 agent 环境禁止回退他人改动、顺手格式化、批量换行、盲目 `git add -A`。
- Rust 文件行尾混合；修改前检测，二进制方式保留原行尾，避免整文件假 diff。
- CHANGELOG/README 顶部曾被外部进程两次覆盖：提交前备份，提交后再次跑 `version:check`；失败时先检查版本节是否被删。
- 入库前：精确暂存/干跑检查、密钥扫描、提交后复验版本。
- `.gitignore` 已覆盖备份、取证截图/监控、飞书流水线临时目录。
