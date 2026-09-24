//! KEY4 浮窗提示词库（Phase 3）。
//!
//! 一块贴着鼠标光标的无边框置顶浮窗：在不离开当前应用的前提下，浏览当前方案的
//! 提示词并一键上屏；也能直接在浮窗里增删改提示词。硬件侧由 KEY4 与编码器驱动 ——
//! KEY4 开合浮窗、左旋上移高亮、右旋下移高亮、按下确认上屏
//! （四个具名动作在下发固件前被改写为保留 UUID，固件原样回传，见 `payload`）。
//!
//! 提示词属于方案（`Profile::prompts`），因此「内容随方案切换」是数据层自带的：
//! 任何一次方案变更（固件按键切方案、主窗口切方案、下发配置）都会重新发布一次快照。
//!
//! 三个关键约束：
//! 1. **不抢任务栏**：`skip_taskbar` + `decorations(false)`，浮窗只作为临时操作面板。
//! 2. **焦点可还原**：弹出前用 `GetForegroundWindow` 记录原前台窗口，收起时用
//!    `AttachThreadInput` + `SetForegroundWindow` 还焦。直接用 `SetForegroundWindow`
//!    通常会失败（Windows 前台锁定规则），必须先与当前前台线程做输入队列绑定。
//! 3. **定位复用字幕浮层逻辑**：`crate::cursor_subtitle_pos` 已实现「光标附近居中 +
//!    钳制到光标所在显示器内 + 下方放不下时翻到光标上方」，传自己的宽高（逻辑像素）即可。

use std::ffi::c_void;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::log_buffer;
use crate::model::Profile;
use crate::payload;

/// 浮窗窗口 label（同时用于 capabilities、事件定位与 `voice_edit` 的自窗口识别）。
pub(crate) const LABEL: &str = "workbench";

/// 浮窗尺寸（**逻辑**像素，即 CSS 像素）。宽度需容纳「序号 + 提示词预览 + 操作按钮」，
/// 高度按 `workbench.html` 的行规格（`min-height: 34px` + 4px 间距 + 头部 + 编辑区）约 7-8 行。
///
/// 与 `WebviewWindowBuilder::inner_size` 的语义一致：Windows 下由系统按显示器缩放比放大
/// 成物理尺寸（200% 缩放 = 960x720 物理像素）。把这两个常量当物理像素用会直接导致
/// 定位错位，换算统一由 `cursor_subtitle_pos` 负责，调用方不要自己乘缩放比。
///
/// 尺寸固定不变：条目再多也由列表内部滚动。（浮窗按光标定位，改高度会让它从锚点漂开。）
pub(crate) const WORKBENCH_W: f64 = 480.0;
pub(crate) const WORKBENCH_H: f64 = 360.0;

/// 归还焦点后等待前台稳定在原窗口的时间上限（两条路径共用）。
///
/// `WebviewWindow::hide` 是**异步生效**的：窗口管理器要等这次隐藏处理完，才会去挑
/// 「下一个」前台窗口，那个时刻完全可能晚于我们的 `SetForegroundWindow`，把前台重新
/// 抢回 easyop 自己。这是 Win32 的既定行为，只能靠「回读 + 重试」覆盖，不能靠定长
/// 延时赌时序。这里给的是一个**上限**而非固定等待：`ensure_foreground` 一旦连续两次
/// 读到目标窗口就会立刻返回，正常路径只需 20~60ms。
const FOREGROUND_SETTLE: Duration = Duration::from_millis(300);

/// 浮窗弹出前的前台窗口句柄，收起时用于还原焦点。
///
/// 以 `isize` 保存而非 `HWND`，避免在静态变量中存放裸指针包装类型。
static PREV_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);

/// 最近一次「不属于本进程」的前台窗口句柄，由后台跟踪线程持续刷新。
///
/// 存在的理由是 `PREV_FOREGROUND` 的采集时机有先天缺陷：浮窗的两种触发方式里，
/// 只有硬件 KEY4 能保证「弹出前前台是别的应用」。主窗口上的「浮窗工作台」按钮
/// （以及点击浮窗自身）会先激活 easyop，等 `show()` 去读 `GetForegroundWindow` 时，
/// 前台已经是本进程窗口，外部还原目标就被漏掉了 —— 收起后焦点无处可还，
/// Ctrl+V 静默贴进主窗口里没有输入框的页面，日志却只报「上屏完成」。
///
/// 后台线程以固定周期采样前台，只记录非本进程的窗口，于是**无论**浮窗是被硬件
/// 还是被界面按钮唤起的，都仍有「上一个真正在用的应用」可用作还原目标。
pub(crate) static LAST_EXTERNAL_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);

/// 后台采样前台窗口的间隔。200ms 足以覆盖「切换应用 → 点浮窗按钮」的人工节奏，
/// 代价仅为每 200ms 一次 `GetForegroundWindow`（无参数、不阻塞的轻量查询）。
const FOREGROUND_POLL: Duration = Duration::from_millis(200);

/// 浮窗命令事件载荷：主窗口 / 硬件动作 → 浮窗页面。
#[derive(serde::Serialize, Clone, Debug)]
pub struct WorkbenchCommand {
    /// 命令名：`prev` / `next` / `confirm` / `activate`。
    pub command: String,
    /// 附带文本：`activate` 传触发源的正文，页面据此定位要高亮的条目。
    pub text: Option<String>,
}

/// 浮窗列表项：当前方案提示词库里的一条提示词。
#[derive(serde::Serialize, Clone, Debug)]
pub struct WorkbenchItem {
    /// 条目 ID（即 `Prompt::id`）。页面的增 / 删 / 改都按它回传定位目标。
    pub id: String,
    /// 正文，面板预览与上屏都用它。
    pub text: String,
}

/// 发给浮窗的列表快照：方案名 + 该方案的提示词。
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct BoardInputEvent {
    /// 方案展示名（面板标题用）。
    pub profile: String,
    /// 方案 ID。页面增删改提示词时要把它回传给命令，才改得动正确的方案。
    pub profile_id: String,
    /// 提示词条目，顺序即展示顺序。
    pub items: Vec<WorkbenchItem>,
}

// ── 窗口生命周期 ──

/// 构建浮窗（在 `setup` 中调用一次）。初始隐藏，等 Host Action 触发再定位显示。
///
/// 与 `overlay` / `edit_overlay` 保持同一套窗口属性：无边框、透明、置顶、
/// 不进任务栏、不带阴影，构建时不抢焦点。
pub(crate) fn build_window(app: &AppHandle) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(
        app,
        LABEL,
        tauri::WebviewUrl::App("workbench.html".into()),
    )
    .title("easyop 提示词库")
    .inner_size(WORKBENCH_W, WORKBENCH_H)
    .position(80.0, 80.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .shadow(false)
    .visible(false)
    .build()?;
    Ok(())
}

/// 浮窗是否处于可见状态。
pub(crate) fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 显示浮窗：记录当前前台窗口 → 定位到光标下方居中 → 显示并聚焦。
///
/// 聚焦是必要的：浮窗要用键盘 Tab / 方向键 / Enter，没有焦点就收不到按键。
/// 这也是必须做焦点还原的原因（浮窗抢焦点会让原应用的输入光标失活）。
pub(crate) fn show(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        log_buffer::warn("浮窗工作台未创建，忽略显示请求");
        return;
    };

    // 记录「弹出前」的前台窗口，供收起时归还焦点。
    //
    // 判据必须是「这个句柄属于**本进程**吗」，而不是「它是不是浮窗自己」：浮窗的触发
    // 方式除了硬件 KEY4，还有主窗口上的「浮窗工作台」按钮，而点击那个按钮本身会先把
    // 主窗口激活。此时若把主窗口记成还原目标，收起后焦点会「正确地」还给主窗口，
    // 上屏的 Ctrl+V 于是贴进主窗口里没有输入框的页面 —— 这次粘贴是静默的，
    // `deliver` 仍返回 Ok，日志只报「上屏完成」，从日志上完全看不出失败。
    // 硬件路径下前台是别的应用，不受此影响。命中本进程时跳过写入并**保留**上一次的
    // 有效记录（连续开合场景下也该继续沿用最早那个外部窗口）。
    if let Some(fg) = foreground_hwnd() {
        if is_own_window(fg) {
            // 前台已是自己：这一次采样拿不到外部目标，回退到后台跟踪线程记录的
            // 「最近一次外部前台窗口」。界面按钮唤起浮窗走的正是这条路。
            match LAST_EXTERNAL_FOREGROUND.lock().ok().and_then(|slot| *slot) {
                Some(prev) if !is_own_window(prev) => {
                    if let Ok(mut slot) = PREV_FOREGROUND.lock() {
                        *slot = Some(prev);
                    }
                    log_buffer::info(format!(
                        "浮窗工作台：前台是本应用窗口，沿用后台记录的原前台窗口 0x{prev:x}"
                    ));
                }
                _ => {
                    log_buffer::info("浮窗工作台：前台是本应用窗口，跳过原前台记录");
                }
            }
        } else if let Ok(mut slot) = PREV_FOREGROUND.lock() {
            *slot = Some(fg);
            log_buffer::info(format!("浮窗工作台：记录原前台窗口 0x{fg:x}"));
        }
    }

    if let Some((x, y)) = crate::cursor_subtitle_pos(&win, WORKBENCH_W, WORKBENCH_H) {
        let _ = win.set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ));
    }
    let _ = win.show();
    let _ = win.set_focus();
    log_buffer::info("浮窗工作台已显示");
}

/// 隐藏浮窗窗口本体，并取回弹出时记录的原前台窗口句柄（取后即清空）。
///
/// 只做「隐藏」这一件事，**不动焦点**：归还必须等隐藏生效之后再做，否则窗口管理器
/// 挑选「下一个」前台窗口的激活流程会把刚还回去的焦点重新抢回 easyop。上屏路径与
/// 收起路径都走这里，差别只在 `ensure_foreground` 的等待上限（同为
/// `FOREGROUND_SETTLE`），所以把「隐藏」和「归还」拆开，由调用方决定后续动作。
fn hide_window(app: &AppHandle) -> Option<isize> {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }

    // 取出即清空：避免下次弹出时误用上一次的句柄（原窗口可能已关闭）。
    PREV_FOREGROUND.lock().ok().and_then(|mut slot| slot.take())
}

/// 隐藏浮窗并把焦点还给弹出前的应用。
pub(crate) fn hide(app: &AppHandle) {
    match hide_window(app) {
        Some(hwnd) => {
            ensure_foreground(hwnd, FOREGROUND_SETTLE);
            log_buffer::info("浮窗工作台已收起，焦点已归还");
        }
        None => log_buffer::info("浮窗工作台已收起（无记录的还原目标）"),
    }
}

/// 开合浮窗，返回操作后是否可见。
pub(crate) fn toggle(app: &AppHandle) -> bool {
    if is_open(app) {
        hide(app);
        false
    } else {
        show(app);
        true
    }
}

/// 确认上屏：收起浮窗 → 等焦点回到原窗口 → 用剪贴板把 `text` 注入原应用。
///
/// **必须先收起再注入**：注入走的是模拟 Ctrl+V，若浮窗仍是前台窗口，粘贴目标
/// 就变成浮窗自己。
///
/// **归还与注入之间不留空档**：这里不用固定 `sleep` 过渡，而是由
/// `ensure_foreground` 等到前台稳定在原窗口后**立刻**注入。理由见 `FOREGROUND_SETTLE`：
/// 抢走前台的激活发生在归还**之后**，任何定长 sleep 都可能正好停在那段空档里，
/// 让 Ctrl+V 贴给没有输入框的窗口 —— 这次粘贴失败是静默的，日志只会报「上屏完成」。
///
/// 整个函数阻塞约 1.2~1.8s（含 `text_delivery::deliver` 的消费等待），
/// 调用方必须把它放到阻塞线程上执行，不能直接占用 Tauri 的事件循环。
pub(crate) fn commit_selected(app: &AppHandle, text: &str) -> Result<(), String> {
    log_buffer::info(format!(
        "浮窗工作台：开始上屏（{} 字符）",
        text.trim().chars().count()
    ));
    match hide_window(app) {
        Some(hwnd) => {
            ensure_foreground(hwnd, FOREGROUND_SETTLE);
            log_buffer::info("浮窗工作台已收起，焦点已归还");
        }
        None => log_buffer::info("浮窗工作台已收起（无记录的还原目标）"),
    }
    let result = crate::text_delivery::deliver(text);
    match &result {
        Ok(()) => log_buffer::info("浮窗工作台：上屏完成"),
        Err(err) => log_buffer::warn(format!("浮窗工作台：上屏失败（{err}）")),
    }
    result
}

/// 向浮窗页面广播一条命令（编码器上下移动、确认、筛选切换）。
///
/// 只发给 `workbench` 这一个目标，主窗口不会收到这些噪声事件。
pub(crate) fn emit_command(app: &AppHandle, command: &str, text: Option<String>) {
    let _ = app.emit_to(
        LABEL,
        "workbench-command",
        WorkbenchCommand {
            command: command.to_string(),
            text,
        },
    );
}

// ── 列表快照 ──

/// 把一份方案推导成浮窗要显示的列表快照。
///
/// 快照**完全由方案自身推导**（`Profile::prompts`），不依赖任何下发产物。这与改造前
/// 「从本次下发产物的 UUID 反向索引里反查」有本质区别：那时列表项要跟固件里正在生效
/// 的 UUID 逐一对上，只能与下发共用同一次遍历；而提示词根本不进固件，同一个方案在
/// 任何时刻推导出的快照都相同，因此可以随手计算，也不必再缓存。
pub(crate) fn snapshot(profile: &Profile) -> BoardInputEvent {
    BoardInputEvent {
        profile: profile.name.clone(),
        profile_id: profile.id.clone(),
        items: profile
            .prompts
            .iter()
            .map(|prompt| WorkbenchItem {
                id: prompt.id.clone(),
                text: prompt.text.clone(),
            })
            .collect(),
    }
}

/// 广播列表快照给浮窗页面。
///
/// 调用时机是「当前显示内容可能变了」的每一处：切换方案、下发配置、增删改提示词。
/// `board-input` 只发给浮窗；浮窗尚未创建或页面尚未加载完成时事件自然丢失，
/// 页面每次打开都会通过 `workbench_list` 命令补拉一次当前方案。
pub(crate) fn publish_board_input(app: &AppHandle, profile: &Profile) {
    let event = snapshot(profile);
    log_buffer::info(format!(
        "浮窗提示词库：列表已更新（方案「{}」/{} 条）",
        event.profile,
        event.items.len()
    ));
    let _ = app.emit_to(LABEL, "board-input", event);
}

// ── 保留 UUID 分流 ──

/// 处理浮窗工作台的保留 UUID，返回 `true` 表示该 UUID 已被工作台消费。
///
/// 硬件侧四个绑定在下发固件前会被 `payload` 改写成保留 UUID，固件原样回传：
/// - `workbench`：KEY4 短按 → 开合浮窗；
/// - `workbench_prev` / `workbench_next`：编码器左旋 / 右旋 → 移动高亮；
/// - `workbench_confirm`：编码器按下 → 确认上屏。
///
/// 这里按「规范名」而不是按 UUID 常量分发：UUID ↔ 具名的对应关系只维护在 `payload`
/// 一处（并有往返测试守着），避免同一个映射在两个模块里各写一遍。
///
/// 高亮位置不在这里维护：筛选后的列表由浮窗页面持有，页面才是权威。所以这里只
/// 转发意图，页面收到 `confirm` 后再带着当前项的正文回调 `workbench_commit`。
pub(crate) fn handle_action(app: &AppHandle, uuid: &str) -> bool {
    match payload::named_for_reserved_uuid(uuid) {
        Some("workbench") => {
            toggle(app);
            true
        }
        Some("workbench_prev") => {
            emit_command(app, "prev", None);
            true
        }
        Some("workbench_next") => {
            emit_command(app, "next", None);
            true
        }
        Some("workbench_confirm") => {
            emit_command(app, "confirm", None);
            true
        }
        _ => false,
    }
}

// ── 前台窗口记录与还原 ──

/// 读取当前前台窗口句柄。
#[cfg(windows)]
pub(crate) fn foreground_hwnd() -> Option<isize> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: GetForegroundWindow 无参数，返回可能为 NULL 的 HWND。
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        None
    } else {
        Some(hwnd.0 as isize)
    }
}

#[cfg(not(windows))]
pub(crate) fn foreground_hwnd() -> Option<isize> {
    None
}

/// 判断某个窗口句柄是否属于本进程（即 easyop 自己的窗口）。
///
/// 用 `GetWindowThreadProcessId` 取窗口所属进程号，与 `std::process::id()` 比对。
/// 比按 label 列举窗口（见 `voice_edit::wait_for_target_focus` 的写法）更稳妥：
/// 那套写法要显式维护 `["overlay", "edit_overlay", "workbench", "main"]` 这份清单，
/// 以后新增或改名窗口时漏掉一项就会判定错误；而「是不是自己的窗口」这个判据不该
/// 随窗口数量变化。
#[cfg(windows)]
pub(crate) fn is_own_window(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    // SAFETY: hwnd 由 GetForegroundWindow 取得，调用方已确认非空；pid 为本地栈变量，
    // 其生命周期完整覆盖本次调用。
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut c_void), Some(&mut pid)) };
    pid == std::process::id()
}

#[cfg(not(windows))]
pub(crate) fn is_own_window(_hwnd: isize) -> bool {
    false
}

/// 启动后台前台窗口跟踪线程（在 `setup` 中调用一次）。
///
/// 每 `FOREGROUND_POLL` 采样一次 `GetForegroundWindow`，只要它不属于本进程且与上次
/// 记录不同，就刷新 `LAST_EXTERNAL_FOREGROUND`。这是 `show()` 在「弹出时前台已是
/// 自己」的情况下唯一的还原目标来源，原因见该静态变量的文档。
///
/// 与 Host Action 监听不同，这里**不需要召回开关**：线程无副作用（只读一个句柄、
/// 写一个 `Mutex<Option<isize>>`），随进程退出自然终止，也不会被配置变更影响。
#[cfg(windows)]
pub(crate) fn spawn_foreground_tracker() {
    std::thread::spawn(|| {
        // 本地缓存上一次写过值，避免同一窗口在前台时反复加锁与刷日志。
        let mut tracked: Option<isize> = None;
        loop {
            if let Some(fg) = foreground_hwnd() {
                if !is_own_window(fg) && tracked != Some(fg) {
                    tracked = Some(fg);
                    if let Ok(mut slot) = LAST_EXTERNAL_FOREGROUND.lock() {
                        *slot = Some(fg);
                    }
                    log_buffer::info(format!("浮窗工作台：跟踪到外部前台窗口 0x{fg:x}"));
                }
            }
            sleep(FOREGROUND_POLL);
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn spawn_foreground_tracker() {}

/// 在 `timeout` 内把前台焦点稳定到 `target`：连续两次读到目标窗口才认为收敛。
///
/// 为什么需要「看门狗」而不是一次 `restore_foreground`：把前台抢回 easyop 的激活
/// 发生在归还**之后**（隐藏收尾挑选「下一个」前台窗口、点击浮窗内容留下的延迟激活），
/// 一次性的归还会被随后到达的激活覆盖。这里以 20ms 为周期回读前台，
/// 一旦发现被抢走就再归还一次；连续两次确认才算稳定。
///
/// 目标窗口已被销毁时直接放弃，避免无效重试刷日志。
#[cfg(windows)]
pub(crate) fn ensure_foreground(target: isize, timeout: Duration) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};

    // SAFETY: target 来自 GetForegroundWindow 的记录值，先用 IsWindow 做有效性判定；
    // GetForegroundWindow 无参数，返回可能为 NULL 的 HWND。
    if !unsafe { IsWindow(HWND(target as *mut c_void)) }.as_bool() {
        log_buffer::warn("浮窗工作台：原窗口已关闭，放弃焦点归还");
        return;
    }

    let deadline = Instant::now() + timeout;
    let mut streak = 0u8;
    while Instant::now() < deadline {
        if unsafe { GetForegroundWindow() }.0 as isize == target {
            streak += 1;
            if streak >= 2 {
                return;
            }
        } else {
            streak = 0;
            restore_foreground(target);
        }
        sleep(Duration::from_millis(20));
    }

    log_buffer::warn("浮窗工作台：等待焦点归还超时，前台仍不是原窗口");
}

#[cfg(not(windows))]
pub(crate) fn ensure_foreground(_target: isize, _timeout: Duration) {}

/// 把前台焦点还给 `target` 句柄对应的窗口。
///
/// 流程与原因：
/// 1. 目标窗口可能已被销毁 / 最小化，先 `IsWindow` + `IsIconic` 判定并 `SW_RESTORE`。
/// 2. 直接 `SetForegroundWindow` 会被前台锁定规则拒绝（调用者不是当前前台线程）。
///    通过 `AttachThreadInput` 把当前线程挂到当前前台线程的输入队列上，
///    共享输入状态后 `SetForegroundWindow` 才会生效，最后再解除绑定。
/// 3. 归还动作与浮窗自身的隐藏收尾存在竞态，`SetForegroundWindow` 返回成功也未必
///    真的切过去。因此每次调用后回读 `GetForegroundWindow` 校验，未命中再试一次。
#[cfg(windows)]
pub(crate) fn restore_foreground(target: isize) {
    use windows::Win32::Foundation::{BOOL, HWND};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
        ShowWindow, SW_RESTORE,
    };

    let hwnd = HWND(target as *mut c_void);

    // SAFETY: hwnd 来自 GetForegroundWindow，用前先做有效性判定；其余调用均为 Win32 标准用法。
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            log_buffer::info("浮窗工作台：原窗口已关闭，跳过焦点还原");
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        // 归还后回读校验：窗口管理器可能在 SetForegroundWindow 之后又把前台改回
        // easyop 自己（隐藏流程收尾），因此最多尝试两次，命中即停。
        for attempt in 0..2 {
            if GetForegroundWindow() == hwnd {
                break;
            }

            let current = GetForegroundWindow();
            let current_thread = GetWindowThreadProcessId(current, None);
            let our_thread = GetCurrentThreadId();

            let attached = if current_thread != 0 && current_thread != our_thread {
                AttachThreadInput(current_thread, our_thread, BOOL(1)).as_bool()
            } else {
                false
            };

            let ok = SetForegroundWindow(hwnd).as_bool();

            if attached {
                let _ = AttachThreadInput(current_thread, our_thread, BOOL(0));
            }

            if !ok && attempt == 1 {
                log_buffer::warn("浮窗工作台：焦点还原失败（前台窗口可能被系统锁定）");
            }

            sleep(Duration::from_millis(30));
        }

        if GetForegroundWindow() != hwnd {
            log_buffer::warn("浮窗工作台：焦点还原未生效，前台仍是其他窗口");
        }
    }
}

#[cfg(not(windows))]
fn restore_foreground(_target: isize) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Prompt;

    /// 快照按提示词库的原始顺序输出，并带上方案 ID 供页面回传增删改。
    #[test]
    fn snapshot_maps_prompts_in_stored_order() {
        let mut p = Profile::factory_default("写作", "p1");
        p.prompts = vec![
            Prompt { id: "a".into(), text: "帮我润色这段文字".into() },
            Prompt { id: "b".into(), text: "总结要点".into() },
        ];

        let event = snapshot(&p);

        assert_eq!(event.profile, "写作");
        assert_eq!(event.profile_id, "p1");
        let ids: Vec<&str> = event.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(event.items[0].text, "帮我润色这段文字");
        assert_eq!(event.items[1].text, "总结要点");
    }

    /// 没有提示词的方案输出空列表，而不是沿用上一个方案的条目 ——
    /// 「切换方案后内容随之切换」这条需求里，切换到空方案是最容易被漏掉的一侧。
    #[test]
    fn snapshot_of_empty_library_is_empty() {
        let p = Profile::factory_default("默认方案", "default");
        let event = snapshot(&p);
        assert!(event.items.is_empty());
        assert_eq!(event.profile_id, "default");
    }
}
