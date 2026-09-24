# Wi-Fi 链路：自动检测 / 自动连接 / 历史记录 / 快速重连

> **适用版本**：v0.37.0 起
> **代码位置**：`src-tauri/src/wifi_link.rs`（链路本体）、`src-tauri/src/payload.rs`（载荷构造）、
> `src-tauri/src/keyboard_audio.rs`（UDP 直投通道）
> **一句话**：把"板子该连哪个 Wi-Fi"这件事，从"用户每次手输"变成"上位机按证据自动决定并写进板子的 NVS"。

---

## 一、先摆事实：这条链路的物理约束（决定方案形状的四条）

| # | 事实 | 出处 | 推论 |
|---|---|---|---|
| 1 | 音频**单向**：板子 → 上位机，UDP 17333 | 固件 `keyboard_audio.cpp` / 上位机 `keyboard_audio.rs` | "能不能用"的唯一判据是**上位机收到 EIHB 心跳**；"配置下发成功"不是判据 |
| 2 | 板子必须与上位机**同网** | 同上 | "板子该连哪张网"= **上位机当前连的那张网**。答案在上位机自己身上 |
| 3 | 固件**已经会**自动重连：上电 `load_config(NVS)` → `configure()` → `preconnect_wifi("config")`，此后每 ~10 s 重试 | 固件 `app_main.cpp:2941-2956`、`keyboard_audio.cpp:456-551`、`:987-995` | 真正缺的不是"重连逻辑"，而是**从来没人把端点写进 NVS** |
| 4 | 固件配置是**整体替换**：`ConfigState::apply_json` 执行 `current_ = parsed.config`；`profiles`/`keys`/`encoder` 缺失直接拒包 | 固件 `config_state.cpp:5-20`、`config_payload.cpp:704-882` | 下发载荷必须**完整**；只发 Wi-Fi 字段不但不生效，还会冲掉键位方案 |

### 顺带查实的两个真 bug（本次一并修掉）

1. **`wifi_connect` 从来没有生效过。** 旧实现发 `{"cmd":"wifi_connect",…}`，
   而固件配置解析器**没有 `cmd` 这个字段**，且 `schema` 必填（`ai_keyboard.v1`）
   ⇒ 整包判 `InvalidSchema` 被丢弃；`transport::send_command` **不等回执**
   ⇒ 上位机日志照写"已下发"。用户看到的是"点了连接、界面变绿、麦克风不出声"。
   同一理由，`wifi_disconnect` 也从未生效过。
2. **每次「下发方案」都会清空板子的 Wi-Fi 端点。** 旧 `push_config` 只发
   `schema` + `target_platform` + `profiles`；`apply_json` 整体替换后
   `wifi_ssid` / `audio_host` 回到默认空串 ⇒ 固件 `audio_enabled()` 为假
   ⇒ 主动释放 Wi-Fi。现象是"改完键位，板子麦克风就掉线"，且没有任何报错。

---

## 二、四个环节的逻辑

### 2.1 自动检测

**探测对象是上位机自己**，不是板子。理由见事实 2：判据是"同网"，不是"信号最强"。
让板子扫 AP 需要在固件里加扫描能力（新 Report ID + 回执通道），而结果并不比
上位机自己读一次更准。

| 量 | 来源 | 代价 |
|---|---|---|
| 当前所连 SSID | `netsh wlan show interfaces` → 解析 `^\s*SSID\s*:` | 便宜（~几十 ms） |
| 可见 AP 列表 | `netsh wlan show networks` → 解析 `^\s*SSID\s+\d+\s*:` | **贵**：会触发一次无线扫描，只在需要时读 |
| 本机 IPv4 候选 | ① UDP `connect("192.0.2.1:9")` 让内核选源地址（不发包）② 从 `netsh interface ipv4 show addresses` 扫全部 IPv4 字面量 | 便宜，结果缓存 30 s |

**跨语言解析**：`SSID` / `BSSID` 这两个标签 Windows 在中文系统上**不翻译**
（实测），所以按 ASCII 标签锚定即可；`^\s*SSID\s*:` 天然排除 `BSSID`（多一个 `B`）。
IPv4 只按数字扫（`scan_ipv4_tokens`），更与语言无关。

**过滤规则**（`is_candidate_lan_ipv4`）：剔除 `127.*`（环回）、`0.*`、`169.254.*`
（APIPA：没拿到 DHCP，连不上板子）、组播/保留段（`>=224`）、广播。

**环境指纹** `epoch = "{当前SSID}|{首选IPv4}"`。
⛔ 必须带 IPv4：只比 SSID 会漏掉"同一张网、本机 IP 变了"——那正是"板子连上了却
永远收不到音频"的原因（它还在往旧 IP 发）。

### 2.2 自动连接

一次下发 = 一个**完整**载荷：

```json
{
  "schema": "ai_keyboard.v1",
  "target_platform": "windows",
  "profiles": [ { "keys": {...}, "encoder": {...} } ],
  "wifi_ssid": "…", "wifi_password": "…",
  "audio_host": "192.168.x.y", "audio_port": 17333
}
```

`audio_host` 取**本机 IPv4 候选的首选**（默认路由出口）；固件 `config_ready`
要求它非空，否则连 Wi-Fi 都不会尝试。

**两条下发通道，谁通谁算数：**

| 通道 | 机制 | 适用 |
|---|---|---|
| **USB HID `0x10`** | `HidD_SetFeature` 分块 + `S3C` 魔数 + CRC16，**等 `0x11` kind `0x03` 回执** | **首次配网唯一可用**；能拿到 `saved`（= 已写 NVS） |
| **Wi-Fi UDP 直投** | 把 JSON 作为数据报发给板子控制端口；固件那条"配置直达通道"（首字节 `{` 且 `from == dest`）走与 USB/BLE 完全相同的应用+存盘路径 | 板子**已经在线**时的快通道，不必插 USB |

⚠️ UDP 通道的边界（写在这里，免得下次有人以为它万能）：
- 判据是 `from == dest` ⇒ **本机 IP 一变这条路就断**，只能靠 USB 重新配网。
  这是固件的安全设计，不是缺陷。
- 目的地来自 EIHB 心跳 ⇒ **没收到过心跳就没有目的地**。
- 固件 `kConfigMaxJsonLen = 2048`，超长包被整包丢弃（上位机侧同值校验）。

### 2.3 历史 Wi-Fi 记录

文件：`%APPDATA%\com.easyop.app\wifi_known.json`（与 `config.json` 同目录）

```json
{ "version": 1, "entries": [ {
  "ssid": "MyHomeNet", "password": "…",
  "last_success_unix": 1758..., "success_count": 3,
  "last_attempt_unix": 1758..., "last_error": ""
} ] }
```

- **"成功"以收到板子心跳为准**（`mark_success` 只在在线跃迁时调用），
  **不是**以下发回执为准。下发成功只代表板子存了盘。
- 一次失败**不清掉**"曾经成功过"（`mark_failure` 只写 `last_error`）——
  一次失败不代表这条网络作废，下次仍该被优先尝试。
- 排序 = **证据强度**：`last_success_unix` 降序 → `last_attempt_unix` 降序 → `ssid` 升序兜底。
  兜底不是洁癖：并列是常态（同一次开机连着写几条），不给兜底顺序会随文件系统漂移。
- 上限 16 条。**裁剪必须豁免刚插入的那条**（它的证据是零、按证据排序必然垫底，
  否则会出现"用户刚填的新网络，自动连接永远看不到它"——只在历史 ≥16 条时才复现）。
- **迁移**：启动时把 `config.json` 里已有的 `wifi_ssid` 补进历史。
  不做这一步，老用户第一次启动会看到"还没有成功连接过的网络"，而他明明有一个一直在用的网络。
- `set_wifi_config`（用户在界面上填一次）也会 upsert 进历史：填过 = 声明"我可能会用它"。

日志/展示里 SSID 一律打码（`mask_ssid`，如 `vin**st`）。

### 2.4 快速重连

**两层，分工明确：**

| 层 | 谁做 | 周期 | 前提 |
|---|---|---|---|
| 第一层（常态） | **板子自己** | ~10 s 一轮，由固件控制任务负责 | NVS 里有端点 |
| 第二层（补位） | **上位机看护线程** | 只在环境变了 / 长时间收不到心跳时 | 上位机能下发（USB 或 UDP 通道） |

上位机侧的退避序列：**0 → 5 → 15 → 30 → 60 s**（此后固定 60 s）。
在线或环境变化时立即清零。

为什么上位机不每 10 秒陪着重试：那会和板子自己的重试打架，而且路由器坏掉时
会持续刷 HID。上位机只在"板子自己解决不了"的场景出手 —— 换网、换 IP、首次配网。

---

## 三、触发条件（表 T）

看护线程每 1 秒评估一次，只有下列条件成立才动手。

| # | 触发 | 判定条件 | 动作 | 备注 |
|---|---|---|---|---|
| **T1** | 应用启动 | 首轮 tick 且已过 `STARTUP_DELAY_MS`(3 s) | 探测 → 选网 → 下发 | 3 秒是留给 UDP 绑定与 HID 枚举的 |
| **T2** | 板子 HID 出现 | `device_status()` 由 Err 变 Ok | 探测 → 选网 → 下发 | **首次配网必须走这条**（端点没进 NVS 时 UDP 通道也不存在） |
| **T3** | 环境变化 | `epoch`（当前 SSID + 首选 IPv4）≠ 上次下发时 | 立即下发，退避清零 | **在线时也触发** —— 换 IP 时板子虽然连着，但音频发不回来 |
| **T4** | 心跳丢失 | 已离线 ≥ `OFFLINE_RESEND_MS`(20 s) | 按退避重下发 | 先给板子 20 秒自愈窗口 |
| **T5** | 手动「连接」 | 用户点按钮 | **同步**下发并返回真实结果；纳入历史 | 与自动路径**同一个函数**，不另开一条 |

**前置门槛（优先级最高）：**

- `mic_source != wifi` ⇒ 整条链路停摆，状态 `disabled`。
  用户明确选了别的麦克风来源，后台不该还在改板子的网络配置。
- 历史为空 ⇒ 状态 `no-history`，如实告诉用户"无法自动连接，请手动连一次"。
  **刻意不去猜**：自动连一个用户从没成功连过的网络，只会把板子带到错误的网里，
  比不连更难排查。

**触发后的状态机**（`wifi_link_status` 的 `state`）：

```
disabled → no-history → idle → provisioning → waiting-heartbeat → online
                                     ↑                              │
                                     └──── T3/T4 触发重下发 ────────┘
```

| state | 含义 |
|---|---|
| `disabled` | 麦克风来源不是 Wi-Fi，本链路不介入 |
| `no-history` | 没有历史网络，无法自动连接（要用户先手动连一次） |
| `idle` | 有目标但未到触发/退避时间（含"板子未以 USB 连接，等待插入"） |
| `provisioning` | 正在下发完整载荷 |
| `waiting-heartbeat` | 端点已发出，等心跳确认（**此时还不算连上**） |
| `online` | 收到板子心跳，链路成立 |

**在线确认**：只有"离线 → 在线"的**跃迁**那一刻才调 `mark_success`。
`` 由此也顺手解决了"心跳每 2 秒一次，成功计数不能每 2 秒 +1"的问题。

---

## 四、选网优先级（`pick_candidate`）

按**证据强度**排，不按信号强度：

1. 上位机**当前所连**的 SSID 且在历史里 —— 证据最强（同网是硬要求）；
2. 可见 AP 中在历史里的 —— 说明历史网络在场，只是上位机连的是别的网；
3. 历史里证据最强的一条 —— 环境读不到时的兜底（无无线网卡 / WLAN 服务未启动）。

历史为空 ⇒ `None`。

---

## 五、验证状态表

| 项 | 方式 | 结果 |
|---|---|---|
| 载荷含四个端点字段且在 2048 B 内 | 单测 `wifi_endpoint_fields_are_emitted` | ✅ |
| 不带端点时不出现该组字段 | 单测 `wifi_endpoint_omitted_when_not_requested` | ✅ |
| `netsh` 中文输出解析（当前 SSID / 可见 AP） | 单测（**脱敏后**的输出样本） | ✅ |
| 不把 `BSSID` 误认成 `SSID`、空值不当 SSID | 单测 | ✅ |
| IPv4 扫描过滤环回 / APIPA / 越界 / 去重 | 单测 | ✅ |
| 选网三级优先级 + 无历史返回 None | 单测 ×4 | ✅ |
| 历史增删改、排序、裁剪、坏文件容错、落盘回读 | 单测 ×5 | ✅ |
| 表满时新插入条目不被裁掉（回归钉子） | 单测 `freshly_added_entry_survives_trimming` | ✅ |
| 环境指纹把 IPv4 变化也算作"环境变了" | 单测 | ✅ |
| 退避序列单调且封顶 | 单测 | ✅ |
| SSID 打码不泄露原名 | 单测 | ✅ |
| 全量库测试 | `cargo test --lib` | ✅ 437 passed / 0 failed |
| 前端类型检查 + 构建 | `npm run build` | ✅ exit 0 |
| **实板端到端（板子真的连上并推流）** | 真机：插 USB + 真板，连开两次 | ✅ 2026-09-24 跑通（见 5.1） |

### 5.1 真机验证记录（2026-09-24）

**第一次启动**（迁移既有凭据 → 自动下发 → 收到心跳确认）：

```
[info] Wi-Fi 链路：已把 config.json 里的网络迁移进历史（SSID=vin**st）
[info] Wi-Fi 链路：USB 配置回执 ok=true saved=true bytes=599
[info] Wi-Fi 链路：端点已下发（自动，SSID=vin**st，通道=usb-hid，板子已存 NVS=true）
[info] keyboard_audio: 收到首次心跳 addr=10.x.y.z:54021 streaming=false audio_ready=true
[info] Wi-Fi 链路：已确认连接（SSID=vin**st，心跳到达）
```

随后 `wifi_link_status` 读到 `state=online` / `board_online=true` / `saved_to_board=true` /
`last_channel=usb-hid`；`wifi_known.json` 写入 `success_count=1`。

**第二次启动 = 快速重连的直接证据**：**没有**迁移日志、**没有**"端点已下发"
（即零配网动作），板子先于心搏空窗期送达心跳，直接进入 `online`；`success_count` 1 → 2。

```
[info] keyboard_audio: 收到首次心跳 addr=10.x.y.z:54021 …
[info] Wi-Fi 链路：收到板子心跳，链路在线
[info] Wi-Fi 链路：已确认连接（SSID=vin**st，心跳到达）
```

前端同步核过：徽标由 `board_online` 驱动（非硬写）；状态块 `winVisible=true` 且元素
在视口内（`offsetParent` 非空 / `opacity=1` / `getBoundingClientRect` 落在视口）
⇒ 真的看得见，不只是 DOM 里有字。

> `attempts` 在在线时被刻意归零（`watchdog_loop` 的在线分支 `attempt.store(0)`），
> 所以"刚下发过却显示第 0 次"是正确的，不是计数丢失。

**本次真机的前提**：开发板 **USB 与蓝牙都在位**
（`USB\VID_303A&PID_1006\EASY-INPUT-V2` 与 `HID\...&COL03` 供应商定义集合均为 OK），
`usb-hid` 通道因此可用。**纯蓝牙形态下配置类 Feature Report 走不通** —— 首次配网必须插 USB。

---

## 六、⚠️ 未做 / 已知边界

1. **板子侧的多网络扫描未做**（要改固件：新增 Report ID 放"扫到的 AP 列表"，
   并在 NVS 里存多槽网络表）。本设计用"上位机读自己的网络"替代 ——
   判据是"同网"而非"信号最强"，所以功能上等价，且不需要动固件。
   若将来要支持"板子脱离上位机时自己切网"，才需要这一步。
2. **首次配网必须插 USB**：端点没进 NVS 之前，板子不在网上，UDP 直投无从谈起。
   这是物理约束，不是实现偷懒。
3. **本机 IPv4 多网卡时的选择是启发式**：默认路由出口优先，其次按
   `netsh` 里出现的顺序。多出口机器（同时插网线 + Wi-Fi）上可能选中错的那个；
   缓解手段是候选列表 + 失败后退避重试，但**没有**做"逐个候选试一遍"的穷举。
4. **删除历史网络的界面入口未做**：现在只能手改 `wifi_known.json`。
5. 密码以明文存在 `wifi_known.json`（与既有的 `config.json` 同口径，未引入钥匙串）。
