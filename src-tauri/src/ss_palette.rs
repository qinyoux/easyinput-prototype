//! ⌘SS 命令面板（Super StartUp 缩写）。
//!
//! 一个屏幕居中的无边框置顶浮窗：用户输入关键词，实时搜索本机应用并一键启动。
//! 硬件侧由绑定到 `ss_palette` 具名动作的按键触发开合（KEY1/KEY4/任意键均可配置）。
//!
//! 设计原则：
//! 1. 屏幕居中显示，不跟随光标（与 workbench 的"光标旁浮窗"定位模式不同）
//! 2. 焦点管理同 workbench：弹出前记录前台窗口，收起时归还
//! 3. 搜索结果来自 system_apps（开始菜单扫描）+ 用户应用库 + 系统命令

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::log_buffer;
use crate::model::AppEntry;
use crate::pinyin::{abbrev_of, initials_of};

/// 面板窗口 label。
pub(crate) const LABEL: &str = "ss_palette";

/// 面板窗口尺寸（逻辑像素）。
///
/// 宽度 = 面板 600 + 左右各 `SS_PALETTE_PAD` 的留白，留白用于让面板的边框和阴影
/// 不贴住窗口边缘（贴住会被裁掉）。高度只是一个初始值，前端会按内容量出真实高度后
/// 调用 `ss_resize` 同步。
pub(crate) const SS_PALETTE_PAD: f64 = 12.0;
pub(crate) const SS_PALETTE_W: f64 = 600.0 + SS_PALETTE_PAD * 2.0;
pub(crate) const SS_PALETTE_H: f64 = 182.0;

/// 焦点归还等待上限（同 workbench）。
const FOREGROUND_SETTLE: Duration = Duration::from_millis(300);

/// 弹出前的前台窗口句柄，收起时归还。
static PREV_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);

// ── 窗口生命周期 ──

/// 构建面板窗口（在 setup 中调用一次）。初始隐藏，等触发再显示。
pub(crate) fn build_window(app: &AppHandle) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(
        app,
        LABEL,
        tauri::WebviewUrl::App("ss_palette.html".into()),
    )
    .title("easyop · ⌘SS")
    .inner_size(SS_PALETTE_W, SS_PALETTE_H)
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

/// 面板是否可见。
pub(crate) fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 显示面板：记录前台窗口 → 屏幕居中 → 显示并聚焦。
pub(crate) fn show(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        log_buffer::warn("⌘SS 面板未创建，忽略显示请求");
        return;
    };

    // 记录原前台窗口（逻辑同 workbench）
    if let Some(fg) = foreground_hwnd() {
        if is_own_window(fg) {
            match crate::workbench::LAST_EXTERNAL_FOREGROUND.lock().ok().and_then(|slot| *slot) {
                Some(prev) if !is_own_window(prev) => {
                    if let Ok(mut slot) = PREV_FOREGROUND.lock() {
                        *slot = Some(prev);
                    }
                    log_buffer::info(format!(
                        "⌘SS 面板：前台是本应用窗口，沿用后台记录的原前台窗口 0x{prev:x}"
                    ));
                }
                _ => {
                    log_buffer::info("⌘SS 面板：前台是本应用窗口，跳过原前台记录");
                }
            }
        } else if let Ok(mut slot) = PREV_FOREGROUND.lock() {
            *slot = Some(fg);
            log_buffer::info(format!("⌘SS 面板：记录原前台窗口 0x{fg:x}"));
        }
    }

    // 屏幕居中
    if let Ok(Some(monitor)) = win.current_monitor() {
        let size = monitor.size();
        let x = (size.width as f64 - SS_PALETTE_W * monitor.scale_factor()) / 2.0;
        let y = (size.height as f64 - SS_PALETTE_H * monitor.scale_factor()) / 3.0; // 偏上 1/3
        let _ = win.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }

    let _ = win.show();
    let _ = win.set_focus();

    // 通知前端「面板被唤出」。
    // 面板窗口只在应用启动时创建一次并常驻，页面不会重新加载；前端依赖这个事件
    // 来清空上一次的搜索词、重置光标，并重新拉取「精简右键菜单」等后端设置。
    // 之前这里漏发了该事件，导致前端一直停留在首次加载的状态（开关改了也不生效）。
    let _ = app.emit("ss-palette-show", ());

    log_buffer::info("⌘SS 命令面板已显示");
}

/// 隐藏面板并归还焦点。
pub(crate) fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
    let hwnd = PREV_FOREGROUND.lock().ok().and_then(|mut slot| slot.take());
    match hwnd {
        Some(h) => {
            ensure_foreground(h, FOREGROUND_SETTLE);
            log_buffer::info("⌘SS 命令面板已收起，焦点已归还");
        }
        None => log_buffer::info("⌘SS 命令面板已收起（无记录的还原目标）"),
    }
}

/// 隐藏面板但**不归还焦点**。
///
/// 用于「右键菜单里执行了某个动作」之后的收尾：动作往往会把焦点交给新启动的
/// 窗口（打开文件 → 记事本/播放器；属性 → 属性对话框），此时再把焦点抢回原来的
/// 前台窗口就会打断用户。所以这里只藏窗口，并清掉记录的还原目标。
pub(crate) fn hide_silent(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
    if let Ok(mut slot) = PREV_FOREGROUND.lock() {
        *slot = None;
    }
}

/// 开合面板，返回操作后是否可见。
pub(crate) fn toggle(app: &AppHandle) -> bool {
    if is_open(app) {
        hide(app);
        false
    } else {
        show(app);
        true
    }
}

/// Host Action 入口：被 `host_action.rs` 调用，命中 UUID_SS_PALETTE 时开合面板。
///
/// 返回 true 表示已处理该 UUID，调用方不再向下分发；返回 false 表示未命中。
pub(crate) fn handle_action(app: &AppHandle, uuid: &str) -> bool {
    if uuid != crate::constants::UUID_SS_PALETTE {
        return false;
    }
    toggle(app);
    true
}

// ── 使用频次统计 ──

/// 使用频次记录：key = `{kind}:{target}`，value = 执行次数。
///
/// 用于搜索结果排序：使用频次越高，排名越靠前。
/// 持久化到配置目录 `ss_usage_stats.json`。
static USAGE_STATS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

/// 使用频次持久化文件路径（在 Tauri 配置目录下）。
fn usage_stats_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join("ss_usage_stats.json")
}

/// 加载使用频次统计（启动时调用一次），顺带做一次性的历史键迁移。
pub(crate) fn load_usage_stats(config_dir: &PathBuf) {
    let path = usage_stats_path(config_dir);
    let raw: HashMap<String, u32> = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // v0.5.0 之前，「内置工具」用的 kind 是 "miniapp"（那时它被错叫成「小程序」）。
    // kind 拆分后内置工具改成 "tool"，旧键不搬过来的话，用户攒下的频次权重会静默失效 ——
    // 表现为「以前越常用的工具，升级后反而排得越后」。
    //
    // 所以这里做一次幂等的键迁移：`miniapp:<tool>` → `tool:<tool>`。
    // 之所以敢整段搬，是因为旧版本里 kind="miniapp" 只用于内置工具，不存在歧义；
    // 迁移后原键即消失，下次启动不会重复计。
    let mut stats: HashMap<String, u32> = HashMap::with_capacity(raw.len());
    let mut migrated = 0usize;
    for (key, count) in raw {
        match key.strip_prefix("miniapp:") {
            Some(tool) => {
                let new_key = format!("tool:{}", tool);
                // 理论上新旧键不会并存，真并存了取较大值，避免越迁越小
                let merged = stats.get(&new_key).copied().unwrap_or(0).max(count);
                stats.insert(new_key, merged);
                migrated += 1;
            }
            None => {
                stats.insert(key, count);
            }
        }
    }

    let count = stats.len();
    if let Ok(mut slot) = USAGE_STATS.lock() {
        *slot = Some(stats);
    }

    if migrated > 0 {
        save_usage_stats(config_dir);
        log_buffer::info(format!(
            "⌘SS：使用频次统计已加载（{} 条），其中 {} 条旧「小程序」键已迁移为内置工具键",
            count, migrated
        ));
    } else {
        log_buffer::info(format!("⌘SS：使用频次统计已加载（{} 条）", count));
    }
}

/// 保存使用频次统计（执行成功后异步保存，失败不阻塞）。
fn save_usage_stats(config_dir: &PathBuf) {
    if let Ok(slot) = USAGE_STATS.lock() {
        if let Some(stats) = slot.as_ref() {
            let path = usage_stats_path(config_dir);
            if let Ok(json) = serde_json::to_string_pretty(stats) {
                let _ = fs::create_dir_all(config_dir);
                let _ = fs::write(path, json);
            }
        }
    }
}

/// 获取某条目的使用频次。
fn get_usage_count(kind: &str, target: &str) -> u32 {
    let key = format!("{}:{}", kind, target);
    USAGE_STATS
        .lock()
        .ok()
        .as_deref()
        .and_then(|opt| opt.as_ref())
        .and_then(|m| m.get(&key).copied())
        .unwrap_or(0)
}

/// 记录一次成功执行（频次 +1），并异步持久化。
fn record_usage(kind: &str, target: &str, config_dir: Option<PathBuf>) {
    let key = format!("{}:{}", kind, target);
    if let Ok(mut slot) = USAGE_STATS.lock() {
        let stats = slot.get_or_insert_with(HashMap::new);
        *stats.entry(key).or_insert(0) += 1;
    }
    // 异步持久化（这里简化为同步写入，因为频次不会太高）
    if let Some(dir) = config_dir {
        save_usage_stats(&dir);
    }
}

// ── 搜索 ──

/// 搜索结果项。
#[derive(serde::Serialize, Clone, Debug)]
pub struct SearchResult {
    /// 结果类型：app / library / system。
    pub kind: String,
    /// 显示标题。
    pub title: String,
    /// 副标题（路径/描述）。
    pub subtitle: String,
    /// 执行时用的路径或命令。
    pub target: String,
    /// 应用分类：browser / dev / office / communication / media / game / utility / other
    #[serde(default)]
    pub category: String,
    /// 匹配分数（0-100 + 频次加成，可能超过 100）。
    pub score: f32,
    /// 标题中匹配字符的字符索引（前端用于高亮显示）。
    pub match_indices: Vec<usize>,
    /// 匹配模式：prefix / contains / pinyin / abbrev（前端可选展示）。
    pub match_mode: String,
}

// ── 内置工具注册表 ──
//
// 注意命名：这里的「工具」**不是**「小程序」。
//
//   - **内置工具**（本表）：easyop 自带的一小块界面（计算器 / 待办 / 截图 / 翻译…），
//     进入后由前端渲染器接管面板主体，不启动任何外部进程（见 ss_palette.html 的
//     `state.miniapp` 视图）。结果项的 kind 是 `"tool"`。
//   - **小程序**（`mini_apps` 模块）：用户把**外部** App / Web 项目 / 项目目录集成
//     进 easyop，点一下真正启动那个外部目标。结果项的 kind 是 `"miniapp"`。
//
// 两者以前被混在同一个 kind 里，于是「小程序」这个词被理解成了「面板内置的小工具」。
// 拆开之后语义才对得上：小程序 = 外部应用的集成入口。
//
// 内置工具的两个入口：
// - **关键词**：`keywords` 中任意一个词被输入前缀命中，就出现「打开 xx」的结果项；
// - **特殊语法**：计算器额外支持 `=` 前缀直接带算式（见 `search()` 里的分支），
//   这样 `= 12*34` 一步就能看到答案，不用先打开计算器再输入。
//
// `id` 同时是前端渲染器的 key（MINI_APP_RENDERERS 的键）和结果项的 `target`，
// 两边必须严格一致，否则前端会拿到一个没有渲染器的工具。
struct ToolDef {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    keywords: &'static [&'static str],
}

const BUILTIN_TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "miniapps",
        title: "小程序",
        subtitle: "管理集成进 easyop 的外部应用 / Web 项目",
        keywords: &["小程序", "集成", "外部应用", "miniapp", "miniapps", "xcx"],
    },
    ToolDef {
        id: "calculator",
        title: "计算器",
        subtitle: "输入算式直接算，回车复制结果",
        keywords: &["计算器", "计算", "calculator", "calc", "jsq"],
    },
    ToolDef {
        id: "todo",
        title: "待办清单",
        subtitle: "临时记一笔，回车添加",
        keywords: &["待办", "todo", "db", "清单", "任务"],
    },
    ToolDef {
        id: "screenshot",
        title: "截图",
        subtitle: "抓取整个屏幕并保存到图片文件夹",
        keywords: &["截图", "截屏", "screenshot", "capture", "jt"],
    },
    ToolDef {
        id: "translate",
        title: "翻译",
        subtitle: "输入文本，回车打开在线翻译",
        keywords: &["翻译", "translate", "fanyi", "fy"],
    },
];

/// 搜索应用：三层匹配算法（前缀 → 包含 → 拼音/缩写）+ 频次加权排序。
///
/// 匹配层次与基础分：
/// - 第一层：前缀匹配  → 基础分 95
/// - 第二层：包含匹配  → 基础分 70
/// - 第三层：拼音首字母 → 基础分 45
/// - 第三层：单词缩写   → 基础分 40
///
/// 频次加成：`min(usage_count * 0.5, 15)` 分，最多加 15 分（避免老应用霸榜）。
///
/// 排序规则：得分降序 → 频次降序 → 标题字母序。
pub(crate) fn search(query: &str, user_apps: &[AppEntry]) -> Vec<SearchResult> {
    let t0 = Instant::now();
    let q = query.trim().to_lowercase();
    // 原始大小写版本：计算器的算式要保持原样（`= 1e3` 之类），不能被 to_lowercase 之外的处理影响
    let raw_query = query.trim();
    if q.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<SearchResult> = Vec::new();

    // 辅助：计算频次加成（最多 15 分）
    let usage_bonus = |kind: &str, target: &str| -> f32 {
        let count = get_usage_count(kind, target) as f32;
        (count * 0.5).min(15.0)
    };

    // 辅助：构造结果项
    let make_result = |kind: &str, title: &str, subtitle: &str, target: &str,
                       category: &str, base_score: f32, indices: Vec<usize>, mode: &str| -> SearchResult {
        let bonus = usage_bonus(kind, target);
        SearchResult {
            kind: kind.to_string(),
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            target: target.to_string(),
            category: category.to_string(),
            score: base_score + bonus,
            match_indices: indices,
            match_mode: mode.to_string(),
        }
    };

    // ── 1. 系统应用（注册表 / 开始菜单 / 常见安装目录） ──
    //
    // 走共享索引：扫描与拼音检索键预计算只在缓存失效时做一次，
    // 搜索路径上只用预计算好的 name_lower / pinyin_initials / abbrev，
    // 且不为 800 项做任何克隆。
    let system_index = crate::system_apps::system_app_index();
    for item in system_index.iter() {
        let app = &item.app;
        let name_lower = &item.name_lower;
        let pinyin_initials = &item.pinyin_initials;
        let abbrev = &item.abbrev;

        if let Some(indices) = prefix_match_indices(name_lower, &q) {
            results.push(make_result("app", &app.name, &app.path, &app.path, &app.category, 95.0, indices, "prefix"));
        } else if let Some(indices) = contains_match_indices(name_lower, &q) {
            results.push(make_result("app", &app.name, &app.path, &app.path, &app.category, 70.0, indices, "contains"));
        } else if let Some(indices) = pinyin_initials_match(pinyin_initials, &q, &app.name) {
            results.push(make_result("app", &app.name, &app.path, &app.path, &app.category, 45.0, indices, "pinyin"));
        } else if let Some(indices) = abbrev_match(abbrev, &q, &app.name) {
            results.push(make_result("app", &app.name, &app.path, &app.path, &app.category, 40.0, indices, "abbrev"));
        }
    }

    // ── 2. 用户应用库 ──
    for entry in user_apps {
        let title_lower = entry.title.to_lowercase();
        let pinyin_initials = initials_of(&entry.title);
        let abbrev = abbrev_of(&entry.title);

        let subtitle = match &entry.target {
            crate::model::AppTarget::App { path, .. } => path.clone(),
            crate::model::AppTarget::File { path } => path.clone(),
            crate::model::AppTarget::Url { url } => url.clone(),
            crate::model::AppTarget::Folder { path } => path.clone(),
        };

        // 用户应用库基础分略高（用户自己加的，更可能想找）
        let (prefix_score, contains_score, pinyin_score, abbrev_score) = (98.0, 75.0, 50.0, 45.0);

        if let Some(indices) = prefix_match_indices(&title_lower, &q) {
            results.push(make_result("library", &entry.title, &subtitle, &entry.uuid, "library", prefix_score, indices, "prefix"));
        } else if let Some(indices) = contains_match_indices(&title_lower, &q) {
            results.push(make_result("library", &entry.title, &subtitle, &entry.uuid, "library", contains_score, indices, "contains"));
        } else if let Some(indices) = pinyin_initials_match(&pinyin_initials, &q, &entry.title) {
            results.push(make_result("library", &entry.title, &subtitle, &entry.uuid, "library", pinyin_score, indices, "pinyin"));
        } else if let Some(indices) = abbrev_match(&abbrev, &q, &entry.title) {
            results.push(make_result("library", &entry.title, &subtitle, &entry.uuid, "library", abbrev_score, indices, "abbrev"));
        }
    }

    // ── 3. 系统命令（白名单） ──
    let sys_commands = [
        ("锁屏", "lock", "锁定电脑屏幕"),
        ("关机", "shutdown", "关闭电脑"),
        ("重启", "restart", "重启电脑"),
        ("睡眠", "sleep", "让电脑进入睡眠"),
    ];
    for (name, cmd, desc) in sys_commands {
        let name_lower = name.to_lowercase();
        let pinyin_initials = initials_of(name);

        if let Some(indices) = prefix_match_indices(&name_lower, &q) {
            results.push(make_result("system", name, desc, cmd, "system", 92.0, indices, "prefix"));
        } else if name_lower.contains(&q) || cmd.contains(&q) {
            // 命令名包含或英文命令匹配
            let indices = contains_match_indices(&name_lower, &q).unwrap_or_default();
            results.push(make_result("system", name, desc, cmd, "system", 72.0, indices, "contains"));
        } else if let Some(indices) = pinyin_initials_match(&pinyin_initials, &q, name) {
            results.push(make_result("system", name, desc, cmd, "system", 48.0, indices, "pinyin"));
        }
    }

    // ── 4. 内置工具 ──
    //
    // 4a. 计算器的 `=` 语法：`= 12*34` 这种输入直接把算式带进面板，
    //     用户回车就能在面板里看到结果，不用先打开计算器再敲一遍。
    //     给 100 分是为了让它稳定压过同名的应用结果 —— 用户写下 `=` 时意图非常明确。
    if let Some(expr) = raw_query.strip_prefix('=') {
        let expr = expr.trim();
        let (title, subtitle, target) = if expr.is_empty() {
            (
                "计算器".to_string(),
                "输入算式，例如 = 12*34".to_string(),
                "calculator".to_string(),
            )
        } else {
            (
                format!("= {expr}"),
                format!("计算 {expr}"),
                format!("calculator:{expr}"),
            )
        };
        results.push(make_result(
            "tool", &title, &subtitle, &target, "tool", 100.0, Vec::new(), "tool",
        ));
    }

    // 4b. 其余内置工具按关键词/标题匹配。分数夹在系统命令（92/72/48）与普通应用（95/70/45）
    //     之间偏低一档：内置工具是「锦上添花」，不该把真正要打开的应用挤下去。
    for app in BUILTIN_TOOLS {
        let title_lower = app.title.to_lowercase();
        let initials = initials_of(app.title);

        let mut hit: Option<(f32, Vec<usize>, &str)> = None;
        for kw in app.keywords {
            if let Some(indices) = prefix_match_indices(kw, &q) {
                hit = Some((88.0, indices, "prefix"));
                break;
            }
        }
        if hit.is_none() {
            for kw in app.keywords {
                if let Some(indices) = contains_match_indices(kw, &q) {
                    hit = Some((62.0, indices, "contains"));
                    break;
                }
            }
        }
        if hit.is_none() {
            if let Some(indices) = prefix_match_indices(&title_lower, &q) {
                hit = Some((88.0, indices, "prefix"));
            }
        }
        if hit.is_none() {
            if let Some(indices) = pinyin_initials_match(&initials, &q, app.title) {
                hit = Some((55.0, indices, "pinyin"));
            }
        }

        if let Some((score, indices, mode)) = hit {
            results.push(make_result(
                "tool", app.title, app.subtitle, app.id, "tool", score, indices, mode,
            ));
        }
    }

    // ── 5. 小程序 ──
    //
    // 小程序 = 用户集成进 easyop 的**外部**目标（本机 exe / Web 项目 / 项目目录）。
    // 这里只负责把它们搜出来；真正「点一下启动那个外部应用」发生在 execute()。
    //
    // 分数与内置工具同档（88/62/55）—— 小程序是用户自己登记进来的，权重不该比
    // 系统应用低，否则刚集成完搜不到、体验像没生效。
    for hit in crate::mini_apps::search_hits(&q) {
        let subtitle = mini_app_subtitle(&hit.app);
        results.push(make_result(
            "miniapp",
            &hit.app.name,
            &subtitle,
            &hit.app.id,
            "miniapp",
            hit.score,
            hit.match_indices,
            hit.match_mode,
        ));
    }

    // ── 4c. 插件命令（plugin_registry） ──
    //
    // 「写插件」的收益在这里兑现：不改 easyop 源码，往 plugins/ 放一个目录，
    // 它声明的命令就直接出现在 ⌘SS 搜索里。
    //
    // kind = "plugin"，target = 命令 id（`<namespace>.<action>`）。execute() 据此
    // 交给 plugin_host 激活执行（激活层开发中，见 execute 的 "plugin" 分支）。
    //
    // 分数固定 86：与内置工具 / 小程序同档。插件是用户自己装的，权重不该被系统应用压过去。
    for hit in crate::plugin_registry::search_hits(&q) {
        results.push(make_result(
            "plugin",
            &hit.title,
            &hit.subtitle,
            &hit.command_id,
            "plugin",
            86.0,
            Vec::new(),
            "contains",
        ));
    }

    // ── 排序：得分降序 → 频次降序 → 标题升序 ──
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ua = get_usage_count(&a.kind, &a.target);
                let ub = get_usage_count(&b.kind, &b.target);
                ub.cmp(&ua)
            })
            .then_with(|| a.title.cmp(&b.title))
    });

    results.truncate(20);

    // 耗时打点：只在偏慢时输出，避免逐键搜索刷屏。
    let elapsed = t0.elapsed();
    if elapsed.as_millis() >= 20 {
        log_buffer::info(format!(
            "⌘SS 搜索「{}」耗时 {}ms（系统应用索引 {} 项）",
            q,
            elapsed.as_millis(),
            system_index.len()
        ));
    }

    results
}

// ── 匹配辅助函数 ──

/// 前缀匹配：返回匹配字符的**字符索引**（前端用于高亮，JS 字符串按字符处理）。
fn prefix_match_indices(text: &str, query: &str) -> Option<Vec<usize>> {
    if text.starts_with(query) {
        // 计算 query 对应的字符数（不是字节数）
        let char_count = query.chars().count();
        Some((0..char_count).collect())
    } else {
        None
    }
}

/// 包含匹配：返回第一个匹配位置的字符索引。
fn contains_match_indices(text: &str, query: &str) -> Option<Vec<usize>> {
    if let Some(byte_pos) = text.find(query) {
        // 把字节位置转换成字符位置
        let char_pos = text[..byte_pos].chars().count();
        let char_count = query.chars().count();
        Some((char_pos..char_pos + char_count).collect())
    } else {
        None
    }
}

/// 拼音首字母匹配：`initials` 是每个字首字母拼成的字符串，
/// `query` 是用户输入的小写串，`original` 是原始标题（用于生成索引）。
///
/// 返回的是**字符索引**（每个字对应一个索引）。
fn pinyin_initials_match(initials: &str, query: &str, original: &str) -> Option<Vec<usize>> {
    if query.len() > initials.len() {
        return None;
    }
    if !initials.starts_with(query) {
        return None;
    }
    // 匹配前 N 个字符（每个字对应一个索引）
    let count = query.chars().count().min(original.chars().count());
    Some((0..count).collect())
}

/// 单词缩写匹配：`abbrev` 是单词首字母串，`query` 是用户输入。
/// 匹配逻辑：abbrev 以 query 开头。
///
/// 例如：abbrev="vsc" (Visual Studio Code) query="vs" → 匹配前两个单词
/// 为了简化高亮，直接按缩写长度对每个单词首字母位置做标记。
/// 但因为我们只有 original 字符串，不好定位单词首字母位置，所以返回空 indices。
/// （前端如果 indices 为空，就不做高亮，只显示结果）
fn abbrev_match(abbrev: &str, query: &str, _original: &str) -> Option<Vec<usize>> {
    if query.len() > abbrev.len() {
        return None;
    }
    if abbrev.starts_with(query) {
        // 缩写匹配暂时不做字符级高亮（定位单词首字母比较复杂）
        // 返回空 vec，前端会显示结果但不高亮
        Some(Vec::new())
    } else {
        None
    }
}

// ── 匹配函数的公开包装（供 mini_apps 复用）──
//
// 「小程序」注册表住在 mini_apps.rs，但它的搜索必须与本文件的搜索**完全同构**
// —— 同一个关键词在系统应用和小程序上命中的方式不一样，用户会立刻察觉。
// 所以这里只做一层薄包装把上面三个函数暴露给同 crate 的模块用，而不另写一套。

/// 前缀匹配（对 mini_apps 暴露）。
pub(crate) fn prefix_match_indices_pub(text: &str, query: &str) -> Option<Vec<usize>> {
    prefix_match_indices(text, query)
}

/// 包含匹配（对 mini_apps 暴露）。
pub(crate) fn contains_match_indices_pub(text: &str, query: &str) -> Option<Vec<usize>> {
    contains_match_indices(text, query)
}

/// 拼音首字母匹配（对 mini_apps 暴露）。
///
/// 高亮索引只取决于 query 的字符数，所以这里用 `initials` 自身当 `original`
/// 占位 —— 与 [`pinyin_initials_match`] 里 `min(query.chars(), original.chars())`
/// 的结果一致（initials 至少和 query 一样长，否则前面就返回 None 了）。
pub(crate) fn pinyin_initials_match_pub(initials: &str, query: &str) -> Option<Vec<usize>> {
    pinyin_initials_match(initials, query, initials)
}

/// 小程序结果项的第二行：说清「这是个集成进来的什么东西、指向哪里」。
///
/// 用户在小程序窗口里点一下就会真的启动外部目标，所以第二行必须给足信息，
/// 避免「名字一样但指错地方」的误启动。
fn mini_app_subtitle(app: &crate::mini_apps::MiniApp) -> String {
    let kind_label = match app.kind.as_str() {
        "app" => "外部应用",
        "url" => "Web 项目",
        _ => "项目目录",
    };
    if app.note.is_empty() {
        format!("已集成 · {kind_label} · {}", app.target)
    } else {
        format!("已集成 · {kind_label} · {}", app.note)
    }
}

// ── 焦点管理（复用 workbench 的 Win32 函数）──

fn foreground_hwnd() -> Option<isize> {
    // 复用 workbench 的实现（同 crate 可直接调用）
    crate::workbench::foreground_hwnd()
}

fn is_own_window(hwnd: isize) -> bool {
    crate::workbench::is_own_window(hwnd)
}

fn ensure_foreground(hwnd: isize, timeout: Duration) {
    crate::workbench::ensure_foreground(hwnd, timeout)
}

// ── 执行搜索结果 ──

/// 执行搜索结果项。
///
/// - kind="app" / kind="shell"：直接启动可执行文件或用 ShellExecute 打开
/// - kind="library"：target 是 UUID，在应用库中查找并打开
/// - kind="system"：执行系统命令（lock / shutdown / restart / sleep）
///
/// `config_dir` 用于持久化使用频次，传 None 则只在内存中计数。
pub(crate) fn execute(
    kind: &str,
    target: &str,
    title: &str,
    apps: &[AppEntry],
    config_dir: Option<PathBuf>,
) -> Result<(), String> {
    let result = match kind {
        "app" | "shell" => {
            log_buffer::info(format!("⌘SS：打开应用「{}」({})", title, target));
            // 复用 launcher 的 shell 打开逻辑
            let entry = AppEntry {
                uuid: String::new(),
                title: title.to_string(),
                target: if kind == "app" {
                    crate::model::AppTarget::App {
                        path: target.to_string(),
                        args: String::new(),
                        work_dir: String::new(),
                    }
                } else {
                    crate::model::AppTarget::File { path: target.to_string() }
                },
            };
            crate::launcher::open_entry(&entry)?;
            Ok(())
        }
        "library" => {
            log_buffer::info(format!("⌘SS：打开应用库项「{}」(UUID: {})", title, target));
            crate::launcher::open_by_uuid(apps, target)?;
            Ok(())
        }
        "system" => {
            log_buffer::info(format!("⌘SS：执行系统命令「{}」({})", title, target));
            execute_system_command(target)
        }
        "file" | "folder" => {
            log_buffer::info(format!("⌘SS：打开文件「{}」({})", title, target));
            // 用系统默认方式打开文件/文件夹
            let entry = AppEntry {
                uuid: String::new(),
                title: title.to_string(),
                target: crate::model::AppTarget::File { path: target.to_string() },
            };
            crate::launcher::open_entry(&entry)?;
            Ok(())
        }
        // 内置工具不启动任何进程：界面切换完全由前端渲染器接管（见 ss_palette.html 的
        // state.miniapp）。后端到这里只做一件事 —— 记一次使用频次，让「常用工具」
        // 在后续搜索里能凭 usage_bonus 往上浮。
        //
        // target 形如 "miniapps" / "calculator"，或 "calculator:12*34"（`=` 语法带上了
        // 算式）。前端按【第一个】冒号切分：冒号前是渲染器 id，冒号后是可选载荷。
        "tool" => {
            log_buffer::info(format!("⌘SS：打开内置工具「{}」({})", title, target));
            Ok(())
        }
        // 小程序 = 用户集成进 easyop 的**外部**目标（exe / 网址 / 项目目录）。
        // 回车这一下就是要真的把它启动起来，所以这里直接交给 launcher，
        // 行为与在资源管理器里双击那个目标完全一致。
        //
        // target 是 mini_apps 注册表里的 id（uuid），不是路径 —— 路径存在注册表里，
        // 由 `mini_apps::launch` 查出来再交给 launcher。
        "miniapp" => {
            log_buffer::info(format!("⌘SS：启动小程序「{}」({})", title, target));
            crate::mini_apps::launch(target).map(|_| ())
        }
        // 插件命令：由 `ss_execute` 在调 `execute` 之前拦截，走 `plugin_host::activate`。
        // 正常不会走到这里——保留作为安全网，防止 `execute` 被其他路径直接调用时静默成功。
        "plugin" => {
            log_buffer::warn(format!("⌘SS：插件命令「{}」走了 fallback 分支（应由 ss_execute 拦截）", target));
            Err(format!(
                "插件命令「{}」应由 ss_execute 拦截后交给 plugin_host 激活，走到这里说明调用路径异常",
                target
            ))
        }
        // 用系统默认浏览器打开一个 URL。翻译小程序回车时走这条路径。
        "url" => {
            log_buffer::info(format!("⌘SS：打开链接「{}」({})", title, target));
            let entry = AppEntry {
                uuid: String::new(),
                title: title.to_string(),
                target: crate::model::AppTarget::Url { url: target.to_string() },
            };
            crate::launcher::open_entry(&entry)?;
            Ok(())
        }
        _ => Err(format!("未知的结果类型: {}", kind)),
    };

    // 执行成功后记录使用频次
    if result.is_ok() {
        record_usage(kind, target, config_dir);
        crate::usage_log::record("ss_exec", target, "palette", None);
    }

    result
}

/// 执行系统命令。
#[cfg(windows)]
fn execute_system_command(cmd: &str) -> Result<(), String> {
    use std::process::Command;
    match cmd {
        "lock" => {
            Command::new("rundll32.exe")
                .args(["user32.dll,LockWorkStation"])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("锁屏命令执行失败: {}", e))
        }
        "shutdown" => {
            Command::new("shutdown")
                .args(["/s", "/t", "0"])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("关机命令执行失败: {}", e))
        }
        "restart" => {
            Command::new("shutdown")
                .args(["/r", "/t", "0"])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("重启命令执行失败: {}", e))
        }
        "sleep" => {
            // rundll32.exe powrprof.dll,SetSuspendState 0,1,0
            Command::new("rundll32.exe")
                .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("睡眠命令执行失败: {}", e))
        }
        _ => Err(format!("未知系统命令: {}", cmd)),
    }
}

#[cfg(not(windows))]
fn execute_system_command(_cmd: &str) -> Result<(), String> {
    Err("系统命令仅支持 Windows 平台".to_string())
}

// ── 资源管理器右键菜单 ──

/// 把一条搜索结果还原成「可以在资源管理器里右击」的文件系统路径。
///
/// - `file` / `folder` / `app`：`target` 本身就是路径（Everything 命中项、开始菜单
///   扫到的 .exe/.lnk），直接可用。
/// - `library`：`target` 是应用库条目的 UUID，需要回查条目拿真实路径；URL 类条目
///   没有本地路径，返回 None。
/// - `system`：锁屏/关机这类命令没有对应文件，返回 None。
fn menu_path_for(kind: &str, target: &str, apps: &[AppEntry]) -> Option<String> {
    if target.trim().is_empty() {
        return None;
    }
    match kind {
        "library" => apps.iter().find(|e| e.uuid == target).and_then(|e| {
            match &e.target {
                crate::model::AppTarget::App { path, .. } => Some(path.clone()),
                crate::model::AppTarget::File { path } => Some(path.clone()),
                crate::model::AppTarget::Folder { path } => Some(path.clone()),
                crate::model::AppTarget::Url { .. } => None,
            }
        }),
        "system" => None,
        _ => Some(target.to_string()),
    }
}

/// 弹出 Windows 资源管理器的原生右键菜单（与 Everything 右键效果一致）。
///
/// 返回 true 表示用户确实选了菜单里的某一项，false 表示菜单被取消。
///
/// 实现思路就是资源管理器本身的做法：
/// `SHCreateItemFromParsingName` 把路径变成 `IShellItem` → 通过 `BHID_SFUIObject`
/// 拿到 `IContextMenu` → `QueryContextMenu` 把「打开/打开方式/复制/属性…」这些项
/// 填进一个弹出菜单 → `TrackPopupMenuEx` 显示并等用户选择 → `InvokeCommand` 把选择
/// 交回 Shell 执行（所以第三方 Shell 扩展项也能正常工作）。
///
/// 注意：`TrackPopupMenuEx` 是模态的，会一直阻塞到菜单收起；因此必须在主线程调用
/// （Tauri 的同步命令默认就跑在主线程）。`hwnd` 必须是本进程自己的窗口句柄，
/// 否则点击菜单外部时菜单不会自动消失。
pub(crate) fn show_context_menu(
    kind: &str,
    target: &str,
    title: &str,
    apps: &[AppEntry],
    hwnd: isize,
) -> Result<bool, String> {
    let path = menu_path_for(kind, target, apps)
        .ok_or_else(|| format!("「{}」没有关联的文件路径，无法显示右键菜单", title))?;

    #[cfg(windows)]
    {
        show_context_menu_win(&path, hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("右键菜单仅支持 Windows 平台".to_string())
    }
}

#[cfg(windows)]
fn show_context_menu_win(path: &str, hwnd: isize) -> Result<bool, String> {
    use windows::Win32::Foundation::{HWND, POINT, S_OK};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, IBindCtx};
    use windows::Win32::UI::Shell::{
        IContextMenu, IShellItem, SHCreateItemFromParsingName, BHID_SFUIObject, CMINVOKECOMMANDINFO,
        CMF_NORMAL,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenuEx,
        PostMessageA, TPM_LEFTALIGN, TPM_TOPALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        WM_NULL,
    };

    // 菜单项 ID 的分配区间：Shell 会用 idCmdFirst + 偏移量 作为每项的 ID。
    const ID_FIRST: u32 = 1;
    const ID_LAST: u32 = 0x7FFF;

    let owner = HWND(hwnd as *mut core::ffi::c_void);
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: 下面整段都要求 STA；Shell 扩展在 MTA 下行为不确定。
    // CoInitializeEx 返回 S_FALSE（已按同模式初始化）或 RPC_E_CHANGED_MODE（已被
    // 别人改成 MTA）都不是致命错误，只有 S_OK 才由我们负责 CoUninitialize。
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owns_com = hr == S_OK;

        let outcome = (|| -> Result<bool, String> {
            let item: IShellItem =
                SHCreateItemFromParsingName(windows::core::PCWSTR(wide.as_ptr()), None::<&IBindCtx>)
                    .map_err(|e| format!("无法解析路径「{}」：{}", path, e))?;

            let ctx: IContextMenu = item
                .BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)
                .map_err(|e| format!("无法获取右键菜单接口：{}", e))?;

            let menu = CreatePopupMenu().map_err(|e| format!("创建弹出菜单失败：{}", e))?;

            // Shell 负责把菜单项填进去；失败时菜单是空的，直接当取消处理。
            let filled = ctx.QueryContextMenu(menu, 0, ID_FIRST, ID_LAST, CMF_NORMAL).is_ok();

            let mut chosen = 0u32;
            if filled {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);

                // 弹出前先把面板设为前台窗口：这是 TrackPopupMenuEx 的硬性要求，
                // 否则菜单不会因为「点到菜单外面」而收起，会一直挂在屏幕上。
                let _ = SetForegroundWindow(owner);

                let cmd = TrackPopupMenuEx(
                    menu,
                    (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_TOPALIGN).0,
                    pt.x,
                    pt.y,
                    owner,
                    None,
                );
                // 微软官方推荐：TrackPopupMenuEx 返回后给属主窗口发一条 WM_NULL，
                // 防止在某些系统版本上菜单收起后仍"挂住"鼠标捕获的问题。
                use windows::Win32::Foundation::{LPARAM, WPARAM};
                let _ = PostMessageA(owner, WM_NULL, WPARAM(0), LPARAM(0));
                // 返回的是菜单项 ID（不是偏移量），0 表示用户按 ESC / 点了别处取消。
                let id = cmd.0 as u32;
                if cmd.0 != 0 && (ID_FIRST..=ID_LAST).contains(&id) {
                    chosen = id;
                }
            }

            if chosen != 0 {
                // CMINVOKECOMMANDINFO.lpVerb 传「偏移量」而不是完整 ID，
                // 这种低位当指针用的写法就是 Win32 的 MAKEINTRESOURCE 约定。
                let offset = (chosen - ID_FIRST) as usize;
                let info = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    fMask: 0,
                    hwnd: owner,
                    lpVerb: windows::core::PCSTR(offset as *const u8),
                    ..Default::default()
                };
                ctx.InvokeCommand(&info)
                    .map_err(|e| format!("执行菜单命令失败：{}", e))?;
            }

            let _ = DestroyMenu(menu);
            Ok(chosen != 0)
        })();

        if owns_com {
            CoUninitialize();
        }

        match &outcome {
            Ok(true) => log_buffer::info(format!("⌘SS：右键菜单执行了动作「{}」", path)),
            Ok(false) => log_buffer::info(format!("⌘SS：右键菜单已取消「{}」", path)),
            Err(err) => log_buffer::warn(format!("⌘SS：右键菜单失败（{}）：{}", path, err)),
        }
        outcome
    }
}

// ── 精简右键菜单：各动作的执行函数 ──
//
// 当用户开启「精简右键菜单」时，前端不再调 ss_show_context_menu 弹出原生 Shell
// 菜单，而是显示一个自绘的 8 项精简菜单，每项点击后调下面这些 Rust 命令来执行。
// 这样菜单更干净，和 Everything 的右键体验一致；代价是没有第三方 Shell 扩展。

/// 执行指定的 Shell 动词（open / openas / properties / print 等）。
#[cfg(windows)]
pub(crate) fn shell_verb(verb: &str, target: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if target.trim().is_empty() {
        return Err("目标路径不能为空".into());
    }
    let target_wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let verb_wide: Vec<u16> = verb.encode_utf16().chain(std::iter::once(0)).collect();

    // 工作目录 = 文件所在目录
    let work_dir_wide: Vec<u16> = std::path::Path::new(target)
        .parent()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            if s.is_empty() {
                target.encode_utf16().chain(std::iter::once(0)).collect()
            } else {
                s.encode_utf16().chain(std::iter::once(0)).collect()
            }
        })
        .unwrap_or_default();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        hwnd: windows::Win32::Foundation::HWND(std::ptr::null_mut()),
        lpVerb: PCWSTR(verb_wide.as_ptr()),
        lpFile: PCWSTR(target_wide.as_ptr()),
        lpParameters: PCWSTR::null(),
        lpDirectory: if work_dir_wide.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(work_dir_wide.as_ptr())
        },
        nShow: SW_SHOWNORMAL.0 as i32,
        ..Default::default()
    };

    let result = unsafe { ShellExecuteExW(&mut sei) };
    if result.is_err() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        let code = err.0 as i32;
        return Err(format!("执行动词「{verb}」失败（错误码 {code}）"));
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn shell_verb(_verb: &str, _target: &str) -> Result<(), String> {
    Err("精简右键菜单仅支持 Windows".into())
}

/// 把文件路径复制到剪贴板。
pub(crate) fn copy_path_to_clipboard(path: &str) -> Result<(), String> {
    crate::log_buffer::info(format!("⌘SS：复制路径到剪贴板「{}」", path));
    #[cfg(windows)]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("clip.exe")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 clip.exe：{e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(path.as_bytes())
                .map_err(|e| format!("写入剪贴板失败：{e}"))?;
        }
        // 等待 clip.exe 处理完
        let _ = child.wait();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("复制路径仅支持 Windows".into())
    }
}

/// 打开文件所在的文件夹，并在资源管理器中选中该文件。
#[cfg(windows)]
pub(crate) fn open_containing_folder(path: &str) -> Result<(), String> {
    use std::process::Command;
    // explorer /select, "path" 会打开资源管理器并定位到指定文件
    let status = Command::new("explorer.exe")
        .arg("/select,")
        .arg(path)
        .spawn()
        .map_err(|e| format!("无法打开所在文件夹：{e}"))?;
    let _ = status; // 不需要等待
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn open_containing_folder(_path: &str) -> Result<(), String> {
    Err("打开所在文件夹仅支持 Windows".into())
}

// ── 测试 ──
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinyin::initials_of;

    #[test]
    fn test_prefix_match_ascii() {
        let result = prefix_match_indices("visual studio code", "visual");
        assert!(result.is_some());
        let indices = result.unwrap();
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5]); // "visual" 6 个字符
    }

    #[test]
    fn test_prefix_match_chinese() {
        let result = prefix_match_indices("微信", "微");
        assert!(result.is_some());
        let indices = result.unwrap();
        assert_eq!(indices, vec![0]); // 1 个汉字
    }

    #[test]
    fn test_contains_match() {
        let result = contains_match_indices("visual studio code", "studio");
        assert!(result.is_some());
        let indices = result.unwrap();
        // "visual " 是 7 个字符，所以 "studio" 从第 7 个字符开始
        assert_eq!(indices, (7..7 + 6).collect::<Vec<_>>());
    }

    #[test]
    fn test_contains_match_chinese() {
        let result = contains_match_indices("谷歌浏览器", "浏览");
        assert!(result.is_some());
        let indices = result.unwrap();
        assert_eq!(indices, vec![2, 3]); // "谷歌" 2 个字后是 "浏览"
    }

    #[test]
    fn test_pinyin_initials_weixin() {
        let initials = initials_of("微信");
        assert_eq!(initials, "wx");
        let result = pinyin_initials_match(&initials, "w", "微信");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), vec![0]);
    }

    #[test]
    fn test_pinyin_initials_full() {
        let initials = initials_of("微信");
        let result = pinyin_initials_match(&initials, "wx", "微信");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), vec![0, 1]);
    }

    #[test]
    fn test_pinyin_initials_no_match() {
        let initials = initials_of("微信");
        let result = pinyin_initials_match(&initials, "qq", "微信");
        assert!(result.is_none());
    }

    #[test]
    fn test_abbrev_match() {
        let abbrev = crate::pinyin::abbrev_of("Visual Studio Code");
        assert_eq!(abbrev, "vsc");
        let result = abbrev_match(&abbrev, "vs", "Visual Studio Code");
        assert!(result.is_some());
    }
}
