//! 小程序 —— 把**外部** App / Web 项目集成进 easyop。
//!
//! 这里说的「小程序」不是 easyop 自带的工具界面（计算器之类），而是
//! **外部应用的集成入口**：用户把本机的 exe、一个 Web 项目的网址、或者项目目录
//! 登记进来，easyop 在 ⌘SS 面板的「小程序」窗口里为它留一个位置，点一下就能启动。
//! 本质上是把别的应用收进 easyop 自己的启动器。
//!
//! 三种集成目标（`kind`）：
//!   - `app`    本机可执行文件（在面板里点一下 = 启动这个程序）
//!   - `url`    Web 项目 / 网址（点一下 = 用默认浏览器打开）
//!   - `folder` 本地项目目录（点一下 = 打开这个目录）
//!
//! 注册表持久化在配置目录 `mini_apps.json`（一个 `MiniApp` 数组）。
//! 模块内用全局 `Vec<MiniApp>` 缓存，搜索路径上直接读缓存，不碰磁盘。

use crate::log_buffer;
use crate::model::{AppEntry, AppTarget};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 注册表文件名（位于配置目录）。
const FILE_NAME: &str = "mini_apps.json";

/// 允许的集成类型。
pub const KINDS: [&str; 3] = ["app", "url", "folder"];

/// 一条集成进来的外部应用。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MiniApp {
    /// 唯一 ID（uuid v4），同时作为搜索结果里 `run:<id>` 的载荷。
    pub id: String,
    /// 展示名（面板里显示的名字，也是搜索关键词来源）。
    pub name: String,
    /// 集成类型：app / url / folder。
    pub kind: String,
    /// 启动目标：exe 路径 / 网址 / 目录路径。
    pub target: String,
    /// 启动参数（仅 `kind = "app"` 有意义）。
    #[serde(default)]
    pub args: String,
    /// 一句话说明，显示在名字下方。
    #[serde(default)]
    pub note: String,
    /// 来源标记：manual = 用户手动添加。
    #[serde(default)]
    pub source: String,
    /// 添加时间（Unix 秒）。
    #[serde(default)]
    pub created_at: u64,
}

static STORE: OnceLock<Mutex<Vec<MiniApp>>> = OnceLock::new();
static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

fn store() -> &'static Mutex<Vec<MiniApp>> {
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

fn registry_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// 启动时加载注册表；返回加载到的条目数。
pub fn init(dir: &Path) -> usize {
    let _ = CONFIG_DIR.set(dir.to_path_buf());
    let list = read_from(dir);
    let count = list.len();
    if let Ok(mut guard) = store().lock() {
        *guard = list;
    }
    log_buffer::info(format!("小程序：已加载 {count} 个集成应用"));
    count
}

fn read_from(dir: &Path) -> Vec<MiniApp> {
    let path = registry_path(dir);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<MiniApp>>(&data) {
        Ok(list) => list
            .into_iter()
            .filter(|m| !m.id.is_empty() && !m.target.trim().is_empty())
            .collect(),
        Err(e) => {
            log_buffer::warn(format!("小程序：注册表解析失败，按空处理：{e}"));
            Vec::new()
        }
    }
}

/// 把当前内存注册表写回磁盘。
fn persist() -> Result<(), String> {
    let Some(dir) = CONFIG_DIR.get() else {
        return Err("配置目录尚未初始化".into());
    };
    let list = list();
    std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
    let json = serde_json::to_string_pretty(&list).map_err(|e| format!("序列化失败：{e}"))?;
    std::fs::write(registry_path(dir), json).map_err(|e| format!("写入注册表失败：{e}"))
}

/// 当前已集成的全部小程序（按加入时间升序）。
pub fn list() -> Vec<MiniApp> {
    store()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// 集成一个新的外部应用。
///
/// `name` 留空时从 target 推导；`kind = "url"` 会自动补 `https://`。
pub fn add(
    name: &str,
    kind: &str,
    target: &str,
    args: &str,
    note: &str,
) -> Result<MiniApp, String> {
    let kind = kind.trim().to_lowercase();
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("不支持的集成类型「{kind}」，只支持 app / url / folder"));
    }

    let mut target = target.trim().trim_matches('"').trim().to_string();
    if target.is_empty() {
        return Err("启动目标不能为空".into());
    }
    // Web 项目常被写成 example.com 这种裸域名，补上协议头更省事。
    if kind == "url" && !target.contains("://") {
        target = format!("https://{target}");
    }

    let name = {
        let n = name.trim();
        if n.is_empty() {
            derive_name(&kind, &target)
        } else {
            n.to_string()
        }
    };

    // 去重：同一个目标不重复集成（路径/网址大小写不敏感）。
    if let Ok(guard) = store().lock() {
        if guard
            .iter()
            .any(|m| m.kind == kind && m.target.eq_ignore_ascii_case(&target))
        {
            return Err(format!("「{name}」已经在小程序里了"));
        }
    }

    let item = MiniApp {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        kind,
        target,
        args: args.trim().to_string(),
        note: note.trim().to_string(),
        source: "manual".into(),
        created_at: now_secs(),
    };

    if let Ok(mut guard) = store().lock() {
        guard.push(item.clone());
    }
    persist()?;
    log_buffer::info(format!(
        "小程序：已集成「{}」kind={} target={}",
        item.name, item.kind, item.target
    ));
    Ok(item)
}

/// 移除一个小程序；返回是否真的删掉了。
pub fn remove(id: &str) -> Result<bool, String> {
    let removed = {
        let mut guard = store().lock().map_err(|_| "注册表被占用".to_string())?;
        let before = guard.len();
        guard.retain(|m| m.id != id);
        before != guard.len()
    };
    if removed {
        persist()?;
        log_buffer::info(format!("小程序：已移除 {id}"));
    }
    Ok(removed)
}

/// 启动一个小程序（把集成目标交给 launcher 打开）。
pub fn launch(id: &str) -> Result<String, String> {
    let item = store()
        .lock()
        .ok()
        .and_then(|g| g.iter().find(|m| m.id == id).cloned())
        .ok_or_else(|| format!("未找到小程序「{id}」，可能已被移除"))?;

    log_buffer::info(format!(
        "小程序：启动「{}」（{} → {}）",
        item.name, item.kind, item.target
    ));

    let entry = AppEntry {
        uuid: item.id.clone(),
        title: item.name.clone(),
        target: match item.kind.as_str() {
            "app" => AppTarget::App {
                path: item.target.clone(),
                args: item.args.clone(),
                work_dir: String::new(),
            },
            "url" => AppTarget::Url {
                url: item.target.clone(),
            },
            _ => AppTarget::Folder {
                path: item.target.clone(),
            },
        },
    };
    crate::launcher::open_entry(&entry)
}

/// 搜索命中的一条小程序（供 ⌘SS 搜索结果使用）。
pub struct MiniAppHit {
    pub app: MiniApp,
    pub score: f32,
    pub match_indices: Vec<usize>,
    pub match_mode: &'static str,
}

/// 按关键词匹配已集成的小程序（名字前缀 → 包含 → 拼音首字母）。
pub fn search_hits(query_lower: &str) -> Vec<MiniAppHit> {
    if query_lower.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for app in list() {
        let name_lower = app.name.to_lowercase();
        let initials = crate::pinyin::initials_of(&app.name);

        if let Some(indices) = crate::ss_palette::prefix_match_indices_pub(&name_lower, query_lower) {
            hits.push(MiniAppHit {
                app,
                score: 88.0,
                match_indices: indices,
                match_mode: "prefix",
            });
            continue;
        }
        if let Some(indices) = crate::ss_palette::contains_match_indices_pub(&name_lower, query_lower) {
            hits.push(MiniAppHit {
                app,
                score: 62.0,
                match_indices: indices,
                match_mode: "contains",
            });
            continue;
        }
        if let Some(indices) =
            crate::ss_palette::pinyin_initials_match_pub(&initials, query_lower)
        {
            hits.push(MiniAppHit {
                app,
                score: 55.0,
                match_indices: indices,
                match_mode: "pinyin",
            });
        }
    }
    hits
}

/// 依据关键词在本机应用索引里找候选（供「+ 添加」时挑选目标）。
pub fn suggest_system(query: &str, limit: usize) -> Vec<crate::system_apps::SystemApp> {
    let q = query.trim().to_lowercase();
    let index = crate::system_apps::system_app_index();
    let mut out = Vec::new();
    for item in index.iter() {
        if q.is_empty()
            || item.name_lower.contains(&q)
            || item.pinyin_initials.starts_with(&q)
        {
            out.push(item.app.clone());
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn derive_name(kind: &str, target: &str) -> String {
    if kind == "url" {
        // https://example.com/a/b → example.com
        return target
            .split("://")
            .nth(1)
            .unwrap_or(target)
            .split('/')
            .next()
            .unwrap_or(target)
            .to_string();
    }
    Path::new(target)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| target.to_string())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_name_from_target() {
        assert_eq!(derive_name("url", "https://example.com/a/b"), "example.com");
        assert_eq!(derive_name("app", "C:\\tools\\figma.exe"), "figma");
        assert_eq!(derive_name("folder", "E:\\work\\proj"), "proj");
    }
}
