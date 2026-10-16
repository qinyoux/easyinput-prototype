//! 实时翻译会话编排（live-translate 插件后端）。
//!
//! 职责：把「说话人分离流式识别」与「LLM 流式翻译」串成一条会话：
//!
//! ```text
//!   电脑麦克风 ──PCM──> asr::run_translate ──定稿句──> 本模块翻译消费任务
//!                                                        │
//!                                        LLM chat_stream（逐句并发，上限 5）
//!                                                        │
//!                                    emit lt-translate-chunk / lt-translate-final
//!                                                        │
//!                                                 记录到 sentences（导出用）
//! ```
//!
//! # 与 `session.rs` 的关系
//!
//! **刻意不复用** `SessionManager` / `TriggerMode`：那条链路是「PTT 键触发 + 上屏注入」，
//! 改它的模式枚举会破坏 `config.json` 兼容性。实时翻译是**插件页签触发 + 只上屏到插件面板**，
//! 两条链路的生命周期与去向都不同，独立成模块比硬塞进去更安全。
//!
//! # 一期范围
//!
//! 录音固定用**电脑麦克风**（plan 决策 5）。开发板 Wi-Fi 音频源留待后续。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, Semaphore};

use crate::asr::{self, TranscriptSentence};
use crate::audio::AudioCapture;
use crate::llm_client::{ChatMessage, ChatOptions, LlmClient};
use crate::log_buffer;

/// 翻译并发上限：LLM 限流友好，又不至于把队列堵死。
const MAX_CONCURRENT_TRANSLATIONS: usize = 5;

/// 翻译模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranslateMode {
    /// 对话模式：2 人，spk0=我方 / spk1=对方。
    Conversation,
    /// 会议模式：多人，spk0/spk1/spk2...（人数越多说话人分离准确率越降）。
    Meeting,
}

impl TranslateMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "meeting" => TranslateMode::Meeting,
            _ => TranslateMode::Conversation,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TranslateMode::Conversation => "conversation",
            TranslateMode::Meeting => "meeting",
        }
    }
}

/// 会话状态快照（返回前端）。
#[derive(Clone, Debug, Serialize)]
pub struct LtStatus {
    pub running: bool,
    pub mode: String,
    pub source_lang: String,
    pub target_lang: String,
    pub sentences_count: usize,
    pub duration_secs: u64,
}

/// 已记录的一句（导出用）。
#[derive(Clone, Debug, Serialize)]
pub struct RecordedSentence {
    pub utterance_id: u64,
    pub speaker_id: String,
    pub original: String,
    pub translation: String,
    /// 句起点（毫秒，服务端可能不给）。
    pub start_ms: Option<i64>,
    /// 句终点（毫秒，服务端可能不给）。
    pub end_ms: Option<i64>,
}

struct LtInner {
    running: AtomicBool,
    /// 停止信号：置位后 ASR 主循环收尾。
    stop_flag: Mutex<Option<Arc<AtomicBool>>>,
    mode: Mutex<TranslateMode>,
    source_lang: Mutex<String>,
    target_lang: Mutex<String>,
    /// 完整记录（会话内的每一句）。
    sentences: Mutex<Vec<RecordedSentence>>,
    started_at: Mutex<Option<Instant>>,
    llm_client: LlmClient,
}

/// 实时翻译管理器（线程安全，可克隆）。
///
/// 克隆的是 `Arc`，与 `LlmClient` 相同的模式 —— 便于 `app.manage()` 注入后
/// 命令侧按值取用。
#[derive(Clone)]
pub struct LiveTranslateManager {
    inner: Arc<LtInner>,
}

impl LiveTranslateManager {
    pub fn new(llm_client: LlmClient) -> Self {
        Self {
            inner: Arc::new(LtInner {
                running: AtomicBool::new(false),
                stop_flag: Mutex::new(None),
                mode: Mutex::new(TranslateMode::Conversation),
                source_lang: Mutex::new("en-US".to_string()),
                target_lang: Mutex::new("zh-CN".to_string()),
                sentences: Mutex::new(Vec::new()),
                started_at: Mutex::new(None),
                llm_client,
            }),
        }
    }

    /// 是否正在翻译。
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::Relaxed)
    }

    /// 查询状态快照。
    pub fn status(&self) -> LtStatus {
        let duration = self
            .inner
            .started_at
            .try_lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed().as_secs()))
            .unwrap_or(0);
        let sentences_count = self
            .inner
            .sentences
            .try_lock()
            .map(|s| s.len())
            .unwrap_or(0);
        LtStatus {
            running: self.is_running(),
            mode: self
                .inner
                .mode
                .try_lock()
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|_| "conversation".to_string()),
            source_lang: self
                .inner
                .source_lang
                .try_lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "en-US".to_string()),
            target_lang: self
                .inner
                .target_lang
                .try_lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "zh-CN".to_string()),
            sentences_count,
            duration_secs: duration,
        }
    }

    /// 启动实时翻译会话。
    ///
    /// `asr_*` 参数由调用方（`lt_start` 命令）从全局配置取出 —— 本模块不碰配置，
    /// 保持"识别/翻译"与"配置读取"两件事解耦。
    pub async fn start(
        &self,
        app: tauri::AppHandle,
        mode: TranslateMode,
        source_lang: String,
        target_lang: String,
        asr_key: String,
        asr_url: String,
        asr_resource_id: String,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("实时翻译已在运行中".to_string());
        }
        if asr_key.trim().is_empty() {
            return Err("ASR API Key 未配置，请先在「模型配置」中填写".to_string());
        }

        // 记录会话参数。
        *self.inner.mode.lock().await = mode;
        *self.inner.source_lang.lock().await = source_lang.clone();
        *self.inner.target_lang.lock().await = target_lang.clone();
        self.inner.sentences.lock().await.clear();
        *self.inner.started_at.lock().await = Some(Instant::now());

        let stop_flag = Arc::new(AtomicBool::new(false));
        *self.inner.stop_flag.lock().await = Some(stop_flag.clone());
        self.inner.running.store(true, Ordering::Relaxed);

        // 启动录音（电脑麦克风）。
        //
        // ⛔ AudioCapture 不是 Send（cpal::Stream 内含裸指针），**不能 move 进 async 任务**。
        // 沿用 `session.rs` 的做法：在专属 std::thread 里创建并保活，轮询停止标志后 drop
        // 释放麦克风。闭包本身不含 AudioCapture（它在闭包内创建），因此闭包是 Send 的。
        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        {
            let stop_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        log_buffer::err(format!("live_translate: 录音启动失败：{e}"));
                        let _ = app_rec.emit(
                            "lt-state",
                            serde_json::json!({
                                "running": false,
                                "error": format!("录音启动失败：{e}"),
                            }),
                        );
                        // 让 ASR 任务收尾（它会随之复位运行状态）。
                        stop_rec.store(true, Ordering::Relaxed);
                        return;
                    }
                };
                while !stop_rec.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                drop(capture);
                log_buffer::info("live_translate: 录音线程退出，麦克风已释放");
            });
        }

        // ASR → 翻译 的句通道。
        let (sentence_tx, sentence_rx) = tokio::sync::mpsc::channel::<TranscriptSentence>(32);

        // 会话结束时统一复位状态（ASR 任务收尾后触发）。
        let app_for_state = app.clone();
        let this = self.clone();

        // ASR 识别任务（不持有 AudioCapture，只消费 audio_rx）。
        let app_asr = app.clone();
        let asr_stop = stop_flag.clone();
        let asr_key_cl = asr_key.clone();
        let asr_url_cl = asr_url.clone();
        let asr_rid_cl = asr_resource_id.clone();
        let src_cl = source_lang.clone();
        tauri::async_runtime::spawn(async move {
            let result = asr::run_translate(
                app_asr.clone(),
                asr_key_cl,
                asr_url_cl,
                asr_rid_cl,
                src_cl,
                true, // enable_speaker：一期默认开启说话人分离
                audio_rx,
                asr_stop.clone(),
                sentence_tx,
            )
            .await;
            // ⛔ 无论识别因何结束，都要置位停止标志：录音线程据此退出、释放麦克风。
            // 漏掉这一步，麦克风会被一直占着，下一次启动必然失败。
            asr_stop.store(true, Ordering::Relaxed);
            if let Err(e) = result {
                log_buffer::err(format!("live_translate: ASR 任务结束（错误）{e}"));
                let _ = app_asr.emit(
                    "lt-state",
                    serde_json::json!({ "running": false, "error": e }),
                );
            }
            // 会话收尾：复位运行标志（必须在 ASR 返回之后）。
            this.finish(&app_for_state).await;
        });

        // 翻译消费任务：逐句取定稿，spawn 并发翻译。
        let app_trans = app.clone();
        let llm = self.inner.llm_client.clone();
        let tgt_cl = target_lang.clone();
        let this2 = self.clone();
        tokio::spawn(async move {
            let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_TRANSLATIONS));
            let mut sentence_rx = sentence_rx;
            while let Some(sentence) = sentence_rx.recv().await {
                // 迟到修订：服务端回补了已定稿句的文本。只改记录里的原文，
                // **不重翻译** —— 翻译昂贵，且这句译文早已上屏，不能让用户看到
                // 译文和原文对不上，所以宁可让译文略滞后于原文修订。
                if sentence.correction {
                    this2.apply_correction(sentence.utterance_id, sentence.text).await;
                    continue;
                }
                // 非定稿句不进翻译（半句翻译会误导用户）；识别侧只投定稿句，这里是双保险。
                if !sentence.definite {
                    continue;
                }
                // 定稿事件由**识别侧**（asr.rs 的 UtteranceTracker）发出——那一刻文本最新。
                // 翻译侧不再重复发，避免同句两次定稿把 UI 覆盖两次。

                let permit = match sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let llm = llm.clone();
                let app_c = app_trans.clone();
                let tgt = tgt_cl.clone();
                let mgr = this2.clone();
                let uid = sentence.utterance_id;
                let spk = sentence.speaker_id.clone();
                let original = sentence.text.clone();
                let start_ms = sentence.start_ms;
                let end_ms = sentence.end_ms;
                tokio::spawn(async move {
                    let _permit = permit; // 释放并发额度
                    if let Err(e) = translate_one(
                        &llm,
                        &app_c,
                        uid,
                        &spk,
                        &original,
                        &tgt,
                        start_ms,
                        end_ms,
                        &mgr,
                    )
                    .await
                    {
                        log_buffer::err(format!(
                            "live_translate: 第 {uid} 句翻译失败：{e}"
                        ));
                    }
                });
            }
            // 通道关闭 = ASR 结束，翻译队列自然退出。
        });

        let _ = app.emit(
            "lt-state",
            serde_json::json!({ "running": true, "mode": mode.as_str() }),
        );
        log_buffer::info(format!(
            "live_translate: 会话启动 mode={} src={} tgt={}",
            mode.as_str(),
            source_lang,
            target_lang
        ));
        Ok(())
    }

    /// 迟到修订：改写已记录句的原文（页面与导出保持一致），不重翻译。
    async fn apply_correction(&self, uid: u64, text: String) {
        let mut guard = self.inner.sentences.lock().await;
        match guard.iter_mut().find(|s| s.utterance_id == uid) {
            Some(rec) => {
                log_buffer::info(format!(
                    "live_translate: 第 {uid} 句收到迟到修订，已更新记录原文"
                ));
                rec.original = text;
            }
            // 修订比定稿先到（极端乱序）：忽略即可，下一次定稿会带最新文本。
            None => log_buffer::warn(format!(
                "live_translate: 第 {uid} 句的修订找不到对应记录，丢弃"
            )),
        }
    }

    /// 停止会话：置停止标志，ASR 收尾后由 `finish` 复位。
    pub async fn stop(&self, app: &tauri::AppHandle) -> Result<(), String> {
        if !self.is_running() {
            return Err("实时翻译未在运行".to_string());
        }
        if let Some(flag) = self.inner.stop_flag.lock().await.as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
        log_buffer::info("live_translate: 收到停止请求，等待 ASR 收尾");
        let _ = app.emit(
            "lt-state",
            serde_json::json!({ "running": false, "stopping": true }),
        );
        Ok(())
    }

    /// 会话收尾（ASR 任务返回后调用）：复位运行标志、清停止信号。
    async fn finish(&self, app: &tauri::AppHandle) {
        self.inner.running.store(false, Ordering::Relaxed);
        *self.inner.stop_flag.lock().await = None;
        let _ = app.emit("lt-state", serde_json::json!({ "running": false }));
        log_buffer::info("live_translate: 会话已结束");
    }

    /// 导出当前会话记录。
    pub async fn export(
        &self,
        app: &tauri::AppHandle,
        format: &str,
    ) -> Result<String, String> {
        let sentences = self.inner.sentences.lock().await.clone();
        if sentences.is_empty() {
            return Err("当前会话还没有翻译记录".to_string());
        }
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|e| format!("无法定位配置目录：{e}"))?
            .join("plugins")
            .join("live-translate")
            .join("exports");
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建导出目录失败：{e}"))?;
        let ts = chrono_like_timestamp();
        let (ext, body) = match format {
            "json" => ("json", render_json(&sentences)?),
            "txt" => ("txt", render_text(&sentences)),
            _ => ("md", render_markdown(&sentences)),
        };
        let path = dir.join(format!("{ts}.{ext}"));
        std::fs::write(&path, body).map_err(|e| format!("写文件失败：{e}"))?;
        let p = path.to_string_lossy().to_string();
        log_buffer::info(format!("live_translate: 已导出 {p}"));
        Ok(p)
    }
}

/// 翻译一句：调 LLM 流式，推 chunk / final，并记录结果。
async fn translate_one(
    llm: &LlmClient,
    app: &tauri::AppHandle,
    utterance_id: u64,
    speaker_id: &str,
    original: &str,
    target_lang: &str,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    mgr: &LiveTranslateManager,
) -> Result<(), String> {
    let messages = vec![ChatMessage::user(original.to_string())];
    let opts = ChatOptions {
        system: Some(translate_prompt(target_lang)),
        temperature: Some(0.2),
        clean: Some(true),
        max_tokens: None,
    };

    let app_c = app.clone();
    let mut full = String::new();
    let result = llm
        .chat_stream(messages, opts, |delta| {
            full.push_str(delta);
            let _ = app_c.emit(
                "lt-translate-chunk",
                serde_json::json!({ "utterance_id": utterance_id, "delta": delta }),
            );
        })
        .await;

    match result {
        Ok(text) => {
            let translation = if text.trim().is_empty() {
                full.trim().to_string()
            } else {
                text.trim().to_string()
            };
            let _ = app.emit(
                "lt-translate-final",
                serde_json::json!({
                    "utterance_id": utterance_id,
                    "translation": translation,
                }),
            );
            mgr.inner.sentences.lock().await.push(RecordedSentence {
                utterance_id,
                speaker_id: speaker_id.to_string(),
                original: original.to_string(),
                translation,
                start_ms,
                end_ms,
            });
            Ok(())
        }
        Err(e) => {
            // 翻译失败：仍把原文记下来（导出时有原文总比空好），译文标注失败。
            let _ = app.emit(
                "lt-translate-final",
                serde_json::json!({
                    "utterance_id": utterance_id,
                    "translation": format!("[翻译失败] {e}"),
                }),
            );
            mgr.inner.sentences.lock().await.push(RecordedSentence {
                utterance_id,
                speaker_id: speaker_id.to_string(),
                original: original.to_string(),
                translation: format!("[翻译失败] {e}"),
                start_ms,
                end_ms,
            });
            Err(e)
        }
    }
}

/// 按目标语言选择翻译 system prompt。
///
/// 术语保护是硬要求：技术会议上 FPGA / beamforming 被"翻译"成中文，
/// 反而让懂行的人看不懂 —— 保留原词比强行本地化更有用。
fn translate_prompt(target_lang: &str) -> String {
    match target_lang {
        "en-US" => "你是专业的技术翻译。将用户输入准确翻译为英文。要求：\
                     1. 保留专业术语（如 FPGA、beamforming、ADC、IEC 60601 等）不翻译；\
                     2. 语气自然流畅，符合英文技术文档习惯；\
                     3. 只输出译文，不要加任何解释、前后缀或标注；\
                     4. 保持原句标点与断句。"
            .to_string(),
        _ => "你是专业的技术翻译。将用户输入准确翻译为简体中文。要求：\
              1. 保留专业术语（如 FPGA、beamforming、ADC、IEC 60601 等）不翻译；\
              2. 语气自然流畅，符合中文技术文档习惯；\
              3. 只输出译文，不要加任何解释、前后缀或标注；\
              4. 保持原句标点与断句。"
            .to_string(),
    }
}

/// 时间戳（`YYYYMMDD-HHMMSS`），零依赖实现。
fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 秒 → UTC 日期时间的纯算术换算（够用即可，不引时区库）。
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // 1970-01-01 起的天数 → 年月日
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// 天数 → (年, 月, 日)。Howard Hinnant 的 civil_from_days 算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 把毫秒时间戳格式化为 `[mm:ss–mm:ss]`；两端都缺则返回空串。
fn fmt_span(start: Option<i64>, end: Option<i64>) -> String {
    fn clock(ms: i64) -> String {
        let total = ms.max(0) / 1000;
        format!("{:02}:{:02}", total / 60, total % 60)
    }
    match (start, end) {
        (Some(s), Some(e)) => format!(" [{}–{}]", clock(s), clock(e)),
        (Some(s), None) => format!(" [{}]", clock(s)),
        (None, Some(e)) => format!(" [–{}]", clock(e)),
        (None, None) => String::new(),
    }
}

fn render_markdown(sentences: &[RecordedSentence]) -> String {
    let mut out = String::from("# 实时翻译记录\n\n");
    for s in sentences {
        out.push_str(&format!(
            "## {} [{}]{}\n",
            s.speaker_id,
            s.utterance_id,
            fmt_span(s.start_ms, s.end_ms)
        ));
        out.push_str(&format!("{}\n", s.original));
        out.push_str(&format!("> {}\n\n", s.translation));
    }
    out
}

fn render_text(sentences: &[RecordedSentence]) -> String {
    let mut out = String::new();
    for s in sentences {
        out.push_str(&format!(
            "[{}]{}\n{}\n",
            s.speaker_id,
            fmt_span(s.start_ms, s.end_ms),
            s.original
        ));
        out.push_str(&format!("      {}\n\n", s.translation));
    }
    out
}

fn render_json(sentences: &[RecordedSentence]) -> Result<String, String> {
    serde_json::to_string_pretty(sentences).map_err(|e| format!("序列化失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_mode_parses_known_values() {
        assert_eq!(TranslateMode::parse("meeting"), TranslateMode::Meeting);
        assert_eq!(TranslateMode::parse("conversation"), TranslateMode::Conversation);
        // 未知值回退对话模式（人数最少、分离最稳），而不是报错。
        assert_eq!(TranslateMode::parse("whatever"), TranslateMode::Conversation);
    }

    /// 术语保护：英→中 prompt 必须点名保留 FPGA 等原词。
    /// 这条错了，懂行的人会在译文里看到"现场可编程门阵列"而失去可读性。
    #[test]
    fn translate_prompt_zh_protects_terms() {
        let p = translate_prompt("zh-CN");
        assert!(p.contains("简体中文"), "目标是中文: {p}");
        assert!(p.contains("FPGA"), "必须点名保留术语: {p}");
        assert!(p.contains("只输出译文"), "必须禁止解释性输出: {p}");
    }

    #[test]
    fn translate_prompt_en_targets_english() {
        let p = translate_prompt("en-US");
        assert!(p.contains("英文"), "目标是英文: {p}");
    }

    /// 导出渲染：三种格式都要带原文与译文，否则导出等于丢失一半信息。
    #[test]
    fn export_renders_original_and_translation() {
        let rows = vec![RecordedSentence {
            utterance_id: 0,
            speaker_id: "spk0".to_string(),
            start_ms: Some(0),
            end_ms: Some(1200),
            original: "Hello".to_string(),
            translation: "你好".to_string(),
        }];
        let md = render_markdown(&rows);
        assert!(md.contains("Hello") && md.contains("你好"), "md 必须两者都有: {md}");
        let txt = render_text(&rows);
        assert!(txt.contains("Hello") && txt.contains("你好"), "txt 必须两者都有: {txt}");
        let js = render_json(&rows).unwrap();
        assert!(js.contains("Hello") && js.contains("你好"), "json 必须两者都有: {js}");
    }

    /// 时间戳必须是 8 位日期 + 6 位时间（文件名可排序、可读）。
    #[test]
    fn timestamp_has_expected_shape() {
        let ts = chrono_like_timestamp();
        assert_eq!(ts.len(), 15, "格式应为 YYYYMMDD-HHMMSS，实际 {ts}");
        assert_eq!(&ts[8..9], "-");
    }
}