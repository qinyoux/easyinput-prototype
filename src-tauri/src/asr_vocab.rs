//! ASR 自定义词库 —— 管理语音识别热词，提升专业术语 / 高频词汇的识别准确率。
//!
//! 存储路径：app_config_dir()/asr_vocab.json
//! 数据格式：{ "words": [{ "text": "easyop", "weight": 10 }, ...] }
//!
//! 火山引擎 ASR 热词对接在 `asr.rs` 中消费（步骤 5）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::log_buffer;

/// 一条热词。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VocabWord {
    /// 热词文本。
    pub text: String,
    /// 权重（1-10，越高越优先匹配）。
    #[serde(default = "default_weight")]
    pub weight: i32,
}

fn default_weight() -> i32 {
    5
}

/// 词库文件结构。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct VocabFile {
    #[serde(default)]
    words: Vec<VocabWord>,
}

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
static VOCAB: OnceLock<Mutex<Vec<VocabWord>>> = OnceLock::new();

/// 初始化词库（启动时调用）。
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());

    let path = config_dir.join("asr_vocab.json");
    let words = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<VocabFile>(&s).ok())
            .map(|f| f.words)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let vocab = VOCAB.get_or_init(|| Mutex::new(words));
    log_buffer::info(format!("ASR 词库已加载：{} 个热词", vocab.lock().unwrap().len()));
}

fn vocab() -> &'static Mutex<Vec<VocabWord>> {
    VOCAB.get_or_init(|| Mutex::new(Vec::new()))
}

fn save() {
    if let Some(dir) = CONFIG_DIR.get() {
        let path = dir.join("asr_vocab.json");
        let file = VocabFile {
            words: vocab().lock().unwrap().clone(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&path, s) {
                    log_buffer::warn(format!("ASR 词库保存失败: {e}"));
                }
            }
            Err(e) => log_buffer::warn(format!("ASR 词库序列化失败: {e}")),
        }
    }
}

/// 获取当前词表（供 `asr.rs` 构造请求时使用）。
pub fn words() -> Vec<VocabWord> {
    vocab().lock().unwrap().clone()
}

// ── Tauri commands ──

/// 列出所有热词。
#[tauri::command]
pub fn asr_vocab_list() -> Vec<VocabWord> {
    vocab().lock().unwrap().clone()
}

/// 添加热词（去重：已有相同 text 则更新权重）。
#[tauri::command]
pub fn asr_vocab_add(text: String, weight: Option<i32>) -> Result<Vec<VocabWord>, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("热词文本不能为空".to_string());
    }
    let w = weight.unwrap_or(5).clamp(1, 10);

    let mut guard = vocab().lock().unwrap();
    if let Some(existing) = guard.iter_mut().find(|w| w.text == text) {
        existing.weight = w;
    } else {
        guard.push(VocabWord { text, weight: w });
    }
    let result = guard.clone();
    drop(guard);
    save();
    Ok(result)
}

/// 删除热词（按文本匹配）。
#[tauri::command]
pub fn asr_vocab_remove(text: String) -> Result<Vec<VocabWord>, String> {
    let mut guard = vocab().lock().unwrap();
    guard.retain(|w| w.text != text);
    let result = guard.clone();
    drop(guard);
    save();
    Ok(result)
}

// ── 测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_word_default_weight() {
        let json = r#"{"text":"hello"}"#;
        let w: VocabWord = serde_json::from_str(json).unwrap();
        assert_eq!(w.text, "hello");
        assert_eq!(w.weight, 5);
    }

    #[test]
    fn vocab_file_empty() {
        let json = r#"{}"#;
        let f: VocabFile = serde_json::from_str(json).unwrap();
        assert!(f.words.is_empty());
    }
}