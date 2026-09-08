//! 枚举本机已注册/可见的应用（供「打开应用」快速选择）。
//!
//! 来源：Windows「开始菜单」的 Programs 目录（所有用户 + 当前用户），递归收集
//! `.lnk` 快捷方式、`.url` 网址快捷方式与 `.exe` 可执行文件。这些正是用户在
//! 开始菜单里能看到的、可被点击启动的项，因此是「本机已注册应用」的可靠来源。
//!
//! 返回的每一项都给出了一个「可直接交给启动器」的路径：
//!   - `.exe`        -> kind = "app"      （运行时直接用 `Command::new` 启动）
//!   - `.lnk`/`.url` -> kind = "shell"    （运行时交给 `cmd /C start` 以系统方式打开；
//!                                         .lnk 内嵌了它自身的目标与参数，无需额外解析）
//!
//! 不做注册表/商店枚举——开始菜单覆盖绝大多数用户可见应用，且无需额外依赖，稳定可靠。

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 一个被枚举到的本机应用项。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SystemApp {
    /// 展示名（去掉扩展名的文件名）。
    pub name: String,
    /// 可直接交给启动器的路径（可能是 .exe / .lnk / .url）。
    pub path: String,
    /// "app" = 直接启动；"shell" = 用系统默认方式打开（.lnk/.url）。
    pub kind: String,
}

/// 枚举开始菜单中所有可启动项，按名称排序并去重。
pub fn list_system_apps() -> Vec<SystemApp> {
    let mut seen = HashSet::new();
    let mut out: Vec<SystemApp> = Vec::new();

    for root in start_menu_roots() {
        scan_dir(&root, &mut seen, &mut out);
    }

    // 按名称排序；同名按路径，保证输出稳定。
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    // 遏制极端体积，避免一次性撑爆前端。
    out.truncate(600);
    out
}

/// 计算开始菜单的根目录（所有用户 + 当前用户），跳过不存在的。
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

/// 递归扫描一个目录，收集可启动项。
fn scan_dir(dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<SystemApp>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, seen, out);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext_l = ext.to_ascii_lowercase();
        let kind = match ext_l.as_str() {
            "exe" => "app",
            "lnk" | "url" => "shell",
            _ => continue,
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        // 过滤掉明显的卸载/帮助项，让列表更贴近「真正想打开的应用」。
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
        out.push(SystemApp { name, path: path_str, kind: kind.to_string() });
    }
}
