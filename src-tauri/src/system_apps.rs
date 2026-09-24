//! 枚举本机已注册/可见的应用（供「打开应用」快速选择）。
//!
//! 来源优先级（从高到低）：
//!   1. 注册表 App Paths（HKLM + HKCU）— 系统注册的可执行文件
//!   2. 开始菜单 Programs 目录 — 用户可见的快捷方式
//!   3. 常见安装目录一级扫描 — Program Files / AppData / 桌面
//!
//! 每个应用项包含：名称、路径、类型（app/shell）、分类（浏览器/开发/办公/...）。
//!
//! 性能：结果缓存 1 小时 + 检索键预计算，避免每次搜索都全盘扫描。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ── 缓存 ──

/// 带预计算检索键的应用项。
///
/// 检索键（小写名 / 拼音首字母 / 单词缩写）在**扫描时一次性算好**并随缓存复用，
/// 搜索路径上不再逐项重算 —— 这是「首次搜索卡顿」的核心修复点之一：
/// 800 项 × 每次搜索重新跑 `initials_of`，成本远超实际匹配本身。
pub struct IndexedApp {
    pub app: SystemApp,
    pub name_lower: String,
    pub pinyin_initials: String,
    pub abbrev: String,
}

impl IndexedApp {
    fn build(app: SystemApp) -> Self {
        IndexedApp {
            name_lower: app.name.to_lowercase(),
            pinyin_initials: crate::pinyin::initials_of(&app.name),
            abbrev: crate::pinyin::abbrev_of(&app.name),
            app,
        }
    }
}

struct AppCache {
    index: Arc<Vec<IndexedApp>>,
    scanned_at: Instant,
}

const CACHE_TTL_SECS: u64 = 3600; // 1 小时

static APP_CACHE: OnceLock<Mutex<Option<AppCache>>> = OnceLock::new();

/// 构建锁：保证同一时刻只有一次全盘扫描在跑。
///
/// 后台预热线程与「用户第一次搜索」很可能几乎同时到达，若不串行化，
/// 两边会各扫一遍磁盘 + 各算一遍拼音，反而让首次搜索更慢。
static BUILD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<AppCache>> {
    APP_CACHE.get_or_init(|| Mutex::new(None))
}

fn build_lock() -> &'static Mutex<()> {
    BUILD_LOCK.get_or_init(|| Mutex::new(()))
}

/// 一个被枚举到的本机应用项。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SystemApp {
    /// 展示名（去掉扩展名的文件名 / 注册表显示名）。
    pub name: String,
    /// 可直接交给启动器的路径（可能是 .exe / .lnk / .url）。
    pub path: String,
    /// "app" = 直接启动；"shell" = 用系统默认方式打开（.lnk/.url）。
    pub kind: String,
    /// 应用分类：browser / dev / office / communication / media / game / utility / other
    #[serde(default)]
    pub category: String,
    /// 来源：registry / startmenu / commondir
    #[serde(default)]
    pub source: String,
}

/// 取带检索键的应用索引（`Arc` 共享，不克隆 800 项）。
///
/// 搜索路径应当调用这个函数；命中缓存时只做一次 `Arc::clone`，成本可忽略。
pub fn system_app_index() -> Arc<Vec<IndexedApp>> {
    // 快路径：缓存有效时只做一次 Arc::clone。
    if let Some(idx) = cached_index() {
        return idx;
    }

    // 慢路径：串行化。拿不到锁说明另一条线程正在扫描，
    // 它会阻塞在这里等锁释放，然后走下面的二次检查直接复用结果，
    // 而不是再扫一遍磁盘。
    let guard = build_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(idx) = cached_index() {
        drop(guard);
        return idx;
    }

    let idx = build_index();
    drop(guard);
    idx
}

/// 命中有效缓存则返回共享索引，否则 `None`。
fn cached_index() -> Option<Arc<Vec<IndexedApp>>> {
    let guard = cache().lock().ok()?;
    let c = guard.as_ref()?;
    if c.scanned_at.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
        Some(c.index.clone())
    } else {
        None
    }
}

/// 全量扫描 + 构建索引，并写入缓存。带耗时日志。
fn build_index() -> Arc<Vec<IndexedApp>> {
    let t0 = Instant::now();
    let apps = scan_all_apps();
    let count = apps.len();
    let index = Arc::new(apps.into_iter().map(IndexedApp::build).collect::<Vec<_>>());

    if let Ok(mut guard) = cache().lock() {
        *guard = Some(AppCache {
            index: index.clone(),
            scanned_at: Instant::now(),
        });
    }

    crate::log_buffer::info(format!(
        "应用索引：扫描完成 {} 项，耗时 {}ms（检索键已预计算）",
        count,
        t0.elapsed().as_millis()
    ));
    index
}

/// 枚举本机所有可启动应用（兼容既有调用方；内部走共享索引）。
pub fn list_system_apps() -> Vec<SystemApp> {
    system_app_index()
        .iter()
        .map(|i| i.app.clone())
        .collect()
}

/// 强制刷新应用缓存（例如用户主动触发重新扫描）。
/// 同样走构建锁，避免与后台预热/搜索并发扫描。
pub fn refresh_app_cache() -> Vec<SystemApp> {
    let guard = build_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let index = build_index();
    drop(guard);
    index.iter().map(|i| i.app.clone()).collect()
}

/// 预热应用索引：后台线程提前完成扫描 + 检索键预计算。
///
/// 目的是把「首次搜索」的扫描成本挪到应用启动阶段，
/// 用户第一次敲字时搜索路径上只剩匹配计算。已有有效缓存时直接返回。
pub fn warm_up() {
    if cached_index().is_some() {
        crate::log_buffer::info("应用索引：已有有效缓存，跳过预热");
        return;
    }
    crate::log_buffer::info("应用索引：后台预热已启动");
    std::thread::spawn(|| {
        let idx = system_app_index();
        crate::log_buffer::info(format!("应用索引：后台预热完成，{} 项可检索", idx.len()));
    });
}

// ── 总扫描入口 ──

fn scan_all_apps() -> Vec<SystemApp> {
    let mut seen = HashSet::new();
    let mut out: Vec<SystemApp> = Vec::new();

    // 1. 注册表 App Paths（质量最高，优先加入）
    scan_registry_apps(&mut seen, &mut out);

    // 2. 开始菜单（用户最熟悉的来源）
    scan_start_menu(&mut seen, &mut out);

    // 3. 常见安装目录（补充遗漏的绿色软件）
    scan_common_dirs(&mut seen, &mut out);

    // 排序：名称升序，保证稳定输出
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));

    // 控制总量
    out.truncate(800);
    out
}

// ── 1. 注册表 App Paths 扫描 ──
#[cfg(windows)]
fn scan_registry_apps(seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>) {
    use winreg::enums::*;
    use winreg::RegKey;

    const APP_PATHS_PATH: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths";

    let hives = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER];

    for &hive in &hives {
        let Ok(app_paths_key) = RegKey::predef(hive).open_subkey(APP_PATHS_PATH) else {
            continue;
        };

        let Ok(subkeys) = app_paths_key.enum_keys().collect::<Result<Vec<_>, _>>() else {
            continue;
        };

        for subkey_name in subkeys {
            // 子键名就是类似 "chrome.exe" 的文件名
            let exe_name = subkey_name.clone();

            let Ok(subkey) = app_paths_key.open_subkey(&subkey_name) else {
                continue;
            };

            // 读取默认值（空字符串值名）
            let path_str: Result<String, _> = subkey.get_value("");
            let Ok(path_str) = path_str else {
                continue;
            };

            if path_str.is_empty() {
                continue;
            }

            // 处理环境变量展开（有些路径用 %ProgramFiles% 等）
            let path_str = expand_env_vars(&path_str);

            let path_buf = PathBuf::from(&path_str);
            if !seen.insert(path_buf.clone()) {
                continue;
            }

            // 展示名：去掉 .exe 后缀
            let display_name = exe_name
                .strip_suffix(".exe")
                .unwrap_or(&exe_name)
                .strip_suffix(".EXE")
                .unwrap_or(&exe_name)
                .to_string();

            if display_name.trim().is_empty() {
                continue;
            }

            // 过滤卸载/安装项
            let lower = display_name.to_ascii_lowercase();
            if lower.ends_with("uninstall")
                || lower.contains("卸载")
                || lower.contains("setup")
                || lower.starts_with("unins")
            {
                continue;
            }

            let category = categorize_app(&display_name, &path_str);

            out.push(SystemApp {
                name: display_name,
                path: path_str,
                kind: "app".to_string(),
                category,
                source: "registry".to_string(),
            });
        }
    }
}

/// 展开路径中的环境变量（如 %ProgramFiles% → C:\Program Files）。
#[cfg(windows)]
fn expand_env_vars(path: &str) -> String {
    let mut result = path.to_string();
    // 简单处理几个常见的
    if result.contains('%') {
        // 用 Windows API 展开太复杂，这里手动替换常见的
        if let Ok(pf) = std::env::var("ProgramFiles") {
            result = result.replace("%ProgramFiles%", &pf);
            result = result.replace("%programfiles%", &pf);
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            result = result.replace("%ProgramFiles(x86)%", &pf86);
            result = result.replace("%ProgramFiles% (x86)", &pf86);
        }
        if let Ok(ad) = std::env::var("APPDATA") {
            result = result.replace("%APPDATA%", &ad);
            result = result.replace("%AppData%", &ad);
        }
        if let Ok(ld) = std::env::var("LOCALAPPDATA") {
            result = result.replace("%LOCALAPPDATA%", &ld);
            result = result.replace("%LocalAppData%", &ld);
        }
        if let Ok(up) = std::env::var("USERPROFILE") {
            result = result.replace("%USERPROFILE%", &up);
            result = result.replace("%UserProfile%", &up);
        }
        if let Ok(sys) = std::env::var("SystemRoot") {
            result = result.replace("%SystemRoot%", &sys);
            result = result.replace("%systemroot%", &sys);
        }
    }
    result
}

#[cfg(not(windows))]
fn scan_registry_apps(_seen: &mut HashSet<PathBuf>, _out: &mut Vec<SystemApp>) {}

// ── 2. 开始菜单扫描 ──

fn scan_start_menu(seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>) {
    for root in start_menu_roots() {
        scan_dir_recursive(&root, seen, out, "startmenu");
    }
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    // 所有用户： %ProgramData%\Microsoft\Windows\Start Menu\Programs
    if let Ok(pd) = std::env::var("ProgramData") {
        if !pd.trim().is_empty() {
            let p = PathBuf::from(pd)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            if p.is_dir() {
                roots.push(p);
            }
        }
    }

    // 当前用户： %APPDATA%\Microsoft\Windows\Start Menu\Programs
    if let Ok(ad) = std::env::var("APPDATA") {
        if !ad.trim().is_empty() {
            let p = PathBuf::from(ad)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            if p.is_dir() {
                roots.push(p);
            }
        }
    }

    roots
}

// ── 3. 常见安装目录扫描 ──

fn scan_common_dirs(seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>) {
    let dirs = common_install_dirs();
    for dir in dirs {
        // 只扫描一级子目录里的 .exe（避免递归太深）
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 在子目录里找 .exe（再深一层）
                scan_dir_for_exes(&path, seen, out, 1);
            } else if path.is_file() {
                add_if_exe(&path, seen, out, "commondir");
            }
        }
    }
}

fn common_install_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Program Files
    if let Ok(pf) = std::env::var("ProgramFiles") {
        if !pf.trim().is_empty() {
            let p = PathBuf::from(pf);
            if p.is_dir() { dirs.push(p); }
        }
    }
    // Program Files (x86)
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        if !pf86.trim().is_empty() {
            let p = PathBuf::from(pf86);
            if p.is_dir() { dirs.push(p); }
        }
    }
    // AppData\Local
    if let Ok(ld) = std::env::var("LOCALAPPDATA") {
        if !ld.trim().is_empty() {
            let p = PathBuf::from(&ld);
            if p.is_dir() {
                dirs.push(p.clone());
            }
            // AppData\Local\Programs
            let pp = p.join("Programs");
            if pp.is_dir() { dirs.push(pp); }
        }
    }
    // 桌面
    if let Ok(ud) = std::env::var("USERPROFILE") {
        if !ud.trim().is_empty() {
            let desktop = PathBuf::from(&ud).join("Desktop");
            if desktop.is_dir() { dirs.push(desktop); }
            let public = PathBuf::from(&ud).join("Public").join("Desktop");
            if public.is_dir() { dirs.push(public); }
        }
    }

    dirs
}

/// 在目录中扫描 .exe（最多 depth 层）。
fn scan_dir_for_exes(dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>, depth: u32) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth > 0 {
            scan_dir_for_exes(&path, seen, out, depth - 1);
        } else if path.is_file() {
            add_if_exe(&path, seen, out, "commondir");
        }
    }
}

fn add_if_exe(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>, source: &str) {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return };
    if ext.to_ascii_lowercase() != "exe" {
        return;
    }
    if !seen.insert(path.to_path_buf()) {
        return;
    }
    let Some(name) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else { return };
    if name.trim().is_empty() {
        return;
    }
    // 过滤明显的非应用
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("uninstall")
        || lower.contains("卸载")
        || lower.contains("setup")
        || lower.contains("installer")
        || lower.ends_with("update")
        || lower.starts_with("unins")
        || lower.contains("crash")
        || lower.contains("debug")
        || lower.contains("helper") && lower != "helper"
    {
        return;
    }
    // 过滤太小的文件（可能是卸载器/补丁）
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() < 50 * 1024 { // < 50KB
            return;
        }
    }
    let path_str = path.to_string_lossy().to_string();
    let category = categorize_app(&name, &path_str);
    out.push(SystemApp {
        name,
        path: path_str,
        kind: "app".to_string(),
        category,
        source: source.to_string(),
    });
}

// ── 通用目录扫描（开始菜单用） ──

fn scan_dir_recursive(dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>, source: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, seen, out, source);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
        let ext_l = ext.to_ascii_lowercase();
        let kind = match ext_l.as_str() {
            "exe" => "app",
            "lnk" | "url" => "shell",
            _ => continue,
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("uninstall")
            || lower.contains("卸载")
            || lower.contains("help")
            || lower.contains("documentation")
        {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let category = categorize_app(&name, &path_str);
        out.push(SystemApp {
            name,
            path: path_str,
            kind: kind.to_string(),
            category,
            source: source.to_string(),
        });
    }
}

// ── 应用分类 ──

/// 根据应用名和路径判断分类。
/// 返回：browser / dev / office / communication / media / game / utility / other
fn categorize_app(name: &str, path: &str) -> String {
    let name_lower = name.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();

    // 浏览器
    const BROWSER_KEYWORDS: &[&str] = &[
        "chrome", "firefox", "edge", "safari", "opera", "brave", "vivaldi",
        "thorium", "centbrowser", "360chrome", "qqbrowser", "搜狗高速浏览器",
        "browser", "arc", "orca",
    ];
    for kw in BROWSER_KEYWORDS {
        if name_lower.contains(kw) {
            return "browser".to_string();
        }
    }

    // 开发工具
    const DEV_KEYWORDS: &[&str] = &[
        "code", "vscode", "visual studio", "intellij", "pycharm", "webstorm",
        "goland", "clion", "rider", "idea", "eclipse", "sublime", "atom",
        "notepad++", "notepadplus", "vim", "neovim", "nvim", "emacs",
        "git", "tortoisegit", "sourcetree", "fork", "postman", "insomnia",
        "docker", "vmware", "virtualbox", "winscp", "filezilla", "putty",
        "xshell", "mobaxterm", "termius", "navicat", "datagrip", "dbeaver",
        "workbench", "studio 3t", "robo 3t", "redis", "mongo", "node",
        "python", "java", "go ", "golang", "rust", "cargo",
    ];
    for kw in DEV_KEYWORDS {
        if name_lower.contains(kw) {
            return "dev".to_string();
        }
    }
    // 路径包含开发相关目录
    if path_lower.contains("jetbrains") || path_lower.contains("microsoft vs code") {
        return "dev".to_string();
    }

    // 办公
    const OFFICE_KEYWORDS: &[&str] = &[
        "word", "excel", "powerpoint", "outlook", "onenote", "access",
        "visio", "project", "wps", "office", "adobe acrobat", "acrobat",
        "foxit", "福昕", "wps 文字", "wps 表格", "wps 演示",
        "notion", "obsidian", "typora", "语雀", "飞书文档",
    ];
    for kw in OFFICE_KEYWORDS {
        if name_lower.contains(kw) {
            return "office".to_string();
        }
    }

    // 通讯
    const COMM_KEYWORDS: &[&str] = &[
        "wechat", "微信", "weixin", "qq", "tim", "dingtalk", "钉钉",
        "feishu", "飞书", "lark", "skype", "teams", "zoom", "slack",
        "discord", "telegram", "whatsapp", "signal", "thunderbird",
        "outlook", "foxmail", "网易邮箱",
    ];
    for kw in COMM_KEYWORDS {
        if name_lower.contains(kw) {
            return "communication".to_string();
        }
    }

    // 媒体 / 影音
    const MEDIA_KEYWORDS: &[&str] = &[
        "vlc", "potplayer", "mpv", "kmplayer", "暴风", "迅雷看看",
        "爱奇艺", "腾讯视频", "优酷", "bilibili", "b站",
        "spotify", "网易云音乐", "qq音乐", "qqmusic", "酷狗", "酷我",
        "audacity", "premiere", "after effects", "photoshop", "ps ",
        "illustrator", "figma", "sketch", "xd", "blender",
        "剪映", "capcut", "obs", "handbrake", "格式工厂",
    ];
    for kw in MEDIA_KEYWORDS {
        if name_lower.contains(kw) {
            return "media".to_string();
        }
    }

    // 游戏
    const GAME_KEYWORDS: &[&str] = &[
        "steam", "epic", "origin", "uplay", "battlenet", "战网",
        "wegame", "qq游戏", "game", "原神", "minecraft", "lol",
        "英雄联盟", "王者", "和平精英",
    ];
    for kw in GAME_KEYWORDS {
        if name_lower.contains(kw) {
            return "game".to_string();
        }
    }

    // 工具 / 系统
    const UTILITY_KEYWORDS: &[&str] = &[
        "7-zip", "7zip", "winrar", "bandizip", "压缩",
        "everything", "listary", "utools", "powertoys",
        "ccleaner", "geek", "revo", "uninstall",
        "任务管理器", "资源管理器", "explorer",
        "cmd", "powershell", "terminal", "windows terminal",
        "计算器", "calculator", "notepad", "记事本",
        "画图", "paint", "截图", "snipping",
        "设置", "settings", "control", "控制面板",
    ];
    for kw in UTILITY_KEYWORDS {
        if name_lower.contains(kw) {
            return "utility".to_string();
        }
    }

    "other".to_string()
}
