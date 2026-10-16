//! 插件注册表（plugin registry）—— easyop 扩展机制的「发现层」。
//!
//! 插件 = 放在 `app_config_dir()/plugins/<id>/` 下的一个目录，内含 `plugin.json`（清单）
//! 与 `main` 指向的 JS 入口。easyop 启动时扫描该目录，把各插件声明的命令 / 工具读进内存，
//! 使它们出现在 ⌘SS 搜索结果与按键配置里。
//!
//! **本模块只负责「发现与声明」**：扫描、解析、校验、检索。真正「激活」（建隐藏 webview、
//! 注入 `eo.*` API、执行命令）由 `plugin_host` 承担——先有登记，才有加载，两者职责分离。
//!
//! 存储范式对齐 `mini_apps.rs`：`static Mutex` 持内存态 + `init(dir)` 启动加载 + 全程可重扫。
//!
//! 安全边界（重要）：当前 easyop 的 `withGlobalTauri=true` + `csp=null` + 自定义命令不进 ACL，
//! 因此清单里的 `capabilities` 字段**当前只是文档承诺，无法强制隔离**。详见
//! 《easyop 扩展机制规范》§六。这里不做"假装有沙箱"的设计，避免给出错误的安全感。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::log_buffer;

/// 插件根目录名（位于 `app_config_dir()` 下）。
const DIR_NAME: &str = "plugins";
/// 清单文件名。
const MANIFEST_NAME: &str = "plugin.json";
/// 清单大小上限：防误放超大文件拖慢启动。
const MANIFEST_MAX_BYTES: u64 = 256 * 1024;

// ---------------------------------------------------------------------------
// 清单结构（plugin.json）
// ---------------------------------------------------------------------------

/// `contributes.commands[]`：插件声明的命令——插件的「动作单元」。
/// 可被 ⌘SS 搜到、可被物理键绑定、可被其他插件调用。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CommandDecl {
    /// 全局唯一命令 id，约定 `<namespace>.<action>`（如 `md.preview`）。
    pub id: String,
    /// 展示名，出现在搜索结果里。
    pub title: String,
    /// 附加搜索关键词（拼音缩写、英文别名等）。
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// `contributes.tools[]`：把某个命令挂进 ⌘SS 面板的「内置工具」区。
///
/// 只有被 `tools` 显式引用的命令才进 ⌘SS 搜索。
/// `tools` 为空 = 作者明确表示没有命令需要进搜索（插件走页签等其他入口）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDecl {
    /// 指向同插件内已声明的命令 id。
    pub command: String,
    /// 搜索结果副标题。
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// `contributes.pages[]`：插件声明的主窗页签——常驻界面入口。
/// 宿主启动时收集，追加到侧栏导航；点击时懒激活插件并渲染。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDecl {
    /// 页签 id（如 `studio.board`）。
    pub id: String,
    /// 展示名（如「内容台」）。
    pub title: String,
    /// 图标名（非 SVG/HTML 字符串，宿主侧映射）。
    #[serde(default)]
    pub icon: String,
    /// 排序权重（≥ 50，不与宿主静态页签 0-49 冲突）。
    #[serde(default = "default_page_order")]
    pub order: i32,
    /// 搜索关键词。
    #[serde(default)]
    pub keywords: Vec<String>,
}

fn default_page_order() -> i32 {
    50
}

/// 插件对外贡献点。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub tools: Vec<ToolDecl>,
    #[serde(default)]
    pub pages: Vec<PageDecl>,
}

/// `plugin.json` 完整结构（camelCase 对齐 VSCode 扩展惯例）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// 插件唯一 id（建议与目录名一致）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 版本（可选）。
    #[serde(default)]
    pub version: Option<String>,
    /// JS 入口文件（插件目录内的相对路径）。
    pub main: String,
    /// 激活事件，如 `onCommand:md.preview` / `onKeyword:markdown` / `onKey:KEY8`。
    /// 发现层只收录不消费；懒激活由 `plugin_host` 在命令被触发时读取。
    #[serde(default)]
    pub activation_events: Vec<String>,
    /// 贡献点。
    #[serde(default)]
    pub contributes: Contributes,
    /// 声明需要的能力（`inject` / `llm` / `selected-text` / `clipboard` / `storage` / `runner`）。
    /// **当前无法强制**，仅作文档约束（见模块头注释）。
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// 一句话说明。
    #[serde(default)]
    pub description: Option<String>,
    /// 标记为内部 / 示范插件：不进入按键菜单等用户面向列表（如 demo 示范插件）。
    #[serde(default)]
    pub hidden: bool,
}

/// 已加载插件（清单 + 磁盘位置 + 来源）。
#[derive(Clone, Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    /// 是否内置（随 easyop 发布）。第一阶段恒为 false——只扫用户目录。
    pub builtin: bool,
}

/// 对外插件视图（给前端；显式暴露磁盘路径便于「打开插件目录」）。
#[derive(Clone, Debug, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub dir: String,
    pub builtin: bool,
    /// 是否对用户隐藏（demo 等示范插件）。
    pub hidden: bool,
    pub commands: Vec<CommandDecl>,
    pub tools: Vec<ToolDecl>,
    pub capabilities: Vec<String>,
}

/// ⌘SS 搜索命中的一条插件命令。
#[derive(Clone, Debug, Serialize)]
pub struct PluginHit {
    pub plugin_id: String,
    pub plugin_name: String,
    pub command_id: String,
    pub title: String,
    pub subtitle: String,
}

// ---------------------------------------------------------------------------
// 内存态
// ---------------------------------------------------------------------------

static STORE: OnceLock<Mutex<Vec<LoadedPlugin>>> = OnceLock::new();
static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

fn store() -> &'static Mutex<Vec<LoadedPlugin>> {
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 插件根目录（`app_config_dir()/plugins`）。
pub fn plugins_root() -> Option<PathBuf> {
    CONFIG_DIR.get().map(|d| d.join(DIR_NAME))
}

/// 插件根目录的字符串形式（供前端展示 / 打开）。
pub fn plugins_root_string() -> String {
    plugins_root().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 生命周期
// ---------------------------------------------------------------------------

/// 启动时初始化：记录配置目录并完成首次扫描。
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    let n = rescan();
    log_buffer::info(format!("插件注册表：已加载 {n} 个插件（{DIR_NAME}/）"));
}

/// 重新扫描插件目录，整体替换内存态。返回加载成功的插件数。
pub fn rescan() -> usize {
    let Some(dir) = plugins_root() else {
        return 0;
    };
    let loaded = scan_dir(&dir);
    let n = loaded.len();
    if let Ok(mut guard) = store().lock() {
        *guard = loaded;
    }
    register_action_uuids();
    n
}

fn scan_dir(dir: &Path) -> Vec<LoadedPlugin> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // 目录不存在是正常状态（用户还没放过插件），不是错误。
        return Vec::new();
    };

    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // 排序保证加载顺序稳定（否则每次启动的搜索结果次序会漂）。
    subdirs.sort();

    let mut out: Vec<LoadedPlugin> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for sdir in subdirs {
        match load_one(&sdir) {
            Ok(plugin) => {
                if !seen_ids.insert(plugin.manifest.id.clone()) {
                    log_buffer::warn(format!(
                        "插件 id 重复，已跳过：{}（{}）",
                        plugin.manifest.id,
                        sdir.display()
                    ));
                    continue;
                }
                out.push(plugin);
            }
            // 单个插件坏掉不得影响其他插件加载——这是「单源失败隔离」。
            Err(e) => log_buffer::warn(format!("插件加载失败 {}：{e}", sdir.display())),
        }
    }
    out
}

fn load_one(dir: &Path) -> Result<LoadedPlugin, String> {
    let manifest_path = dir.join(MANIFEST_NAME);

    let meta = std::fs::metadata(&manifest_path).map_err(|e| format!("缺少 {MANIFEST_NAME}：{e}"))?;
    if meta.len() > MANIFEST_MAX_BYTES {
        return Err(format!("{MANIFEST_NAME} 超过 {}KB 上限", MANIFEST_MAX_BYTES / 1024));
    }

    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| format!("清单读取失败：{e}"))?;
    let manifest: PluginManifest =
        serde_json::from_str(&raw).map_err(|e| format!("清单解析失败：{e}"))?;

    validate(&manifest)?;

    // 入口文件必须存在——否则插件是死的，早报比晚报好。
    let main_path = dir.join(&manifest.main);
    if !main_path.is_file() {
        return Err(format!("入口文件不存在：{}", manifest.main));
    }

    Ok(LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
        builtin: false,
    })
}

/// 清单校验：把「必然出错」的情况挡在加载期，而不是留到执行期才炸。
fn validate(m: &PluginManifest) -> Result<(), String> {
    if m.id.trim().is_empty() {
        return Err("id 为空".to_string());
    }
    if m.name.trim().is_empty() {
        return Err("name 为空".to_string());
    }
    if m.main.trim().is_empty() {
        return Err("main 为空".to_string());
    }
    // 入口必须在插件目录内——禁止 `../` 逃逸与绝对路径。
    if m.main.contains("..") || Path::new(&m.main).is_absolute() {
        return Err("main 必须是插件目录内的相对路径".to_string());
    }

    let mut cmd_ids: HashSet<&str> = HashSet::new();
    for c in &m.contributes.commands {
        if c.id.trim().is_empty() {
            return Err("存在 id 为空的命令声明".to_string());
        }
        if !cmd_ids.insert(c.id.as_str()) {
            return Err(format!("命令 id 重复：{}", c.id));
        }
    }
    // tools 只能引用本插件已声明的命令（跨插件引用第一期不支持，避免隐式依赖）。
    for t in &m.contributes.tools {
        if !cmd_ids.contains(t.command.as_str()) {
            return Err(format!("tools 引用了未声明的命令：{}", t.command));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 插件命令 → 硬件按键动作（保留 UUID）注册
// ---------------------------------------------------------------------------

/// 插件动作命名空间（uuid v5 的 name 空间），与系统其它 v5 命名空间隔离。
const ACTION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x9b, 0x1c, 0x4e, 0x2a, 0x7f, 0x33, 0x51, 0xe0, 0x8c, 0x12, 0x44, 0x9a, 0xb7, 0x66,
    0x0d, 0x5f,
]);

/// 插件命令 → 保留 UUID（供硬件按键触发）。value 为 `&'static str`（启动时一次性 leak）。
static ACTION_UUIDS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
/// UUID → (plugin_id, command_id) 逆映射。
static UUID_TO_ACTION: OnceLock<Mutex<HashMap<String, (&'static str, &'static str)>>> =
    OnceLock::new();

/// 为每个插件命令生成确定性保留 UUID 并写入双向映射表。
///
/// 在 `rescan()` 之后调用，保证「清单已加载」与「UUID 表」同源。
/// uuid v5 由 action_id 派生，跨会话稳定——已烧录固件在下次重新下发前一致。
fn register_action_uuids() {
    let mut fwd: HashMap<String, &'static str> = HashMap::new();
    let mut rev: HashMap<String, (&'static str, &'static str)> = HashMap::new();
    if let Ok(guard) = store().lock() {
        for p in guard.iter() {
            if p.manifest.hidden {
                continue;
            }
            for c in &p.manifest.contributes.commands {
                let uuid = Uuid::new_v5(&ACTION_NAMESPACE, c.id.as_bytes()).to_string();
                let uuid_s: &'static str = Box::leak(uuid.into_boxed_str());
                let pid: &'static str = Box::leak(p.manifest.id.clone().into_boxed_str());
                let cid: &'static str = Box::leak(c.id.clone().into_boxed_str());
                fwd.insert(c.id.clone(), uuid_s);
                rev.insert(uuid_s.to_string(), (pid, cid));
            }
        }
    }
    let count = rev.len();
    if let Ok(mut g) = ACTION_UUIDS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        *g = fwd;
    }
    if let Ok(mut g) = UUID_TO_ACTION.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        *g = rev;
    }
    log_buffer::info(format!("插件动作 UUID 注册：{} 条", count));
}

/// 取插件动作的保留 UUID（供 `payload::reserved_uuid_for_named` 下发固件）。
pub fn plugin_action_uuid(action_id: &str) -> Option<&'static str> {
    ACTION_UUIDS.get()?.lock().ok()?.get(action_id).copied()
}

/// 由保留 UUID 反查 `(plugin_id, command_id)`（供 `handle_host_action` 路由）。
pub fn resolve_action_uuid(uuid: &str) -> Option<(&'static str, &'static str)> {
    UUID_TO_ACTION.get()?.lock().ok()?.get(uuid).copied()
}

/// 所有插件动作 id（供 `get_vocabulary` 合并进 `named_actions`）。
pub fn action_uuids() -> HashMap<String, &'static str> {
    ACTION_UUIDS
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// 动作 id → 中文标签（供 `get_vocabulary` 的 `action_labels`）。
pub fn action_labels() -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(g) = store().lock() {
        for p in g.iter() {
            for c in &p.manifest.contributes.commands {
                out.insert(c.id.clone(), c.title.clone());
            }
        }
    }
    out
}

/// 该具名动作是否是插件声明的动作（供 `validate_action` 放行）。
pub fn is_plugin_action(name: &str) -> bool {
    ACTION_UUIDS
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.contains_key(name))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// 查询
// ---------------------------------------------------------------------------

/// 列出全部插件（前端视图）。
pub fn list() -> Vec<PluginInfo> {
    store()
        .lock()
        .map(|g| g.iter().map(to_info).collect())
        .unwrap_or_default()
}

fn to_info(p: &LoadedPlugin) -> PluginInfo {
    PluginInfo {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        version: p.manifest.version.clone(),
        description: p.manifest.description.clone(),
        dir: p.dir.to_string_lossy().to_string(),
        builtin: p.builtin,
        hidden: p.manifest.hidden,
        commands: p.manifest.contributes.commands.clone(),
        tools: p.manifest.contributes.tools.clone(),
        capabilities: p.manifest.capabilities.clone(),
    }
}

/// 全部插件命令（plugin_id, 声明）。供 `plugin_host` 建「命令 → 插件」索引。
pub fn all_commands() -> Vec<(String, CommandDecl)> {
    store()
        .lock()
        .map(|g| {
            g.iter()
                .flat_map(|p| {
                    p.manifest
                        .contributes
                        .commands
                        .iter()
                        .map(|c| (p.manifest.id.clone(), c.clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 全部插件页签（plugin_id, 声明）。供宿主启动时追加到侧栏导航。
pub fn all_pages() -> Vec<(String, PageDecl)> {
    store()
        .lock()
        .map(|g| {
            g.iter()
                .flat_map(|p| {
                    p.manifest
                        .contributes
                        .pages
                        .iter()
                        .map(|pg| (p.manifest.id.clone(), pg.clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 按命令 id 找它所属的插件。
pub fn find_plugin_by_command(command_id: &str) -> Option<String> {
    store().lock().ok().and_then(|g| {
        g.iter()
            .find(|p| p.manifest.contributes.commands.iter().any(|c| c.id == command_id))
            .map(|p| p.manifest.id.clone())
    })
}

/// 取某个插件的已加载记录（含 manifest）。供 `plugin_host` 读入口文件用。
pub fn find_loaded(plugin_id: &str) -> Option<LoadedPlugin> {
    store().lock().ok().and_then(|g| {
        g.iter().find(|p| p.manifest.id == plugin_id).cloned()
    })
}

/// 取某个插件的 `activationEvents`。
pub fn activation_events(plugin_id: &str) -> Vec<String> {
    store()
        .lock()
        .ok()
        .and_then(|g| {
            g.iter()
                .find(|p| p.manifest.id == plugin_id)
                .map(|p| p.manifest.activation_events.clone())
        })
        .unwrap_or_default()
}

/// ⌘SS 搜索：按 query（**须已小写**）匹配插件命令。
///
/// 与前缀/包含匹配的排序规则无关——调用方（`ss_palette::search`）负责给命中项定档，
/// 这里只做「哪些命中」。返回顺序稳定（插件按目录名序，命令按声明序）。
pub fn search_hits(query_lower: &str) -> Vec<PluginHit> {
    let q = query_lower.trim();
    if q.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<PluginHit> = Vec::new();
    let Ok(g) = store().lock() else {
        return hits;
    };

    for p in g.iter() {
        let cs = &p.manifest.contributes;


        for cmd in &cs.commands {
            let tool = cs.tools.iter().find(|t| t.command == cmd.id);

            // 只有被 tools 显式引用的命令才进 ⌘SS 搜索。
            // tools 为空 = 作者明确表示没有命令需要进搜索（插件走页签等其他入口）。
            if tool.is_none() {
                continue;
            }

            let mut hay: Vec<String> = Vec::with_capacity(4 + cmd.keywords.len());
            hay.push(cmd.title.to_lowercase());
            hay.push(cmd.id.to_lowercase());
            hay.push(p.manifest.name.to_lowercase());
            hay.extend(cmd.keywords.iter().map(|k| k.to_lowercase()));
            if let Some(t) = tool {
                hay.extend(t.keywords.iter().map(|k| k.to_lowercase()));
            }

            if hay.iter().any(|h| !h.is_empty() && h.contains(q)) {
                let subtitle = tool
                    .and_then(|t| t.subtitle.clone())
                    .unwrap_or_else(|| format!("插件 · {}", p.manifest.name));
                hits.push(PluginHit {
                    plugin_id: p.manifest.id.clone(),
                    plugin_name: p.manifest.name.clone(),
                    command_id: cmd.id.clone(),
                    title: cmd.title.clone(),
                    subtitle,
                });
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(commands: &[(&str, &str)], tools: &[&str]) -> PluginManifest {
        PluginManifest {
            hidden: false,
            id: "p1".to_string(),
            name: "测试插件".to_string(),
            version: None,
            main: "index.js".to_string(),
            activation_events: vec![],
            contributes: Contributes {
                commands: commands
                    .iter()
                    .map(|(id, title)| CommandDecl {
                        id: id.to_string(),
                        title: title.to_string(),
                        keywords: vec![],
                    })
                    .collect(),
                tools: tools
                    .iter()
                    .map(|c| ToolDecl {
                        command: c.to_string(),
                        subtitle: None,
                        keywords: vec![],
                    })
                    .collect(),
                pages: vec![],
            },
            capabilities: vec![],
            description: None,
        }
    }

    #[test]
    fn validate_rejects_empty_id() {
        let mut m = manifest_with(&[("a.b", "A")], &[]);
        m.id = "  ".to_string();
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_rejects_main_escape() {
        let mut m = manifest_with(&[("a.b", "A")], &[]);
        m.main = "../evil.js".to_string();
        assert!(validate(&m).is_err());
        m.main = "C:/evil.js".to_string();
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_command() {
        let m = manifest_with(&[("a.b", "A"), ("a.b", "B")], &[]);
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_rejects_tool_referencing_unknown_command() {
        let m = manifest_with(&[("a.b", "A")], &["a.nope"]);
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_accepts_well_formed_manifest() {
        let m = manifest_with(&[("a.b", "A"), ("a.c", "C")], &["a.b"]);
        assert!(validate(&m).is_ok());
    }

    /// 端到端：在临时目录造一个真插件，验证扫描能发现它并解析出命令。
    #[test]
    fn scan_discovers_valid_plugin() {
        let base = std::env::temp_dir().join(format!("easyop-plugin-ok-{}", std::process::id()));
        let pdir = base.join("demo");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join(MANIFEST_NAME),
            r#"{
                "id": "demo",
                "name": "演示插件",
                "main": "index.js",
                "contributes": {
                    "commands": [{ "id": "demo.hello", "title": "打招呼", "keywords": ["nihao"] }],
                    "tools": [{ "command": "demo.hello" }]
                }
            }"#,
        )
        .unwrap();
        std::fs::write(pdir.join("index.js"), "export function activate(){}").unwrap();

        let loaded = scan_dir(&base);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].manifest.id, "demo");
        assert_eq!(loaded[0].manifest.contributes.commands.len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 坏插件（入口文件不存在）应被隔离，不拖垮同目录下的好插件。
    #[test]
    fn scan_isolates_broken_plugin() {
        let base = std::env::temp_dir().join(format!("easyop-plugin-bad-{}", std::process::id()));
        let good = base.join("good");
        let bad = base.join("bad");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            good.join(MANIFEST_NAME),
            r#"{"id":"good","name":"好","main":"index.js"}"#,
        )
        .unwrap();
        std::fs::write(good.join("index.js"), "").unwrap();
        // bad：main 指向不存在的文件
        std::fs::write(
            bad.join(MANIFEST_NAME),
            r#"{"id":"bad","name":"坏","main":"missing.js"}"#,
        )
        .unwrap();

        let loaded = scan_dir(&base);
        assert_eq!(loaded.len(), 1, "坏插件应被跳过");
        assert_eq!(loaded[0].manifest.id, "good");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 搜索：命中 tool 关键词与标题。本测试独占修改全局 STORE。
    #[test]
    fn search_hits_by_keyword_and_title() {        let plugin = LoadedPlugin {
            manifest: PluginManifest {
                hidden: false,
                id: "demo".to_string(),
                name: "演示".to_string(),
                version: None,
                main: "index.js".to_string(),
                activation_events: vec![],
                contributes: Contributes {
                    commands: vec![CommandDecl {
                        id: "demo.hello".to_string(),
                        title: "打招呼".to_string(),
                        keywords: vec!["nihao".to_string()],
                    }],
                    tools: vec![ToolDecl {
                        command: "demo.hello".to_string(),
                        subtitle: Some("问个好".to_string()),
                        keywords: vec![],
                    }],
                    pages: vec![],
                },
                capabilities: vec![],
                description: None,
            },
            dir: PathBuf::from("."),
            builtin: false,
        };
        *store().lock().unwrap() = vec![plugin];

        assert_eq!(search_hits("nihao").len(), 1, "关键词应命中");
        assert_eq!(search_hits("打招呼").len(), 1, "标题应命中");
        assert_eq!(search_hits("zzz").len(), 0, "无关词不应命中");

        *store().lock().unwrap() = vec![];
    }

    /// tools 为空时，命令不进 ⌘SS 搜索（插件走页签等其他入口）。
    #[test]
    fn search_hits_empty_tools_no_search() {
        let plugin = LoadedPlugin {
            manifest: PluginManifest {
                hidden: false,
                id: "solo".to_string(),
                name: "独立".to_string(),
                version: None,
                main: "index.js".to_string(),
                activation_events: vec![],
                contributes: Contributes {
                    commands: vec![CommandDecl {
                        id: "solo.run".to_string(),
                        title: "跑起来".to_string(),
                        keywords: vec!["paoqi".to_string()],
                    }],
                    tools: vec![],
                    pages: vec![],
                },
                capabilities: vec![],
                description: None,
            },
            dir: PathBuf::from("."),
            builtin: false,
        };
        *store().lock().unwrap() = vec![plugin];

        assert_eq!(search_hits("paoqi").len(), 0, "tools 为空时关键词不应命中");
        assert_eq!(search_hits("跑起来").len(), 0, "tools 为空时标题不应命中");
        assert_eq!(search_hits("独立").len(), 0, "tools 为空时插件名不应命中");

        *store().lock().unwrap() = vec![];
    }

    /// 真机验证：扫描本机 `%APPDATA%\com.easyop.app\plugins`。
    ///
    /// 默认 `#[ignore]` —— 无插件环境（CI / 新机器）不该因此失败。手动跑：
    ///   cargo test --lib plugin_registry -- --ignored --nocapture
    #[test]
    #[ignore]
    fn scan_real_user_plugins_dir() {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            println!("非 Windows 或无 APPDATA，跳过");
            return;
        };
        let dir = PathBuf::from(appdata).join("com.easyop.app").join(DIR_NAME);
        let loaded = scan_dir(&dir);
        println!("扫描 {} → 发现 {} 个插件", dir.display(), loaded.len());
        for p in &loaded {
            println!(
                "  · {} [{}] main={} 命令={} tool={}",
                p.manifest.name,
                p.manifest.id,
                p.manifest.main,
                p.manifest.contributes.commands.len(),
                p.manifest.contributes.tools.len(),
            );
        }
        assert!(
            !loaded.is_empty(),
            "未发现插件：确认插件已放入 {}",
            dir.display()
        );
    }
}
