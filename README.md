# easyop —— 语音输入 + EasyInput 开发板按键配置

> 一款面向 ESP32-S3 可编程键盘开发板的 Windows 桌面应用，
> 让「哑键盘」变成会说话、会编辑、会切换方案的 AI 输入工具。

## 项目简介

easyop 是 [第七期 AI 硬件训练营](https://example.com) 第 2 课的作业项目，基于 **Tauri 2 + React + Rust** 构建。它将一块 EasyInput V2.0 开发板（ESP32-S3）连接到 Windows 主机，实现以下能力：

| 能力 | 说明 |
|------|------|
| **按键配置** | 可视化编辑 8 个按键 + 编码器的动作映射，一键下发到开发板 |
| **语音输入** | 按住 PTT 键说话，流式识别实时上屏；支持按住说话 / 连续通话两种模式 |
| **语音编辑** | 选中文本 → 按编辑键 → 说出修改要求 → LLM 自动编辑 → 替换原文 |
| **方案切换** | 按键触发方案切换，主机端接管切换逻辑并重新下发 |
| **固定文本** | 按键一键注入预设文本，支持中文（剪贴板 + Ctrl+V） |
| **打开应用** | 按键一键拉起本机应用（程序 / 文件 / 网址 / 文件夹） |

### 技术栈

- **前端**：React + Vite + TypeScript
- **后端**：Rust + Tauri 2（Win32 FFI：HID / 键盘钩子 / SendInput / ShellExecuteW）
- **语音识别**：火山引擎流式 ASR（WebSocket WSS）
- **LLM 编辑**：OpenAI 兼容 API（火山引擎豆包 / OpenAI / Anthropic / Ollama / 自定义）
- **开发板**：EasyInput V2.0（VID `0x303A` / PID `0x1006`，固件协议 `ai_keyboard.v1`）

---

## 功能特性

### 1. 按键映射

可视化 4×2 按键网格 + 圆形编码器，每个输入可配置为：

| 动作类型 | 示例 | 说明 |
|----------|------|------|
| 禁用 | — | 该按键无功能 |
| 功能动作 | `copy` / `paste` / `toggle_profile` | 预定义命名动作 |
| 组合键 | `Ctrl+Alt+A` | 模拟键盘快捷键 |
| 输入文本 | `你好世界` | 一键注入固定文本（支持中文） |
| 打开应用 | `host_action:<uuid>` | 按键触发时拉起本机应用 |

支持多方案库：创建 / 编辑 / 删除 / 切换方案，每个方案独立保存按键映射。

### 2. 语音输入

- **按住说话（Hold）**：按住 PTT 键录音，松手后一次性上屏
- **连续通话（Call）**：按一次开始，说完一句自动上屏，再按一次结束
- **实时字幕**：后台浮层显示识别中的文字
- **麦克风来源**：自动 / 电脑 / Wi-Fi / 蓝牙

### 3. 语音编辑

选中文本后按 `Ctrl+Shift+E`：

1. 浮层显示「正在听修改要求…」
2. 说出修改要求（如「把第二段删掉」）
3. 再按 `Ctrl+Shift+E` 确认
4. LLM 根据原文 + 修改要求生成编辑后的文本
5. 自动替换选中的原文

支持流式输出：LLM 编辑过程实时显示在浮层上。

### 4. 方案切换

在按键上绑定「切换方案」「上一个方案」「下一个方案」功能动作，按下时：

1. 固件回发 Host Action 报文到主机
2. 主机切换激活方案
3. 自动下发新方案到固件
4. 前端 UI 同步更新

### 5. 固定文本（支持中文）

按键绑定含中文的固定文本（如「你好」）时：

- 纯 ASCII 文本 → 走固件原生 `{"text": ...}` 通道
- 含中文文本 → 主机端用剪贴板 + Ctrl+V 注入，兼容所有应用

---

## 快速开始

### 前置依赖

| 依赖 | 版本要求 | 说明 |
|------|----------|------|
| Node.js | ≥ 18 | 前端构建 |
| Rust 工具链 | stable | 后端编译 |
| MSVC 构建工具 | — | Tauri Windows 编译硬依赖 |
| EasyInput V2.0 固件 | `ai_keyboard.v1` | 开发板已烧录 |

### 安装 MSVC 构建工具（如未安装）

1. 下载 [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. 运行安装器，勾选「**使用 C++ 的桌面开发**」
3. 安装完成后重开终端，验证 `link.exe /?` 可识别

### 开发运行

```bash
# 安装依赖
npm install

# 启动开发模式（前端热更新 + 后端自动重编译）
npm run tauri dev
```

### 构建发布版

```bash
npm run tauri build
# 产物：src-tauri/target/release/*.exe
```

### 验证环境

```bash
npm run build                  # 前端 + TypeScript 类型检查
cd src-tauri && cargo check    # Rust 编译检查
cd src-tauri && cargo test     # 单元测试（19 项）
```

---

## 使用教程

### 第一步：配置语音识别

1. 打开 easyop，点击「**语音设置**」页签
2. 填写火山引擎 ASR 配置：
   - **API Key**：在[火山引擎控制台](https://console.volcengine.com/speech)获取
   - **ASR URL**：`wss://openspeech.bytedance.com/api/v2/asr`
   - **Resource ID**：`volc.bigasr.sauc.duration`
3. 点击「**测试连接**」验证

### 第二步：配置 LLM 编辑（可选）

在「语音设置」页签的 LLM 配置区域：

1. 选择后端类型（火山引擎豆包 / OpenAI / Anthropic / Ollama / 自定义）
2. 填写 Endpoint、Model、API Key
3. 点击「**测试连接**」验证
4. 设置编辑快捷键（默认 `Ctrl+Shift+E`）

### 第三步：配置按键映射

1. 点击「**按键映射**」页签
2. 在侧栏选择一个方案（或点击「新建」创建新方案）
3. 点击要配置的按键（KEY1-KEY8 或编码器）
4. 在右侧面板选择动作类型：
   - **功能动作**：从下拉列表选择（复制、粘贴、切换方案等）
   - **组合键**：输入快捷键（如 `Ctrl+C`）
   - **输入文本**：输入要注入的文本（支持中文）
   - **打开应用**：从应用库选择已添加的应用
5. 点击顶部「**下发到开发板**」按钮

### 第四步：配置本机应用（可选）

1. 点击「**本机应用**」页签
2. 点击「**从开始菜单添加**」自动枚举本机应用
3. 或手动添加：填写名称、目标路径（程序 / 文件 / 网址 / 文件夹）
4. 点击「**测试打开**」验证
5. 回到「按键映射」页签，将按键动作设为「打开应用」并选择刚添加的应用
6. 下发到开发板

### 第五步：使用语音输入

**按住说话模式**：
1. 确保光标在目标输入框中
2. 按住 PTT 键（默认 `F13`，可在设置中修改）
3. 说话，后台浮层实时显示识别文字
4. 松开 PTT 键，文字自动注入到光标处

**连续通话模式**：
1. 按 PTT 键一次开始录音
2. 说完一句自动上屏，继续说下一句
3. 再按 PTT 键一次结束

### 第六步：使用语音编辑

1. 在任意应用中**选中要编辑的文本**
2. 按 `Ctrl+Shift+E`，浮层显示「正在听修改要求…」
3. **说出修改要求**（如「把这段改得更正式一点」）
4. 再按 `Ctrl+Shift+E` 确认
5. LLM 编辑结果实时显示在浮层上
6. 编辑完成后自动替换选中的文本

### 第七步：使用方案切换

1. 创建多个方案（如「办公方案」「游戏方案」）
2. 在某个按键上绑定「**切换方案**」功能动作
3. 下发到开发板
4. 按下该按键，自动切换到下一个方案并下发

---

## 三通道通信架构

| 通道 | 协议 | 方向 | 用途 |
|------|------|------|------|
| **HID** | USB/BLE HID Feature Report 0x10/0x11 | 双向 | 按键配置下发 + Host Action 回执 |
| **Wi-Fi UDP** | EIAU/EIHB/EICC/EICA（端口 17333） | 双向 | 16kHz PCM 音频流 + 心跳 + 流控 |
| **云端 ASR** | WebSocket WSS | APP→云端 | 火山引擎流式语音识别 |

- **EIHB 心跳**：开发板每 2 秒广播，APP 据此判定在线（5 秒超时）
- **EIAU v2 音频**：32 字节头 + PCM 负载，20ms/帧
- **EICC 控制**：APP 发送 start/stop/keepalive，开发板回 EICA 确认
- **麦克风来源**：自动（优先键盘 Wi-Fi，回退电脑）/ 电脑 / Wi-Fi / 蓝牙

---

## 主链路说明

| 链路环节 | 位置 |
|----------|------|
| 按键方案下发 | `commands.rs` → `payload::to_firmware_json` → `transport::send_config` |
| 配置回执解析 | `payload::parse_ack`（0x11 kind 0x03） |
| Host Action 分发 | `host_action.rs`（0x11 kind 0x05）→ 方案切换 / 文本注入 / 打开应用 |
| 语音 PTT 触发 | `ptt_hook.rs` → `session.rs` → `asr.rs` → `text_delivery.rs` |
| 语音编辑 | `session.rs` → `asr.rs`（Hold 模式）→ `voice_edit.rs` → `llm_client.rs` |
| 键盘 Wi-Fi 音频 | `keyboard_audio.rs` → `ei_protocol.rs` |
| 麦克风源选择 | `session.rs` MicSource → `start_hold`/`start_call` |
| HID 集合过滤 | `device.rs` `has_feature_report()` → `host_action.rs` `try_open_all` |

---

## 目录结构

> 详细的文件架构说明请参阅 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

```
easyinput-prototype/
├── src/                          # 前端 React + Vite + TypeScript
│   ├── main.tsx                  # React 挂载入口
│   ├── App.tsx                   # 主壳：顶部状态栏 + 侧栏方案库 + 三页签
│   ├── api.ts                    # Tauri invoke 封装
│   ├── types.ts                  # TS 类型定义
│   ├── constants.ts              # 输入定义 / 动作中文标签
│   ├── validation.ts             # 前端预校验
│   ├── styles.css                # 全局样式
│   └── components/
│       ├── ProfilePanel.tsx      # 方案库侧栏
│       ├── RemapEditor.tsx       # 可视化键盘 + 配置面板
│       ├── VisualKeyboard.tsx    # 4×2 按键网格 + 圆形编码器
│       ├── KeyEditor.tsx         # 单个输入的动作编辑器
│       ├── HotkeyBuilder.tsx     # 组合键编辑器
│       ├── AppLibrary.tsx        # 本机应用库
│       ├── VoicePanel.tsx        # 语音输入页签
│       ├── VoiceSettings.tsx     # 语音设置页签
│       └── LogPanel.tsx          # 日志面板
├── src-tauri/
│   ├── src/
│   │   ├── main.rs               # 程序入口
│   │   ├── lib.rs                # 核心：状态 + 命令 + 托盘 + 浮层 + PTT 调度
│   │   ├── commands.rs           # Tauri 命令层
│   │   ├── model.rs              # ActionSpec / Profile / AppEntry
│   │   ├── payload.rs            # 固件 JSON 构造 / 回执解析
│   │   ├── constants.rs          # 协议常量（与固件源码对齐）
│   │   ├── device.rs             # Win32 HID 枚举 + 句柄
│   │   ├── transport.rs          # 0x10 下发 + 0x11 读取
│   │   ├── host_action.rs        # Host Action 监听 + 方案切换 + 文本注入
│   │   ├── launcher.rs           # 打开程序 / 文件 / URL
│   │   ├── state.rs              # 共享状态与持久化
│   │   ├── system_apps.rs        # 本机应用枚举（开始菜单扫描）
│   │   ├── validate.rs           # 后端配置校验
│   │   ├── ptt_hook.rs           # WH_KEYBOARD_LL PTT 钩子引擎
│   │   ├── session.rs            # 录音识别会话状态机
│   │   ├── audio.rs              # 电脑麦克风采集 + 重采样（cpal）
│   │   ├── asr.rs                # 火山引擎流式识别
│   │   ├── voice_edit.rs         # 语音编辑编排器
│   │   ├── llm_client.rs         # LLM 客户端（SSE 流式）
│   │   ├── text_delivery.rs      # 四阶段文本注入（剪贴板 + Ctrl+V）
│   │   ├── clipboard_util.rs     # 剪贴板读写（选中文本提取 / 替换）
│   │   ├── ei_protocol.rs        # EIAU/EIHB/EICC/EICA 协议编解码
│   │   ├── keyboard_audio.rs     # UDP 17333 音频监听 + 心跳
│   │   ├── crc16.rs              # CRC16 校验
│   │   └── log_buffer.rs         # 环形日志缓冲区
│   ├── capabilities/             # Tauri 2 权限声明
│   │   ├── default.json
│   │   ├── overlay.json
│   │   └── edit_overlay.json
│   ├── Cargo.toml
│   └── tauri.conf.json
├── public/
│   ├── overlay.html              # 普通字幕浮层
│   └── edit_overlay.html         # 编辑字幕浮层
├── docs/
│   └── ARCHITECTURE.md           # 文件架构说明
└── package.json
```

---

## 已知限制

- 仅支持 **Windows**（Win32 HID / 键盘钩子 / SendInput 均为 Windows 专有）
- 语音注入复用剪贴板 + 模拟 Ctrl+V，会短暂占用剪贴板（约 1.2s）
- 开发板按键配置依赖固件协议 `ai_keyboard.v1`（VID `0x303A` / PID `0x1006`）
- 蓝牙音频通道暂不支持（BLE HID 带宽不足），蓝牙选项仅控制 HID 通道开关
- 固件不支持多方案，方案切换由主机端接管（借 Host Action 通道5 5回传）
- 固件文本注入对汉字不工作，含中文的固定文本由主机端用剪贴板注入

---

## 版本历史

### v0.3.0

- **语音编辑**：选中文本 → 说出修改要求 → LLM 流式编辑 → 自动替换
- **方案切换**：按键触发方案切换，主机端接管（借 Host Action 通道绕过固件限制）
- **中文固定文本**：含非 ASCII 字符的文本由主机端用剪贴板 + Ctrl+V 注入
- **LLM SSE 修复**：`resp.bytes()` + `from_utf8_lossy` 绕过 reqwest text() 解码问题
- **编辑键初始化修复**：`set_edit_ptt_key` 移到 `PttEngine::start` 之后
- **去重时间窗口**：Call 模式改为 2 秒窗口去重，允许用户有意重复说相同内容
- **浮层状态清理**：每次编辑/识别开始时清除上次残留字幕

### v0.2.0

- **三通道通信架构**：EIAU/EIHB/EICC/EICA 协议编解码 + UDP 17333 音频监听
- **通信配置命令**：Wi-Fi 连接 / 蓝牙控制 / 麦克风源选择
- **可视化键盘布局**：4×2 按键网格 + 圆形编码器
- **HID 冲突修复**：仅打开 app-command 集合，跳过键盘集合

### v0.1.0

- 初始版本：语音输入 + 开发板按键配置 + Host Action v1 + 应用库 + 方案库
