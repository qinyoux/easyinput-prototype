# 实时翻译（live-translate）

easyop 内置插件。把英文会议 / 面对面交流的**多人语音**实时识别、按说话人分离，
同步翻译为中文并上屏。语音识别与翻译均走云端（豆包 ASR + LLM），复用 easyop 全局模型配置。

## 功能

- **说话人分离流式识别**：豆包流式 ASR（`enable_speaker_info` + `show_utterances`），返回每句的 `speaker_id`（spk0 / spk1 / ...）。
- **实时翻译**：每定稿一句立即调 LLM 流式翻译为中文，逐字上屏。
- **双模式**：
  - **对话模式**：2 人场景，spk0=我方 / spk1=对方，左右分栏（像聊天）。
  - **会议模式**：多人场景，Speaker 1 / 2 / 3...，左对齐 + 颜色区分。
- **导出**：Markdown / TXT / JSON，含说话人、原文、译文。

## 配置

**不需要新增 API Key**：直接复用 easyop「模型配置」页的 ASR（豆包）与 LLM（豆包）设置。
用户只需在全局填一次，语音输入与实时翻译共用。

插件自有偏好（模式 / 语言对 / 自动滚动）存插件私有 `eo.storage`，每用户独立。

## 使用

1. 依次打开：侧栏页签「实时翻译」。
2. 选择模式（对话 / 会议）与语言对（默认 英 → 中）。
3. 点「开始」，开始说话；字幕流实时显示原文与译文。
4. 点「停止」结束会话；点「导出」保存记录。

也可通过 ⌘SS 命令面板触发 `live-translate.start` / `.stop` / `.export`。

## 后端契约

| 命令 | 参数 | 返回 | 作用 |
|------|------|------|------|
| `lt_start` | `{mode, sourceLang, targetLang}` | `Result<(), String>` | 启动会话 |
| `lt_stop` | `{}` | `Result<(), String>` | 停止会话 |
| `lt_status` | `{}` | `LtStatus` | 查询状态 |
| `lt_export` | `{format: "md"\|"txt"\|"json"}` | `Result<String, String>` | 导出并返回路径 |

| 事件 | payload | 触发时机 |
|------|---------|----------|
| `lt-state` | `{running, error?, stopping?}` | 状态变化 / 错误 |
| `lt-partial` | `{utterance_id, speaker_id, text}` | 识别中间结果 |
| `lt-final` | `{utterance_id, speaker_id, text, start_ms?, end_ms?}` | 一句定稿 |
| `lt-translate-chunk` | `{utterance_id, delta}` | 翻译流式增量 |
| `lt-translate-final` | `{utterance_id, translation}` | 一句翻译完成 |

## 已知限制

- **单麦克风 + 云端说话人分离**：不接多路麦克风硬件，分离依赖豆包算法。
  对话模式（2 人）最准；会议模式人数越多准确率越降（如实标注，不夸大）。
- **录音源**：一期固定用**电脑麦克风**，不使用 EasyInput 开发板 Wi-Fi 音频。
- **专业术语**：翻译 prompt 已要求保留 FPGA / beamforming / ADC 等原词；
  识别侧可经 ASR 自定义词库（「ASR 词库」页）补充领域热词。
- **背景噪音**：建议用单向麦克风 / 耳机，降低误识别。

## 目录

```
plugins/builtin/live-translate/
├── plugin.json   # 插件清单（页签 live-translate.board + 3 个命令）
├── index.js      # 页签 UI（原生 JS + DOM）
└── README.md
```

Rust 后端（平台侧）：
- `src-tauri/src/live_translate.rs` —— 会话编排（启动/停止/翻译并发/导出）
- `src-tauri/src/asr.rs` —— `run_translate()`（说话人分离识别）
- `src-tauri/src/lib.rs` —— 4 个 `lt_*` 命令注册 + 管理器注入

## 部署

内置插件放在源码树 `plugins/builtin/`，但 easyop 只扫描用户配置目录
（`%APPDATA%/com.easyop.app/plugins/`）。用部署脚本拷过去：

```bash
node scripts/deploy-plugins.mjs live-translate
```

然后重启 easyop（或在插件管理页点刷新），侧栏即出现「实时翻译」页签。