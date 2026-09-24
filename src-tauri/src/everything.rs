//! Everything 搜索集成。
//!
//! 通过调用 Everything 官方的命令行工具 `es.exe` 执行搜索。
//! 要求用户已安装 Everything 且正在后台运行，并配置了 es.exe 的路径。
//!
//! es.exe 命令：`es.exe "query" -n 50 -csv -size -attributes`
//!
//! CSV 格式（UTF-8）：
//!   Filename, Size, Attributes
//!
//! Attributes: 16=目录(D), 32=存档(A), 1=只读(R), 2=隐藏(H), 4=系统(S)

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
pub fn search(everything_path: &str, query: &str, max_results: usize) -> Result<Vec<EverythingResult>, String> {
    let es_path = resolve_es_path(everything_path)
        .ok_or_else(|| "Everything 未配置或 es.exe 不存在".to_string())?;

    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    // 调用 es.exe 搜索并输出 CSV
    let output = Command::new(&es_path)
        .arg(query)
        .arg("-n")
        .arg(max_results.to_string())
        .arg("-csv")
        .arg("-size")
        .arg("-attributes")
        .output()
        .map_err(|e| format!("启动 es.exe 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Everything 搜索失败，退出码: {:?}, 错误: {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results = parse_csv(&stdout, max_results)?;
    Ok(results)
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
}
