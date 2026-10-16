# stylecam —— 风格调色盘

> 内置插件，侧栏页签 `stylecam.board`，命名空间 `stylecam.*`。
> 上游：`PRD.md`（许清楚）、`DESIGN.md`（高见远）。冲突时以 `DESIGN.md` 的**源码结论**为准。

```
素材 → 风格 → 强度 → 生图 → 结果 → 创意显示 → 作品集
```

**零配置目标**：打开页签、拖一张图、按一次 Enter 就能出图。API Key 复用 easyop 已有的
`llm_api_key`（火山方舟）；用户不需要注册新账号、不需要填表单。

> ⚠ **这个目标在当前机器上不成立**：easyop 的 `llm_api_key` 实测是方舟 **Agent Plan（套餐）网关 Key**，
> 只对 `/api/plan/` 有效，**不能生图**。直连 `/api/v3` 一律 401。
> 详见 [§ 已知环境限制：easyop 的 Key 是 Agent Plan Key](#已知环境限制easyop-的-key-是-agent-plan-key)。
> 生图仍需自备一把能生图的 Key。**当前默认档已改为 `easysu`（EasySU 中转）并实测出图**，
> 换 provider 只改 `tools/stylecam_io.env` 四行 —— 见
> [§ provider 注册表：换 API 只需三步](#provider-注册表换-api-只需三步) 与
> [§ 已知环境限制](#已知环境限制easyop-的-key-是-agent-plan-key)。

---

### 版本历史
- **0.5.3**（线上事故修复）：修复所有配置写入必然失败（`configFailure` 漏 `await`）——硬件键开关切不动、设置面板保存静默失效的根因；修复 browse 阶段 Enter 无响应（`dispatchAction` 缺 confirm 分支）；修复注册命令丢参数（⌘SS / 硬件键带参调用全废）；`provider list` 不再误传 `--model`。测试 295+29+18 全绿。
- **0.5.2**（防跳顶 + 风格样品图）：修复点击按钮后页面跳回顶部的问题；风格轨道卡片新增真实样品展示图（猫型轮足机器人 × 10 组风格 × 强度 100，EasySU 实测出图）；prompt 强度档位词改英文，修复中文档位名被画进图里的缺陷；bundle 体积上限放宽到 512KB。测试 287+29+18 全绿。
- **0.5.1**（UI 紧凑化）：收紧工作台与设置面板间距、消除分区间空白间隙；「保存 Key」按钮移入 API Key 输入框同行就近；测试 287/287 通过。
- **0.5.0**：集中配置 v2、provider 注册表、EasySU 实测出图、DeskMate 风格工作台四区 UI。

## 0. 集中配置（v2，插件 0.5.3）

StyleCam 的非敏感生图参数统一保存在 `stylecam.config`；设置面板分为「图像参数」和「API 配置」两区，命令接口提供 `stylecam.config.get/set/validate/reset`、`stylecam.provider.list`、`stylecam.key.status/set`。`plugin.json` 当前共 **21 条命令、6 条 tools、1 个页签**。

### 0.1 配置项、默认值与限制

| 配置项 | 默认值 | 合法范围 / 作用 | provider 限制 |
| --- | --- | --- | --- |
| `provider` | `easysu` | `easysu` / `ark` / `openai_compat` / `zhipu` | 决定端点、模型默认值、尺寸映射和能力矩阵 |
| `baseUrl` | 空 | 空值使用 provider 默认端点；最长 2048 字符；必须 HTTPS，仅 `localhost` / `127.0.0.1` / `::1` 可用 HTTP | 可传 API 根地址或完整 `/images/generations` / `/images/edits` 地址 |
| `model` | 空 | 空值使用 provider 默认模型；最长 256 字符 | `openai_compat` 的能力会随模型变化；`dall-e-3` 和非 `gpt-image-*` 模型只接受 `format=auto`、`n=1`、无 `quality` |
| `resolution` | `1K` | `1K` / `2K` / `4K`，表示服务商的分辨率等级 | EasySU 发 `resolution`；Ark 发 `size=resolution`；像素型 provider 主要使用宽高 |
| `width` / `height` | `1024` / `1024` | 各为 256–4096 且是 64 的倍数；乘积不超过 16,777,216 | EasySU 映射到最近的 `1:1/16:9/9:16/4:3/3:4`，相对误差须 ≤3%；Ark 只接受方形；OpenAI 兼容/智谱发 `宽x高` |
| `format` | `auto` | `auto` / `jpeg` / `png` | 智谱只支持 `auto`；Ark、EasySU 支持三种；OpenAI 兼容取决于模型 |
| `quality` | `null` | `null` 表示自动；或 1–100 整数；仅 `format=jpeg` 时有效 | EasySU 支持；OpenAI 兼容的 `gpt-image-*` 支持；Ark、智谱不支持 |
| `n` | `1` | 1–4，单次生成数量 | EasySU、`gpt-image-*` 最多 4；Ark、智谱及受限 OpenAI 模型最多 1 |
| `httpTimeoutSecs` | `120` | 10–240 秒，单次 HTTP 尝试超时 | `eo.run` 超时会按请求尝试、URL 下载、退避和 30 秒收尾余量动态计算，并夹在 60–1260 秒 |
| `retryCount` | `1` | 0–3，额外重试次数 | 仅重试 408/425/429/500/502/503/504 和明确的连接前失败；POST 已发送后的读超时/连接中断不重试，避免重复计费 |
| `outDir` | 空 | 空值落到 `%USERPROFILE%\Pictures\easyop-stylecam`；最长 2048 字符 | 写入前由脚本验证目录可创建、可读写 |
| `API Key` | 不存入 config | 设置面板密码框永不预填，保存成功或失败后都清空输入值 | Key 单独写入 `tools/stylecam_io.env`；状态接口只返回来源、掩码和长度 |

另有 `hwKeys`、`privacyAcked`、`hwIntroSeen`、`pyExe` 等插件运行状态；它们不属于生图集中配置表，但仍随 `stylecam.config` 持久化。

### 0.2 生效优先级与单次覆盖

非敏感配置的最终优先级从高到低为：

1. Python CLI 显式参数；
2. 本次 job / 命令 patch（只影响单次调用）；
3. `stylecam.config` 持久化配置；
4. `tools/stylecam_io.env` 的对应环境项；
5. provider 注册表默认值。

配置修改后，下一次 `doctor`、`generate --dry` 或真实生成即生效，无需重启 easyop。`stylecam.config.get` 返回脱敏后的生效配置和来源；`stylecam.config.validate` 只验证不保存；`stylecam.config.reset` 只重置非敏感配置，不删除 API Key。

旧配置自动迁移到 v2：旧 `size=1K/2K/4K` 转为 `resolution`，旧 `size=<宽>x<高>` 转为 `width/height`；若新旧字段同时存在，以新字段为准。迁移时会剔除 `apiKey`、`IMAGE_API_KEY`、`ARK_API_KEY`，避免历史 Key 继续留在 `storage.json`。

### 0.3 API Key 安全边界

- Key **不会进入** `stylecam.config`、`stylecam.job`、`eo.run` 参数、命令信封或日志。
- UI 保存 Key 时只把 `{nonce,value,createdAt}` 临时写入 `stylecam.secret`；`eo.run` 仅携带 storage key 和 nonce。
- Python 读取 secret 后会先原子写回 `null`，再校验 nonce/有效期并更新 `.env`；`.env` 更新使用临时文件 + `os.replace()`。
- 前端在 `finally` 再清理一次 `stylecam.secret`，覆盖成功、失败和超时路径。
- `key status` 仅输出 `KEY_SRC`、`KEY_MASKED`、`KEY_LEN` 等脱敏信息。命令行 `--api-key` 仅用于脚本级临时调试，不应写入日志或提交到仓库。
- Key 查找顺序为 CLI `--api-key` > `.env` 的 `IMAGE_API_KEY` / 兼容项 `ARK_API_KEY` > easyop `llm_api_key`；最后一项仅供 Ark 兼容兜底。

### 0.4 QA 验证口径

- Node harness：274/274；`clamp-extra.mjs`：29/29；Python unittest：18/18。
- 根 `npm run plugins:test` 串行执行 studio harness、StyleCam harness、clamp-extra 和 Python unittest。
- 设置面板 DOM 断言覆盖两个分区、12 个非敏感字段、空 password 输入框、`width:100%` / `min-width:0` 防溢出、浅色主题变量和无 `position:fixed`。
- 2026-09-28 的 QA 环境未发现可用 Chrome/Edge/Chromium/Firefox，因此没有生成浏览器截图；这是 DOM/CSS 降级验证，不等同于真实 WebView2 目视验收。

---

## 0.5 创作工作台 UI（DeskMate 空间方法）

本轮将原来的七段表单式页面重排为四个持续可见的创作区域，不改变生图、配置 v2、21 条命令、硬件键、作品集和创意显示协议：

- `.sc-material-shelf`：顶部横向素材架，包含添加/拖拽/粘贴入口与最多 4 张素材卡；当前主体明确标为“已装载”。
- `.sc-creation-stage`：中部创作机器，以当前风格中文名为视觉标题，包含素材预览、10 种风格横向轨道、生成状态与从机器下沿出现的结果容器。
- `.sc-strength-dial`：独立强度控制区，以环形百分比、文字档位、0/25/50/75/100 刻度和底层可访问 range 共同表达；键盘/旋钮仍走原命令链。
- `.sc-gallery-strip`：底部横向作品带，当前作品有选中边框，并提供查看、原图对比、创意显示、保存和删除动作；30 条满额拦截语义不变。

设置面板仍完整保留 12 个非敏感字段和安全 Key 通道，但入口退到右上角工具按钮，overlay 继续使用 `position:absolute`。布局在 900px 以下把强度区移到创作台下方，640px 以下改为单列；只有素材、风格和作品轨道允许局部横向滚动。生成脉冲、风格聚焦和出片动画均受 `prefers-reduced-motion` 控制。

---

## 1. 文件清单

| 路径 | 行数 | 说明 |
| --- | --- | --- |
| `plugin.json` | — | 21 条命令 + 6 条 tools + 1 个页签 + capabilities |
| `src/constants.js` | 408 | 10 组风格 / 6 种创意显示 / 限流阈值 / storage key / `CODE` 表（文案逐字来自 PRD 5.2）/ `PROVIDER_HINTS` / `STAGE_HINTS.privacy`（含 `{provider}` 模板） |
| `src/logic.js` | 662 | 全部纯函数（禁 `document` / `window` / `eo.` / `await`）；`clampStrength` 非法值回落 50；`fillTemplate` 认 `{provider}` |
| `src/host.js` | 218 | `eo.*` 门面，不做业务判断 |
| `src/state.js` | 330 | state 单例 + config / gallery / job 持久化 + 订阅 |
| `src/cmds.js` | — | 21 条命令，统一信封，无 DOM；含配置、provider、Key 与动态 timeout 路由 |
| `src/keys.js` | 157 | window keydown 挂摘 + 四道规避 + 自愈 |
| `src/ui.js` | 1290 | DOM 构建 / 七分区渲染 / 浮层 / 6 种创意显示；隐私浮层按 `{provider}` 填充 |
| `src/styles.js` | 912 | CSS 字符串，`--sc-*` 变量，双主题，**无 fixed 定位** |
| `src/main.js` | 251 | `activate` / `deactivate` / `renderPage` / `__test` |
| `index.js` | 3847 / **138.3 KB** | **打包产物，禁止手改**（esbuild 生成） |
| `tools/stylecam_io.py` | — | 纯标准库脚本；含 doctor/generate/save/dir/key/config/provider 子命令与 provider 注册表（ark / openai_compat / zhipu / **easysu**） |
| `tools/stylecam_io.env.example` | — | 环境变量模板，**不含真 Key** |
| `test/harness.mjs` / `clamp-extra.mjs` / `test_stylecam_io.py` | — | QA 验证台：274 + 29 + 18 项；均纳入根 `plugins:test` |

### 打包（esbuild v0.21.5）

```bash
cd E:\MY_AI\trae\ESP32_S3_BOT\easyinput-prototype
node_modules\@esbuild\win32-x64\esbuild.exe plugins/builtin/stylecam/src/main.js \
  --bundle --format=esm --target=es2022 --outfile=plugins/builtin/stylecam/index.js
```

宿主走 `blob:` 动态 import 加载入口（`src/pluginHost.ts:171-173`），因此 `index.js` 必须自包含：
**不能有任何静态 `import ... from '...'`**。打包后用这条命令自检：

```bash
grep -c "^import" plugins/builtin/stylecam/index.js   # → 0
grep -c "position:fixed" plugins/builtin/stylecam/index.js   # → 0
```

---

## 2. 数据流

```
┌──────────── 前端（index.js，单文件 ESM） ────────────┐
│                                                      │
│  ui.js ──(用户操作)──▶ cmds.js ──▶ logic.js（纯函数） │
│   ▲                       │                          │
│   │                       ├──▶ state.js ──▶ host.js ──┼──▶ eo.storage
│   └──── subscribe ◀───────┘                          │      （stylecam.job / .config / .gallery）
│                            └──▶ host.js ──▶ eo.run ───┼──▶ python tools/stylecam_io.py
└──────────────────────────────────────────────────────┘            │
                                                                     ▼
                                                          火山方舟 /api/v3/images/generations
```

**一次完整生图（DESIGN §7 时序）**

1. 用户拖图/粘贴 → `ui.ingestOne`：`createImageBitmap` → `logic.planImageFit(w,h,bytes,'main')`
   → canvas 缩图 → `toDataURL('image/jpeg', q)` → `cmds.cmdImageAdd({dataUrl, bytes, w, h})`
2. `cmdImageAdd` 校验（4 张上限 / 单张 900KB / 合计 2.4MB）→ `state.addImage` → 订阅触发 `renderAll`
3. 点风格卡 / `1`–`9`、`0` / `←→` → `cmdStylePick` → `state.setStyle`
4. `←→` 调强度（±5，`Shift` 为 ±1）→ `cmdStrengthSet`（边界是 **toast** 不是错误）
5. `Enter` → `cmdGenerate`：
   - 校验（无素材 → `NO_IMAGES`；无风格 → `NO_STYLE`；缓存 `doctor.keyOk===false` → `NO_KEY`）
   - **首次生图**且 `!config.privacyAcked` → 返回 `PRIVACY_REQUIRED`，UI 弹页内确认浮层
   - `state.writeJob` → `eo.storage.set('stylecam.job', job)`（**图片 base64 走这里中转**）
   - `host.runScript(buildGenerateArgs(job,…), 300)` → Python 子进程
6. Python：读 `storage.json` 的 job → POST 方舟 → 字节嗅探扩展名 → 落盘 `outDir` → stdout 分块回传
7. `logic.parseScriptOutput(stdout, exit_code, timed_out)`：`CHUNK=i/n` **按 i 落位（天然容忍乱序）**
8. `state.clearJob()`（**用完立刻置 null**）→ canvas 产出 320px 缩略图 → `state.setResult`
9. 订阅 → `renderResult`：大图 + 元信息行「复古胶片 · 强度 55% · 2K · 用时 9.1 秒 · 277KB」
10. `S1` 保存 → `cmdResultSave` → `state.pushGalleryItem`（上限 30，**满了拦截不淘汰**）
11. `S2` 进创意显示 → 6 种形态**纯前端**切换，零网络请求

### 三条中转通道（为什么这么绕）

| 通道 | 为什么 |
| --- | --- |
| 图片 base64 走 `storage.json`，**不进 `eo.run` 的 args** | Windows `CreateProcess` 命令行上限 32767 字符；DESIGN §10.4.2 要求每个 arg `< 4096` |
| 图片回传走 stdout **分块**（`CHUNK=i/n`，默认 64KB） | `runner.rs:374` 把 stdout 全文 join 成一个 String 回传；分块让解析可容忍乱序 |
| 缩略图由**前端 canvas** 产出，脚本不产 `THUMB=` | 脚本是纯标准库，无 PIL，无法解码/缩放 JPEG（DESIGN 2.1 偏离 #1） |

---

## 3. 脚本契约（`tools/stylecam_io.py`）

```
python -u tools/stylecam_io.py doctor   [--probe] [--provider <key>] [--model <id>] [--base-url <u>] [--out-dir <p>]
python -u tools/stylecam_io.py generate --job-key stylecam.job [--chunk 65536] [--dry] [--provider <key>] [--base-url <u>]
python -u tools/stylecam_io.py save     --from <path> [--out-dir <p>]
python -u tools/stylecam_io.py dir      --out-dir <p>
python -u tools/stylecam_io.py key      status | from-easyop | set-env --value <key>
```

- **stdout 协议**：业务信息一律 `K=V` 单行；`MSG=` 是给人看的一句话；traceback **只进 stderr**。
- **退出码**：`0` 成功 / `2` 前置失败（Key·参数·存储）/ `3` 生图失败（API·网络）/ `4` 落盘失败。
- **错误码**：`E_ARGS E_NO_JOB E_STORAGE E_NO_KEY E_IMG E_API E_NET E_IO E_PARSE`
  → 前端按 `constants.SCRIPT_ERR_MAP` 映射；`E_API` 再按 HTTP 状态码二次判定（401/403 → `AUTH_FAILED`、
  429 → `RATE_LIMITED`、含 `ModelNotOpen` → `MODEL_NOT_OPEN`、其余 → `API_ERROR`）。
- **HTTP 超时 120s**，必须小于 `eo.run` 的 300s。
- **落盘扩展名按响应实际字节嗅探**（`\x89PNG` / `\xFF\xD8\xFF` / `GIF8` / `RIFF…WEBP`），不硬编码。
- **`generate` 成功输出**：
  ```
  OK=1
  PROVIDER=ark
  MODEL=doubao-seedream-5-0-250128
  PATH=C:\Users\...\Pictures\easyop-stylecam\20260925-120000_retro-film.jpg
  BEGIN_IMAGE
  CHUNK=1/23:iVBORw0KGgo...(65536)
  CHUNK=23/23:...
  END_IMAGE
  META={"jobId":"…","styleId":"retro-film","strength":55,"size":"2K","elapsedMs":9120,"bytes":284113,"ext":"jpg"}
  ```
- **超大降级**：base64 累计 > 12MB 时不发 `BEGIN_IMAGE`，改发 `TOO_LARGE=1` + `PATH=`
  → 前端显示「图已存到磁盘，但太大没法在页签里预览」并给「打开目录」按钮。
- **Key 三档优先级**：`--api-key` > `tools/stylecam_io.env` 的 `IMAGE_API_KEY` >
  `%APPDATA%\com.easyop.app\config.json` 的 `llm_api_key`（**仅 `PROVIDER=ark` 兜底**）。
- **`I2I=1/0`**：这个 provider 支不支持图生图。`0` 时素材图**不会上传**，脚本退化为纯文生图
  并 `MSG=` 明确说明 —— 不静默丢用户素材。前端据此提示用户。

### provider 注册表：换 API 只需三步

脚本里有一张**声明式注册表** `PROVIDERS`（`tools/stylecam_io.py` 顶部），
每个 provider 一条，字段说明「端点 / 模型 / 鉴权 / 图生图怎么传 / size 传等级还是像素 /
响应怎么解析 / 怎么探测」。**主流程不含任何 provider 分支**，换 API 不碰代码。

内置四个（**默认档是 `easysu`**）：

| key | 谁 | 端点 | 默认模型 | size | 图生图 | 响应 |
| --- | --- | --- | --- | --- | --- | --- |
| `easysu` | **EasySU 中转（当前默认）** | `https://aigc.easysu.cn/v1/images/generations` | `gpt-image-2.5-flare` | `1K` + `ratio`（`res_ratio` 档，不发 `size`） | ✅ `/v1/images/edits` **JSON body**，`image` 是 base64 **字符串** | `b64_json` 或 `url` |
| `ark` | 火山方舟 | `…/api/v3/images/generations` | `doubao-seedream-5-0-250128` | `2K`（等级） | ✅ `image` 数组 | `data[].b64_json` |
| `openai_compat` | OpenAI 兼容 / 中转站 | `https://api.openai.com/v1/images/generations` | `gpt-image-1` | `1024x1024`（像素） | ❌ 退化文生图 | `b64_json`，没有就下载 `url` |
| `zhipu` | 智谱 | `https://open.bigmodel.cn/api/paas/v4/images/generations` | `cogview-4` | `1024x1024`（像素） | ❌ 退化文生图 | 只回 `data[].url` → 下载 |

**换 API 三步**（以换智谱为例）：

1. 照 `tools/stylecam_io.env.example` 复制出 `tools/stylecam_io.env`；
2. 改四行：
   ```
   PROVIDER=zhipu
   IMAGE_API_KEY=你的智谱 Key
   IMAGE_BASE_URL=            # 留空就用注册表里的默认端点
   IMAGE_MODEL=cogview-4      # 留空就用默认模型
   ```
3. 跑一次自检看它认不认：`python tools/stylecam_io.py doctor --probe`
   （输出 `PROVIDER=` / `PROVIDER_NAME=` / `ENDPOINT=` / `MODEL=` / `I2I=`；`OK=1` 才可用）。

中途想试别家不用改文件：命令行参数优先级最高 ——
`python tools/stylecam_io.py generate --dry --provider openai_compat --api-key <key>`。

> 前端传的是**等级串**（`DEFAULTS.size = '2K'`，用户可在设置里改）。脚本按 provider 的
> `size_mode` 翻译：px 档（OpenAI 兼容 / 智谱）把非 `1024x1024` 的换成自己的默认值，
> **不会把 "2K" 原样发出去**；`res_ratio` 档（easysu）把等级放进 `resolution`，再带一个
> `ratio`，**不发 `size` 字段**（`1K` / `2K` / `4K` 白名单外的值回落到该档默认 `1K`）。
>
> **1K 的实测体积（真机出图那一档）**：1K PNG 约 **931 KB**，base64 后约 **1.24 MB** ——
> `CHUNK` 64 KB 分块可承载，远在脚本 `MAX_B64_BYTES = 12MB` 之下。
> **为什么默认档落在 1K**：easysu 的 `default_size` 就是 `1K`（前端不传或传了白名单外的值
> 时用它），画质够用、一次生成耗时可控；上 2K / 4K 体积与耗时成倍增长，4K 很容易顶到
> 分块与 12MB 上限。要更清晰的图，改设置里的尺寸即可（easysu 认到 4K）。
>
> 首次生图的隐私提示（`STAGE_HINTS.privacy`）**已动态化**：模板写
> 「这张图会上传到`{provider}`用于生成，不会用于训练。继续？」，`{provider}` 用 `doctor`
> 回传的 `PROVIDER_NAME=` 填（如「EasySU 中转」），没自检过就回落「当前生图服务」。
> 换 provider 不必再改文案。

### 如何新增一个 provider

只往 `PROVIDERS` 里加一项，**主流程零改动**：

```python
PROVIDERS["my_provider"] = {
    "name": "我的生图服务",
    "endpoint": "https://api.example.com/v1/images/generations",
    "models": ["model-a", "model-b"],       # 第一个是默认模型
    "auth": "bearer",
    "i2i": None,                            # None=不支持图生图；"image_array"=image 是 base64 数组
    "size_mode": "px",                      # "px" 传 1024x1024；"tier" 传 2K
    "default_size": "1024x1024",
    "response": "b64_or_url",               # b64_json_field / url_field / b64_or_url
    "probe": "models_list",                 # 零成本 GET .../models；"generate" = 真生一张
    "probe_fallback": "generate",           # 列表里没匹配模型时：generate / fail
    "extra": {"response_format": "b64_json"},   # 该家特有的固定字段
    "easyop_fallback": False,               # 没配 Key 时不要用 easyop 的方舟网关 Key 兜底
    "plan_probe": False,                    # 直连 401 时不发 /api/plan/ 判 Key 种类（方舟特有）
}
```

加完自测：`python tools/stylecam_io.py generate --dry --provider my_provider --api-key <key>`
（`--dry` 不联网，打印将要发出的摘要）+ `doctor --probe`。
响应字段若是新花样（比如 `data[].image_base64`），才需要在 `extract_image_bytes`
里加一个 `response` 分支 —— 那是唯一可能需要动主流程的地方。

### 输出目录

默认 `%USERPROFILE%\Pictures\easyop-stylecam`（DESIGN §9.2）。
在页签「⚙ 设置」里可改：**改完必须先跑 `dir --out-dir` 验证可写，只有 `OK=1` 才落盘**。
不提供系统目录选择对话框 —— 宿主没有 fs/对话框能力，用 `eo.run` 开 `FolderBrowserDialog`
会阻塞调用且抢焦点。

### `generate --dry` 的正确姿势

`--dry` 不联网，只打印"将要发出的请求摘要"，但它**仍要读 `storage.json` 拿 job**。
`storage.json` 只在**运行目录**落盘（`%APPDATA%\com.easyop.app\plugins\stylecam\storage.json`），
dev 源目录 `plugins/builtin/stylecam/` 下**没有**它 —— 所以在 dev 源目录直接跑
`generate --dry` 必然 `E_STORAGE`。两条路：

```bash
# ① 显式指向一个已存在 job 的 storage.json
python tools/stylecam_io.py generate --dry \
  --storage "%APPDATA%\com.easyop.app\plugins\stylecam\storage.json"

# ② 或者从运行目录下跑（脚本按 __file__ 推导，默认就是同目录的 storage.json）
cd "%APPDATA%\com.easyop.app\plugins\stylecam"
python tools\stylecam_io.py generate --dry
```

---

## 已知环境限制：easyop 的 Key 是 Agent Plan Key

> 2026-09-25 实测（花了几分钱真金白银换来的结论，**别让它丢失**）。
> 结论：**「零配置用 easyop 的 Key 生图」这条路走不通 —— 是环境限制，不是代码缺陷。**
>
> **2026-09-28 更新（现状，以本节为准）：这条路已被 `easysu`（EasySU 中转）取代。**
> 默认档改成中转后**不需要用户去换方舟原生 Key**，且图生图经 `/v1/images/edits`
> JSON body **真机出图验证成功**（构图 1:1 保留，不是退化成文生图）。
> 「必须换方舟原生 Key」是 0.3.0 时的结论，**已作废**。

被实测的 Key：easyop 配置 `%APPDATA%\com.easyop.app\config.json` 的 `llm_api_key`
（`ark-` 前缀、46 字符）。

| 端点 | 结果 |
| --- | --- |
| `POST https://ark.cn-beijing.volces.com/api/v3/chat/completions` | **401** AuthenticationError |
| `POST https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions` | **200**（能用） |
| `POST .../api/plan/v3/images/generations` 空 body | 400 MissingParameter `model`（路径存在） |
| 同上，model 传 `doubao-seedream-3-0-t2i-250415` / `4-0-250828` / `4-5-251128` / `5-0-lite-260128` | **全部 404 `UnsupportedModel`**："The requested model does not support the agent plan feature" |

**判读**

1. 这把 Key 由方舟 **Agent Plan（套餐）网关**签发，**只对 `/api/plan/` 路径有效**，直连 `/api/v3` 一律 401。
2. 套餐白名单里**只有聊天模型，没有任何生图模型** —— 4 个 Seedream 全部 `UnsupportedModel`。
3. 于是注册表里 `PROVIDERS["ark"]["endpoint"]`（`/api/v3/images/generations`）写死是对的，
   从 `llm_endpoint` 推端点更是错的（见 §7 已否决方案 #6）。

**自救路径**（用户视角，按优先级）

**① 当前默认：走 `easysu` 中转（已配好，已实测出图，不用动）**
`tools/stylecam_io.env` 里 `PROVIDER=easysu` + 中转 Key + `IMAGE_BASE_URL=https://aigc.easysu.cn/v1`。
`.env` 已配好且**不要动它**；要换别的 Key / 别家才改。

**② 想用方舟原生 Key（可选，不是必须）**

1. 去方舟控制台「API Key 管理」**新建一把原生 API Key**；
2. 在「开通管理」里**勾选 Seedream 模型**；
3. 填进 `tools/stylecam_io.env` 的 `IMAGE_API_KEY`，并把 `PROVIDER=` 改回 `ark`，或跑
   `python tools/stylecam_io.py key set-env --value <key>`。
   （**或者干脆换别家**：`PROVIDER=openai_compat` / `zhipu`，见 §3「换 API 只需三步」。）

**③ 图生图现状（easysu 档）**

`/v1/images/edits` 吃 **JSON body**，`image` 字段是 base64 **字符串**（不是数组；
裸 b64 与 `data:` 前缀都收），服务端真的会解析 —— 注册表里记为 `i2i="edits_json"`。
真机结果：**构图 1:1 保留，图生图真实生效**。1K PNG 实测约 **931 KB**
（base64 后约 1.24 MB，64 KB 分块可承载），这也是默认取 `1K` 的取舍：
再往上体积成倍增长，4K 容易顶到 `MAX_B64_BYTES = 12MB`。

**`doctor --probe` 会自己说出这件事**（只在显式 `--probe` 时联网；直连 401 才兜底发一条
`max_tokens=1` 的聊天请求到 `/api/plan/`，成本可忽略）：

```
PROVIDER=ark             # 当前生效的 provider
ENDPOINT=https://ark.cn-beijing.volces.com/api/v3/images/generations
KEY_KIND=agent_plan      # native = 原生 Key（继续探测可用 model）；unknown = 两边都不通
KEY_DIRECT=0             # 直连 /api/v3 是否通
KEY_PLAN=1               # /api/plan/ 是否通
MSG=这是方舟 Agent Plan（套餐）网关 Key：只对 /api/plan/ 路径有效，且套餐白名单不含任何生图模型
（实测 4 个 Seedream 模型全部 UnsupportedModel）。请去方舟控制台「API Key 管理」新建一把原生 API Key，
并在「开通管理」勾选 Seedream 模型，然后填进 tools/stylecam_io.env 的 IMAGE_API_KEY。
```

**默认档 `easysu` 的自检输出就不长这样**（探测走零成本 `GET /v1/models`，不判 `KEY_KIND`）：

```
PROVIDER=easysu                                # 当前生效的 provider（key）
PROVIDER_NAME=EasySU 中转                      # 人读名：前端隐私文案的 {provider} 就用它
ENDPOINT=https://aigc.easysu.cn/v1/images/generations
I2I=1                                          # 图生图可用
OK=1                                           # 可用
```

`PROVIDER_NAME=` 与 `RATIO=` 是 0.4.0 新增的 stdout 字段（`META` 里另有 `resolution` /
`ratio`）；`parseDoctorOutput` 读它存进 `doctor.providerName`，隐私浮层据此填充。

---

## 4. 命令与 tools

15 条命令（见 `plugin.json`）。**只 3 条进 `tools`**：`doctor` / `state` / `gallery.list`。

> ⚠ 这三条除了 `return` **还必须 `eo.log.info(message)`**：
> ⌘SS 里对 `kind=="plugin"` 只调 `plugin_host::activate` 就 `return Ok(())`（`lib.rs:555-562`），
> `pluginHost.ts:229-243` 是 `await fn()` **丢弃返回值**，不打日志用户看不到任何反馈（DESIGN 2.1 偏离 #7）。

`gallery.list` 必须返回**一句话摘要**，不能是裸数组（PRD A5）：

```
作品集 12 / 30 条，最近 3 条：水墨国风·强度 70（09-25 14:20）、赛博霓虹·强度 55（09-25 12:03）、复古胶片·强度 30（09-24 20:41）
```

其余 12 条一律不进 `tools`：它们依赖页签瞬时状态，从 ⌘SS 触发只会稳定拿到 `NOT_READY`。
`config.set` 尤其不进：改完配置已挂载的页签不会自己刷新，用户会以为「改了没反应」。

---

## 5. 页签契约落实

| 契约 | 落实位置 |
| --- | --- |
| P1 同步返回 | `main.js:renderPage` → `{ refresh, dispose }`，**不返回 Promise** |
| P2 禁 fixed 定位 | 全仓 `grep -c "position:fixed"` = 0；放大/设置/隐私/作品集浮层一律 `absolute`，定位基准是插件根 `.stylecam-panel{position:relative}` |
| P3 不动容器 | 只 `container.appendChild(root)`，不 `remove()`、不挪走 |
| P4 样式只作用本插件 | 变量 `--sc-*`，class 收口 `.stylecam-panel`，注入 `<style data-sc="style">` |
| P5 重复注册覆盖 | `renderPage` **幂等**：先 `destroyPage()` 再重建，`<style>` 始终只有 1 份 |

**`dispose` 一次摘全**（DESIGN §10.4.5）：window keydown、window paste、注入 `<head>` 的 `<style>`、
插件根 DOM、订阅。

**keydown 自愈**：`PluginPageContainer.tsx:14-28` 用 async IIFE 拿 `lifecycle`，
若激活未完成时卸载，`lifecycle` 仍为 null → `dispose` 永不执行，keydown 会留在 window 上。
因此 `keys.js` 的 handler **首行**判断 `root/container.isConnected`，不在文档里就自己摘掉自己（偏离 #8）。

**四道门控**：`state.mounted` → `config.hwKeys`（默认开）→ 非 Ctrl/Meta/Alt → 非 INPUT/TEXTAREA/SELECT/contentEditable。
**已消费才 `preventDefault()`**；旋转类（方向键/滑杆）允许 repeat + 60ms 节流；命令类 `e.repeat` 直接 return。

---

## 6. 6 种创意显示（DESIGN §9.1 裁定）

全部**纯前端 Canvas + CSS，零网络请求**，本质是「本地重新呈现同一张结果图」，不是重新生成。

| # | 形态 | 实现 |
| --- | --- | --- |
| ① | 原图对比 | 两层 `<img>` 叠放，上层 `clip-path: inset(0 X% 0 0)`，指针拖动 / `←→` 改 X |
| ② | 拍立得相框 | 不均等白边（下留白最宽）+ 投影 + `rotate(-2deg)` + 手写体回退的caption |
| ③ | 杂志跨页 | CSS grid 两列 + 两个 div 的 `background-position: left/right` 各显一半 + 1px 中缝阴影 |
| ④ | 色板拆解 | **唯一需要 canvas**：64×64 离屏取样 → 4bit/通道分桶计数 → Top6 色带 + 占比条 |
| ⑤ | **细节放大镜** | 4 个 `border-radius:50%` 取四象限 + 1 个中心，`background-size: 300%`（**替换原「胶片连拍」**） |
| ⑥ | 前后轮播 | 两层 `<img>` + `@keyframes` 交叉淡入淡出 2s；`prefers-reduced-motion` 降级为静态 |

`④` 是异步的（`createImageBitmap`），填充失败只把当前形态换成「这个显示形态渲染失败了，换一种试试。」，
不影响其它 5 种与已保存的结果（T40）。

---

## 7. 已否决方案存档

| # | 方案 | 否决理由 | 现状 |
| --- | --- | --- | --- |
| 1 | **`__TAURI_INTERNALS__` 后门**（`withGlobalTauri:true` + `csp:null` 确实敞开，可直接调 fs/http） | easyop 未注册任何 fs/http 命令；走后门绕过插件契约，**未来必断**；且绕过了「插件只能做 capabilities 允许的事」这条底线 | 一切落盘/联网都走 `eo.run` + 自带 Python 脚本 |
| 2 | **`file://` + `convertFileSrc` 直接渲染磁盘图片** | 需要 `asset:` 协议与 scope 配置（在 `src-tauri/` 里，本轮禁止改动）；且图片可能落在 `Pictures` 等 scope 外目录，配了也不稳 | 图片以 stdout 分块 base64 回传 → `data:` URL 渲染 |
| 3 | **风格样片用真图** | `plugin_host.rs:44` 是 `fs::read_to_string` 全量读入再 emit，入口越大激活越慢；10 张真图会让 `index.js` 从 136KB 涨到 MB 级 | 10 组风格用 CSS 渐变 + 4 色色板占位（`STYLES[].img` 字段已预留，二期可切） |
| 4 | **作品集满 30 条自动淘汰最旧的** | 会让用户「不知情地丢图」，直接违反 PRD §1 的设计底线「用户应当始终知道自己正在调什么」 | **拦截并提示删除**（`GALLERY_FULL`），不自动淘汰 |
| 5 | **「胶片连拍」创意显示** | 语义上承诺「多张不同的连拍帧」，而本链路一次生图**只产出一张结果图**；用同一张图填满 5 个画格会让用户以为生成了 5 张 | 替换为 **⑤ 细节放大镜**（强度 >85 时用户恰恰需要看局部细节，且零歧义）。<br>胶片外观不废弃：二期若要这个外观，只能做成「原图→结果的 N 档不透明度渐变胶片条」，并在文案写明「同一张的过渡」，**不得**用重复的同帧冒充连拍 |
| 6 | **由 `config.json.llm_endpoint` 推导生图端点** | `llm_endpoint` 是 `https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions`，中间带 `/plan/` 网关段，替换路径会拼错 | 端点在 `constants.ARK_ENDPOINT` **写死**（`https://ark.cn-beijing.volces.com/api/v3/images/generations`） |

**附：其它被源码推翻的计划/PRD 结论**（详见 `DESIGN.md` 2.1 的 9 处偏离）

- `output_format` 默认 **jpeg** 而非 png —— 2K PNG 经 base64 + IPC 回传体积 4–8MB，JPEG 约 0.5–1.2MB。
- `timeoutSecs` 一律写 **`timeout_secs`** —— `plugin_host.rs:196-201` 字段名是 `timeout_secs` 且无 `rename_all`，
  `timeoutSecs` 被 serde 静默忽略（studio 里有 8 处因此失效）。本插件两个都写，前者生效。
- **剪贴板图片只能走 DOM `paste` 事件** —— `eo.clipboard_read` 只返回 String 文本（`plugin_host.rs:91-95`），拿不到图片。
- `plugins/README.md` 是**新建**不是改（`plugins/` 下原本没有 README）。

---

## 8. 工程侧补码（PRD 5.2 未列）

与 `CODE.IO_FAILED` 同理，这几个码是工程侧补的，已在此存档：

| code | 触发 | 文案 |
| --- | --- | --- |
| `IO_FAILED` | 脚本 `OK=0` 但错误码无法归入 PRD 5.2 任何一项 | `生成失败：<message原文>。点『自检』看完整输出。` |
| `PRIVACY_REQUIRED` | 首次生图且 `!config.privacyAcked` | `这张图会上传到火山方舟用于生成，不会用于训练。继续？` → UI 弹页内确认浮层，确认后带 `ackPrivacy` 重试 |
| `NEED_RESULT` | 保存/导出/创意显示时内存里没有结果 | `还没有可展示的结果：先生成一张。` |

---

## 9. 测试缝 `__test`

`index.js` 导出 `__test`，QA 可脱离 DOM 断言。硬要求 7 项：

`planImageFit` / `mapHardwareKey` / `clampStrength` / `buildPrompt` / `parseScriptOutput` / `STYLES`（`length===10`）/ `CREATIVE_MODES`（`length===6`）

另附：`strengthBand` `buildGenerateArgs` `isTypingTarget` `summarizeState` `summarizeGallery`
`quantizePalette` `sniffExt` `applyGalleryLimit` `CODE` `STAGE_HINTS` `COMMANDS`（15 条）
`TOOLS_COMMANDS`（3 条）`activate` `deactivate` `renderPage` `getState` `getAttachCount` / `getDetachCount`（挂摘计数）。

**静态源码扫描断言**（DESIGN §10.3）：
`src/logic.js` 与 `src/cmds.js` 中出现 `document.` / `window.` 任一即视为违规（当前均为 0 命中）。

---

## 10. 待明确 / 遗留

| # | 事项 | 现状 |
| --- | --- | --- |
| 1 | 方舟 `GET /api/v3/models` 能否作零成本探测 | **本机实测走不通**：easyop 的 Key 直连 `/api/v3` 一律 401（见 §已知环境限制）。换成真 Key 后仍需验证；脚本已加 404/405 → 退化为真实生图探测（会在 `MSG=` 写明「本次探测消耗 1 次调用」） |
| 2 | 2K JPEG 实际体积分布 | 未实测。已有 12MB 降级路径兜底；真机出图后把实测值补进本节 |
| 3 | `eo.run` 能否稳定承载 ~1MB stdout | 未压测。若失败，退路是脚本只回 `PATH` + 前端改走「打开目录」 |
| 4 | WebView2 对 `createImageBitmap` 在 ≥8192px 图上的行为 | 已按「抛异常 → `IMG_DECODE_FAILED`」处理 |
| 5 | `test/harness.mjs`、`package.json`、`deploy-plugins.mjs` | **QA 那批的活**，本批未动。本批新增的 `clampStrength` 非法值断言放在 `test/clamp-extra.mjs`（29 条），因为 harness 不归本批改 |
| 6 | `contributes.pages[].icon` | 写了不显示（`Sidebar.tsx:49-59`）。仍写 `icon:"spark"`，宿主未来支持即自动生效 |
