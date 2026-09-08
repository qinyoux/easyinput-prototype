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

/// PTT 事件（主键按下 / 松开 / 编辑键 toggle）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    Down,
    Up,
    /// 编辑 PTT 键 toggle（按下切换开始/确认）
    EditToggle,
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

struct HookShared {
    ptt: PttKey,
    /// 编辑 PTT 键（toggle 模式：按一下开始，再按一下确认）。
    edit_ptt: Option<PttKey>,
    /// 修饰键当前按下的位掩码（由真实按键事件记账）。
    mods_state: u8,
    /// 主键当前是否已被判定为 PTT 按下（用于 keyup 匹配与自动重复抑制）。
    engaged: bool,
    /// 编辑键当前是否 engaged（toggle 状态）。
    edit_engaged: bool,
    tx: std::sync::mpsc::Sender<PttEvent>,
}

static SHARED: OnceLock<Mutex<HookShared>> = OnceLock::new();
/// 消息泵心跳计数（由 WM_TIMER 驱动，watchdog 据此判断泵线程是否卡死）。
static TICK: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

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

    let shared = match SHARED.get() {
        Some(m) => m,
        None => return pass(),
    };
    let mut s = match shared.lock() {
        Ok(g) => g,
        Err(_) => return pass(),
    };

    let vk = kb.vkCode;
    let down = matches!(wparam.0, WM_KEYDOWN | WM_SYSKEYDOWN);

    // 编辑 PTT 键判定（toggle 模式：仅在 keydown 时触发，不抑制按键）
    if let Some(ref edit_key) = s.edit_ptt {
        if vk == edit_key.main_vk {
            let mods_ok = s.mods_state & edit_key.mods == edit_key.mods;
            if down && mods_ok && !s.edit_engaged {
                s.edit_engaged = true;
                let _ = s.tx.send(PttEvent::EditToggle);
                return LRESULT(1);
            } else if !down && s.edit_engaged {
                s.edit_engaged = false;
                return LRESULT(1);
            }
            // 编辑键的 keyup 在 toggle 模式下不触发事件（仅在 keydown 时 toggle）
            // 但仍需抑制自动重复的 keydown
            if down && s.edit_engaged {
                return LRESULT(1);
            }
            return pass();
        }
    }

    // 主键判定先于修饰键记账：支持「按住右 Ctrl 说话」这类单修饰键 PTT。
    if vk == s.ptt.main_vk {
        let mods_ok = s.mods_state & s.ptt.mods == s.ptt.mods;
        if down {
            if mods_ok {
                if !s.engaged {
                    s.engaged = true;
                    let _ = s.tx.send(PttEvent::Down);
                }
                return LRESULT(1); // 抑制（含系统自动重复的 keydown）
            }
        } else if s.engaged {
            s.engaged = false;
            let _ = s.tx.send(PttEvent::Up);
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
                    tx,
                })
            });
            let mut g = shared.lock().map_err(|e| e.to_string())?;
            g.ptt = ptt;
            g.mods_state = 0;
            g.engaged = false;
            g.edit_engaged = false;
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
    Ok(())
}

/// 运行中热替换编辑 PTT 键定义（传 None 清除编辑键）。
pub fn set_edit_ptt_key(ptt: Option<PttKey>) -> Result<(), String> {
    let shared = SHARED.get().ok_or("PTT 引擎未启动")?;
    let mut g = shared.lock().map_err(|e| e.to_string())?;
    g.edit_ptt = ptt;
    g.edit_engaged = false;
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
