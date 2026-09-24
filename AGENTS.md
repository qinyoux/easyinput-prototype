# AGENTS.md — easyop 协作公约

> 适用对象：**所有**参与本项目的 AI 编码助手（WorkBuddy / Trae / CodeArts Doer / Codex CLI 等）
> 与人类维护者。**开工前先读本文件，收工前按 §2 逐项自检。**
> 本项目由多个 agent 并行修改同一个工作树，凡是"看起来只影响自己"的随意改动，都会变成别人的事故。

---

## 1. 一句话铁律

**没有同步更新版本号与变更说明的改动，等于没做完的改动。**

原因不是形式主义：多 agent 并行时，代码 diff 会互相覆盖、难以归因，
而 `CHANGELOG.md` + 版本号是唯一能回答"这个行为是哪版引入的、谁改的、为什么"的东西。

---

## 2. 收工清单（Definition of Done）

改动完成后，逐项确认：

- [ ] **1. 代码/文档实际改完**，且本地验证过（见 §5 验证口径）
- [ ] **2. 升版本**：`npm run version:bump -- minor`（或 `patch` / `major`，见 §3）
      —— 若同一批改动由他人先升过版，则**不再重复升**，只补自己的条目
- [ ] **3. 填 `CHANGELOG.md`** 新版本节：按 `新增 / 变更 / 修复 / 移除` 归类，
      写清**改了什么 + 为什么 + 对既有配置有无影响**；没做完的功能必须写进 `### ⚠️ 未完成`
- [ ] **4. 填 `README.md` §版本历史** 同版本摘要（面向用户，1～5 条，说人话）
- [ ] **5. 跑 `npm run version:check`**，必须输出 `结论：✅`（退出码 0）
- [ ] **6. 跑构建**：`cd src-tauri && cargo check`；动了前端再跑 `npm run build`
- [ ] **7. 提交**（见 §4）：`git add -A && git commit -m "vX.Y.Z: 一句话"`

> 第 2 步的脚本会**自动插入骨架**，骨架只是占位符。
> **占位符没填成真内容就提交，视同未完成**——`version:check` 只校验"条目存在"，不校验"内容为真"。

---

## 3. 版本号怎么定

版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)，**唯一真源是 `package.json` 的 `version`**：

| 档位 | 什么时候用 | 例 |
|---|---|---|
| `major` | **破坏性变更** —— 见下方"对外契约"清单 | 配置格式不兼容、命令签名变更、固件协议变更 |
| `minor` | **新增能力** —— 新命令、新面板、新插件能力、新集成类型 | 本版 v0.7.0 插件发现层 |
| `patch` | **修复 / 文案 / 样式 / 纯文档**，不改行为 | v0.6.1 提示词库文案统一 |

**对外契约清单（动其中任何一项都是 `major`，且必须在 CHANGELOG 显式标注「破坏性」）：**

- 固件保留的 **Host Action UUID 映射**（硬件键通道）
- 配置文件格式：`profiles.json` / `mini_apps.json` / 插件清单 `plugin.json` 字段
- 前端 `invoke` 调用的 **Tauri 命令名与参数签名**（`src/api.ts` ↔ `lib.rs` 的 `generate_handler!`）
- 三类通信协议编解码（EIAU / EIHB / EICC / EICA）

> 拿不准是 minor 还是 major，就问一句："老用户的配置/固件还能不能直接升级上来？"
> 能 → minor；不能 → major。

---

## 4. 多 agent 协作纪律

**工作前**

1. `git status` + `git log --oneline -5`：工作树脏先搞清是谁的改动，别当成自己的基线
2. 读 `CHANGELOG.md` 最新两节，确认自己不是在重做别人的活
3. 大改动先写进 `docs/ROADMAP.md` 或对应规范文档，再动代码

**工作中**

4. **禁止回退他人未提交的改动**：`git checkout -- <file>` / `git reset --hard` / `git clean -fd`
   一律不用；确需回退先问人
5. **同一文件不要长时间持有**：改完尽快提交，缩短并行窗口
6. **不做"顺手格式化"**：不要重排无关文件、不要换行符转换、不要批量改引号/
   缩进风格 —— 这是多 agent 协作里最典型的假 diff 制造机
7. 一次提交只做一件事；不要把"重构 + 新功能 + 修 bug"塞进一个 commit

**提交信息格式**

```
vX.Y.Z: 一句话概括

- 要点一（含关键文件）
- 要点二
- ⚠️ 未完成 / 已知问题（如有）
```

**边界必须说清**

8. 没写完的功能，在 CHANGELOG 的 `### ⚠️ 未完成` 里写明**缺什么、卡在哪、下一步走哪条路**。
   宁可留一条明确的"没做完"，也不要让下一个 agent 以为它能用。

---

## 5. 验证口径

"改完了"的判定标准是**跑出来的**，不是"看起来对"：

| 改动类型 | 必须跑的验证 |
|---|---|
| 版本 / 文档 | `npm run version:check` 退出码 0 |
| Rust 逻辑 | `cd src-tauri && cargo check`；有单测的模块跑 `cargo test --lib <模块名>` |
| 前端 / 面板 | `npm run build`（含 `tsc`）；面板改动另需人眼在 `npm run tauri dev` 里确认 |
| 硬件联动 | 明确写"未经真机验证"，不要描述成已验证 |

**未验证的改动必须在 CHANGELOG 或回复里标出来**，用词："未在真机验证"、"仅编译通过"。

---

## 6. 文件地图（改哪先看哪）

| 文件 | 职责 | 谁维护 |
|---|---|---|
| `AGENTS.md` | 本文件：协作与版本纪律 | 全体 |
| `CHANGELOG.md` | **开发者视角**变更说明，每版一节，只增不改旧节 | 全体 |
| `README.md` | 用户视角说明 + §版本历史摘要 | 全体 |
| `docs/easyop 插件系统设计文档.md` | **插件系统综合设计文档**（v2.0）：合并原 5 份文档，涵盖架构 / 接口 / 生命周期 / 通信 / 安全 / 内容台 / 调试 / 已知问题 | 改插件系统时同步 |
| `docs/任务台使用说明.md` | **任务台 / 执行器面向使用者的完整说明**：定位（异步+通知 / 可追溯两半）、设计依据五条、实操、播报三项设置、**产物判据**、**任务历史**、与手动终端的对比、排错对照表、**未实现能力清单** | 改任务台 / 执行器 / 槽位 / 完成判定 / 产物发现 / 任务历史时同步 |
| `docs/小智式AI接入easyop实施方案.md` | 小智式语音 Agent 接入 easyop 的目标、架构、阶段实施、验证、验收、风险与优化基线 | 改 Voice Core / ASR / LLM / TTS / 工具 / MCP / ESP32 音频下行时同步 |
| `docs/ARCHITECTURE.md` | 系统架构与模块划分（已合并入综合设计文档，保留作详细参考） | 结构性改动时同步 |
| `docs/easyop 扩展机制规范.md` | 插件/扩展机制规范（已合并入综合设计文档，保留作详细参考） | 改扩展机制时同步 |
| `docs/插件集成指南.md` | **插件开发者视角**（已合并入综合设计文档，保留作详细参考） | 改插件机制时同步 |
| `docs/路径A-插件页签接口契约.md` | **冻结契约**（已合并入综合设计文档，保留作详细参考） | **任一侧改接口前先改它** |
| `docs/内容台规范.md` | 内容台插件规范（已合并入综合设计文档，保留作详细参考） | 改内容台时同步 |
| `docs/灵感来源扩展评估.md` | 灵感源可行性与难度评估 | 调研类，仅追加 |
| `docs/桌面宠物-可行性与优化评估.md` | 桌面宠物的可行性与优化空间 | 调研类，仅追加 |
| `docs/企业助手方案v3-架构符合性评估.md` | 《详细实现方案v3》逐项核验 + 模块落点订正 | 调研类，仅追加 |
| `scripts/version.mjs` | 版本号同步 + 校验工具 | 改流程时同步 |
| `scripts/deploy-plugins.mjs` | 把 `plugins/builtin/*` 部署到 `%APPDATA%\com.easyop.app\plugins\`（`--list` 干跑列清单、`--dry` 只看不写） | 改插件目录约定时同步 |
| `scripts/tidy-target.mjs` | 回收 `target/` 里本 crate 的死产物（`cache:tidy`）。**别把 `cargo clean -p easyop` 那步去掉** —— 它同时删 fingerprint，只删文件会让下次编译误判"新鲜" | 改构建/清理流程时同步 |
| `plugins/README.md` | 插件目录约定 + 命名空间铁律 + `eo.*` 宿主 API 实测状态表 | 改插件体系时同步 |
| `plugins/builtin/studio/` | **内容台插件**（六阶段台账 + 生成 + 组稿导出）；`plugin.json` 声明 11 命令 5 tools，`index.js` 单文件自包含 ESM，`tools/studio_io.py` 纯标准库组稿脚本，`test/harness.mjs` 111 项验证台 | 改内容台时同步 |
| `plugins/builtin/demo-quick-note/` | 示范插件（`demo.quick_note`），用于验证发现层→激活层最小闭环 | 改激活层契约时同步 |
| `src/pluginHost.ts` | 前端宿主：监听 `plugin:activate` → blob `import()` → `activate(context)`；`context` 含 `pluginDir` | 改宿主 API 时同步 |
| `src-tauri/src/plugin_registry.rs` | Rust 发现层：扫 `app_config_dir()/plugins/<id>/plugin.json`（**只扫一层，不递归**） | 改清单字段时同步（属对外契约） |
| `src-tauri/src/plugin_host.rs` | Rust 激活层：emit `plugin:activate`（payload 含 `id`/`dir`） | 改激活契约时同步 |
| `src-tauri/src/agent_desk/mod.rs` | **任务台编排器**（唯一对外模块）：认领 → 执行 → 收终态 → 播报 → 落历史 → 推下一条。`pump_slot` 的五步顺序有依赖，改前先读那一段的注释 | 改编排顺序 / 完成判定 / 播报触发时同步 |
| `src-tauri/src/agent_desk/history.rs` | 任务历史：`index.jsonl`（追加语义）+ 每条一个 `.log`，留 200 条，裁剪时**连日志一起删**。新命令 `desk_task_history` / `desk_task_log` | 改历史字段 / 上限 / 落盘位置时同步（**字段属对外契约**，前端面板读它） |
| `src-tauri/src/agent_desk/artifact.rs` | 产物发现（时间窗 + 可选 glob）+ 打开策略。**glob 是相对工作目录匹配**，跳过名单不含 `dist`/`build`/`out` | 改判据 / 排除名单 / glob 语义时同步 |
| `src-tauri/src/agent_desk/delivery.rs` | 仅剩"等外部终端到前台"的通用工具（出口 A 删除后收窄），供 `cli_terminal.rs` 用 | 改终端判定时同步 |

**代码侧关键约定**：配置目录 = `%APPDATA%\com.easyop.app`（`app_config_dir()`）；
`tauri.conf.json` 的 `withGlobalTauri: true` 与 `csp: null` 是现有实现的前提，改动需说明理由。

---

## 7. 禁区

1. **不提交**：`node_modules/`、`src-tauri/target/`、`dist/`、`*.log`、
   `.codeartsdoer/`、`.codegraph/`、编辑器目录（已在 `.gitignore`）
2. **不擅自改**：固件保留 UUID 映射、`tauri.conf.json` 的安全项、已验证的对外契约
3. **不删他人文件**：包括看似废弃的文档、日志、评估稿 —— 拿不准就问
4. **不在源码里写死密钥**：API key / token 一律走配置目录，不入库

---

## 8. 命令速查

```bash
npm run version:check        # 校验：五处版本号一致 + CHANGELOG/README 有对应条目
npm run version:sync         # 以 package.json 为准，同步其余四处版本号（含 src-tauri/Cargo.lock）
npm run version:bump -- minor   # 升版本 + 同步 + 插入 CHANGELOG/README 骨架（patch|minor|major|x.y.z）

npm run plugins:list         # 干跑：列出将被部署的插件（不改磁盘）
npm run plugins:deploy       # 把 plugins/builtin/* 拷到 %APPDATA%\com.easyop.app\plugins\
npm run plugins:test         # 跑到插件验证台（studio/test/harness.mjs）

cd src-tauri && cargo check     # Rust 编译检查
cd src-tauri && cargo test --lib plugin_registry   # 单测（--ignored 可跑真机扫描）
cd src-tauri && cargo test --lib -- --test-threads=1   # ★ 全量单测：必须加这个参数，见下方注
npm run build                   # 前端构建（tsc + vite）
npm run tauri dev               # 联调

npm run cache:tidy -- --dry     # 干跑：看 target/ 能回收多少（只删死产物，保依赖缓存）
npm run cache:tidy              # 回收 target/ 里本 crate 的死产物（秒级，保依赖缓存）
npm run cache:purge             # 备选：cargo clean 全清（连依赖，下次全编 15~20 分钟）
```

> **`target/` 是只增不减的**：产物按 fingerprint（rustc 版本 / feature / profile / 环境变量）
> 寻址，任一项一变就换一套新文件名，旧的 Cargo 永不回收；编译被中断（存盘触发连环重编、
> Ctrl+C、强杀）还会额外留下 `.rcgu.o` 中间文件。实测本项目 6 天积到 54 GB。
> **动手前若 `du -sh src-tauri/target` 已到几十 GB，先跑 `npm run cache:tidy`** ——
> 它秒级回收、只重编本 crate（2~4 分钟），比 `cargo clean` 的全编（15~20 分钟）划算得多。
> 另：`Cargo.toml` 的 `crate-type` **只留 `rlib`**，别把 staticlib/cdylib 加回来（桌面版用不到，
> 每轮编译白写 1.1 GB），除非要构建移动端。

> **插件部署后需重启 easyop**，发现层在启动时扫描 `%APPDATA%\com.easyop.app\plugins\`。
> 冒烟路径：侧边栏点「内容台」→ 六阶段台账可用。

### ⚠️ 三个会吃掉你半小时的坑（2026-09-22 实测）

**1. 跑全量单测要加 `--test-threads=1`。**
`cargo test --lib`（默认并行）**卡死过一次**：编译早已结束，测试二进制进程活着、
内存只有 26 MB（不是在干活），**23 分钟没有任何输出**。
同一份代码加 `--test-threads=1` 后 **399 passed / 0 failed / 3 ignored，44 秒**跑完。
看起来是某个用例在并行下等一个永不到来的信号，**具体是哪条尚未定位**。

- 排查技巧：Rust 的测试输出是按行缓冲的，卡住时 `tail` 也拿不到东西。
  想看"卡在哪条"，用 `> 日志 2>&1` + `--test-threads=1`，然后
  `grep -E "^test " 日志 | tail -1` 就是最后一条跑过的用例。

**2. 改 Rust 代码前先停掉 `npm run tauri dev`。**
- 两者争 `target/debug/.cargo-build-lock`，`cargo check` 会报
  `failed to open: …\.cargo-build-lock / 拒绝访问。(os error 5)`。
- 更要紧的是：dev 实例**有文件监听**。改了 `src-tauri/src/**` 它会自动重建并重启，
  **启动时就会把配置迁移跑掉**。本项目的配置迁移默认是幂等的，但
  **有损迁移**（如删除退役执行器）一旦跑过就回不去了。
  想先备份真实配置再迁移，顺序只能是：**停 dev → 备份 → 改代码**。
- **顺带一提：`npm run version:bump` 本身就会踩这条。** 它会写
  `package.json` / `src-tauri/Cargo.toml` / `tauri.conf.json` ——
  如果此时有个实例正跑着，`package.json` 的变动可能让 vite 重刷页面
  （用户正在界面上填的东西会丢），`Cargo.toml` 的变动在 `tauri dev` 下会触发整轮重编译重启。
  ⇒ **收工前 bump 版本，要么先确认没人在用这个实例，要么接受"界面会闪一下"**。
  实测：直接跑 `target/debug/easyop.exe`（不走 `tauri dev`）时只发生页面重刷，
  进程未被重启（pid 不变），但 webview 内存会掉一截——那就是重刷过的痕迹。

**3. 对已启动的 GUI 做自动点测前，先确认没人在用这个实例。**

本项目是多人（多 agent）+ 同一个人机共用一台机器。模拟点击是**往别人的鼠标底下抢**：
- 症状：连拍几张截图**内容都在变**（页面自己跳段、输入框轮番获得焦点、模型名被人改过、
  弹出"已保存到 …"），怎么算坐标都点不中 —— 因为坐标对应的东西已经不在那儿了。
- 更坏的结果：对方正在编辑、未保存，你这一下可能点到「保存」或「删除」，
  直接覆盖掉他的输入。

**开工前的三个探针（都很快）：**

```bash
# ① 配置文件近期有没有被写（< 2 分钟 = 有人正在操作界面）
ls -la --time-style=full-iso "$APPDATA/com.easyop.app/config.json" \
                            "$APPDATA/com.easyop.app/agent_desk.json"

# ② 隔 2 秒连拍 3 张，md5 相同才是静止的
for i in 1 2 3; do ...shot shot_$i.png; sleep 2; done; md5sum shot_1.png shot_2.png shot_3.png

# ③ 真要点，用"聚焦并**校验**前台窗口"的脚本；SetForegroundWindow 会静默失败，
#    不校验就会点进浏览器（详见 docs/ 里的截图脚本约定）
```

三条里任意一条报警，就**别点**，改成"让使用者自己点"或"改用 DOM 级驱动"。 
