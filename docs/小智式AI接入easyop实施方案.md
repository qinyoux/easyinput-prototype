# 小智式 AI 接入 easyop 实施方案

> 版本：v0.35.0
> 日期：2026-09-23
> 状态：实施设计基线，尚未开始编码

## 1. 结论先行

easyop 可以实现“小智式 AI 助手”，但不应直接把 `xiaozhi-esp32` 固件整体移植进 EasyInput。两者的核心边界不同：

- 小智：以独立语音硬件为中心，ESP32 是语音终端，云端负责智能。
- easyop：以 Windows 桌面宿主为中心，EasyInput 是输入/状态反馈外设，easyop 负责语音、模型、桌面工具和 Agent 编排。

推荐目标形态：

```text
EasyInput 按键 / 麦克风
        ↓
     easyop Voice Core
        ↓
   ASR → 对话编排 → LLM
        ↓              ↓
      TTS        本地工具 / MCP / Agent
        ↓
 PC 播放（第一阶段）或 ESP32 扬声器（第二阶段）
```

当前项目已有约 60%～70% 的底层条件：ESP32-S3 Wi-Fi 音频输入、流式 ASR、LLM、TTS、PC 播放、任务台、插件和 Host Action。尚缺的三个核心闭环是：

1. 通用 Conversation Orchestrator；
2. Tool Registry / MCP Runtime；
3. TTS 到 ESP32 扬声器的下行链路。

第一版必须先做“按键说话 → ASR → LLM → TTS → PC 播放”，不要一开始同时做离线唤醒、声纹、摄像头、MCP 和板载播放。

---

## 2. 目标与非目标

### 2.1 总体目标

把 easyop 从“语音输入 + 文本注入 + 任务台”扩展为可打断、可多轮对话、可调用桌面工具的语音 Agent 宿主：

- 用户按住 EasyInput 语音键说话；
- easyop 接收 ESP32 或 PC 麦克风音频；
- 流式 ASR 输出最终文本；
- LLM 基于对话上下文回答或选择工具；
- 工具执行结果回到 LLM；
- TTS 生成语音；
- 第一阶段由 PC 播放，后续可由 ESP32 播放；
- EasyInput 灯效反映监听、思考、调用工具、播报、失败等状态；
- 用户可以用新一轮语音打断当前播报。

### 2.2 MVP 必须支持

| 能力 | MVP 要求 |
|---|---|
| 输入 | PTT 按键；ESP32 Wi-Fi UDP 或 PC 麦克风二选一 |
| 识别 | 流式 ASR，输出 partial/final |
| 对话 | 至少 5 轮上下文；新会话可清空 |
| 模型 | 复用现有 LLM Client 的 OpenAI 兼容/豆包等后端 |
| 输出 | TTS 后由 PC 默认声卡播放 |
| 打断 | 录音开始时停止当前 TTS/播放 |
| 工具 | 至少 `open_app`、`read_selection`、`inject_text`、`run_agent`、`get_device_status` |
| 状态 | 前端显示当前阶段；设备灯效与阶段保持一致 |
| 失败 | ASR、LLM、工具、TTS 任一失败都能结束 turn，不得永久卡在运行中 |

### 2.3 明确不纳入 MVP

- 离线唤醒词；
- 声纹识别；
- 摄像头视觉输入；
- 4G 通信；
- 完整小智云端协议兼容；
- ESP32 板载扬声器；
- 多设备同步；
- 任意第三方插件直接获得执行权限；
- 在没有权限确认的情况下执行破坏性系统操作。

---

## 3. 当前条件盘点

### 3.1 已有且可复用

| 项目 | 当前落点 | 复用方式 |
|---|---|---|
| ESP32 音频上行 | `src-tauri/src/keyboard_audio.rs`、`ei_protocol.rs` | 作为 `AudioInput` 适配器 |
| PC 麦克风 | `src-tauri/src/audio.rs` | 作为第二种输入源 |
| PTT 会话 | `src-tauri/src/session.rs` | 保留输入生命周期，向 Voice Core 发事件 |
| 流式 ASR | `src-tauri/src/asr.rs` | 封装为 `AsrAdapter` |
| LLM | `src-tauri/src/llm_client.rs` | 封装为 `LlmAdapter` |
| TTS | `src-tauri/src/tts.rs` | 封装为 `TtsAdapter` |
| PC 播放 | `src-tauri/src/agent_desk/audio_out.rs` | 作为 `AudioOutput::Pc` |
| 任务队列和取消 | `src-tauri/src/agent_desk/` | 复用取消、租约和状态灯思路，不直接当对话状态机 |
| 插件 API | `src/pluginHost.ts`、`plugin_host.rs` | 迁移为受控工具来源 |
| 应用与 Agent | 应用库、Host Action、CLI Driver | 实现第一批本地工具 |

### 3.2 当前缺口

- 统一对话会话、turn、历史和状态机；
- LLM 流式输出的统一事件模型；
- tool call 的 schema、权限、超时、取消和结果归一化；
- 句子级 TTS；
- 播放和录音之间的打断传播；
- TTS PCM 到 ESP32 的下行协议；
- Windows DPAPI/Credential Manager 密钥存储；
- 插件可信/不可信边界；
- `eo_run` 的程序与参数白名单。

### 3.3 外部实现条件

- ESP32-S3 已烧录可发送音频的 EasyInput 固件；
- ESP32 与 Windows 主机处于同一局域网；
- Windows 防火墙允许约定 UDP 端口；
- 可用 ASR、LLM、TTS 服务和凭据；
- 目标设备具有稳定麦克风音频质量；
- 若启用板载播放，需要 I2S 功放、扬声器、DMA buffer 和播放回执；
- 若启用 MCP，需要可运行的 MCP Server 及明确的授权策略。

小智官方项目当前要求 ESP-IDF 6.0.1 以上、推荐 6.1；这不等于 EasyInput 固件必须立即迁移。easyop 应优先复用现有 EasyInput 协议，避免引入第二套设备协议。

---

## 4. 总体架构与不变量

### 4.1 分层架构

```text
┌─────────────────────────────────────────────┐
│ React UI                                    │
│ 对话窗口 / 状态 / 历史 / 工具确认 / 设置       │
└─────────────────────────────────────────────┘
                     │ Tauri commands/events
┌─────────────────────────────────────────────┐
│ Conversation Orchestrator                   │
│ conversation / turn / cancel / interrupt     │
└─────────────────────────────────────────────┘
          │                 │                │
     AsrAdapter        LlmAdapter       TtsAdapter
          │                 │                │
      音频输入          模型 Provider       音频输出
          │                 │                │
 PC / ESP32 UDP       豆包/OpenAI等      PC / ESP32
                     │
              Tool Registry
             /      |       \
       本地工具   插件工具    MCP 工具
```

### 4.2 核心状态机

```text
Idle
  → Listening
  → Recognizing
  → Thinking
  → ToolCalling (可重复 0..N 次)
  → Speaking
  → Completed
```

任意非终态都允许：

```text
→ Cancelling → Interrupted
→ Failed
```

规则：

1. 同一 conversation 同时只能有一个 active turn；
2. 新的 PTT 开始时，必须取消旧 turn 的 ASR、LLM、工具和播放；
3. `ToolCalling` 结束后必须回到 `Thinking`，不能直接假设任务已完成；
4. `Speaking` 被打断时不能把未播完文本标记为完整播报；
5. 每个 turn 必须最终到达 `Completed`、`Interrupted` 或 `Failed`；
6. 所有异步任务必须带取消 token/代数，旧 turn 不能写入新 turn 的状态；
7. 设备灯效只是状态投影，不能成为完成判定的唯一依据。

### 4.3 音频坐标与协议不变量

输入统一为：

```text
16 kHz / 16-bit / mono / PCM little-endian
```

音频包必须使用统一的：

- `session_id`：区分不同语音会话；
- `sequence`：检测丢包/乱序，不能单独作为跨重启门禁；
- `payload_len`：校验实际长度；
- `flags`：start/data/end/cancel；
- `sample_rate`、`channels`：由会话协商或固定契约决定。

如果新增主机到 ESP32 的播放协议，必须新建协议版本和独立消息类型，不修改已有 HID Report ID 的既有语义。输入和输出的 session 生命周期必须可关联，但不能共用同一个 sequence 计数器。

---

## 5. 分阶段实施计划

## Phase 0：冻结边界与基线

### 目标

不写业务代码，先固定现有通信、配置和验收基线，避免并行开发时误改已有协议。

### 工作项

1. 清点 `session.rs`、`keyboard_audio.rs`、`asr.rs`、`llm_client.rs`、`tts.rs`、`audio_out.rs` 的现有接口；
2. 写出 `VoiceEvent`、`ConversationEvent`、`ToolDescriptor`、`AudioFrame` 数据结构；
3. 确定 MVP 只走 PTT + PC 播放；
4. 确定工具白名单和必须二次确认的操作；
5. 建立音频、取消、状态灯和错误码契约；
6. 备份真实配置后再停止正在运行的开发实例，避免热重载触发迁移副作用。

### 退出条件

- 架构评审通过；
- 没有把 Agent Desk 当成 Conversation Orchestrator；
- 没有改变现有固件协议；
- 所有输入、输出、取消和完成信号均有唯一归属。

## Phase 1：Voice Core + 对话 MVP

### 目标

打通：

```text
PTT → ASR → LLM → TTS → PC 播放
```

### 建议新增模块

```text
src-tauri/src/voice_core/
├── mod.rs
├── events.rs
├── turn.rs
├── history.rs
├── orchestrator.rs
└── cancellation.rs
```

建议接口：

```rust
pub trait AudioInput {
    fn start(&self, session_id: u64) -> Result<AudioStream, AudioError>;
    fn stop(&self, session_id: u64);
}

pub trait AsrAdapter {
    async fn recognize(
        &self,
        input: AudioStream,
        cancel: CancellationToken,
    ) -> Result<AsrResult, AsrError>;
}

pub trait LlmAdapter {
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        cancel: CancellationToken,
    ) -> Result<LlmStream, LlmError>;
}

pub trait TtsAdapter {
    async fn synthesize(
        &self,
        text: &str,
        cancel: CancellationToken,
    ) -> Result<TtsAudio, TtsError>;
}
```

### 关键实现

- 将现有 ASR、LLM、TTS provider-specific 代码留在 adapter 内；
- 编排层只处理统一事件，不判断火山/豆包协议细节；
- LLM 输出按句切分，句子完成即送 TTS；
- 先采用完整句 TTS，避免第一版直接处理复杂增量 PCM 拼接；
- 播放开始前登记 playback lease；
- 新一轮 PTT 启动时停止旧播放并取消旧 turn；
- 前端显示“正在听 / 正在思考 / 正在播报 / 已打断 / 失败”。

### Phase 1 验收

- 10 次连续 PTT 对话成功率 ≥ 90%；
- 5 轮上下文中模型不丢失会话主题；
- 从松手到首个可听语音的 P95 ≤ 5 秒；
- 新 PTT 开始后旧播放停止时间 P95 ≤ 500 ms；
- ASR、LLM、TTS 任一断网时，10 秒内进入可恢复失败态；
- 连续运行 30 分钟无永久卡死、线程泄漏或状态错乱。

## Phase 2：本地工具层

### 目标

让语音助手能够安全操作 easyop 和 Windows 桌面。

### 第一批工具

| 工具 | 风险 | 默认策略 |
|---|---:|---|
| `get_device_status` | 低 | 直接执行 |
| `read_selection` | 低 | 直接执行 |
| `open_app` | 中 | 仅限应用库登记项 |
| `inject_text` | 中 | 预览或用户确认 |
| `switch_profile` | 中 | 用户确认 |
| `run_agent` | 高 | 必须确认，限制工作目录和超时 |
| `run_program` | 高 | MVP 禁止泛化，只允许能力 ID |

### 工具注册结构

```rust
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub risk: ToolRisk,
    pub requires_confirmation: bool,
}
```

禁止直接把插件传入的任意程序路径交给 `std::process::Command`。应从：

```text
eo.run(program, args)
```

收紧为：

```text
eo.run_capability(capability_id, args)
```

### Phase 2 验收

- 每个工具均可列出 schema；
- 非法参数在执行前被拒绝；
- 高风险工具没有确认不能执行；
- 工具超时、取消、异常均回传结构化错误；
- 工具结果不会把 API Key、密码和完整敏感路径播报给用户；
- LLM 不会绕过工具层直接执行系统命令。

## Phase 3：ESP32 播放下行

### 目标

把输出从 PC 扩展到 EasyInput 板载扬声器：

```text
TTS PCM → 主机发送器 → Wi-Fi → ESP32 buffer → I2S → 功放 → 扬声器
```

### 硬件条件

- ESP32-S3；
- I2S 功放，如 MAX98357A 同类器件；
- 扬声器；
- 稳定 5 V/3.3 V 电源与功放去耦；
- 足够 DMA buffer；
- 必要时 PSRAM。

### 协议最小集合

```text
SPEAKER_START(session_id, sample_rate, channels, format)
SPEAKER_DATA(session_id, sequence, pcm)
SPEAKER_STOP(session_id)
SPEAKER_CANCEL(session_id)
SPEAKER_ACK(session_id, sequence)
SPEAKER_DONE(session_id)
SPEAKER_UNDERRUN(session_id)
```

### 流控要求

- 发送端不能无界缓存整段音频；
- 接收端必须有高低水位；
- 低于低水位时触发补包或降级；
- 播放取消后必须清空旧 session 的 buffer；
- 设备重启后旧 session 的包必须丢弃；
- 播放完成以设备回执或已知完整 PCM 消耗为依据，不能只看发送完成。

### Phase 3 验收

- 1 分钟连续语音播放无明显爆音、断裂和持续 underrun；
- 100 次 start/data/stop 中无错播旧 session 音频；
- cancel 后 500 ms 内停止输出；
- 丢包、乱序、重传可观测；
- 设备重启后能重新建立会话；
- 实测电流、温升和扬声器音量均在硬件设计范围内。

## Phase 4：MCP Runtime

### 目标

将本地工具、插件工具和外部 MCP Server 统一为 Tool Registry。

### 实施顺序

1. MCP stdio transport；
2. JSON-RPC 请求/响应和错误；
3. `initialize`；
4. `tools/list`；
5. `tools/call`；
6. schema 校验；
7. 工具超时与取消；
8. 用户确认；
9. 再考虑 HTTP/SSE transport。

### 不做的事

- 不把 MCP 直接嵌进 `session.rs`；
- 不让外部工具直接获得 API Key；
- 不默认允许文件删除、任意命令、网络扫描等高风险操作；
- 不同时实现多种 transport；
- 不把“工具调用返回”直接当成“用户最终答案”，必须回到 LLM 生成自然语言结果。

### Phase 4 验收

- 可发现 MCP 工具并显示 schema；
- 合法参数能调用，非法参数被拒绝；
- Server 无响应时可超时退出；
- 用户打断后 tool call 能取消或进入明确的不可取消状态；
- 工具结果经过大小限制和敏感信息过滤；
- 至少一个真实 MCP Server 完成端到端演示。

## Phase 5：安全、稳定性和高级体验

这一阶段不是“有空再做”的装饰，而是准备对外分发前的门槛：

- Windows DPAPI 或 Credential Manager；
- 前端只读取密钥掩码和 configured 状态；
- trusted/untrusted 插件分级；
- 收紧 `eo_run`；
- CSP 和 Tauri capability 最小权限；
- 崩溃恢复与 turn journal；
- 离线队列和网络恢复；
- 可观测性面板；
- 语音热词、自定义唤醒词；
- 声纹与多用户；
- 摄像头和视觉输入。

---

## 6. 验证方法

### 6.1 单元测试

必须覆盖：

- 状态机合法转移和非法转移；
- turn 代数隔离；
- cancel 后不再写入新状态；
- sentence splitter；
- 上下文截断；
- 工具 schema 校验；
- 风险等级与确认策略；
- 音频包编码、解码、CRC、长度检查；
- session/sequence 重启语义；
- TTS PCM/WAV 转换；
- 播放完成与中断判定。

### 6.2 集成测试

使用本地 mock server：

1. mock ASR：返回 partial、final、错误和延迟；
2. mock LLM：返回普通文本、工具调用、多个工具调用和异常；
3. mock TTS：返回固定 PCM；
4. mock Tool Server：测试成功、超时、拒绝和取消；
5. mock ESP32 transport：测试丢包、乱序、重复包、重启和回执。

集成测试不得依赖真实 API Key，不得把用户文本写入日志。

### 6.3 真机测试

测试矩阵至少包括：

| 场景 | 观察项 |
|---|---|
| 安静环境 | ASR 正确率、首包延迟 |
| 键盘/风扇噪声 | 误识别、RMS、VAD |
| Wi-Fi 弱信号 | 丢包、重连、状态恢复 |
| 连续 30 分钟 | 内存、任务堆积、温升 |
| 播放中再次按键 | 打断延迟、旧音频是否残留 |
| 设备断电重启 | session 是否隔离 |
| PC 睡眠唤醒 | UDP、声卡、配置恢复 |
| 高风险工具 | 是否严格确认、是否可取消 |

### 6.4 性能指标

MVP 建议验收指标：

- PTT 松开到 ASR final：P95 ≤ 2 秒；
- ASR final 到首个 LLM 文本：P95 ≤ 2 秒；
- 松手到首个可听语音：P95 ≤ 5 秒；
- 新语音打断旧播放：P95 ≤ 500 ms；
- 30 分钟连续运行：无永久卡死；
- turn 终态覆盖率：100%；
- 工具超时后恢复率：100%；
- 旧 session 音频误播：0 次。

这些是工程验收目标，不是当前已经达成的实测结果。

---

## 7. 验收标准

### 7.1 功能验收

- [ ] 可通过 EasyInput PTT 启动和结束语音 turn；
- [ ] ASR 能产生 partial/final；
- [ ] LLM 能保持至少 5 轮上下文；
- [ ] TTS 能由 PC 播放；
- [ ] 新一轮语音能打断旧播放；
- [ ] 至少 5 个本地工具可被模型调用；
- [ ] 高风险工具需要确认；
- [ ] 工具异常能以自然语言或明确错误提示结束；
- [ ] 设备灯效和 UI 状态一致；
- [ ] 手动清空会话后不再引用旧历史。

### 7.2 工程验收

- [ ] `npm run version:check` 退出码为 0；
- [ ] 前端构建通过；
- [ ] Rust `cargo check` 通过；
- [ ] 相关单元测试通过；
- [ ] mock 集成测试通过；
- [ ] 真机测试报告单独记录；
- [ ] 未烧录、未实测的能力不得写成“已验证”；
- [ ] 不修改既有固件 UUID、Report ID 和配置格式；
- [ ] 不提交密钥、用户文本、设备地址和本机私有路径；
- [ ] 文档、CHANGELOG、README 版本历史同步完成。

### 7.3 交付物

每个阶段至少交付：

1. 源码与测试；
2. 接口/协议文档；
3. 配置迁移说明；
4. 测试结果和失败项；
5. 已知限制；
6. 下一阶段入口条件。

---

## 8. 风险与应对

| 风险 | 根因 | 影响 | 应对措施 |
|---|---|---|---|
| 直接移植小智导致两套架构冲突 | 小智以独立硬件为中心，easyop 以桌面宿主为中心 | 高 | 只借鉴协议/模块思想，保留 easyop 现有边界 |
| 对话状态与任务台混淆 | 任务台是队列，conversation 是 turn | 高 | 独立 `voice_core`，只复用底层 LLM/TTS/播放能力 |
| 取消后旧结果覆盖新 turn | 异步任务没有代数隔离 | 高 | 每个 turn 使用 generation + cancellation token，提交前校验归属 |
| 播放状态误判 | 发送完成不等于设备播放完成 | 高 | PC 用播放控制回执，ESP32 用 DONE/ACK/水位信号 |
| UDP 丢包和乱序 | Wi-Fi 不提供可靠字节流 | 中高 | sequence、CRC、缓冲、水位、丢包统计，必要时重传 |
| 音频协议不兼容 | 采样率/声道/字节序不一致 | 高 | 固定 PCM 契约，start 帧协商，单测锁死编码 |
| API Key 泄露 | 明文 config 和前端 command | 高 | DPAPI/Credential Manager，前端不返回原始密钥 |
| 插件越权 | `eo_run` 可启动任意程序 | 高 | capability ID + 白名单 + 审批 + 超时 + 输出脱敏 |
| LLM 错误调用工具 | 模型输出不可完全信任 | 高 | schema 校验、风险分级、用户确认、幂等设计 |
| 高风险操作误执行 | 语音识别或模型判断错误 | 高 | 默认 fail-closed；删除、写文件、执行命令必须确认 |
| 云端依赖导致不可用 | 网络/API/配额波动 | 中高 | 明确错误、超时、重试上限、PC 文本降级；不静默重试 |
| TTS 延迟过高 | 等完整回复后才合成 | 中 | 句子级切分；第一版不追求逐 token 音频 |
| ESP32 内存不足 | ASR/唤醒/音频 buffer/显示同时占用 | 中高 | 第一版不在板端跑大模型；限制 buffer；按需启用 PSRAM |
| IDF 版本冲突 | 小智官方要求 IDF 6.0.1+，现有固件基线不同 | 高 | 不直接搬固件；先在现有固件协议上增量开发 |
| 热重载破坏状态 | 修改配置/代码时 dev 实例自动重启 | 中 | 真机/配置操作前停 dev，先备份配置 |
| 日志泄露用户隐私 | 语音文本、密钥、地址进入日志 | 高 | 日志脱敏；只记事件类型、长度、错误码和统计量 |

---

## 9. 后续值得优化的方向

按优先级排序：

### P0：安全和正确性

1. Secret Store；
2. 工具 capability 白名单；
3. 插件权限分级；
4. turn 取消和代数隔离；
5. 完整错误码和可恢复状态；
6. 日志脱敏。

### P1：体验和实时性

1. 流式 LLM → 句子级 TTS；
2. barge-in：用户说话自动打断播报；
3. VAD 与自动端点检测；
4. 音频抖动缓冲；
5. 网络恢复和断线重连；
6. 对话历史摘要，避免上下文无限增长。

### P2：设备能力

1. ESP32 板载扬声器；
2. 多颗灯的语义状态动画；
3. OLED/LCD 表情与字幕；
4. 电量、休眠和功耗管理；
5. OTA；
6. 自定义唤醒词。

### P3：智能能力

1. MCP HTTP/SSE；
2. 本地知识库检索；
3. 语音控制内容台和任务台；
4. 多 Agent 协同；
5. 声纹和多用户画像；
6. 摄像头视觉和屏幕理解。

### P4：产品化

1. Provider 抽象和成本统计；
2. 首次启动配置向导；
3. 本地隐私模式；
4. 导入/导出配置但不导出密钥；
5. 崩溃恢复与诊断包；
6. 设备兼容矩阵和自动化回归测试；
7. 安全更新和版本兼容策略。

---

## 10. 推荐的实际执行顺序

```text
第 1 步：冻结协议、状态机、工具白名单
第 2 步：新增 Voice Core，打通 PTT → ASR → LLM → TTS → PC
第 3 步：加入 5 个受控本地工具
第 4 步：做取消、打断、失败恢复和 30 分钟稳定性验证
第 5 步：修复密钥存储与插件权限
第 6 步：定义并实现 ESP32 播放下行协议
第 7 步：接入 I2S 功放和板载扬声器
第 8 步：增加 MCP stdio Runtime
第 9 步：最后再做离线唤醒、声纹、摄像头和多设备
```

判断是否进入下一步，不看“代码写了多少”，只看上一阶段的验收证据是否齐全。

---

## 11. 当前明确未完成项

本文是实施方案，不代表功能已经实现。当前以下能力仍属于设计/规划：

- Conversation Orchestrator；
- 统一 Voice Core；
- 本地 Tool Registry；
- MCP Client/Runtime；
- TTS 到 ESP32 的下行协议与固件播放；
- 播放回执和设备流控；
- Windows 安全密钥存储；
- 插件真实沙箱；
- 离线唤醒、声纹、摄像头、OTA。

当前 easyop 已有的 ASR、LLM、TTS、PC 播放、ESP32 音频输入和任务台能力，仍需按本文接口边界重新编排后，才能称为完整的小智式语音 Agent。
