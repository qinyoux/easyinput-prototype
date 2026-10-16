//! Everything 搜索集成。
//!
//! 通过调用 Everything 官方的命令行工具 `es.exe` 执行搜索。
//! 要求用户已安装 Everything 且正在后台运行，并配置了 es.exe 的路径。
//!
//! es.exe 命令：`es.exe "query" -n 50 -export-csv <file> -utf8-bom -size -attributes`
//!
//! CSV 格式（**UTF-8 with BOM**）：
//!   Filename, Size, Attributes
//!
//! Attributes: 16=目录(D), 32=存档(A), 1=只读(R), 2=隐藏(H), 4=系统(S)
//!
//! ## 为什么必须用 -export-csv + -utf8-bom（而不是直接读 stdout）
//!
//! es.exe 是 **ANSI（本地代码页 ACP）程序**，不是 UTF-8 程序：
//! 它把 UTF-16 文件名按系统 ACP（本机936 = GBK）转换后写 stdout。
//! Windows 管道是**裸字节流，没有编码协商机制**，子进程不会因为父进程
//! 想要 UTF-8 就改变编码。所以 `es.exe ... -csv` 的 stdout 是 GBK 字节，
//! 例如「文档」= `CE C4 B5 B5`。
//!
//! 若用 `String::from_utf8_lossy` 按 UTF-8 解这些字节，4 个字节会各自变成
//! 一个 U+FFFD（`�`），得到「�ĵ�」—— 即中文乱码，且**信息不可恢复**。
//! 用 `from_utf8_lossy` 只是不 panic，并不代表解码正确。
//!
//! `-export-csv <file> -utf8-bom` 是 es.exe 唯一能产出 UTF-8 的方式：
//! 写文件时强制加 UTF-8 BOM（实测首字节 EF BB BF）。因此本模块读临时文件、
//! 按 UTF-8 解码并剥掉 BOM，而不是读管道。
//!
//! 注意：`-csv` 单独使用时**不会**让 stdout 变成 UTF-8，别被参数名误导。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Everything 搜索结果项。
#[derive(Serialize, Clone, Debug)]
pub struct EverythingResult {
    /// 文件名（含扩展名）。
    pub name: String,
    /// 完整路径。
    pub path: String,
    /// 文件大小（字节），0 表示文件夹或未知。
    pub size: u64,
    /// 是否是目录。
    pub is_dir: bool,
}

/// 检查 Everything 是否可用（路径有效 + es.exe 存在）。
pub fn is_available(es_path: &str) -> bool {
    if es_path.is_empty() {
        return false;
    }
    resolve_es_path(es_path).is_some()
}

/// 根据 Everything.exe 路径推断 es.exe 路径。
/// 如果用户配置的是 Everything.exe，则尝试在同目录找 es.exe。
pub fn resolve_es_path(everything_path: &str) -> Option<String> {
    if everything_path.is_empty() {
        return None;
    }
    let p = Path::new(everything_path);
    let file_name = p.file_name()?.to_string_lossy().to_lowercase();

    if file_name == "es.exe" {
        if p.exists() {
            return Some(everything_path.to_string());
        }
        return None;
    }

    if file_name == "everything.exe" {
        // 同目录找 es.exe
        if let Some(parent) = p.parent() {
            let es_path = parent.join("es.exe");
            if es_path.exists() {
                return Some(es_path.to_string_lossy().to_string());
            }
        }
        return None;
    }

    // 都不是，直接返回原路径（如果存在）
    if p.exists() {
        return Some(everything_path.to_string());
    }

    None
}

/// 使用 Everything 搜索文件。
///
/// - `everything_path`: Everything.exe 或 es.exe 的完整路径
/// - `query`: 搜索关键词（支持 Everything 的搜索语法）
/// - `max_results`: 最多返回多少条结果
///
/// 返回匹配的文件/文件夹列表。
///
/// 实现说明：不读 stdout（那是 ACP 字节），改用 `-export-csv` + `-utf8-bom`
/// 落临时文件后按 UTF-8 读取。详见文件头「为什么必须用 -export-csv」。
pub fn search(everything_path: &str, query: &str, max_results: usize) -> Result<Vec<EverythingResult>, String> {
    let es_path = resolve_es_path(everything_path)
        .ok_or_else(|| "Everything 未配置或 es.exe 不存在".to_string())?;

    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    // 每次搜索一个唯一临时文件，避免并发搜索互相覆盖
    let csv_path = std::env::temp_dir().join(format!(
        "easyop_es_{}_{}.csv",
        std::process::id(),
        next_search_seq()
    ));

    // 调用 es.exe 搜索并输出 UTF-8 CSV
    let output = Command::new(&es_path)
        .arg(query)
        .arg("-n")
        .arg(max_results.to_string())
        .arg("-export-csv")
        .arg(&csv_path)
        .arg("-utf8-bom")
        .arg("-size")
        .arg("-attributes")
        .output()
        .map_err(|e| format!("启动 es.exe 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // 清理可能残留的半成品文件
        let _ = std::fs::remove_file(&csv_path);
        return Err(format!(
            "Everything 搜索失败，退出码: {:?}, 错误: {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    // 读 UTF-8 CSV 文件并解析（无论成败都要清理临时文件）
    let raw = match std::fs::read(&csv_path) {
        Ok(b) => b,
        Err(e) => {
            let _ = std::fs::remove_file(&csv_path);
            return Err(format!("读取 es.exe 输出文件失败: {}", e));
        }
    };
    let _ = std::fs::remove_file(&csv_path);

    // 剥掉 UTF-8 BOM后按 UTF-8 解码：es.exe -utf8-bom 保证此处必定成功，
    // 用 from_utf8（严格）以便万一不成立时能暴露问题，而不是静默产生乱码。
    let csv_text = match String::from_utf8(strip_bom(&raw).to_vec()) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!(
                "es.exe 输出不是合法 UTF-8（已用 -utf8-bom，应属异常）：{}",
                e
            ));
        }
    };

    let results = parse_csv(&csv_text, max_results)?;
    Ok(results)
}

/// 剥掉 UTF-8 BOM（EF BB BF）。
fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEFu8, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

/// 搜索序号计数器，保证同进程内临时文件名唯一（防止并发搜索覆盖）。
fn next_search_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 解析 es.exe 输出的 CSV。
fn parse_csv(csv_text: &str, max_results: usize) -> Result<Vec<EverythingResult>, String> {
    let mut results = Vec::new();
    let mut is_first_line = true;

    for line in csv_text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        // 跳过表头
        if is_first_line {
            is_first_line = false;
            let lower = line.to_lowercase();
            if lower.contains("filename") || lower.contains("名称") {
                continue;
            }
        }

        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }

        // 第一列是完整路径（es.exe 的 CSV 输出 Filename 列就是完整路径）
        let full_path = fields[0].clone();

        // 解析文件名
        let name = Path::new(&full_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| full_path.clone());

        // 大小（第二列）
        let size: u64 = fields[1].parse().unwrap_or(0);

        // 属性（第三列，Attributes）
        let is_dir = if fields.len() >= 3 {
            // 属性 16 = FILE_ATTRIBUTE_DIRECTORY
            let attrs: u32 = fields[2].parse().unwrap_or(0);
            attrs & 16 != 0
        } else {
            false
        };

        results.push(EverythingResult {
            name,
            path: full_path,
            size,
            is_dir,
        });

        if results.len() >= max_results {
            break;
        }
    }

    Ok(results)
}

/// 简单的 CSV 行解析：支持双引号包裹的字段。
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                // 检查是否是转义的双引号
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' => {
                    in_quotes = true;
                }
                ',' => {
                    fields.push(current);
                    current = String::new();
                }
                _ => {
                    current.push(c);
                }
            }
        }
    }
    fields.push(current);
    fields
}

/// 获取 Everything 建议的默认安装路径。
pub fn default_path() -> Option<PathBuf> {
    // 常见安装位置
    let candidates = [
        r"C:\Program Files\Everything\es.exe",
        r"C:\Program Files (x86)\Everything\es.exe",
        r"C:\Everything\es.exe",
        r"D:\Program Files\Everything\es.exe",
        r"D:\Program Files\Everything1.5\es.exe",
    ];
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_bom() {
        // 有BOM → 剥掉前3 字节
        assert_eq!(strip_bom(&[0xEF, 0xBB, 0xBF, b'a']), b"a");
        // 无 BOM → 原样返回
        assert_eq!(strip_bom(b"Filename,Size"), b"Filename,Size");
        // 空输入
        assert_eq!(strip_bom(&[]), b"");
    }

    #[test]
    fn test_bom_stripped_bytes_are_valid_utf8() {
        // 回归测试：es.exe 旧实现把 GBK 字节交给 String::from_utf8_lossy，
        // 「文档」= CE C4 B5 B5 会变成两个 U+FFFD。现在走 -utf8-bom，
        // 文件内容是合法 UTF-8，剥 BOM 后必须能被严格解码。
        let mut raw: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        raw.extend_from_slice("Filename,Size,Attributes\n".as_bytes());
        raw.extend_from_slice("\"E:\\文档\\a.txt\",100,32\n".as_bytes());
        let text = String::from_utf8(strip_bom(&raw).to_vec()).expect("剥离BOM 后必须是合法 UTF-8");
        assert!(text.starts_with("Filename"));
        assert!(text.contains("文档"));
        assert!(!text.contains('\u{FFFD}'), "不允许出现替换字符");
    }

    #[test]
    fn test_gbk_bytes_would_break_utf8_lossy() {
        // 反向证明：若有人改回from_utf8_lossy 读 stdout，这个用例会失败。
        // 「文档」在 GBK(936) 下是 CE C4 B5 B5，不是合法 UTF-8。
        let gbk = [0xCEu8, 0xC4, 0xB5, 0xB5];
        assert!(String::from_utf8(gbk.to_vec()).is_err(), "GBK 字节不应能按 UTF-8 严格解码");
        let lossy = String::from_utf8_lossy(&gbk);
        assert!(
            lossy.contains('\u{FFFD}'),
            "from_utf8_lossy 应产生替换字符，这正是旧 bug 的乱码来源"
        );
    }

    #[test]
    fn test_parse_csv_line_simple() {
        let line = r#""C:\test\file.txt",1234,32"#;
        let fields = parse_csv_line(line);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], r"C:\test\file.txt");
        assert_eq!(fields[1], "1234");
        assert_eq!(fields[2], "32");
    }

    #[test]
    fn test_parse_csv_line_with_spaces() {
        let line = r#""C:\path with spaces\file.txt",5678,16"#;
        let fields = parse_csv_line(line);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], r"C:\path with spaces\file.txt");
        assert_eq!(fields[1], "5678");
        assert_eq!(fields[2], "16");
    }

    #[test]
    fn test_parse_csv_with_header() {
        let csv = "Filename,Size,Attributes\n\"C:\\test\\a.txt\",100,32\n\"C:\\test\\b.txt\",200,16\n";
        let results = parse_csv(csv, 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "a.txt");
        assert_eq!(results[0].path, r"C:\test\a.txt");
        assert_eq!(results[0].size, 100);
        assert_eq!(results[0].is_dir, false);
        assert_eq!(results[1].name, "b.txt");
        assert_eq!(results[1].is_dir, true); // 属性 16 = 目录
    }

    #[test]
    fn test_parse_csv_max_results() {
        let mut csv = String::from("Filename,Size,Attributes\n");
        for i in 0..20 {
            csv.push_str(&format!("\"C:\\test\\file{}.txt\",100,32\n", i));
        }
        let results = parse_csv(&csv, 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_search_empty_query() {
        let results = search(r"D:\Fake\es.exe", "", 10);
        assert!(results.is_err() || results.unwrap().is_empty());
    }

    #[test]
    fn test_resolve_es_path_from_everything() {
        // 配置 Everything.exe，应自动找到同目录的 es.exe
        let result = resolve_es_path(r"D:\Program Files\Everything1.5\Everything.exe");
        if Path::new(r"D:\Program Files\Everything1.5\Everything.exe").exists() {
            assert!(result.is_some());
            assert!(result.unwrap().contains("es.exe"));
        }
    }

    #[test]
    fn test_search_real_es() {
        // 真实搜索测试（需要本地已安装 Everything）
        let everything_path = r"D:\Program Files\Everything1.5\Everything.exe";
        if !Path::new(everything_path).exists() {
            println!("跳过测试：Everything 未安装在默认路径");
            return;
        }

        let results = search(everything_path, "*.txt", 5).unwrap();
        println!("搜索到 {} 个结果", results.len());
        assert!(results.len() > 0, "应该能找到 .txt 文件");
        assert!(results.len() <= 5, "不超过 5 个结果");
        for r in &results {
            println!("  {} [{}字节] dir={}", r.name, r.size, r.is_dir);
            assert!(!r.name.is_empty());
            assert!(r.path.contains(r.name.as_str()));
        }
    }

    #[test]
    fn test_search_real_es_chinese_not_mojibake() {
        // 端到端回归：搜中文关键词，结果路径里不允许出现 U+FFFD。
        // 旧实现（es.exe -csv 读 stdout + from_utf8_lossy）在这里必然失败。
        let everything_path = r"D:\Program Files\Everything1.5\Everything.exe";
        if resolve_es_path(everything_path).is_none() {
            println!("跳过测试：Everything 未安装在默认路径");
            return;
        }

        let results = search(everything_path, "文档", 10).unwrap();
        if results.is_empty() {
            println!("跳过测试：未搜到含「文档」的结果");
            return;
        }
        for r in &results {
            println!("  {}", r.path);
            assert!(
                !r.path.contains('\u{FFFD}'),
                "路径含替换字符 U+FFFD，说明编码处理有误：{}",
                r.path
            );
            assert!(
                !r.name.contains('\u{FFFD}'),
                "文件名含替换字符 U+FFFD：{}",
                r.name
            );
        }
    }

    #[test]
    fn test_search_real_es_preserves_non_acp_chars() {
        // ACP(936/GBK) 无法表示的字符（ß/Ω/Δ）必须保真，
        // 这是 GBK 管道的必然丢失点，UTF-8 导出才修得回来。
        let everything_path = r"D:\Program Files\Everything1.5\Everything.exe";
        if resolve_es_path(everything_path).is_none() {
            println!("跳过测试：Everything 未安装");
            return;
        }

        let results = search(everything_path, "Ω", 20).unwrap();
        for r in &results {
            assert!(
                !r.path.contains('\u{FFFD}'),
                "非 ACP 字符被破坏：{}",
                r.path
            );
        }
    }
}
