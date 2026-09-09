# 更新日志

所有版本的重要变更都记录在此文件中。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本（Semantic Versioning）](https://semver.org/lang/zh-CN/)。

---

## [v0.1.1] — 2026-09-09

### 变更内容

**构建配置完善**

- `package.json`：新增 `tauri:build` 脚本命令，方便一键打包桌面应用
- `tauri.conf.json`：启用 Tauri 打包配置（`bundle.active = true`）
  - 设置应用图标 `icons/icon.ico`
  - 版权声明：`Copyright (c) 2026 easyop`
  - 应用分类：`Utility`（工具类）
  - 简短描述：easyop · ESP32 按键配置与语音输入工具
  - 详细描述：easyop 是一款配合 ESP32_S3_BOT 使用的桌面配置工具，支持按键方案管理、语音输入（ASR）与 LLM 文本编辑

### 影响

- 现在可以通过 `npm run tauri:build` 生成 Windows 安装包
- 打包产物会包含正确的图标、描述与元信息

---

## [v0.1.0] — 2026-09-08

### 新增

- 初始版本发布
- 前端：React 18 + Vite + TypeScript，三页签界面（按键映射 / 本机应用 / 语音输入）
- 后端：Rust + Tauri 2，Win32 FFI（HID / 键盘钩子 / SendInput / ShellExecuteW）
- 三通道通信：HID 配置下发 + Wi-Fi UDP 音频流 + 云端 ASR 流式识别
- 语音编辑：选中文本 → ASR → LLM 流式编辑 → 自动替换
- Host Action：方案切换、固定文本注入、打开应用
