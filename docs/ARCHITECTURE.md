# easyop 文件架构说明

> 本文档描述 easyop 项目的目录结构、模块职责与主链路数据流，供开发者快速定位代码。

## 一、顶层目录

```
easyinput-prototype/
├── src/                     # 前端（React + Vite + TypeScript）
├── src-tauri/               # 后端（Rust + Tauri 2）
│   ├── src/                 # Rust 源码（全部业务逻辑）
│   ├── Cargo.toml           # Rust 依赖清单
│   ├── Cargo.lock           # 依赖锁定
│   ├── tauri.conf.json      # Tauri 应用配置
│   ├── build.rs             # Tauri 构建脚本
│   ├── capabilities/        # Tauri 2 权限声明
│   └── gen/                 # Tauri 自动生成的 schema（已 gitignore）
├── public/                  # 静态资源（浮层 HTML）
├── dist/                    # 前端构建产物（已 gitignore）
├── docs/                    # 文档
│   └── ARCHITECTURE.md      # 本文件
├── index.html               # Vite 入口
├── package.json             # 前端依赖与脚本
├── tsconfig.json            # TypeScript 配置
├── tsconfig.node.json       # Node 环境 TS 配置
├── vite.config.ts           # Vite 构建配置
├── README.md                # 项目说明
└── .gitignore
```

## 二、前端 `src/`

```
src/
├── main.tsx                 # React 挂载入口
├── App.tsx                  # 主壳：顶部状态栏 + 侧栏方案库 + 三页签主体
├── api.ts                   # Tauri invoke 封装（前端→后端命令调用层）
├── types.ts                 # TypeScript 类型定义（Profile / ActionSpec / AppEntry 等）
├── constants.ts             # 输入 ID、动作中文标签、固定常量
├── validation.ts            # 前端预校验（动作合法性、文本字节长度等）
├── styles.css               # 全局样式
└── components/
    ├── ProfilePanel.tsx      # 侧栏：方案库（创建 / 切换 / 删除）
    ├── RemapEditor.tsx       # 按键映射页签：可视化键盘 + 配置面板
    ├── VisualKeyboard.tsx    # 4×2 按键网格 + 圆形编码器
    ├── KeyEditor.tsx         # 单个输入的动作编辑器
    ├── HotkeyBuilder.tsx     # 组合键编辑器（Ctrl+Alt+A 形式）
    ├── AppLibrary.tsx        # 本机应用页签：从开始菜单枚举 / 手动添加 / 测试打开
    ├── VoicePanel.tsx        # 语音输入页签：PTT 模式切换 / 录音控制
    ├── VoiceSettings.tsx     # 语音设置页签：ASR / LLM / Wi-Fi / 蓝牙配置
    └── LogPanel.tsx          # 日志面板（折叠式）
```

### 前端职责划分

| 文件 | 职责 |
|------|------|
| `App.tsx` | 全局状态管理（方案库、设备状态、监听器、页签切换） |
| `api.ts` | 所有 `invoke()` 调用的薄封装，返回 Promise |
| `types.ts` | 与 Rust 端 `model.rs` 对齐的 TS 类型 |
| `constants.ts` | 输入 ID 列表（KEY1-KEY8 + 编码器）、动作中文标签映射 |
| `validation.ts` | 下发前的前端预校验，拦截不合法配置 |
| `RemapEditor.tsx` | 按键配置核心交互：选键 → 编辑动作 → 下发 |
| `VoicePanel.tsx` | 语音 PTT 触发、模式切换、浮层开关 |
| `VoiceSettings.tsx` | ASR / LLM 连接配置与测试 |

## 三、后端 `src-tauri/src/`

```
src-tauri/src/
├── main.rs                  # 程序入口（仅调用 easyop_lib::run()）
├── lib.rs                   # 核心入口：状态注册、命令注册、托盘、浮层、PTT 调度
├── commands.rs              # Tauri 命令层（前端 invoke 的后端入口）
├── model.rs                 # 领域数据模型（ActionSpec / Profile / AppEntry）
├── payload.rs               # 固件 JSON 构造 / 配置回执解析
├── constants.rs             # 协议常量（VID/PID、Feature Report ID、Host Action kind）
├── device.rs                # Win32 HID 枚举 + 设备句柄管理
├── transport.rs             # 0x10 下发配置 + 0x11 读取回执
├── host_action.rs           # Host Action 监听 + 方案切换 + 文本注入
├── launcher.rs              # 打开程序 / 文件 / URL（ShellExecuteW）
├── state.rs                 # 共享状态与持久化（配置文件读写）
├── system_apps.rs           # 本机应用枚举（从开始菜单 / 注册表扫描）
├── validate.rs              # 后端配置校验（与前端 validation.ts 对齐）
├── ptt_hook.rs              # WH_KEYBOARD_LL 低级键盘钩子（PTT 触发引擎）
├── session.rs               # 录音识别会话状态机（Hold / Call 两种模式）
├── audio.rs                 # 电脑麦克风采集 + 重采样（cpal）
├── asr.rs                   # 火山引擎流式 ASR WebSocket 客户端
├── voice_edit.rs            # 语音编辑编排器（选中文本 → ASR → LLM → 替换）
├── llm_client.rs            # LLM 客户端（SSE 流式，OpenAI 兼容 API）
├── text_delivery.rs         # 四阶段文本注入（剪贴板 + Ctrl+V）
├── clipboard_util.rs        # 剪贴板读写（选中文本提取 / 替换）
├── ei_protocol.rs           # EIAU/EIHB/EICC/EICA 协议编解码
├── keyboard_audio.rs        # UDP 17333 音频监听 + 心跳检测
├── crc16.rs                 # CRC16 校验（固件协议用）
└── log_buffer.rs            # 环形日志缓冲区（前端日志面板数据源）
```

### 后端模块分组

| 分组 | 模块 | 职责 |
|------|------|------|
| **入口** | `main.rs`, `lib.rs` | 程序启动、Tauri 命令注册、托盘菜单、浮层窗口、PTT 调度 |
| **命令层** | `commands.rs` | 前端 invoke 的后端入口，映射到各业务模块 |
| **数据模型** | `model.rs`, `state.rs` | ActionSpec / Profile / AppEntry 定义 + 持久化 |
| **固件通信** | `payload.rs`, `transport.rs`, `device.rs`, `constants.rs` | HID 枚举、配置下发、回执解析、协议常量 |
| **Host Action** | `host_action.rs`, `launcher.rs`, `system_apps.rs` | 后台监听 0x11 报文 → UUID 查找 → 打开目标 |
| **语音输入** | `ptt_hook.rs`, `session.rs`, `audio.rs`, `asr.rs`, `text_delivery.rs` | PTT 钩子 → 会话状态机 → 麦克风采集 → 流式识别 → 文本注入 |
| **语音编辑** | `voice_edit.rs`, `llm_client.rs`, `clipboard_util.rs` | 选中文本 → 听修改要求 → LLM 流式编辑 → 替换原文 |
| **键盘音频** | `ei_protocol.rs`, `keyboard_audio.rs` | EIAU/EIHB 协议编解码 + UDP 17333 音频流监听 |
| **工具** | `validate.rs`, `crc16.rs`, `log_buffer.rs` | 配置校验、CRC16、日志缓冲 |

## 四、主链路数据流

### 4.1 按键配置下发

```
前端 RemapEditor.tsx
  → api.push_config(profile)
  → commands.rs::push_config()
  → validate.rs（校验）
  → payload.rs::to_firmware_value()（转固件 schema）
  → transport.rs::send_config()
  → device.rs（Win32 HID Feature Report 0x10）
  → 开发板
```

### 4.2 配置回执解析

```
开发板
  → device.rs（Win32 HID Input Report 0x11）
  → payload.rs::parse_ack()（解析 kind 0x03 回执）
  → 前端事件 toast 提示
```

### 4.3 Host Action 分发

```
开发板按键触发
  → host_action.rs（监听 0x11 kind 0x05，提取 36 字节 UUID）
  → 方案切换？ → state.rs（切换激活方案）→ 重新下发
  → 文本注入？ → clipboard_util.rs + text_delivery.rs
  → 打开应用？ → launcher.rs::open_target()
```

### 4.4 语音输入（PTT → ASR → 上屏）

```
PTT 键按下
  → ptt_hook.rs（WH_KEYBOARD_LL 拦截）
  → session.rs::start_hold() / start_call()
  → audio.rs（电脑麦克风采集）或 keyboard_audio.rs（键盘 Wi-Fi 音频）
  → asr.rs（火山引擎 WebSocket 流式识别）
  → session.rs（收尾 → 文本定稿）
  → text_delivery.rs（四阶段注入：定位光标 → 剪贴板 → Ctrl+V → 清理）
```

### 4.5 语音编辑

```
选中文本 + Ctrl+Shift+E
  → ptt_hook.rs（编辑 PTT 钩子）
  → voice_edit.rs::start_edit()
  → session.rs（录音识别修改要求）
  → asr.rs（流式识别）
  → voice_edit.rs（原文 + 修改要求 → LLM）
  → llm_client.rs（SSE 流式编辑）
  → voice_edit.rs（编辑结果实时显示在浮层）
  → clipboard_util.rs（替换选中文本）
```

### 4.6 键盘 Wi-Fi 音频通道

```
开发板（UDP 17333）
  → keyboard_audio.rs（监听 UDP 包）
  → ei_protocol.rs（解析 EIAU 音频 / EIHB 心跳 / EICA 确认）
  → EIHB 心跳 → 在线判定（5 秒超时）
  → EIAU PCM → session.rs（送入 ASR）
  → EICC 控制 → start/stop/keepalive
```

## 五、三通道通信架构

| 通道 | 协议 | 方向 | 用途 |
|------|------|------|------|
| **HID** | USB/BLE HID Feature Report 0x10/0x11 | 双向 | 按键配置下发 + Host Action 回执 |
| **Wi-Fi UDP** | EIAU/EIHB/EICC/EICA（端口 17333） | 双向 | 16kHz PCM 音频流 + 心跳 + 流控 |
| **云端 ASR** | WebSocket WSS | APP→云端 | 火山引擎流式语音识别 |

## 六、Tauri 配置

| 文件 | 说明 |
|------|------|
| `tauri.conf.json` | 应用名 `easyop`、窗口尺寸 780×720、前端构建产物路径 `../dist` |
| `capabilities/default.json` | 默认权限（窗口管理、事件、路径） |
| `capabilities/overlay.json` | 浮层窗口权限（置顶、无边框、透明） |
| `capabilities/edit_overlay.json` | 编辑浮层窗口权限 |
| `Cargo.toml` | Rust 依赖：tauri 2、windows（Win32 FFI）、tokio、tokio-tungstenite、cpal、reqwest、flate2 |

## 七、技术栈速查

| 层 | 技术 |
|----|------|
| 前端框架 | React 18 + Vite 5 + TypeScript 5 |
| 桌面框架 | Tauri 2 |
| 后端语言 | Rust（edition 2021） |
| Windows FFI | `windows` crate（HID / 键盘钩子 / SendInput / ShellExecuteW） |
| 异步运行时 | tokio（rt-multi-thread） |
| WebSocket | tokio-tungstenite（ASR 流式识别） |
| HTTP 客户端 | reqwest（LLM SSE 流式） |
| 音频采集 | cpal（电脑麦克风） |
| 开发板 | EasyInput V2.0（ESP32-S3，VID `0x303A` / PID `0x1006`） |
| 语音识别 | 火山引擎流式 ASR |
| LLM 编辑 | OpenAI 兼容 API（豆包 / OpenAI / Anthropic / Ollama / 自定义） |
