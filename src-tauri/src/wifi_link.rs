//! 开发板 Wi-Fi 链路的**自动检测 → 自动连接 → 历史记录 → 快速重连**。
//!
//! # 先说清这条链路的第一性事实（不看清就会把力气花在错的地方）
//!
//! 1. **音频数据是单向的：板子 → 上位机**，走 UDP 17333。所以"能不能用"的唯一
//!    判据是**上位机有没有收到板子的 EIHB 心跳**（`keyboard_audio_online()`）。
//!    "配置下发成功"不等于"连上了" —— 固件只是把它存进了 NVS。
//! 2. **板子要和上位机在同一张网里**才有意义。于是"板子该连哪个 Wi-Fi"这个问题，
//!    答案**就在上位机自己身上**：上位机当前连的是哪个网，板子就该连哪个网。
//!    ⇒ **不需要让板子去扫环境**。让板子扫 AP 需要在固件里加扫描能力
//!    （新 Report ID + 回执通道），而结果反而不如上位机自己读一次准确 ——
//!    因为判据是"同网"，不是"信号最强"。
//! 3. **固件侧本来就会自动连接**：`app_main` 上电时 `load_config(NVS)` →
//!    `config_state.apply_json` → `sync_keyboard_audio_config("boot")` →
//!    `KeyboardAudioLink::configure` → `preconnect_wifi("config")`，之后控制任务
//!    每 ~10 s 重试一次。**前提是 NVS 里已经有一组 SSID + audio_host**。
//!    ⇒ 本模块要解决的真问题不是"板子不会重连"，而是
//!    **"从来没人把端点写进板子的 NVS"**（见下）。
//! 4. **过去上位机从没把 Wi-Fi 端点发给过板子。** 旧 `wifi_connect` 发的是
//!    `{"cmd":"wifi_connect",...}`，而固件配置解析器**只认固定顶层键、
//!    没有 `cmd` 分支**，且 `schema` 必填（`ai_keyboard.v1`）⇒ 整包被判
//!    `InvalidSchema` 拒绝；而 `send_command` **不等回执** ⇒ 上位机日志写
//!    "已下发"，板子那边一个字都没收到。**这是典型的静默失效。**
//!
//! # 本模块的四个环节（对应需求里的四个词）
//!
//! | 环节 | 做法 | 触发条件 |
//! |---|---|---|
//! | **自动检测** | 读上位机自己的网络环境：当前所连 SSID（`netsh wlan show interfaces`）+ 本机 IPv4 候选。可见 AP 列表只在需要时读（`show networks` 会触发一次无线扫描，有代价） | 见下表 T1–T5 |
//! | **自动连接** | 把**完整**配置载荷（键位方案 + Wi-Fi 端点）下发到板子。两条通道：USB HID `0x10`（要 Feature Report）与 Wi-Fi UDP 直投（板子 `keyboard_audio.cpp` 里那条"配置 JSON 直达通道"） | 同上 |
//! | **历史记录** | `wifi_known.json`：每条含 SSID / 密码 / 最近成功时间 / 成功次数 / 最近失败原因。**"成功"以收到心跳为准**，不是以下发成功为准 | 心跳到达时写入 |
//! | **快速重连** | 板子自己 ~10 s 一轮重试（前提是 NVS 有端点）；上位机只在**环境变了**或**长时间收不到心跳**时重下发，退避 0/5/15/30/60 s | 心跳丢失 ≥20 s 或环境 epoch 变化 |
//!
//! # 触发条件（表 T）
//!
//! | # | 触发 | 判定 | 动作 |
//! |---|---|---|---|
//! | T1 | 应用启动 | 首轮 tick（延迟 `STARTUP_DELAY_MS` 让 HID / UDP 就绪） | 探测 + 选网 + 下发 |
//! | T2 | 板子 HID 出现 | `device_status()` 由 Err 变 Ok（插上 USB / 蓝牙连上） | 探测 + 下发（**首次配网必须走这条**：端点没进 NVS 时 UDP 通道也不存在） |
//! | T3 | 环境变化 | `epoch`（当前 SSID + 下发用 IPv4）与上次**成功**时不同 | 立即下发，退避清零 |
//! | T4 | 心跳丢失 | 曾在线，且已离线 ≥ `OFFLINE_RESEND_MS`（20 s） | 按退避重下发（板子自己也在重试，别抢它的活） |
//! | T5 | 手动「连接」 | 用户点按钮 | 立即下发（同步返回真实结果），并纳入历史 |
//!
//! `mic_source != wifi` 时**整条链路停摆**（状态 `disabled`）：用户明确选了别的
//! 麦克风来源，就不该有后台线程还在改板子的网络配置。

use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::log_buffer;

// ---------------------------------------------------------------- 常量

/// 历史网络文件名（与 `config.json` 同目录）。
pub const KNOWN_FILE: &str = "wifi_known.json";
/// 历史上限（超出时丢"最近成功时间最早"的那条）。
pub const KNOWN_MAX: usize = 16;

/// 后台 tick 间隔。
const TICK_MS: u64 = 1000;
/// 启动后等多久再开始第一次下发：给 UDP 绑定、HID 枚举留出时间。
const STARTUP_DELAY_MS: u64 = 3000;
/// 在线且无异常时的环境探测周期（检测用户中途换网 / IP 变化）。
const ENV_TTL_ONLINE_MS: u64 = 30_000;
/// 离线 / 等待确认时的探测周期（要快一点）。
const ENV_TTL_BUSY_MS: u64 = 5_000;
/// 心跳丢失多久后才由上位机重下发（先给板子自己重试的机会）。
const OFFLINE_RESEND_MS: u64 = 20_000;
/// 离线短于此时长又恢复 ⇒ 判为**同一次会话的抖动**，不算一次新的"连接成功"。
///
/// # 为什么需要它（2026-09-24 实测）
///
/// 板子心跳是 UDP 单播，丢包时 `is_online()` 会短暂转假。若不设门槛，每一轮
/// "掉一下又回来"都会被记成一次成功 ⇒ `success_count` 虚高、`last_success_unix`
/// 被无意义刷新，而历史表的**排序依据正是这两项**（选网时"证据最强的优先"），
/// 于是抖动最频繁的那张网会被越推越高 —— 这是纯粹的噪声污染。
///
/// 30 秒 = 3 倍板子侧重连周期（约 10 秒），足以把"偶发抖动"与"真掉线后重连"分开；
/// 同时它比 `OFFLINE_RESEND_MS`(20 s) 大，保证"上位机主动重下发后恢复"那一类
/// 一定会被正常计数。
const RESUMED_FLAP_MS: u64 = 30_000;
/// 下发后等心跳确认的最长时间。
const CONFIRM_TIMEOUT_MS: u64 = 25_000;
/// 本机 IPv4 候选缓存时长。
const LAN_IP_TTL_MS: u64 = 30_000;

/// 重试退避（秒）：前几次给得密，之后压到 1 分钟，避免在路由器坏掉时刷 HID。
const BACKOFF_SECS: &[u64] = &[0, 5, 15, 30, 60];

// ---------------------------------------------------------------- 小工具

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 单调时基的零点。
static MONO_EPOCH: OnceLock<Instant> = OnceLock::new();

/// 单调毫秒时间戳：**只用于算时间间隔**（`offline_since_ms` / 探测周期 / 退避）。
///
/// 与 `keyboard_audio::now_ms` 同款理由：这些量都是"过了多久"，用墙钟会让
/// NTP 校时、改系统时间变成假离线（往前跳）或**漏报离线**（往后跳，`saturating_sub`
/// 归零）。
///
/// 需要**真实时刻**的地方（历史落盘的 `last_success_unix`）用 [`now_unix`]（墙钟），
/// 两者别混 —— 单调时钟的零点没有现实含义，写进文件就再也解释不通了。
///
/// 返回值**恒 ≥ 1**：本文件里 `0` 被当成"未开始 / 从未发生"的哨兵值
/// （`last_probe_ms` / `offline_since_ms` / `next_attempt_ms` 都这么用），
/// 业务值不能是 0，否则启动后第一毫秒的采样会被误读成"还没开始"。
fn now_ms() -> u64 {
    (MONO_EPOCH
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64)
        .saturating_add(1)
}

/// 这次"离线→在线"够不够格算一次连接成功（而不是仅仅抖了一下）。
///
/// - `offline_ms_before`：本轮恢复在线**之前**已离线的时长。
/// - `ever_online_before`：本轮之前**是否曾经在线过**。`false` = 本进程启动以来
///   的首次连接 ⇒ **必须**计成功，否则"第一次连上"永远不进历史，
///   `success_count` 停在 0，而历史表正是选网的依据。
///
/// # 语义边界：`success_count` 会随"应用重启"增长，这是有意为之
///
/// 每次进程启动后的第一次连上都计一笔，所以频繁重启会让计数上涨。
/// 这**不是**噪声（与抖动污染不同）—— "启动后确实连上了"是一次真实连接事件，
/// 它也确实反映了"这张网可用"。不要把它重新解释成"历史累计的网络事件数"，
/// 也不要用它去推断"网络质量"：它既含使用频次也含启动次数。
/// 排序的第一依据是 `last_success_unix`（最近成功），那才是"最近可用"的信号。
///
/// # 为什么"首次连接"必须单独传一个标志，不能靠 `offline_ms_before == 0`
///
/// 真机抓到过：应用启动时板子还没开始发包 ⇒ **第一个 tick 就把 `offline_since_ms`
/// 设上了**（那段起表刻意如此，否则会漏掉"开机时板子本来就没连上"）。
/// 等首包到达时它已经是"启动到首包的时长"，不是 0 ⇒ 首次连接被误判成抖动：
///
/// ```text
/// 16:27:25 keyboard_audio: 收到首次心跳 addr=10.x.y.z:61598
/// 16:27:26 Wi-Fi 链路：心跳恢复（离线 1.0 秒，判为短暂抖动，不计入成功次数）
/// ```
///
/// 判据"表示从未在线过"与"表示刚离线了 0 毫秒"是两件事，不能用同一个数值兼任。
///
/// # 为什么单独抽成纯函数
///
/// 判据本来长在 tick 循环体里，那就**没法测**；而它的失效方式是静默的 ——
/// 统计被污染后界面上看不出任何异常，只是选网排序慢慢变歪。
/// 抽出来之后：① 单测能直接钉住边界；② **日志与历史共用同一个调用**，
/// 不可能出现"日志说抖动、历史却记了一笔成功"这种两份证据互相打架的现场。
fn counts_as_success(offline_ms_before: u64, ever_online_before: bool) -> bool {
    !ever_online_before || offline_ms_before >= RESUMED_FLAP_MS
}

/// 日志里给 SSID 打码。
///
/// 固件仓库的铁律是"运行日志不得输出 SSID / 密码 / 网络主机"；本仓库是用户自己
/// 的机器，但同一份日志常被贴出来排查问题，所以新代码一律打码。旧的两处
/// （`commands::wifi_connect` / `lib.rs` 自动连接）也一并换成本函数。
pub fn mask_ssid(ssid: &str) -> String {
    let chars: Vec<char> = ssid.chars().collect();
    if chars.len() <= 2 {
        return "*".repeat(chars.len().max(1));
    }
    if chars.len() <= 6 {
        return format!("{}{}", chars[0], "*".repeat(chars.len() - 1));
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}**{tail}")
}

/// 第 `attempt` 次（0 起）重试前应等待的秒数。
pub fn backoff_secs(attempt: u32) -> u64 {
    let idx = (attempt as usize).min(BACKOFF_SECS.len() - 1);
    BACKOFF_SECS[idx]
}

// ---------------------------------------------------------------- 历史网络

/// 一条"连成功过"的网络记录。
///
/// `last_success_unix == 0` 表示"记下来了但还没成功过"（例如用户手动填了但
/// 下发被拒）。这种条目仍参与选网，但排序时排在所有成功过的条目之后。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnownWifi {
    pub ssid: String,
    pub password: String,
    pub last_success_unix: u64,
    pub success_count: u32,
    pub last_attempt_unix: u64,
    pub last_error: String,
}

impl KnownWifi {
    pub fn has_password(&self) -> bool {
        !self.password.is_empty()
    }
    pub fn evidenced(&self) -> bool {
        self.last_success_unix > 0
    }
}

#[derive(Clone, Debug)]
pub struct KnownWifiFile {
    pub version: u32,
    pub entries: Vec<KnownWifi>,
}

impl Default for KnownWifiFile {
    fn default() -> Self {
        Self { version: 1, entries: Vec::new() }
    }
}

pub fn known_path(config_dir: &Path) -> PathBuf {
    config_dir.join(KNOWN_FILE)
}

/// 读历史网络；文件不存在 / 坏掉都返回空表（**不报错**：历史丢了只是慢一点，
/// 不值得让它挡住启动）。
pub fn load_known(config_dir: &Path) -> KnownWifiFile {
    let path = known_path(config_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return KnownWifiFile::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        log_buffer::warn(format!("wifi_known.json 解析失败，按空历史处理: {}", path.display()));
        return KnownWifiFile::default();
    };
    let mut entries = Vec::new();
    if let Some(list) = value.get("entries").and_then(|v| v.as_array()) {
        for item in list {
            let ssid = item.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
            if ssid.is_empty() {
                continue;
            }
            entries.push(KnownWifi {
                ssid: ssid.to_string(),
                password: item.get("password").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                last_success_unix: item.get("last_success_unix").and_then(|v| v.as_u64()).unwrap_or(0),
                success_count: item.get("success_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                last_attempt_unix: item.get("last_attempt_unix").and_then(|v| v.as_u64()).unwrap_or(0),
                last_error: item.get("last_error").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    let mut file = KnownWifiFile {
        version: value.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        entries,
    };
    // 文件被手工改大过也要收回到上限（不然历史会无限增长）
    trim_known(&mut file, None);
    sort_entries(file)
}

pub fn save_known(config_dir: &Path, file: &KnownWifiFile) -> Result<(), String> {
    let sorted = sort_entries(file.clone());
    let payload = serde_json::json!({
        // 用回读到的版本号（0 视为 1）：字段读了才不是死字段，将来升 v2 时
        // 这里也不用再改。
        "version": if sorted.version == 0 { 1 } else { sorted.version },
        "entries": sorted.entries.iter().map(|e| serde_json::json!({
            "ssid": e.ssid,
            "password": e.password,
            "last_success_unix": e.last_success_unix,
            "success_count": e.success_count,
            "last_attempt_unix": e.last_attempt_unix,
            "last_error": e.last_error,
        })).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
    let path = known_path(config_dir);
    std::fs::write(&path, text).map_err(|e| format!("写 {} 失败: {e}", path.display()))
}

/// 排序 = **证据强的排前面**：
///
/// 1. `last_success_unix` 降序（真的连上过 > 只是试过）；
/// 2. `last_attempt_unix` 降序（试得越近越靠前）；
/// 3. `ssid` 升序兜底 —— 并列是常态（同一次开机里连着写下几条），不给兜底的话
///    顺序会随文件系统返回顺序漂移，"可复现"也就没了。
///
/// 同一顺序既用于展示、也用于裁剪：**裁剪丢掉的就是"最不可能再用"的那一头**。
fn sort_entries(mut file: KnownWifiFile) -> KnownWifiFile {
    file.entries.sort_by(|a, b| {
        b.last_success_unix
            .cmp(&a.last_success_unix)
            .then(b.last_attempt_unix.cmp(&a.last_attempt_unix))
            .then(a.ssid.cmp(&b.ssid))
    });
    file
}

/// 记下/更新一条网络。`password` 为 `None` 表示"别动已有密码"。
pub fn upsert_known(file: &mut KnownWifiFile, ssid: &str, password: Option<&str>) -> bool {
    let ssid = ssid.trim();
    if ssid.is_empty() {
        return false;
    }
    if let Some(entry) = file.entries.iter_mut().find(|e| e.ssid == ssid) {
        if let Some(pw) = password {
            entry.password = pw.to_string();
        }
        return false;
    }
    file.entries.push(KnownWifi {
        ssid: ssid.to_string(),
        password: password.unwrap_or("").to_string(),
        ..Default::default()
    });
    // ⛔ 裁剪必须**豁免刚插进来的这条**。
    //
    // 新条目此刻的证据是零（`last_success_unix == 0`），按"证据排序"必然排在最后
    // ⇒ 表满时会被立刻裁掉。后果是：**用户刚填了一个新网络，自动连接却永远看不到它**
    // —— 一个只在"历史已有 16 条以上"时才出现的怪毛病，极难复现。
    // （这条是写测试时抓出来的：`trim_keeps_most_recently_successful` 先失败。）
    trim_known(file, Some(ssid));
    true
}

/// 超出上限时丢掉"最不可能再用"的那条（见 [`sort_entries`] 的顺序）。
///
/// `keep` 指定的 SSID **一定不会被丢**（这是刚插入的那条）。
fn trim_known(file: &mut KnownWifiFile, keep: Option<&str>) {
    if file.entries.len() <= KNOWN_MAX {
        return;
    }
    *file = sort_entries(file.clone());
    let mut dropped = 0usize;
    // 从尾部（最弱证据）往前丢，跳过豁免项。
    let mut index = file.entries.len();
    while file.entries.len() > KNOWN_MAX && index > 0 {
        index -= 1;
        if keep.map(|k| file.entries[index].ssid == k).unwrap_or(false) {
            continue;
        }
        file.entries.remove(index);
        dropped += 1;
        if dropped > KNOWN_MAX * 2 {
            break; // 防御：正常情况下不可能走到这里
        }
    }
}

/// **只有收到心跳才调这个** —— 成功是以数据面为准的，不是以下发回执为准。
pub fn mark_success(file: &mut KnownWifiFile, ssid: &str, unix: u64) {
    if let Some(entry) = file.entries.iter_mut().find(|e| e.ssid == ssid) {
        entry.last_success_unix = unix;
        entry.success_count = entry.success_count.saturating_add(1);
        entry.last_attempt_unix = unix;
        entry.last_error.clear();
    }
}

/// 记一次失败（下发被拒 / 通道不通）。**保留 `last_success_unix`** ——
/// 一次失败不代表这条网络作废，它下次仍然该被优先尝试。
pub fn mark_failure(file: &mut KnownWifiFile, ssid: &str, error: &str, unix: u64) {
    if let Some(entry) = file.entries.iter_mut().find(|e| e.ssid == ssid) {
        entry.last_attempt_unix = unix;
        entry.last_error = error.to_string();
    }
}

// ---------------------------------------------------------------- 环境探测

/// 上位机所处的网络环境。`epoch()` 是"环境指纹"，变了就说明该重下发。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WifiEnv {
    /// 上位机当前所连 SSID（未连 Wi-Fi / 无无线网卡时为 `None`）。
    pub current_ssid: Option<String>,
    /// 可见 AP（只在需要时探测；读它会触发一次无线扫描）。
    pub visible: Vec<String>,
    /// 本机 IPv4 候选，**首选在前**（首选 = 默认路由出口）。
    pub lan_ipv4: Vec<String>,
}

impl WifiEnv {
    pub fn primary_ipv4(&self) -> Option<&str> {
        self.lan_ipv4.first().map(|s| s.as_str())
    }

    /// 环境指纹：换网、或本机 IPv4 变了，都要重下发。
    ///
    /// ⚠️ **必须带上 IPv4**：只比 SSID 会漏掉"同一个 SSID、IP 变了"这种情况，
    /// 而那恰恰是板子连上了却永远收不到音频的原因（它往旧 IP 发）。
    pub fn epoch(&self) -> String {
        format!(
            "{}|{}",
            self.current_ssid.as_deref().unwrap_or("-"),
            self.primary_ipv4().unwrap_or("-")
        )
    }
}

/// 跑一次 `netsh`，返回 stdout（lossy 解码）。
///
/// netsh 在中文 Windows 上输出 **GBK**，SSID 一般是 ASCII，`from_utf8_lossy`
/// 足够；非 ASCII 的 SSID 匹配不到时由"最近成功"兜底，不会卡死。
fn run_netsh(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("netsh");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 从 `netsh wlan show interfaces` 的输出里取当前 SSID。
///
/// 判据只认 `SSID` 这个**ASCII 标签**：Windows 在中文系统上仍然保留
/// `SSID` / `BSSID` 不翻译（实测），所以这条解析是**跨语言**的。
/// 用 `^\s*SSID\s*:` 锚定可以天然排除 `BSSID`（前缀多一个 `B`）。
/// 同一段输出里通常还有"配置文件 : <ssid>"，先出现的 `SSID :` 才是真身。
pub fn parse_current_ssid(netsh_output: &str) -> Option<String> {
    for line in netsh_output.lines() {
        let trimmed = line.trim_start();
        let rest = match trimmed.strip_prefix("SSID") {
            Some(rest) => rest,
            None => continue,
        };
        let Some(value) = rest.strip_prefix(char::is_whitespace).map(|s| s.trim_start()) else {
            continue;
        };
        let Some(value) = value.strip_prefix(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

/// 从 `netsh wlan show networks` 的输出里取可见 SSID（形如 `SSID 1 : name`）。
pub fn parse_visible_ssids(netsh_output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in netsh_output.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("SSID") else {
            continue;
        };
        // 必须是 "SSID <数字> :" —— 排除 "SSID :" 与 "BSSID"
        let Some(rest) = rest.strip_prefix(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let after = &rest[digits.len()..];
        let Some(after) = after.trim_start().strip_prefix(':') else {
            continue;
        };
        let name = after.trim();
        if name.is_empty() || out.iter().any(|s| s == name) {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

/// 从任意文本里扫出 IPv4 字面量（只认数字与点 ⇒ **不受系统语言影响**）。
pub fn scan_ipv4_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        // 起点必须不是数字/点（避免从 "1.2.3.4" 的中间切出一段）
        if !bytes[i].is_ascii_digit() || (i > 0 && (bytes[i - 1].is_ascii_digit() || bytes[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let start = i;
        let mut dots = 0usize;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            if bytes[i] == b'.' {
                dots += 1;
            }
            i += 1;
        }
        if dots != 3 {
            continue;
        }
        let token = &text[start..i];
        // 结尾不能紧跟数字/点（已经由循环保证），逐段校验 0..=255 且无前导零过长
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 4 {
            continue;
        }
        let ok = parts.iter().all(|p| {
            !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit())
                && p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)
        });
        if !ok {
            continue;
        }
        if is_candidate_lan_ipv4(token) && !out.iter().any(|s| s == token) {
            out.push(token.to_string());
        }
    }
    out
}

/// 过滤掉不可能用来接收板子音频的地址。
pub fn is_candidate_lan_ipv4(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let octets: Vec<u16> = parts.iter().filter_map(|p| p.parse::<u16>().ok()).collect();
    if octets.len() != 4 {
        return false;
    }
    let [a, b, ..] = [octets[0], octets[1], octets[2], octets[3]];
    if a == 127 || a == 0 || a >= 224 {
        return false; // 环回 / 未指定 / 组播与保留
    }
    if a == 169 && b == 254 {
        return false; // APIPA（没拿到 DHCP，连不上板子）
    }
    if octets == [255, 255, 255, 255] {
        return false;
    }
    true
}

/// 本机 IPv4 候选：**默认路由出口优先**，其余从 `netsh interface ipv4 show addresses`
/// 里扫出来。
///
/// 为什么不用 `GetAdaptersAddresses`：那要给 `windows` crate 再加一个特性、
/// 多编一遍依赖树；而这里的判据只要"哪些地址可能是本机地址"，扫数字就够，
/// 且不依赖任何系统语言。
pub fn local_ipv4_candidates() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // UDP connect 不发任何包，只让内核选出"去公网会走哪个源地址"。
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("192.0.2.1:9").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                let ip = addr.ip().to_string();
                if is_candidate_lan_ipv4(&ip) {
                    out.push(ip);
                }
            }
        }
    }
    if let Some(text) = run_netsh(&["interface", "ipv4", "show", "addresses"]) {
        for ip in scan_ipv4_tokens(&text) {
            if !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

/// 带缓存的本机 IPv4 候选（30 s 内不重复探测）。
pub fn cached_lan_ipv4() -> Vec<String> {
    static CACHE: Mutex<Option<(Vec<String>, u64)>> = Mutex::new(None);
    let now = now_ms();
    if let Ok(guard) = CACHE.lock() {
        if let Some((ips, at)) = guard.as_ref() {
            if now.saturating_sub(*at) < LAN_IP_TTL_MS {
                return ips.clone();
            }
        }
    }
    let ips = local_ipv4_candidates();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((ips.clone(), now));
    }
    ips
}

/// 探测环境。`need_visibility` 会额外跑一次 `show networks`（触发无线扫描，慢）。
pub fn probe_environment(need_visibility: bool) -> WifiEnv {
    let mut env = WifiEnv::default();
    if let Some(text) = run_netsh(&["wlan", "show", "interfaces"]) {
        env.current_ssid = parse_current_ssid(&text);
    }
    if need_visibility {
        if let Some(text) = run_netsh(&["wlan", "show", "networks"]) {
            env.visible = parse_visible_ssids(&text);
        }
    }
    env.lan_ipv4 = cached_lan_ipv4();
    env
}

// ---------------------------------------------------------------- 选网

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub ssid: String,
    pub password: String,
    pub reason: &'static str,
}

/// 从"环境 ∩ 历史"里挑一个网络。优先级是**按证据强度**排的，不是按信号：
///
/// 1. 上位机**当前所连**的 SSID，且历史里有 → 证据最强（同网是硬要求）。
/// 2. 可见 AP 里在历史中的 → 说明历史网络在场，只是上位机连的是别的网。
/// 3. 历史里最近成功过的一条 → 环境读不到（无无线网卡 / 服务未启动）时的兜底。
///
/// 历史为空 ⇒ `None`：**没有历史就没有自动连接**，这是刻意的 —— 自动去连一个
/// 用户从没成功连过的网络只会把板子带到错误的网里，比不连更难排查。
pub fn pick_candidate(env: &WifiEnv, known: &KnownWifiFile) -> Option<Candidate> {
    let sorted = sort_entries(known.clone());

    if let Some(current) = env.current_ssid.as_deref() {
        if let Some(entry) = sorted.entries.iter().find(|e| e.ssid == current) {
            return Some(Candidate {
                ssid: entry.ssid.clone(),
                password: entry.password.clone(),
                reason: "上位机当前所连网络（与板子必须同网）",
            });
        }
    }

    if !env.visible.is_empty() {
        for entry in sorted.entries.iter() {
            if env.visible.iter().any(|s| s == &entry.ssid) {
                return Some(Candidate {
                    ssid: entry.ssid.clone(),
                    password: entry.password.clone(),
                    reason: "可见接入点中的历史成功网络",
                });
            }
        }
    }

    sorted
        .entries
        .iter()
        .find(|e| e.evidenced() || !e.password.is_empty())
        .map(|entry| Candidate {
            ssid: entry.ssid.clone(),
            password: entry.password.clone(),
            reason: "历史最近成功网络（当前读不到可见性）",
        })
}

// ---------------------------------------------------------------- 状态

#[derive(Clone, Debug, Serialize)]
pub struct KnownWifiView {
    pub ssid: String,
    pub last_success_unix: u64,
    pub success_count: u32,
    pub has_password: bool,
    pub last_error: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WifiLinkStatus {
    /// 机器可读状态：`disabled` / `no-history` / `idle` / `provisioning` /
    /// `waiting-heartbeat` / `online` / `blocked`。
    pub state: String,
    /// 给用户看的一句话。
    pub state_text: String,
    /// 是否收到板子心跳（**唯一可信的"连上了"判据**）。
    pub board_online: bool,
    /// HID 是否枚举到（蓝牙 HID 也算枚举到，但不一定有配置通道）。
    pub board_hid_present: bool,
    pub current_ssid: Option<String>,
    pub lan_ipv4: Option<String>,
    /// 本轮自动连接选中的网络。
    pub target_ssid: Option<String>,
    pub target_reason: Option<String>,
    /// 板子是否已把这组端点写进 NVS（= 下次上电能自己连）。
    pub saved_to_board: bool,
    pub attempts: u32,
    pub next_retry_secs: u64,
    pub last_error: Option<String>,
    /// 最近一次下发用的通道：`usb-hid` / `wifi-udp` / 两者。
    pub last_channel: Option<String>,
    pub known: Vec<KnownWifiView>,
}

impl Default for WifiLinkStatus {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            state_text: "未开始".into(),
            board_online: false,
            board_hid_present: false,
            current_ssid: None,
            lan_ipv4: None,
            target_ssid: None,
            target_reason: None,
            saved_to_board: false,
            attempts: 0,
            next_retry_secs: 0,
            last_error: None,
            last_channel: None,
            known: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- 运行态

struct Inner {
    status: Mutex<WifiLinkStatus>,
    config_dir: Mutex<Option<PathBuf>>,
    /// 最近一次**下发**时用的环境指纹（T3 的判据）。
    ///
    /// 用"下发时"而不是"成功时"作为基准：下发是幂等的，环境与基准不一致时
    /// 多发一次只是白做一遍同样的写入，而漏发一次会让板子永久待在旧网里。
    last_pushed_epoch: Mutex<Option<String>>,
    /// 手动请求（T5）：命令已同步下发完，这里只留"等确认"的目标。
    manual_target: Mutex<Option<String>>,
    stop: AtomicBool,
    start_ms: AtomicU64,
    attempt: AtomicU32,
    next_attempt_ms: AtomicU64,
    /// 是否有外部（`wifi_connect` 命令）已经下发过，后台别抢着再发一遍。
    external_attempt_ms: AtomicU64,
}

#[derive(Clone)]
pub struct WifiLink {
    inner: Arc<Inner>,
}

impl Default for WifiLink {
    fn default() -> Self {
        Self::new()
    }
}

impl WifiLink {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                status: Mutex::new(WifiLinkStatus::default()),
                config_dir: Mutex::new(None),
                last_pushed_epoch: Mutex::new(None),
                manual_target: Mutex::new(None),
                stop: AtomicBool::new(false),
                start_ms: AtomicU64::new(now_ms()),
                attempt: AtomicU32::new(0),
                next_attempt_ms: AtomicU64::new(0),
                external_attempt_ms: AtomicU64::new(0),
            }),
        }
    }

    /// 绑定配置目录（历史网络文件的落点）。
    pub fn bind_config_dir(&self, dir: PathBuf) {
        if let Ok(mut guard) = self.inner.config_dir.lock() {
            *guard = Some(dir);
        }
    }

    pub fn config_dir(&self) -> Option<PathBuf> {
        self.inner.config_dir.lock().ok().and_then(|g| g.clone())
    }

    /// 同 [`Self::config_dir`]，但对调用方语义更明确：这是要往历史文件里写。
    pub fn config_dir_for_write(&self) -> Option<PathBuf> {
        self.config_dir()
    }

    /// 读历史网络。
    pub fn known(&self) -> KnownWifiFile {
        match self.config_dir() {
            Some(dir) => load_known(&dir),
            None => KnownWifiFile::default(),
        }
    }

    pub fn status(&self) -> WifiLinkStatus {
        let mut snapshot = self.inner.status.lock().map(|g| g.clone()).unwrap_or_default();
        snapshot.known = self
            .known()
            .entries
            .iter()
            .map(|e| KnownWifiView {
                ssid: e.ssid.clone(),
                last_success_unix: e.last_success_unix,
                success_count: e.success_count,
                has_password: e.has_password(),
                last_error: e.last_error.clone(),
            })
            .collect();
        snapshot
    }

    fn update_status<F: FnOnce(&mut WifiLinkStatus)>(&self, f: F) {
        if let Ok(mut guard) = self.inner.status.lock() {
            f(&mut guard);
        }
    }

    /// 记一次"下发已发出"（自动与手动共用）。
    pub fn note_attempt(
        &self,
        ssid: &str,
        reason: &str,
        channel: &str,
        error: Option<&str>,
        saved: bool,
    ) {
        self.update_status(|st| {
            st.target_ssid = Some(ssid.to_string());
            st.target_reason = Some(reason.to_string());
            st.last_channel = Some(channel.to_string());
            st.saved_to_board = saved;
            st.last_error = error.map(|e| e.to_string());
        });
    }

    /// 手动请求登记：命令同步下发后调用，后台据此等心跳确认。
    pub fn note_manual(&self, ssid: &str) {
        if let Ok(mut guard) = self.inner.manual_target.lock() {
            *guard = Some(ssid.to_string());
        }
        self.inner.external_attempt_ms.store(now_ms(), Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }
}

/// 老配置里的凭据（`config.json` 的 `wifi_ssid` / `wifi_password`）**迁移进历史**。
///
/// 为什么必须迁移：老用户（例如钦友当前这台机器）已经在 `config.json` 里存了
/// `wifi_ssid`，但 `wifi_known.json` 是本次新引入的。不补的话第一次启动会显示
/// "还没有任何成功连接过的网络"，而他明明有一个一直在用的网络 —— 于是"自动连接"
/// 这个功能在他身上第一次就表现为"不工作"。
///
/// 返回是否真的写盘（没迁移就没必要落盘）。
pub fn migrate_legacy_credentials(config_dir: &Path, ssid: &str, password: &str) -> bool {
    if ssid.trim().is_empty() {
        return false;
    }
    let mut file = load_known(config_dir);
    let inserted = upsert_known(&mut file, ssid, Some(password));
    // 密码变了也要落盘
    let changed = inserted
        || file
            .entries
            .iter()
            .find(|e| e.ssid == ssid.trim())
            .map(|e| e.password != password)
            .unwrap_or(false);
    if !changed {
        return false;
    }
    if let Err(e) = save_known(config_dir, &file) {
        log_buffer::warn(format!("Wi-Fi 历史迁移失败：{e}"));
        return false;
    }
    log_buffer::info(format!(
        "Wi-Fi 链路：已把 config.json 里的网络迁移进历史（SSID={}）",
        mask_ssid(ssid)
    ));
    true
}

// ---------------------------------------------------------------- 下发

pub struct ProvisionOutcome {
    /// USB HID 通道结果：`Ok(saved)` / `Err(原因)`。
    pub usb: Option<Result<bool, String>>,
    /// Wi-Fi UDP 通道结果。
    pub udp: Option<Result<(), String>>,
}

impl ProvisionOutcome {
    pub fn any_ok(&self) -> bool {
        matches!(self.usb, Some(Ok(_))) || matches!(self.udp, Some(Ok(_)))
    }
    pub fn saved(&self) -> bool {
        matches!(self.usb, Some(Ok(true))) || matches!(self.udp, Some(Ok(())))
    }
    pub fn channel_label(&self) -> String {
        match (self.usb.is_some(), self.udp.is_some()) {
            (true, true) => "usb-hid + wifi-udp".to_string(),
            (true, false) => "usb-hid".to_string(),
            (false, true) => "wifi-udp".to_string(),
            (false, false) => "-".to_string(),
        }
    }
    /// 给用户/日志看的一句话失败原因（两条通道都失败时取更"有信息量"的那条）。
    pub fn error_text(&self) -> Option<String> {
        let usb = self.usb.as_ref().and_then(|r| r.as_ref().err());
        let udp = self.udp.as_ref().and_then(|r| r.as_ref().err());
        match (usb, udp) {
            (Some(u), Some(_)) => Some(classify_error(u)),
            (Some(u), None) => Some(classify_error(u)),
            (None, Some(d)) => Some(format!("Wi-Fi UDP 通道：{d}")),
            (None, None) => None,
        }
    }
}

/// 把 HID 层那句"技术味"的错误翻成用户能动手的话。
///
/// ⛔ 别把这段合进别处：**"未找到开发板（列表为空）"与"列表非空但全失败"
/// 是两种病**（前者 USB 没插，后者插着 USB 但不是配置通道 / 驱动没起来），
/// 处置方向相反 —— 与 `keyboard_audio::status_note()` 同一条理由。
pub fn classify_error(raw: &str) -> String {
    if raw.contains("未找到开发板") {
        return "板子没有以 USB 连接（首次配网必须插 USB：配置通道是 HID Feature Report，蓝牙 HID 没有它）".to_string();
    }
    if raw.contains("均无法下发") {
        return "找到 HID 设备但没有配置通道（多半只有蓝牙 HID 在线；请插 USB 后重试）".to_string();
    }
    if raw.contains("未收到 0x11 回执") {
        return "配置已发出但板子没回执（板子可能不在配置通道上，或正忙）".to_string();
    }
    raw.to_string()
}

// ---------------------------------------------------------------- 后台线程

/// 启动后台看护线程（`lib.rs` setup 里调用一次）。
pub fn start(app: tauri::AppHandle, link: WifiLink) {
    let inner = link.inner.clone();
    std::thread::spawn(move || watchdog_loop(app, inner));
}

/// 看护循环。
///
/// **每秒跑一次，但绝大多数轮次什么也不做** —— 只有文件头表 T 里的触发条件成立
/// 时才会去碰 HID / UDP。这里刻意不用事件驱动：触发源是三个互不相干的时钟
/// （板子心跳、用户换网、USB 插拔），把它们统一成"每秒重新评估一遍"比维护三条
/// 回调链更不容易漏 —— 而这个模块最怕的就是漏（漏一次 = 板子麦克风静默失效）。
fn watchdog_loop(app: tauri::AppHandle, inner: Arc<Inner>) {
    use tauri::Manager;

    let mut env = WifiEnv::default();
    let mut last_probe_ms = 0u64;
    let mut was_hid_present = false;
    let mut was_online = false;
    // ⛔ 这张"离线起表时间"必须是**本轮离线开始**的时刻，而且**第一次评估到
    // 非在线状态时就要起表**。若只在前一次在线、这一次不在线时才起表，就会漏掉
    // "开机时板子本来就没连上"这种最常见的情况 —— 表永远是 0，
    // T4（心跳丢失重下发）永远不触发，自动重连只发生一次、之后再也不发生。
    let mut offline_since_ms = 0u64;
    // 自本次进程启动以来**是否曾经在线过**。
    //
    // ⛔ 不能用"`offline_since_ms == 0`"来代表"首次连接"：上面那张起表刻意在
    // **第一次评估到非在线状态时**就设上了，而应用启动时板子通常还没开始发包
    // ⇒ 首包到达时 `offline_since_ms` 早就不为 0 了。
    // 实测（2026-09-24 真机）：应用启动后**第一次连上**被判成
    // `心跳恢复（离线 1.0 秒，判为短暂抖动，不计入成功次数）`——
    // 于是"启动以来第一次连上"这件事从来没被记进历史。
    // （注：函数体内的注释用 `//`，`///` 只对 item 有效，用在这里会警告。）
    let mut ever_online = false;

    while !inner.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(TICK_MS));

        let now = now_ms();
        let link = WifiLink { inner: inner.clone() };

        let (mic_is_wifi, board_online, hid_present) = {
            let session = app.state::<crate::session::SessionManager>();
            (
                matches!(session.mic_source(), crate::session::MicSource::Wifi),
                session.keyboard_audio_online(),
                crate::device::device_status().is_ok(),
            )
        };

        // ── 门槛 0：麦克风来源不是 Wi-Fi ⇒ 整条链路停摆。
        // 用户明确选了别的来源时，后台不该还在改板子的网络配置。
        if !mic_is_wifi {
            link.update_status(|st| {
                st.state = "disabled".into();
                st.state_text = "麦克风来源不是「Wi-Fi 键盘麦克风」，本链路不介入".into();
                st.board_online = false;
                st.next_retry_secs = 0;
            });
            was_online = false;
            offline_since_ms = 0;
            continue;
        }

        // ── 在线状态迁移。**心跳是唯一可信的"连上了"判据。**
        //
        // 先取"这一轮恢复之前已经离线了多久"——**必须在下面被清零之前取**：
        // `offline_since_ms` 一旦在线就被重置，之后再也算不出这次离线有多长，
        // 而"这次是真重连还是仅仅抖了一下"正是靠它来分的。
        // `0` 表示本轮之前从未离线（首次连接），按真重连处理。
        let offline_ms_before = offline_since_ms;

        if board_online {
            offline_since_ms = 0;
            link.inner.attempt.store(0, Ordering::Relaxed);
            link.inner.next_attempt_ms.store(0, Ordering::Relaxed);

            if !was_online {
                // 抖动与真重连要**在日志里就能分开**：否则一年后再看这段日志，
                // 只看到一串"在线/离线"来回，根本判断不出网络到底稳不稳。
                // 与历史计数**共用同一个判据函数**，不许各写一份 ——
                // "日志说抖动、历史却记了一笔成功"这种互相打架的现场最难排查。
                if counts_as_success(offline_ms_before, ever_online) {
                    log_buffer::info("Wi-Fi 链路：收到板子心跳，链路在线");
                } else {
                    log_buffer::info(format!(
                        "Wi-Fi 链路：心跳恢复（离线 {:.1} 秒，判为短暂抖动，不计入成功次数）",
                        offline_ms_before as f64 / 1000.0
                    ));
                }
            }
        } else {
            if was_online {
                log_buffer::warn(format!(
                    "Wi-Fi 链路：板子心跳中断（超过 {} 秒未收到）",
                    crate::keyboard_audio::heartbeat_timeout_secs()
                ));
            }
            if offline_since_ms == 0 {
                offline_since_ms = now;
            }
        }
        let online_transition = board_online && !was_online;
        was_online = board_online;

        // ⚠️ "在线确认 + 写历史"**不在这里**，它被挪到了下面**选网之后** ——
        // 因为它需要一个网络名做兜底，那段注释里有真机证据。

        // ── T2：HID 出现（插上 USB / 蓝牙连上）
        let hid_appeared = hid_present && !was_hid_present;
        was_hid_present = hid_present;

        // ── 环境探测。频率按"现在有多需要"分两档；`show networks`（会触发一次
        // 无线扫描、有代价）只在"当前所连网络不在历史里"时才读。
        let probe_ttl = if board_online { ENV_TTL_ONLINE_MS } else { ENV_TTL_BUSY_MS };
        let env_stale = last_probe_ms == 0 || now.saturating_sub(last_probe_ms) >= probe_ttl;
        if env_stale || hid_appeared {
            let known_now = link.known();
            let current = env.current_ssid.clone().unwrap_or_default();
            let need_visibility = known_now.entries.iter().all(|e| e.ssid != current);
            env = probe_environment(need_visibility);
            last_probe_ms = now;
        }

        // ── T3：环境变化（换网 / 本机 IPv4 变了）
        let epoch = env.epoch();
        let env_changed = link
            .inner
            .last_pushed_epoch
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .map(|prev| prev != epoch)
            .unwrap_or(false);

        let known = link.known();
        let candidate = pick_candidate(&env, &known);

        // ── 在线确认：把这一轮的目标写进历史（**只有这里算"成功过"**）
        //
        // # 为什么位置必须在**选网之后**（2026-09-24 真机抓出来的）
        //
        // 原先这段写在状态迁移那里（选网之前），而目标名要靠 `target_ssid` 兜底 ——
        // 那个字段由**下发分支**写入。于是"板子比上位机更快"时目标恒为 None：
        // 板上电常开、心跳在启动延迟（3 s）走完之前就到了，日志只有一行
        // "收到板子心跳，链路在线"，**没有"已确认连接"**，`success_count` 一动不动：
        //
        // ```text
        // 16:33:35 keyboard_audio: 收到首次心跳
        // 16:33:36 Wi-Fi 链路：收到板子心跳，链路在线
        // （没有"已确认连接" ⇒ 历史没记）
        // ```
        //
        // 结果就是**成功记录永远积累不起来**，而历史表正是自动选网的依据。
        // 挪到选网之后，就能拿本轮选网结果兜底。
        //
        // # 抖动仍然不算成功
        //
        // 短于 `RESUMED_FLAP_MS` 的"离线→在线"是**抖动**：心跳走 UDP，丢几个包
        // `is_online()` 就会短暂转假（2026-09-24 实测 9 次/小时）。若也记成成功，
        // `success_count` 与 `last_success_unix` 会被噪声刷高，而这两项正是
        // 历史表的排序依据（选网优先"证据最强"的一条）
        // ⇒ 抖动最频繁的那张网反被越推越高。
        // 状态**仍会**在下面被更新成 online（那是事实），只是不写历史、不刷统计。
        if online_transition && counts_as_success(offline_ms_before, ever_online) {
            let target = link
                .inner
                .status
                .lock()
                .ok()
                .and_then(|g| g.target_ssid.clone())
                .or_else(|| {
                    link.inner
                        .manual_target
                        .lock()
                        .ok()
                        .and_then(|g| g.clone())
                })
                // 兜底：本轮选网结果。启动即在线时上面两个来源都还是空的，
                // 而"本轮选中的那张网"就是正确答案（板子与上位机同网，是整套设计的前提）。
                .or_else(|| candidate.as_ref().map(|c| c.ssid.clone()));
            if let Some(ssid) = target {
                if let Some(dir) = link.config_dir() {
                    let mut file = load_known(&dir);
                    upsert_known(&mut file, &ssid, None);
                    mark_success(&mut file, &ssid, now_unix());
                    let _ = save_known(&dir, &file);
                }
                link.update_status(|st| {
                    st.state = "online".into();
                    st.state_text = "已连上（收到板子心跳）".into();
                    st.last_error = None;
                    st.saved_to_board = true;
                });
                log_buffer::info(format!(
                    "Wi-Fi 链路：已确认连接（SSID={}，心跳到达）",
                    mask_ssid(&ssid)
                ));
            }
            if let Ok(mut guard) = link.inner.manual_target.lock() {
                *guard = None;
            }
        }

        // 在线过就**永久**记住（本进程内不改回 false）——
        // `counts_as_success` 用它区分"启动以来第一次连上"与"重连"。
        // ⚠️ 必须放在上面判定**之后**：提前置位会让首次连接被判成重连。
        if board_online {
            ever_online = true;
        }

        let startup_done =
            now.saturating_sub(link.inner.start_ms.load(Ordering::Relaxed)) >= STARTUP_DELAY_MS;
        let offline_long_enough =
            !board_online && now.saturating_sub(offline_since_ms) >= OFFLINE_RESEND_MS;
        let never_tried = link
            .inner
            .status
            .lock()
            .map(|g| g.target_ssid.is_none())
            .unwrap_or(true);
        let due = now >= link.inner.next_attempt_ms.load(Ordering::Relaxed);
        // 手动下发之后的一段窗口内只等确认、不自动重发（免得和用户的操作打架）
        let confirm_pending =
            now.saturating_sub(link.inner.external_attempt_ms.load(Ordering::Relaxed))
                < CONFIRM_TIMEOUT_MS;

        // ── 没有历史 ⇒ 明确说"为什么不能自动连"，而不是假装在工作
        let Some(candidate) = candidate else {
            link.update_status(|st| {
                st.state = "no-history".into();
                st.state_text =
                    "还没有成功连接过的网络，无法自动连接；请手动填一次 SSID/密码并点「连接」"
                        .into();
                st.board_online = board_online;
                st.board_hid_present = hid_present;
                st.current_ssid = env.current_ssid.clone();
                st.lan_ipv4 = env.primary_ipv4().map(|s| s.to_string());
                st.target_ssid = None;
                st.target_reason = None;
                st.next_retry_secs = 0;
            });
            continue;
        };

        // ── 在线：只做 T3（换网 / 换 IP）。其余触发都与"板子不在线"绑定。
        if board_online {
            link.update_status(|st| {
                st.state = "online".into();
                st.state_text = if env_changed {
                    "已连上；但上位机网络环境已变化，准备重下发".into()
                } else {
                    "已连上（收到板子心跳）".into()
                };
                st.board_online = true;
                st.board_hid_present = hid_present;
                st.current_ssid = env.current_ssid.clone();
                st.lan_ipv4 = env.primary_ipv4().map(|s| s.to_string());
                st.target_ssid = Some(candidate.ssid.clone());
                st.target_reason = Some(candidate.reason.to_string());
                st.next_retry_secs = 0;
            });
            if env_changed {
                link.update_status(|st| {
                    st.state = "provisioning".into();
                    st.state_text = "上位机网络环境已变化，正在重下发端点".into();
                });
                if let Ok(mut guard) = link.inner.last_pushed_epoch.lock() {
                    *guard = Some(epoch);
                }
                let _ = provision_with(&app, &link, None, "环境变化（上位机换网或 IP 变化）", true);
            }
            continue;
        }

        // ── 触发判据（文件头表 T 的实现）
        let fire = startup_done
            && hid_present
            && !confirm_pending
            && (hid_appeared || never_tried || offline_long_enough);

        if !fire || !due {
            let wait = link
                .inner
                .next_attempt_ms
                .load(Ordering::Relaxed)
                .saturating_sub(now);
            link.update_status(|st| {
                st.state = if confirm_pending {
                    "waiting-heartbeat".into()
                } else {
                    "idle".into()
                };
                st.state_text = if confirm_pending {
                    "端点已下发，正在等板子心跳确认".into()
                } else if !hid_present {
                    "板子未以 USB 连接（配置通道不可用），等待插入 USB".into()
                } else {
                    "等待触发（板子正自行重连，每约 10 秒一轮）".into()
                };
                st.board_online = false;
                st.board_hid_present = hid_present;
                st.current_ssid = env.current_ssid.clone();
                st.lan_ipv4 = env.primary_ipv4().map(|s| s.to_string());
                st.target_ssid = Some(candidate.ssid.clone());
                st.target_reason = Some(candidate.reason.to_string());
                st.next_retry_secs = wait / 1000;
            });
            continue;
        }

        // ── 下发（T1 / T2 / T4）
        let attempt = link.inner.attempt.load(Ordering::Relaxed);
        let verbose = attempt == 0 || attempt % 5 == 0;
        link.update_status(|st| {
            st.state = "provisioning".into();
            st.state_text = "正在下发端点（完整配置载荷）".into();
            st.board_online = false;
            st.board_hid_present = hid_present;
            st.current_ssid = env.current_ssid.clone();
            st.lan_ipv4 = env.primary_ipv4().map(|s| s.to_string());
        });
        if let Ok(mut guard) = link.inner.last_pushed_epoch.lock() {
            *guard = Some(epoch);
        }
        if let Err(e) = provision_with(&app, &link, None, "自动", verbose) {
            log_buffer::warn(format!("Wi-Fi 链路：自动下发未成功：{e}"));
        }
        if verbose {
            let note = link.status().state_text;
            log_buffer::info(format!("Wi-Fi 链路：第 {} 次自动下发结束 → {note}", attempt + 1));
        }
        let next_attempt = attempt.saturating_add(1);
        link.inner.attempt.store(next_attempt, Ordering::Relaxed);
        link.inner.next_attempt_ms.store(
            now + backoff_secs(next_attempt) * 1000,
            Ordering::Relaxed,
        );
    }
}
// ---------------------------------------------------------------- 单次下发

/// 组装带端点的完整载荷（键位方案来自当前激活方案）。
///
/// ⛔ 必须是**完整**载荷：固件 `apply_json` 整体替换，缺 `profiles` / `keys` /
/// `encoder` 会直接判 `MissingProfile` / `MissingBinding` 拒整包；只发 Wi-Fi 字段
/// 不但不生效，还会把键位方案一起冲掉。
///
/// 返回 [`crate::payload::FirmwarePayload`] 而不是裸 JSON —— 调用方要的通常是
/// "完整的一次下发产物"（含 `text_map`），分两次构造会出现
/// "JSON 与 text_map 来自两次不同的遍历"这种对不上的隐患。
pub fn build_payload_with_endpoint(
    app: &tauri::AppHandle,
    ssid: &str,
    password: &str,
) -> Result<crate::payload::FirmwarePayload, String> {
    let profile = active_profile(app)?;
    let host = cached_lan_ipv4()
        .into_iter()
        .next()
        .ok_or_else(|| {
            "读不到本机 IPv4（板子需要一个可回连的地址；检查网卡是否已连接）".to_string()
        })?;
    Ok(crate::payload::build_firmware_payload_with_wifi(
        &profile,
        Some(crate::payload::WifiEndpoint {
            ssid,
            password,
            host: &host,
            port: crate::keyboard_audio::DEFAULT_PORT,
        }),
    ))
}

/// 取当前激活方案（没有就退回第一个）。
pub fn active_profile(app: &tauri::AppHandle) -> Result<crate::model::Profile, String> {
    use tauri::Manager;
    let st = app.state::<crate::state::AppState>();
    let profiles = st.profiles.read().unwrap();
    if profiles.is_empty() {
        return Err("方案库为空，无法构造下发载荷".to_string());
    }
    let active_id = st.active_profile_id.read().unwrap().clone();
    let profile = active_id
        .as_deref()
        .and_then(|id| profiles.iter().find(|p| p.id == id))
        .or_else(|| profiles.first())
        .ok_or_else(|| "方案库为空".to_string())?;
    Ok(profile.clone())
}

/// 下发一次端点。`ssid` 为 `None` 时用"环境 ∩ 历史"自动选出来的那个。
///
/// 两条通道都会试（谁通谁算数）：
/// - **USB HID `0x10`**：唯一能在"板子还没上过网"时起作用的通道。
/// - **Wi-Fi UDP 直投**：板子已经在线时的快通道，不需要插 USB。
pub fn provision_with(
    app: &tauri::AppHandle,
    link: &WifiLink,
    explicit_ssid: Option<(&str, &str)>,
    reason: &str,
    log: bool,
) -> Result<ProvisionOutcome, String> {
    let (ssid, password) = match explicit_ssid {
        Some((s, p)) => (s.to_string(), p.to_string()),
        None => {
            let env = probe_environment(false);
            let known = link.known();
            let candidate = pick_candidate(&env, &known)
                .ok_or_else(|| "没有可用的历史网络".to_string())?;
            (candidate.ssid, candidate.password)
        }
    };

    let json = build_payload_with_endpoint(app, &ssid, &password)?.json;

    let mut outcome = ProvisionOutcome { usb: None, udp: None };

    // 通道 A：Wi-Fi UDP 直投（只在板子已在线时才有意义）。
    //
    // 板子那条"配置 JSON 直达通道"的判据是 `from == dest`（来源必须是它已配置的
    // 上位机地址）⇒ **本机 IP 变了之后这条路就断了**，只能靠 USB 重新配。
    // 这不是缺陷而是它的安全设计，所以两条通道的适用场景要写清：
    //   · 首次配网 / IP 变了 → 必须 USB
    //   · 同网且 IP 未变，只是要刷新端点 → UDP 就够
    {
        use tauri::Manager;
        let ka = app.state::<crate::session::SessionManager>().keyboard_audio();
        if let Some(ka) = ka {
            if ka.is_online() {
                let result = ka.send_config_json(&json);
                if let Err(e) = &result {
                    if log {
                        log_buffer::warn(format!("Wi-Fi 链路：UDP 直投失败：{e}"));
                    }
                }
                outcome.udp = Some(result);
            }
        }
    }

    // 通道 B：USB HID 配置下发（等 0x11 回执，从而拿到 saved）。
    match crate::transport::send_config(&json) {
        Ok(ack) => {
            if log {
                log_buffer::info(format!(
                    "Wi-Fi 链路：USB 配置回执 ok={} saved={} bytes={}",
                    ack.ok, ack.saved, ack.bytes
                ));
            }
            outcome.usb = Some(Ok(ack.saved));
        }
        Err(e) => {
            if log {
                log_buffer::warn(format!("Wi-Fi 链路：USB 配置下发失败：{e}"));
            }
            outcome.usb = Some(Err(e));
        }
    }

    if !outcome.any_ok() {
        let text = outcome.error_text().unwrap_or_else(|| "两条通道都不可用".to_string());
        let dir = link.config_dir();
        if let Some(dir) = dir {
            let mut file = load_known(&dir);
            upsert_known(&mut file, &ssid, Some(&password));
            mark_failure(&mut file, &ssid, &text, now_unix());
            let _ = save_known(&dir, &file);
        }
        link.note_attempt(&ssid, reason, &outcome.channel_label(), Some(&text), false);
        return Ok(outcome);
    }

    // 至少一条通道成功：写历史（还不算"成功连接"，那要等心跳）。
    if let Some(dir) = link.config_dir() {
        let mut file = load_known(&dir);
        upsert_known(&mut file, &ssid, Some(&password));
        if let Some(entry) = file.entries.iter_mut().find(|e| e.ssid == ssid) {
            entry.last_attempt_unix = now_unix();
            entry.last_error.clear();
        }
        let _ = save_known(&dir, &file);
    }
    link.note_attempt(&ssid, reason, &outcome.channel_label(), None, outcome.saved());
    link.update_status(|st| {
        st.state = "waiting-heartbeat".into();
        st.state_text = "端点已下发，正在等板子心跳确认".into();
    });
    if log {
        log_buffer::info(format!(
            "Wi-Fi 链路：端点已下发（{reason}，SSID={}，通道={}，板子已存 NVS={}）",
            mask_ssid(&ssid),
            outcome.channel_label(),
            outcome.saved()
        ));
    }
    Ok(outcome)
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// `netsh wlan show interfaces` 在中文系统上的输出样本。
    ///
    /// ⛔ 这里是**脱敏后的**样本。真实抓包里带着网卡型号、GUID、本机物理地址、
    /// 以及单位/邻居的 SSID 与 BSSID —— 测试夹具会和注释一样被永久入库，
    /// 所以一律换成假值。值得留住的只是「中文系统上 `SSID` / `BSSID` 这类标签
    /// **不翻译**」这个跨语言解析依据，以及标签右侧的缩进与冒号形状。
    const IFACES_ZH: &str = "系统上有 1 个接口: \n\n    名称                   : WLAN\n    描述                   : Intel(R) Wi-Fi 6 AX201 160MHz\n    GUID                   : 00000000-1111-2222-3333-444444444444\n    物理地址               : aa:bb:cc:dd:ee:ff\n    状态                   : 已连接\n    SSID                   : MyHomeNet\n    BSSID                  : 11:22:33:44:55:66\n    网络类型               : 结构\n    配置文件               : MyHomeNet\n";

    const NETWORKS_ZH: &str = "接口名称 : WLAN\n当前有 3 个网络可见。\n\nSSID 1 : MyHomeNet\n    Network type            : 结构\n    身份验证                : WPA2 - 个人\n\nSSID 2 : Neighbour-2G\n    身份验证                : WPA2 - 个人\n\nSSID 3 : \n";

    #[test]
    fn parse_current_ssid_from_chinese_netsh() {
        assert_eq!(parse_current_ssid(IFACES_ZH).as_deref(), Some("MyHomeNet"));
        // 没有 SSID 行（未连接）⇒ None
        assert_eq!(parse_current_ssid("系统上有 1 个接口: \n    状态 : 已断开\n"), None);
        // 空值不当作 SSID
        assert_eq!(parse_current_ssid("    SSID                   : \n"), None);
        // BSSID 不能被误认成 SSID
        assert_eq!(
            parse_current_ssid("    BSSID                  : 11:22:33:44:55:66\n"),
            None
        );
    }

    #[test]
    fn parse_visible_ssids_skips_blank_and_dupes() {
        let seen = parse_visible_ssids(NETWORKS_ZH);
        assert_eq!(seen, vec!["MyHomeNet".to_string(), "Neighbour-2G".to_string()]);
        // 重复不重复收集
        let dup = "SSID 1 : a\nSSID 2 : a\n";
        assert_eq!(parse_visible_ssids(dup), vec!["a".to_string()]);
    }

    #[test]
    fn scan_ipv4_filters_loopback_and_apipa() {
        let text = "IP 地址: 192.168.31.24\n子网: 255.255.255.0\n\
                    默认网关: 192.168.31.1\nIP 地址: 127.0.0.1\n\
                    IP 地址: 169.254.10.3\nIP 地址: 10.8.0.2\n";
        let found = scan_ipv4_tokens(text);
        assert!(found.contains(&"192.168.31.24".to_string()));
        assert!(found.contains(&"10.8.0.2".to_string()));
        assert!(found.contains(&"192.168.31.1".to_string()));
        assert!(!found.contains(&"127.0.0.1".to_string()));
        assert!(!found.contains(&"169.254.10.3".to_string()));
        // 去重
        assert_eq!(scan_ipv4_tokens("1.2.3.4 1.2.3.4").len(), 1);
        // 越界 / 段数不对
        assert!(scan_ipv4_tokens("999.1.1.1").is_empty());
        assert!(scan_ipv4_tokens("1.2.3").is_empty());
        assert!(scan_ipv4_tokens("11.2.3.4.5").is_empty());
    }

    #[test]
    fn is_candidate_lan_ipv4_rules() {
        assert!(is_candidate_lan_ipv4("192.168.1.5"));
        assert!(!is_candidate_lan_ipv4("127.0.0.1"));
        assert!(!is_candidate_lan_ipv4("0.0.0.0"));
        assert!(!is_candidate_lan_ipv4("224.0.0.1"));
        assert!(!is_candidate_lan_ipv4("255.255.255.255"));
        assert!(!is_candidate_lan_ipv4("169.254.1.1"));
    }

    fn entry(ssid: &str, pw: &str, success: u64) -> KnownWifi {
        KnownWifi {
            ssid: ssid.to_string(),
            password: pw.to_string(),
            last_success_unix: success,
            success_count: if success > 0 { 1 } else { 0 },
            ..Default::default()
        }
    }

    #[test]
    fn upsert_inserts_then_updates_without_dup() {
        let mut file = KnownWifiFile::default();
        assert!(upsert_known(&mut file, "a", Some("p1")));
        assert!(!upsert_known(&mut file, "a", None));
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].password, "p1");
        // 空 SSID 被拒
        assert!(!upsert_known(&mut file, "   ", Some("p")));
        assert_eq!(file.entries.len(), 1);
    }

    #[test]
    fn mark_success_orders_and_mark_failure_keeps_evidence() {
        let mut file = KnownWifiFile::default();
        upsert_known(&mut file, "a", Some("pa"));
        upsert_known(&mut file, "b", Some("pb"));
        mark_success(&mut file, "a", 100);
        mark_success(&mut file, "b", 200);
        let sorted = sort_entries(file.clone());
        assert_eq!(sorted.entries[0].ssid, "b");
        assert_eq!(sorted.entries[0].success_count, 1);
        // 失败不清掉"曾经成功过"
        mark_failure(&mut file, "b", "下发被拒", 300);
        let b = file.entries.iter().find(|e| e.ssid == "b").unwrap();
        assert_eq!(b.last_success_unix, 200);
        assert_eq!(b.last_error, "下发被拒");
    }

    /// 回归钉子：历史表已满时，**刚插入的那条不能被裁掉**。
    ///
    /// 这条规则来自一个只在"历史 ≥16 条"时才出现的怪毛病：新网络的证据是零
    /// （`last_success_unix == 0`），按证据排序必然垫底 ⇒ 表满时被立刻裁掉 ⇒
    /// 用户刚填的网络，自动连接永远看不到。写 `trim_keeps_most_recently_successful`
    /// 时先失败的就是它。
    #[test]
    fn freshly_added_entry_survives_trimming() {
        let mut file = KnownWifiFile::default();
        for i in 0..KNOWN_MAX as u64 {
            upsert_known(&mut file, &format!("old{i:02}"), Some("p"));
            mark_success(&mut file, &format!("old{i:02}"), i + 1);
        }
        assert_eq!(file.entries.len(), KNOWN_MAX);
        // 再插一条全新的（零证据）
        upsert_known(&mut file, "brand-new", Some("pw"));
        assert!(file.entries.iter().any(|e| e.ssid == "brand-new"));
        assert_eq!(file.entries.len(), KNOWN_MAX);
        // 被挤掉的是证据最弱的那条（old00，last_success 最小）
        assert!(!file.entries.iter().any(|e| e.ssid == "old00"));
    }

    #[test]
    fn trim_keeps_most_recently_successful() {
        let mut file = KnownWifiFile::default();
        for i in 0..(KNOWN_MAX as u64 + 4) {
            upsert_known(&mut file, &format!("net{i:02}"), Some("p"));
            mark_success(&mut file, &format!("net{i:02}"), i + 1);
        }
        let sorted = sort_entries(file);
        assert_eq!(sorted.entries.len(), KNOWN_MAX);
        // 最新的那条一定在（丢的是最老的）
        assert_eq!(sorted.entries[0].ssid, format!("net{:02}", KNOWN_MAX as u64 + 3));
        assert!(!sorted.entries.iter().any(|e| e.ssid == "net00"));
    }

    #[test]
    fn pick_prefers_current_pc_network() {
        let mut known = KnownWifiFile::default();
        upsert_known(&mut known, "office", Some("p1"));
        upsert_known(&mut known, "home", Some("p2"));
        mark_success(&mut known, "office", 100); // 更近
        mark_success(&mut known, "home", 50);

        let env = WifiEnv {
            current_ssid: Some("home".to_string()),
            visible: vec!["office".to_string(), "home".to_string()],
            lan_ipv4: vec!["192.168.1.9".to_string()],
        };
        // 当前所连优先，哪怕它不是最近成功的
        let picked = pick_candidate(&env, &known).unwrap();
        assert_eq!(picked.ssid, "home");
    }

    #[test]
    fn pick_falls_back_to_visible_then_history() {
        let mut known = KnownWifiFile::default();
        upsert_known(&mut known, "office", Some("p1"));
        upsert_known(&mut known, "home", Some("p2"));
        mark_success(&mut known, "home", 200); // 最近成功 = home
        mark_success(&mut known, "office", 100);

        // 上位机连的是历史里没有的网 ⇒ 退到"可见 AP ∩ 历史"
        let env = WifiEnv {
            current_ssid: Some("stranger".to_string()),
            visible: vec!["office".to_string()],
            lan_ipv4: vec![],
        };
        assert_eq!(pick_candidate(&env, &known).unwrap().ssid, "office");

        // 什么都读不到 ⇒ 最近成功的一条
        let env = WifiEnv { current_ssid: None, visible: vec![], lan_ipv4: vec![] };
        assert_eq!(pick_candidate(&env, &known).unwrap().ssid, "home");
    }

    #[test]
    fn pick_none_without_history() {
        let known = KnownWifiFile::default();
        let env = WifiEnv {
            current_ssid: Some("whatever".to_string()),
            visible: vec!["whatever".to_string()],
            lan_ipv4: vec!["10.0.0.2".to_string()],
        };
        assert!(pick_candidate(&env, &known).is_none());
    }

    #[test]
    fn pick_skips_passwordless_entries_only_as_last_resort() {
        let mut known = KnownWifiFile::default();
        upsert_known(&mut known, "open-net", None); // 无密码、无成功记录
        upsert_known(&mut known, "mine", Some("pw"));
        mark_success(&mut known, "mine", 10);
        let env = WifiEnv { current_ssid: None, visible: vec![], lan_ipv4: vec![] };
        assert_eq!(pick_candidate(&env, &known).unwrap().ssid, "mine");
    }

    #[test]
    fn env_epoch_tracks_ssid_and_ipv4() {
        let a = WifiEnv {
            current_ssid: Some("x".into()),
            visible: vec![],
            lan_ipv4: vec!["10.0.0.5".into()],
        };
        let mut b = a.clone();
        assert_eq!(a.epoch(), b.epoch());
        // 只换 IP 也必须算"环境变了" —— 板子会往旧 IP 发音频
        b.lan_ipv4 = vec!["10.0.0.6".into()];
        assert_ne!(a.epoch(), b.epoch());
        b = a.clone();
        b.current_ssid = Some("y".into());
        assert_ne!(a.epoch(), b.epoch());
    }

    #[test]
    fn backoff_is_monotonic_and_capped() {
        assert_eq!(backoff_secs(0), 0);
        assert_eq!(backoff_secs(1), 5);
        assert_eq!(backoff_secs(2), 15);
        assert_eq!(backoff_secs(3), 30);
        assert_eq!(backoff_secs(4), 60);
        assert_eq!(backoff_secs(99), 60);
        assert!(backoff_secs(1) < backoff_secs(2));
    }

    /// 抖动不该被算成一次"连接成功"。
    ///
    /// 失效方式是静默的：`success_count` / `last_success_unix` 被噪声刷高后，
    /// 界面上看不出任何异常，只是历史排序（选网依据）慢慢变歪 ——
    /// 抖得最凶的那张网反而排到最前。
    #[test]
    fn flapping_does_not_count_as_a_success() {
        // 本进程启动以来的**第一次**连上：与"离线了多久"无关，一律计成功。
        //
        // ⛔ 这一条是真机抓出来的：原先用 `offline_ms_before == 0` 代表首次连接，
        // 但应用启动时板子通常还没开始发包 ⇒ **第一个 tick 就把
        // `offline_since_ms` 设上了**（那段起表刻意如此，否则会漏掉"开机时板子
        // 本来就没连上"）⇒ 首包到达时它是"启动到首包的时长"，不是 0。
        // 实测打成了 `心跳恢复（离线 1.0 秒，判为短暂抖动，不计入成功次数）`，
        // 而那一刻其实是启动后的第一次连上（于是这条网永远不被记进历史）。
        assert!(counts_as_success(0, false), "首次连接必须计入成功");
        assert!(
            counts_as_success(1_000, false),
            "首次连接与离线时长无关 —— 启动后 1 秒才收到首包也很正常"
        );
        assert!(counts_as_success(9_999_999, false), "同上，离线表早已开始走");

        // 已经在线过 ⇒ 短离线是抖动，不算成功。边界取门槛两侧各一毫秒。
        assert!(!counts_as_success(1, true));
        assert!(!counts_as_success(RESUMED_FLAP_MS - 1, true));

        // 真离线一段时间后重连：算。门槛本身按"够长"处理。
        assert!(counts_as_success(RESUMED_FLAP_MS, true));
        assert!(counts_as_success(RESUMED_FLAP_MS + 1, true));

        // ⚠️ 门槛必须**大于**"上位机主动重下发"的等待时长：
        // 否则 T4 重下发后恢复的那一类会被误判成抖动，永远进不了历史
        //（离线越久越该被记住，结果反而被吞掉，方向正好反了）。
        assert!(
            RESUMED_FLAP_MS > OFFLINE_RESEND_MS,
            "抖动门槛必须大于 T4 等待时长（{RESUMED_FLAP_MS} vs {OFFLINE_RESEND_MS}）"
        );
    }

    /// 单调时基：恒 > 0 且严格递增。
    ///
    /// `0` 在本文件里是"未开始 / 从未发生"的哨兵值（`offline_since_ms` /
    /// `last_probe_ms` / `next_attempt_ms`），业务值取到 0 会被读成"还没开始"。
    #[test]
    fn monotonic_clock_is_nonzero_and_never_goes_back() {
        let a = now_ms();
        assert!(a > 0, "0 是哨兵值，业务时间戳不能为 0");
        std::thread::sleep(Duration::from_millis(5));
        let b = now_ms();
        assert!(b > a, "单调时钟必须严格递增：{b} 不大于 {a}");
    }

    #[test]
    fn mask_ssid_never_leaks_full_name() {
        assert_eq!(mask_ssid("MyHomeNet"), "MyH**et");
        assert_eq!(mask_ssid("ab"), "**");
        assert_eq!(mask_ssid("abcdef"), "a*****");
        assert!(!mask_ssid("secret-network").contains("secret-network"));
    }

    #[test]
    fn classify_error_maps_hid_failures_to_actionable_text() {
        let a = classify_error("未找到开发板（VID 303A / PID 1006），请确认已通过 USB 连接");
        assert!(a.contains("首次配网必须插 USB"));
        let b = classify_error("所有候选 HID 设备均无法下发配置");
        assert!(b.contains("没有配置通道"));
        let c = classify_error("配置已下发但 1.5s 内未收到 0x11 回执，请确认开发板仍在线");
        assert!(c.contains("没回执"));
        assert_eq!(classify_error("别的错"), "别的错");
    }

    #[test]
    fn known_file_roundtrip_through_disk() {
        let dir = std::env::temp_dir().join(format!("easyop-wifi-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut file = KnownWifiFile::default();
        upsert_known(&mut file, "net-a", Some("pw-a"));
        mark_success(&mut file, "net-a", 1234);
        save_known(&dir, &file).unwrap();
        let back = load_known(&dir);
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].ssid, "net-a");
        assert_eq!(back.entries[0].password, "pw-a");
        assert_eq!(back.entries[0].last_success_unix, 1234);
        assert_eq!(back.entries[0].success_count, 1);
        // 坏文件不 panic、按空处理
        std::fs::write(known_path(&dir), "{ not json").unwrap();
        assert!(load_known(&dir).entries.is_empty());
        // 不存在也不 panic
        let _ = std::fs::remove_file(known_path(&dir));
        assert!(load_known(&dir).entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
