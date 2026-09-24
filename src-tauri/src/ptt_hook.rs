//! PTT 触发引擎 —— WH_KEYBOARD_LL 低级键盘钩子。
//!
//! 替代开发板 PTT 键：任意 USB / 蓝牙 / 2.4G / WiFi 键盘的按键在进入系统
//! 输入队列之前被本钩子拦截，命中 PTT 定义则抑制（不漏给前台应用）并
//! 通知录音会话。蓝牙/2.4G 键盘在此层与有线键盘完全等价：事件到达钩子时
//! 只剩 vkCode + scanCode，连接方式信息已不存在。
//!
//! 两个关键防御点：
//! 1. LLKHF_INJECTED 过滤：注入引擎模拟的 Ctrl+V 会再次进入本钩子，
//!    必须直接放行，否则递归拦截导致注入失效，还会污染修饰键状态机。
//! 2. 修饰键状态机：组合键 PTT（如 Ctrl+Shift+Space）用真实修饰键的
//!    按下/松开记账，主键到达时判定整组成立。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, SetTimer, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
};

const WM_KEYDOWN: usize = 0x0100;
const WM_SYSKEYDOWN: usize = 0x0104;
const INJECTED_FLAG: u32 = 0x10;

// 修饰键位掩码
pub const MOD_CTRL: u8 = 0b0001;
pub const MOD_SHIFT: u8 = 0b0010;
pub const MOD_ALT: u8 = 0b0100;
pub const MOD_WIN: u8 = 0b1000;

// 左右修饰键的原始 VK code（钩子层看到的是具体左右键，不是通用键）
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

/// 回车键。板子的 KEY2 默认就绑在它上面，所以物理键与电脑键盘回车**是同一个键**
/// —— 没必要为板子单开一条路（也开不出来：HID 层看不出是谁按的）。
const VK_RETURN: u32 = 0x0D;

/// 语音链路的**动作事件**。三个入口，三个事件，一一对应，没有中间态。
///
/// # ⛔ 为什么"按下 / 松开"在这里只取一半（这是刻意的，别再"修"回去）
///
/// 固件对 `VoicePttHold` / `EditPttHold` 发的是 **HidKeyDown / HidKeyUp 一对**
/// （`keymap.cpp::event_for_action`：`phase == Pressed ? HidKeyDown : HidKeyUp`）。
/// 也就是说"按住说话、松开结束"在链路上**完全可表达**。
///
/// 但产品规定的语义是 **短按**：**按下**开始，**再按一次**结束。
/// 于是钩子只把这一对里的 keydown 转成事件；keyup 照旧被抑制（不漏给前台窗口），
/// 但**不产生任何事件**。
///
/// ⚠️ 这里踩过一次坑，留个记号：看到"固件明明发了一对 keydown/keyup"就把它改成
/// "松开结束"，是**把硬件能力当成了产品意图** —— 两者不是一回事。四条事实当时都在
/// 文件里，只是彼此矛盾（固件动作名叫 `Hold`、本枚举原先按"成对"写、
/// `keyboard_audio.rs` 的注释说"松开 stop_streaming"、而 `constants.ts` 的显示名
/// 是「语音 · 按下说话」）。**改这一层之前，先去确认"用户看到的那句话"说的是
/// 按住还是按一下** —— 当前（v0.30.0）是后者，用户另行明确过一次。
///
/// ⚠️ 另一个容易混的：任务台**槽位键**用的 `ActionKind::HostAction` 确实没有松开
/// （固件 `Released` 返回空事件，只在按下时回发一次），所以那边只能 toggle。
/// 两边"为什么是 toggle"的理由**不一样**，不要互相引用 —— 用其中一个的现象去论证
/// 另一个，就会得出"KEY1 也没有松开"这种错误结论。
///
/// # 短按带来的一个额外好处
///
/// 整条链路**不再依赖 keyup 送达**：钩子被临时挂起、按键被别的进程吃掉、
/// 宿主的 keyup 处理有 bug —— 都不会让录音卡死（对比：按住语义下，丢一次松开
/// 就是一段永远收不了尾的录音）。少一个必须按时空到的信号，就少一类静默故障。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    /// 主 PTT 键（KEY1 / 界面 ● 按钮）**按下**。
    Down,
    /// 编辑 PTT 键（KEY3）**按下**。toggle：按一下开始听，再按一下收尾改写。
    EditToggle,
    /// 提交键（**回车**）按下，且此刻它该被当作「注入并发送」，而不是普通回车。
    ///
    /// 判据不在钩子里（见 [`set_enter_gate`]）：钩子只负责"问一句、吞掉、报上去"。
    EnterSubmit,
}

/// PTT 按键定义。
#[derive(Debug, Clone)]
pub struct PttKey {
    /// 主键 VK code，按下/松开以它为准。
    pub main_vk: u32,
    /// 需同时按住的修饰键位掩码（0 = 单键 PTT，建议用 F13-F24）。
    pub mods: u8,
    /// 人类可读描述（原样保留用户输入）。
    pub label: String,
}

impl PttKey {
    /// 解析形如 `Ctrl+Shift+Space` / `F13` / `RAlt` 的按键描述。
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("按键定义不能为空".into());
        }
        let tokens: Vec<&str> = spec
            .split('+')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        let (main_tok, mod_toks) = tokens
            .split_last()
            .ok_or_else(|| "按键定义不完整".to_string())?;
        let main_vk = vk_from_name(main_tok)?;

        let mut mods = 0u8;
        for t in mod_toks {
            mods |= match t.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => MOD_CTRL,
                "shift" => MOD_SHIFT,
                "alt" => MOD_ALT,
                "win" | "meta" => MOD_WIN,
                other => return Err(format!("未知修饰键: {other}")),
            };
        }
        Ok(PttKey {
            main_vk,
            mods,
            label: spec.to_string(),
        })
    }
}

fn vk_from_name(name: &str) -> Result<u32, String> {
    let lower = name.to_ascii_lowercase();
    let vk = match lower.as_str() {
        "space" => 0x20,
        "esc" | "escape" => 0x1B,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "backspace" => 0x08,
        "caps" | "capslock" => 0x14,
        "insert" => 0x2D,
        "delete" | "del" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "lctrl" => VK_LCONTROL,
        "rctrl" => VK_RCONTROL,
        "lshift" => VK_LSHIFT,
        "rshift" => VK_RSHIFT,
        "lalt" => VK_LMENU,
        "ralt" => VK_RMENU,
        "lwin" => VK_LWIN,
        "rwin" => VK_RWIN,
        "printscreen" => 0x2C,
        "scrolllock" => 0x91,
        "pause" => 0x13,
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            match c {
                'a'..='z' => c as u32 - 'a' as u32 + 0x41,
                '0'..='9' => c as u32 - '0' as u32 + 0x30,
                _ => return Err(format!("无法识别的按键: {name}")),
            }
        }
        s if s.starts_with('f') && s.len() >= 2 => {
            let n: u32 = s[1..]
                .parse()
                .map_err(|_| format!("无法识别的按键: {name}"))?;
            match n {
                1..=12 => 0x70 + (n - 1),
                13..=24 => 0x7C + (n - 13),
                _ => return Err(format!("F{n} 超出支持范围 (F1-F24)")),
            }
        }
        _ => return Err(format!("无法识别的按键: {name}")),
    };
    Ok(vk)
}

fn mod_bit(vk: u32) -> Option<u8> {
    match vk {
        VK_LCONTROL | VK_RCONTROL => Some(MOD_CTRL),
        VK_LSHIFT | VK_RSHIFT => Some(MOD_SHIFT),
        VK_LMENU | VK_RMENU => Some(MOD_ALT),
        VK_LWIN | VK_RWIN => Some(MOD_WIN),
        _ => None,
    }
}

/// 一次 **keydown** 的处置判定。
///
/// 抽成纯函数，是因为钩子本体（要拿锁、要发事件、要碰 Win32）**测不了**，
/// 而它最要命的那个判断是纯粹的：**"这一下该吞，还是该原样放行？"**
///
/// # ★ 唯一判据：**"吞键"必须与"匹配成功"成对出现**
///
/// 两者一旦分离，那一下按键就在系统里**凭空消失**。历史事故（v0.30.1 修）：
/// 编辑键分支把 `return LRESULT(1)` 写在了 `if mods_ok` **之外**，于是只要 VK
/// 对上主键就吞 —— 默认编辑键是 `Ctrl+Shift+E`，结果是**普通键盘的 `e` 键整个失效**
/// （打开本应用后 e 打不出来，别的键都正常，看起来像键盘坏了）。
/// 这类故障钩子不留任何痕迹、日志也无线索，只能靠"吞/放判据唯一且可测"来防。
///
/// ⚠️ 另一条语义要求：**主键与编辑键必须共用这一个判据**。各写一遍
/// `mods_state & mods == mods` 就是给"两把键行为分叉"留口子（用户得记两套按法）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeydownVerdict {
    /// 属于这个键（VK 命中 **且** 修饰键同时成立）⇒ 抑制，不漏给前台窗口。
    Swallow,
    /// 不属于 ⇒ 原样透传。**修饰键不成立时必须走这条。**
    Pass,
}

/// 判定 `vk` 是否构成 `key` 的一次"按下"（修饰键同时成立）。
pub fn keydown_verdict(key: &PttKey, vk: u32, mods_state: u8) -> KeydownVerdict {
    if vk == key.main_vk && mods_state & key.mods == key.mods {
        KeydownVerdict::Swallow
    } else {
        KeydownVerdict::Pass
    }
}

struct HookShared {
    ptt: PttKey,
    /// 编辑 PTT 键（toggle：按一下开始听，再按一下收尾改写）。
    edit_ptt: Option<PttKey>,
    /// 修饰键当前按下的位掩码（由真实按键事件记账）。
    mods_state: u8,
    /// 主键当前是否已按下。**只用于抑制系统自动重复的 keydown**
    /// —— 短按语义下不需要它来判断"松开"（见 `PttEvent` 的说明）。
    engaged: bool,
    /// 编辑键当前是否已按下（同上，只用于去重）。
    edit_engaged: bool,
    /// 回车键当前是否被我们吞掉了。用于把配对的 keyup 也吞掉，
    /// 不给前台窗口留下一个没有 keydown 的半截 keyup。
    enter_engaged: bool,
    tx: std::sync::mpsc::Sender<PttEvent>,
}

static SHARED: OnceLock<Mutex<HookShared>> = OnceLock::new();
/// 消息泵心跳计数（由 WM_TIMER 驱动，watchdog 据此判断泵线程是否卡死）。
static TICK: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

/// "这个回车该不该被当作『注入并发送』"的判据，由 `lib.rs` 启动时注入。
///
/// # 为什么是**回调**，而不是在钩子里存一份状态
///
/// 钩子每收到一个按键都要回答这个是非题。答案只有**一个真相源** —— 会话状态机
/// （"正在录一段结果会进输入框的话，或这句话的交付还在路上"，见
/// `SessionManager::grabs_enter`）。
///
/// 在钩子里再存一份 bool，就要回答"谁负责清它、早一步晚一步都错"；而它的失效方式
/// 很难受：卡在 `true` 会把用户**所有**的回车都吞掉 —— 表现为"回车键坏了"，
/// 排查方向从一开始就是错的（同类教训：拿 A 通道的状态冒充 B 通道）。
/// 存副本 = 制造一个会静默老化的真相；回调查询 = 没有副本可老化。
///
/// ⚠️ 判据会在**钩子回调里**被调用 ⇒ 必须**无锁、无阻塞、不重入钩子**。
/// 目前它只读一个 `AtomicBool`（并加一句日志）。
static ENTER_GATE: OnceLock<Box<dyn Fn() -> bool + Send + Sync>> = OnceLock::new();

/// 装配回车判据（只生效一次；`lib.rs` 启动时调用）。
pub fn set_enter_gate(gate: impl Fn() -> bool + Send + Sync + 'static) {
    let _ = ENTER_GATE.set(Box::new(gate));
}

/// 问一句：此刻回车算不算"注入并发送"。没装配时一律 **false**（安全侧：
/// 判不出来就不要吞用户的回车）。
fn enter_grabbed() -> bool {
    ENTER_GATE.get().map(|g| g()).unwrap_or(false)
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pass = || CallNextHookEx(None, code, wparam, lparam);
    if code < 0 {
        return pass();
    }

    let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

    // 注入事件直接放行：防递归（注入引擎的 Ctrl+V），且不污染修饰键状态机。
    if kb.flags.0 & INJECTED_FLAG != 0 {
        return pass();
    }

    let vk = kb.vkCode;
    let down = matches!(wparam.0, WM_KEYDOWN | WM_SYSKEYDOWN);

    // 回车判据在**拿锁之前**问。判据（会话层）目前只读一个 `AtomicBool`，
    // 但把它写在锁内，就等于给以后立了一条"判据永远不许碰锁"的隐形规矩 ——
    // 这种约束迟早被踩，而且踩了就是死锁。只在真的是回车时才问，其余按键零开销。
    let enter_grab = vk == VK_RETURN && enter_grabbed();

    let shared = match SHARED.get() {
        Some(m) => m,
        None => return pass(),
    };
    let mut s = match shared.lock() {
        Ok(g) => g,
        Err(_) => return pass(),
    };

    // 编辑 PTT 键：**只认按下**，keyup 只抑制、不发事件。
    //
    // 与主键完全同一套规则 —— 两个键的行为必须一致，否则用户得记两套按法。
    // 一致性靠**共用同一个判据函数**保证（`keydown_verdict`），不再各写一遍。
    if let Some(ref edit_key) = s.edit_ptt {
        if vk == edit_key.main_vk {
            let swallow = keydown_verdict(edit_key, vk, s.mods_state) == KeydownVerdict::Swallow;
            if down {
                if swallow {
                    if !s.edit_engaged {
                        s.edit_engaged = true;
                        let _ = s.tx.send(PttEvent::EditToggle);
                    }
                    return LRESULT(1); // 抑制（含系统自动重复的 keydown）
                }
                // ⛔ 修饰键不成立 ⇒ 这**不是**编辑键的按下，必须原样放行。
                //
                // 曾经这里的 `return LRESULT(1)` 落在 `if mods_ok` **之外**，于是
                // 只要 vk 对上主键就吞 —— 默认编辑键是 `Ctrl+Shift+E`，结果是
                // **普通键盘的 `e` 键整个失效**（打开本应用后 e 打不出来，别的键都正常，
                // 看起来像键盘坏了）。这是本项目"静默失效"里最难查的一类：钩子不留痕。
                //
                // ★ 判据一条：**"吞键"必须与"匹配成功"成对出现。**
                //   判据已抽到 `keydown_verdict`（可单测），别在这里再写一遍条件。
                return pass();
            }
            // keyup：清掉去重标志就够，**不发事件**。
            // 这里若图省事直接 `pass()`，系统的自动重复 keydown 就会被当成第二次按下。
            if s.edit_engaged {
                s.edit_engaged = false;
                return LRESULT(1);
            }
            return pass();
        }
    }

    // 主键判定先于修饰键记账：支持「按住右 Ctrl 说话」这类单修饰键 PTT。
    if vk == s.ptt.main_vk {
        let swallow = keydown_verdict(&s.ptt, vk, s.mods_state) == KeydownVerdict::Swallow;
        if down {
            if swallow {
                if !s.engaged {
                    s.engaged = true;
                    let _ = s.tx.send(PttEvent::Down);
                }
                return LRESULT(1); // 抑制（含系统自动重复的 keydown）
            }
        } else if s.engaged {
            // ⛔ 松开**不发事件**（短按语义，见 `PttEvent` 的说明）。
            // `engaged` 在这里清掉就够；键本身仍要抑制，否则会漏给前台窗口。
            s.engaged = false;
            return LRESULT(1);
        }
        return pass();
    }

    // 回车：把「结束录音 + 注入 + 发送」折叠成一次按键。
    //
    // ⛔ 只在该吞的时候吞。会话层说不算（没在录，或这一段的结果根本不进输入框）时，
    //    回车**原样透传** —— 用户在任何地方敲回车都是正常操作，吞掉它是一次
    //    没有任何提示的静默失效，而且看起来像"回车键坏了"。
    // ⛔ 只认**不带修饰键**的回车：`Ctrl+Enter` / `Shift+Enter` 在聊天窗和编辑器里
    //    各有各的含义（换行 / 发送），不能替用户决定。
    if vk == VK_RETURN {
        if down {
            if enter_grab && s.mods_state == 0 {
                if !s.enter_engaged {
                    s.enter_engaged = true;
                    let _ = s.tx.send(PttEvent::EnterSubmit);
                }
                return LRESULT(1);
            }
            return pass();
        }
        // keyup 的配对判断**只看 `enter_engaged`，不看 gate**：
        // 若看 gate，会话恰好在这一对的中间结束时，被吞掉的 keydown 会配上一个
        // 漏出去的 keyup，而 `enter_engaged` 还会一直卡在 true（下一次回车失效）。
        if s.enter_engaged {
            s.enter_engaged = false;
            return LRESULT(1);
        }
        return pass();
    }

    if let Some(bit) = mod_bit(vk) {
        if down {
            s.mods_state |= bit;
        } else {
            s.mods_state &= !bit;
        }
    }
    pass()
}

/// PTT 引擎句柄：持有事件接收端，由调度线程消费。
pub struct PttEngine {
    rx: Receiver<PttEvent>,
}

impl PttEngine {
    /// 安装低级键盘钩子并启动消息泵 + watchdog。
    pub fn start(ptt: PttKey) -> Result<Self, String> {
        let (tx, rx) = channel();
        {
            let shared = SHARED.get_or_init(|| {
                Mutex::new(HookShared {
                    ptt: ptt.clone(),
                    edit_ptt: None,
                    mods_state: 0,
                    engaged: false,
                    edit_engaged: false,
                    enter_engaged: false,
                    tx,
                })
            });
            let mut g = shared.lock().map_err(|e| e.to_string())?;
            g.ptt = ptt;
            g.mods_state = 0;
            g.engaged = false;
            g.edit_engaged = false;
            g.enter_engaged = false;
        }
        RUNNING.store(true, Ordering::SeqCst);
        thread::Builder::new()
            .name("ptt-hook".into())
            .spawn(hook_thread)
            .map_err(|e| format!("启动钩子线程失败: {e}"))?;
        thread::Builder::new()
            .name("ptt-watchdog".into())
            .spawn(watchdog_thread)
            .map_err(|e| format!("启动看门狗线程失败: {e}"))?;
        Ok(PttEngine { rx })
    }

    /// 非阻塞取事件；超时/断开返回 None。
    pub fn recv_timeout(&self, dur: Duration) -> Option<PttEvent> {
        match self.rx.recv_timeout(dur) {
            Ok(ev) => Some(ev),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }
}

/// 运行中热替换 PTT 键定义。
pub fn set_ptt_key(ptt: PttKey) -> Result<(), String> {
    let shared = SHARED.get().ok_or("PTT 引擎未启动")?;
    let mut g = shared.lock().map_err(|e| e.to_string())?;
    g.ptt = ptt;
    g.engaged = false;
    g.enter_engaged = false;
    Ok(())
}

/// 运行中热替换编辑 PTT 键定义（传 None 清除编辑键）。
pub fn set_edit_ptt_key(ptt: Option<PttKey>) -> Result<(), String> {
    let shared = SHARED.get().ok_or("PTT 引擎未启动")?;
    let mut g = shared.lock().map_err(|e| e.to_string())?;
    g.edit_ptt = ptt;
    g.edit_engaged = false;
    g.enter_engaged = false;
    Ok(())
}

/// 获取当前 PTT 键定义。
pub fn ptt_key() -> Option<PttKey> {
    SHARED.get().and_then(|s| s.lock().ok().map(|g| g.ptt.clone()))
}

/// 获取当前编辑 PTT 键定义。
pub fn edit_ptt_key() -> Option<PttKey> {
    SHARED.get().and_then(|s| s.lock().ok().and_then(|g| g.edit_ptt.clone()))
}

/// 钩子线程：安装钩子 + 3s 心跳定时器 + 消息泵。
/// LL 钩子的回调在本线程的消息循环上下文中执行，线程必须持续泵消息。
fn hook_thread() {
    unsafe {
        let hhk = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[ptt] 安装键盘钩子失败: {e}");
                return;
            }
        };

        // 心跳定时器：保证消息泵每 3 秒至少活动一次（键盘空闲时无按键消息）。
        let timer = SetTimer(None, 1, 3000, None);

        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            TICK.fetch_add(1, Ordering::Relaxed);
            if ret.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = KillTimer(None, timer);
        let _ = UnhookWindowsHookEx(hhk);
    }
}

/// 看门狗：消息泵 6 秒无心跳则重装钩子。
/// 覆盖系统因钩子回调超时（LowLevelHooksTimeout）静默摘除钩子的场景。
fn watchdog_thread() {
    let mut last = TICK.load(Ordering::Relaxed);
    loop {
        thread::sleep(Duration::from_secs(6));
        let now = TICK.load(Ordering::Relaxed);
        if now == last && RUNNING.load(Ordering::Relaxed) {
            eprintln!("[ptt] watchdog 检测到消息泵无心跳，重装键盘钩子");
            let _ = thread::Builder::new()
                .name("ptt-hook-re".into())
                .spawn(hook_thread);
        }
        last = now;
    }
}

#[cfg(test)]
mod tests {
    //! 钩子层唯一能被单测的部分：**吞键 / 放行的判据**。
    //!
    //! 它的失效方式是本项目最难查的一类 —— 钩子不留痕、日志无线索，
    //! 用户看到的现象是"键盘坏了"（而不是"某个功能不好使"），排查方向从一开始就错。
    //! 所以判据必须抽成纯函数、必须被钉死。
    use super::*;

    /// VK 码（用字面量而非 `vk_from_name`，免得测试跟着解析器一起坏）。
    const VK_E: u32 = 0x45;
    const VK_F: u32 = 0x46;
    const VK_F13: u32 = 0x7C;
    const VK_F14: u32 = 0x7D;

    /// ★ 回归测试：**修饰键不成立时绝不能吞键**。
    ///
    /// 事故现场：默认编辑键是 `Ctrl+Shift+E`，而钩子把"吞"写在了"修饰键匹配"之外
    /// ⇒ 普通 `e` 键被全系统吞掉。修完必须钉死"单按主键 = 放行"。
    #[test]
    fn keydown_verdict_matrix() {
        let edit = PttKey::parse("Ctrl+Shift+E").expect("默认编辑键应能解析");
        let single = PttKey::parse("F13").expect("单键 PTT 应能解析");

        // —— 组合键：缺任何一个修饰键都必须放行 ——
        assert_eq!(
            keydown_verdict(&edit, VK_E, 0),
            KeydownVerdict::Pass,
            "只按 e（无修饰键）被吞掉 = 普通键盘的 e 键失效"
        );
        assert_eq!(
            keydown_verdict(&edit, VK_E, MOD_CTRL),
            KeydownVerdict::Pass,
            "只按 Ctrl 还没按 Shift，不成组"
        );
        assert_eq!(
            keydown_verdict(&edit, VK_E, MOD_SHIFT),
            KeydownVerdict::Pass,
            "只按 Shift 还没按 Ctrl，不成组"
        );

        // —— 组合齐了才吞；多按别的修饰键不影响（超集也算成立）——
        assert_eq!(
            keydown_verdict(&edit, VK_E, MOD_CTRL | MOD_SHIFT),
            KeydownVerdict::Swallow
        );
        assert_eq!(
            keydown_verdict(&edit, VK_E, MOD_CTRL | MOD_SHIFT | MOD_ALT),
            KeydownVerdict::Swallow
        );

        // —— VK 不对 ⇒ 无论修饰键如何都放行 ——
        assert_eq!(
            keydown_verdict(&edit, VK_F, MOD_CTRL | MOD_SHIFT),
            KeydownVerdict::Pass
        );

        // —— 单键 PTT（mods == 0）：任何修饰键状态下都成立。
        //    这是「按住右 Ctrl 说话 / 按住 Alt 说话」这类单修饰键 PTT 的判据基础
        //    （主键判定先于修饰键记账）。**别把 mods == 0 特判成"必须没按修饰键"。**
        assert_eq!(keydown_verdict(&single, VK_F13, 0), KeydownVerdict::Swallow);
        assert_eq!(
            keydown_verdict(&single, VK_F13, MOD_CTRL),
            KeydownVerdict::Swallow
        );
        assert_eq!(
            keydown_verdict(&single, VK_F13, MOD_ALT | MOD_WIN),
            KeydownVerdict::Swallow
        );
        assert_eq!(keydown_verdict(&single, VK_F14, 0), KeydownVerdict::Pass);
    }
}
