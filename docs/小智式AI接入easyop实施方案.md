# 小智式 AI 接入 easyop 实施方案

> 版本：v0.40.0  
> 日期：2026-10-09  
> 状态：实施设计基线，速度差异分析与可复现实测方案已纳入；尚未开始相关优化编码

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

| 能力 | MVP 要求                                                                       |
| -- | ---------------------------------------------------------------------------- |
| 输入 | PTT 按键；ESP32 Wi-Fi UDP 或 PC 麦克风二选一                                           |
| 识别 | 流式 ASR，输出 partial/final                                                      |
| 对话 | 至少 5 轮上下文；新会话可清空                                                             |
| 模型 | 复用现有 LLM Client 的 OpenAI 兼容/豆包等后端                                            |
| 输出 | TTS 后由 PC 默认声卡播放                                                             |
| 打断 | 录音开始时停止当前 TTS/播放                                                             |
| 工具 | 至少 `open_app`、`read_selection`、`inject_text`、`run_agent`、`get_device_status` |
| 状态 | 前端显示当前阶段；设备灯效与阶段保持一致                                                         |
| 失败 | ASR、LLM、工具、TTS 任一失败都能结束 turn，不得永久卡在运行中                                       |

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

| 项目         | 当前落点                                               | 复用方式                      |
| ---------- | -------------------------------------------------- | ------------------------- |
| ESP32 音频上行 | `src-tauri/src/keyboard_audio.rs`、`ei_protocol.rs` | 作为 `AudioInput` 适配器       |
| PC 麦克风     | `src-tauri/src/audio.rs`                           | 作为第二种输入源                  |
| PTT 会话     | `src-tauri/src/session.rs`                         | 保留输入生命周期，向 Voice Core 发事件 |
| 流式 ASR     | `src-tauri/src/asr.rs`                             | 封装为 `AsrAdapter`          |
| LLM        | `src-tauri/src/llm_client.rs`                      | 封装为 `LlmAdapter`          |
| TTS        | `src-tauri/src/tts.rs`                             | 封装为 `TtsAdapter`          |
| PC 播放      | `src-tauri/src/agent_desk/audio_out.rs`            | 作为 `AudioOutput::Pc`      |
| 任务队列和取消    | `src-tauri/src/agent_desk/`                        | 复用取消、租约和状态灯思路，不直接当对话状态机   |
| 插件 API     | `src/pluginHost.ts`、`plugin_host.rs`               | 迁移为受控工具来源                 |
| 应用与 Agent  | 应用库、Host Action、CLI Driver                         | 实现第一批本地工具                 |

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

| 工具                  | 风险 | 默认策略              |
| ------------------- | -: | ----------------- |
| `get_device_status` |  低 | 直接执行              |
| `read_selection`    |  低 | 直接执行              |
| `open_app`          |  中 | 仅限应用库登记项          |
| `inject_text`       |  中 | 预览或用户确认           |
| `switch_profile`    |  中 | 用户确认              |
| `run_agent`         |  高 | 必须确认，限制工作目录和超时    |
| `run_program`       |  高 | MVP 禁止泛化，只允许能力 ID |

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

| 场景        | 观察项          |
| --------- | ------------ |
| 安静环境      | ASR 正确率、首包延迟 |
| 键盘/风扇噪声   | 误识别、RMS、VAD  |
| Wi-Fi 弱信号 | 丢包、重连、状态恢复   |
| 连续 30 分钟  | 内存、任务堆积、温升   |
| 播放中再次按键   | 打断延迟、旧音频是否残留 |
| 设备断电重启    | session 是否隔离 |
| PC 睡眠唤醒   | UDP、声卡、配置恢复  |
| 高风险工具     | 是否严格确认、是否可取消 |

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

| 风险             | 根因                         | 影响 | 应对措施                                               |
| -------------- | -------------------------- | -- | -------------------------------------------------- |
| 直接移植小智导致两套架构冲突 | 小智以独立硬件为中心，easyop 以桌面宿主为中心 | 高  | 只借鉴协议/模块思想，保留 easyop 现有边界                          |
| 对话状态与任务台混淆     | 任务台是队列，conversation 是 turn | 高  | 独立 `voice_core`，只复用底层 LLM/TTS/播放能力                 |
| 取消后旧结果覆盖新 turn | 异步任务没有代数隔离                 | 高  | 每个 turn 使用 generation + cancellation token，提交前校验归属 |
| 播放状态误判         | 发送完成不等于设备播放完成              | 高  | PC 用播放控制回执，ESP32 用 DONE/ACK/水位信号                   |
| UDP 丢包和乱序      | Wi-Fi 不提供可靠字节流             | 中高 | sequence、CRC、缓冲、水位、丢包统计，必要时重传                      |
| 音频协议不兼容        | 采样率/声道/字节序不一致              | 高  | 固定 PCM 契约，start 帧协商，单测锁死编码                         |
| API Key 泄露     | 明文 config 和前端 command      | 高  | DPAPI/Credential Manager，前端不返回原始密钥                 |
| 插件越权           | `eo_run` 可启动任意程序           | 高  | capability ID + 白名单 + 审批 + 超时 + 输出脱敏               |
| LLM 错误调用工具     | 模型输出不可完全信任                 | 高  | schema 校验、风险分级、用户确认、幂等设计                           |
| 高风险操作误执行       | 语音识别或模型判断错误                | 高  | 默认 fail-closed；删除、写文件、执行命令必须确认                     |
| 云端依赖导致不可用      | 网络/API/配额波动                | 中高 | 明确错误、超时、重试上限、PC 文本降级；不静默重试                         |
| TTS 延迟过高       | 等完整回复后才合成                  | 中  | 句子级切分；第一版不追求逐 token 音频                             |
| ESP32 内存不足     | ASR/唤醒/音频 buffer/显示同时占用    | 中高 | 第一版不在板端跑大模型；限制 buffer；按需启用 PSRAM                   |
| IDF 版本冲突       | 小智官方要求 IDF 6.0.1+，现有固件基线不同 | 高  | 不直接搬固件；先在现有固件协议上增量开发                               |
| 热重载破坏状态        | 修改配置/代码时 dev 实例自动重启        | 中  | 真机/配置操作前停 dev，先备份配置                                |
| 日志泄露用户隐私       | 语音文本、密钥、地址进入日志             | 高  | 日志脱敏；只记事件类型、长度、错误码和统计量                             |

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

---

## 12. 小智与 easyop 的速度差异分析

### 12.1 先定义“快”到底指什么

两套系统不能只比较“从说完到任务结束”的一个总时间。至少要拆成三个用户可感知指标：

| 指标                                | 定义                                        | 适合回答的问题       |
| --------------------------------- | --------------------------------------- | --------------- |
| 首字时间 TTFT                      | ASR 最终完成后到模型首个可用文本增量                 | 模型/网络何时开始响应  |
| 首音时间 TTFA                      | PTT 松开后到用户听到第一段语音                        | 为什么小智听起来更快    |
| 完整完成时间                             | PTT 松开后到任务终态及播报结束                       | 任务是否真正完成        |

小智类系统通常优化 TTFA：持续连接、流式 ASR、边生成边 TTS、边收到边播放。当前 easyop 的任务台更偏向正确性：等待任务终态、完整结果和完整 TTS 后再播报。因此“更早开始说话”不必然等于“任务整体更快”，测试必须分别记录 TTFA 与完整完成时间。

### 12.2 当前最可能的差异来源

以下是基于现有架构的待验证假设，按优先级排列；它们不是本轮已经完成的实测结论。

| 优先级 | 因素        | 小智式链路                | easyop 可能的额外成本                                          | 初步判断             |
| ---: | --------- | ---------------------- | ------------------------------------------------------- | ----------------- |
|    1 | 输出策略     | 首句生成即 TTS，音频到达即播放        | 任务完成后才播报，TTS 可能先完整合成                                 | 高概率、结构性          |
|    2 | LLM 增量传递  | 长连接流式事件直接进入用户体验          | API Driver 虽可流式读取，但执行过程使用 `NullSink`，用户看不到增量             | 高概率              |
|    3 | 任务流程     | 云端会话直接回答或调用常驻工具           | 槽位排队、CLI 启动、进程监控、完成判定、门禁、历史留存                     | 高概率              |
|    4 | 工具调用     | 固定 MCP/API，结构化 JSON 返回        | CLI/Agent 可能重新读取工作区、加载规则并等待进程退出                         | 高概率，尤其是代码任务     |
|    5 | CLI 收尾    | 服务端事件明确结束               | 进程已退出后仍可能等待 stdout/stderr drain，历史上存在约 2 s 宽限路径         | 高概率，必须直接测量     |
|    6 | TTS       | 分句、流式音频、低播放缓冲        | 完整 PCM 返回后才开始播放                                          | 高概率              |
|    7 | 连接复用     | 长期 WebSocket/会话           | 每次任务可能新建客户端，DNS/TCP/TLS/鉴权不能稳定复用                            | 中概率              |
|    8 | 服务端/网络   | 服务端与模型同地域、实时请求优先级      | 公网 API 网关、排队、限流、代理和地域差异                                  | 中概率              |
|    9 | 模型配置     | 轻量实时模型或专用路由             | endpoint、model、reasoning、max_tokens、prompt 可能不同                      | 中概率              |
|   10 | 上下文长度    | 固定短上下文或服务端会话缓存          | CLI Agent 隐式加载项目规则、历史、工具 schema 和工作区内容                        | API 任务较低，CLI 任务较高 |
|   11 | ASR 收尾    | 音频持续上传并提前 partial        | 松键后等待 final；需以日志确认                                           | 当前证据较低          |
|   12 | ESP32 算力   | 主要负责音频和传输               | 智能推理不在板端                                                       | 低概率，不应优先排查     |

### 12.3 必须保持的对比边界

有效对照必须固定：同一输入文本、同一模型、同一 endpoint、同一 API Key、同一 temperature、同一 max_tokens、同一工具集合、同一并发数、同一网络和相近时间段。不能把“小智纯问答”与“easyop 调 Codex 检查并修复 Rust 项目”直接比较；后者包含工作区读取、工具执行、文件修改和验证，任务复杂度并不相同。

建议先拆成三类任务：

- T1 纯问答：不调用工具，只比较 ASR/LLM/TTS。
- T2 简单工具：读取设备在线状态或调用无副作用 `get_test_value()`。
- T3 复杂 Agent：读取项目、执行命令或修改文件，单独比较工具与任务管线。

### 12.4 当前 easyop 需要重点核对的路径

- API Driver 是否把 `chat_stream` 的首个 delta 继续传给 UI/播报层，而不是只在 `NullSink` 中消费。
- `stream_edit` 等路径是否先等待完整响应后再解析，避免把“流式接口”误认为“流式用户体验”。
- `pump_slot` 是否在任务终态、二次总结、TTS、历史写入之间形成串行等待；任何一段都要独立打点。
- CLI 必须分开记录 `process_exit`、`stdout_eof`、`stderr_eof`、`task_complete`，确认是否命中固定 drain 宽限。
- TTS 当前是否累计完整 PCM 后才调用 `play_pcm()`；若是，首音应改为句子级 TTS，再逐步演进到音频流式播放。
- 任务播报是否把完整 Agent 输出转成语音；语音应优先播报短结果，详细内容留在任务历史和 UI。
- 任务台同槽排队与跨槽并发必须记录 `enqueue`、`driver_start`，避免把排队时间误判为模型慢。

---

## 13. 速度测试与可复现验证方案

### 13.1 统一 trace 事件

每个语音 turn 和每个任务生成一个 `trace_id`，至少记录下列单调时钟事件；墙钟只用于展示时间，不用于耗时计算：

```text
ptt_start
ptt_stop
asr_start
asr_first_partial
asr_final
enqueue
driver_start
driver_first_delta
driver_end
tool_start
tool_end
tts_start
tts_first_frame
tts_end
playback_start
playback_end
```

由这些事件派生：

| 指标                | 计算方式                        | 用途         |
| ----------------- | --------------------------- | ---------- |
| ASR 收尾时间         | `asr_final - ptt_stop`         | 判断识别是否慢    |
| 排队时间             | `driver_start - enqueue`       | 判断槽位是否拥堵   |
| LLM 首字时间          | `driver_first_delta - driver_start` | 判断网络/模型  |
| LLM 完整时间          | `driver_end - driver_start`    | 判断生成总量    |
| 工具耗时             | `tool_end - tool_start`        | 判断工具层    |
| TTS 首帧时间          | `tts_first_frame - tts_start`  | 判断首音瓶颈   |
| TTS 完整耗时          | `tts_end - tts_start`          | 判断合成总量   |
| 首音时间 TTFA         | `playback_start - ptt_stop`    | 用户最关心    |
| 完整体验时间           | `playback_end - ptt_stop`      | 整体体验    |

### 13.2 分层测试矩阵

每组至少执行 20 次，固定同一时间窗口，并记录 P50/P95/P99（用户感知由 P95 和长尾决定，不要只看平均值）。

| 编号 | 测试         | 变量                            | 记录指标                       | 判定规则                                                       |
| -- | ---------- | ----------------------------- | --------------------------- | ---------------------------------------------------------- |
|  A | 纯模型速度       | 同一模型/端点，关闭工具              | TTFB、TTFT、完整响应、输入/输出 token、token/s   | TTFT 差异大→网络或服务端排队；token/s 差异大→模型推理；完整时间差异大而 TTFT 接近→输出长度或收尾策略 |
|  B | 上下文长度       | 0.5K / 2K / 8K / 16K 输入，其余不变   | TTFT、完整响应、输入 token           | TTFT 随长度明显上升→上下文是主因；几乎不变→排除上下文                 |
|  C | ASR 速度        | 同一 WAV 反复回放，16 kHz/16 bit/mono | `asr_first_partial - send_start`、`asr_final - last_packet` | 发送期间无 partial→流式未生效；有 partial 但用户看不到→事件/UI 链路问题 |
|  D | 工具调用速度      | 本地函数 / 本地 MCP / HTTP MCP / CLI / Agent | `tool_call_start`、`tool_first_result`、`tool_end` | 预期排序为 本地函数 < MCP < HTTP MCP < CLI < Agent；差异常在工具实现而非模型 |
|  E | TTS 与输出策略    | T1 仅合成、T2 完整后播、T3 分句合成播、T4 流式流播 | `tts_first_frame`、`playback_start`    | T3 明显早于 T2→确认“等待完整 TTS”是首音瓶颈；T4 再优于 T3→可继续音频流式化    |
|  F | 连接复用        | A 每次新建 client，B 复用长期 client   | DNS、TCP、TLS、TTFB、TTFT           | B 稳定降低连接时间→复用有效；否则排除该项                                   |
|  G | CLI 收尾延迟      | 正常退出路径                      | `process_exit → stdout_eof`、`process_exit → task_complete` | 接近 2000 ms 即确认固定 drain 宽限，是本地确定性损耗             |
|  H | 连续压力        | 单任务 / 同槽连续 / 跨槽并行 / 弱网 / 断网恢复 | P50/P95/P99、失败率、超时率、取消率、排队时间 | 关注 P95 与偶发长尾，不以平均值论成败                                |

优先执行顺序建议：**B → A → C → G → E**，这几组最可能一击命中且成本最低。

### 13.3 结果输出与归因规则

- 每次报告必须带 `trace_id` 列表、样本数、P50/P95/P99、以及当次网络与服务商区域。
- API Key、token、用户音频与文本内容一律脱敏；报告中只保留可公开的元数据。
- 归因必须满足“可复现”：同一组实验重复两次，结论方向一致才可写入结论；否则标记为待定。
- 结论只能写成“该条件下观测到 X”，不得直接写成“模型就是慢于 Y”。

---

## 14. 速度优化执行顺序与验收

### P0：先测量，不先换模型

补齐 `trace_id`、`asr_final`、`queue wait`、`LLM TTFT`、`LLM first delta`、`TTS first frame`、`playback start`、`CLI pipe drain` 八类打点。没有这些数据，换模型、换服务商、改协议都属盲操作。

### P1：修确定性本地延迟

CLI 正常退出时立即关闭读端，确认子句柄未被继承，管道达 EOF 即结束；把约 2 s 的 drain 宽限降到 100～300 ms 并留证据日志。同时让 API Driver 的增量不再只进 `NullSink`，至少让用户看到“正在思考 / 已生成 N 字”。

### P2：改变首音策略

先做句子级 TTS：LLM 每产出一段就合成并排队播放；之后再演进为音频帧流式合成 + 流式播放。

### P3：缩短语音输出

任务播报不再朗读完整 Agent 输出，改为固定短句，例如“完成。已修改 3 个文件，测试通过，详细结果已写入任务历史。”详细内容留在任务历史与 UI。

### P4：验证后再动模型与网络

只有前面完成后，才能判断是否换模型、换服务商、改 endpoint、启用长连接会话或专用实时模型，避免把“TTS 等完整返回”误判成“模型慢”。

速度优化阶段验收目标：

- PTT 松开到 ASR final：P95 ≤ 2 s；
- ASR final 到模型首个 delta：P95 ≤ 2 s；
- MVP 句子级 TTS 的松手到首音 TTFA：P95 ≤ 5 s；
- CLI 正常退出到任务终态：P95 < 100 ms 的 drain 增量；
- 新 PTT 打断旧播放：P95 ≤ 500 ms；
- 每组基准测试至少 20 次，结果含 P50/P95/P99；
- 30 分钟连续运行无永久卡死、旧 turn 覆盖新 turn 或旧音频误播。

上述为目标值，不代表当前已经达成；每一项必须以脱敏测试报告为证据。

---

## 15. 本次分析结论与后续路线

最重要的结论不是“立即换模型”，而是：

> 小智的速度优势更可能来自“持续连接 + 流式处理 + 尽早输出”，easyop 的主要差距很可能来自“任务完成后才播报、增量结果未进入用户体验、CLI/工具链更长、TTS 完整缓存和固定管道收尾”。

因此 easyop 应扬长避短：保留任务台的可靠完成判定、历史追溯、取消和桌面 Agent 能力；借鉴小智的实时输出策略，把首音和完整完成拆成两个指标；先用可复现数据定位瓶颈，再增量优化。这样既不会把复杂 Agent 错当普通问答，也不会因为追求正确性而制造不必要的等待感。
