//! 录音识别会话 —— Hold / Call 两种触发模式的状态机。
//!
//! - Hold（按住说）：PTT 按下启动一次录音识别，松手发负包收尾，最终文本注入。
//! - Call（打电话）：PTT 按一下进入通话态，持续录音，服务端 VAD 自动断句，
//!   每句定稿即注入上屏、自动继续下一句，再按一下挂断收尾。
//!
//! 会话以代数（generation）标识：任务收尾时只有「仍是当前会话」才清空活动槽，
//! 防止快速连按 PTT 时上一会话的文本注入到新会话中间。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::{asr, audio, keyboard_audio, text_delivery};

/// 显示后台字幕浮层：先定位到鼠标光标下方居中，再显示。弹出后不再跟随移动。
fn show_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        if let Some((x, y)) = crate::cursor_subtitle_pos(crate::OVERLAY_W, crate::OVERLAY_H) {
            let _ = win.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = win.show();
    }
}

/// 隐藏后台字幕浮层（若存在）。
fn hide_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}

/// 显示编辑浮层（语音编辑专用）。
fn show_edit_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("edit_overlay") {
        if let Some((x, y)) = crate::cursor_subtitle_pos(400.0, 120.0) {
            let _ = win.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = win.show();
    }
}

/// 隐藏编辑浮层（若存在）。
fn hide_edit_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("edit_overlay") {
        let _ = win.hide();
    }
}

/// 单次会话最长录音时长（对齐原版 max_recording_duration_secs=180）。
const MAX_DURATION: Duration = Duration::from_secs(180);
/// Call 模式最长通话时长（30 分钟兜底，正常由「再按一次」结束）。
const MAX_CALL_DURATION: Duration = Duration::from_secs(1800);

/// 触发模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    Hold,
    Call,
}

/// 麦克风来源（用户在通信配置面板选择）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MicSource {
    /// 自动：优先键盘 Wi-Fi 音频，回退电脑麦克风。
    Auto,
    /// 强制使用电脑本机麦克风。
    Pc,
    /// 强制使用键盘 Wi-Fi 音频（离线时回退电脑）。
    Wifi,
    /// 蓝牙键盘音频（暂不支持，回退电脑）。
    Bluetooth,
}

/// 语音编辑阶段（toggle 模式状态机）。
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EditPhase {
    /// 等待按下编辑键。
    Idle,
    /// 语音识别中，字幕实时显示。
    Listening,
    /// LLM 处理中。
    Processing,
}

#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionInner>,
}

/// 落盘持久化的全部配置，保存在 `app_config_dir/config.json`。
#[derive(Serialize, Deserialize, Default)]
struct PersistConfig {
    // ASR
    #[serde(default)]
    asr_url: Option<String>,
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    // LLM
    #[serde(default)]
    llm_backend: Option<String>,
    #[serde(default)]
    llm_endpoint: Option<String>,
    #[serde(default)]
    llm_model: Option<String>,
    #[serde(default)]
    llm_api_key: Option<String>,
    #[serde(default)]
    llm_timeout_secs: Option<u32>,
    // PTT
    #[serde(default)]
    ptt_spec: Option<String>,
    #[serde(default)]
    edit_ptt_spec: Option<String>,
    #[serde(default)]
    trigger_mode: Option<String>,
    // 通信
    #[serde(default)]
    mic_source: Option<String>,
    #[serde(default)]
    wifi_ssid: Option<String>,
    #[serde(default)]
    wifi_password: Option<String>,
    #[serde(default)]
    ble_enabled: Option<bool>,
    // UI
    #[serde(default)]
    overlay_enabled: Option<bool>,
}

struct SessionInner {
    current: Mutex<Option<Arc<Active>>>,
    generation: AtomicU64,
    api_key: Mutex<String>,
    asr_url: Mutex<String>,
    resource_id: Mutex<String>,
    mode: Mutex<TriggerMode>,
    mic_source: Mutex<MicSource>,
    config_path: Mutex<Option<PathBuf>>,
    overlay_enabled: AtomicBool,
    keyboard_audio: Mutex<Option<keyboard_audio::KeyboardAudioManager>>,
    voice_edit: Mutex<Option<crate::voice_edit::VoiceEditManager>>,
    edit_phase: Mutex<EditPhase>,
    wifi_ssid: Mutex<String>,
    wifi_password: Mutex<String>,
    ble_enabled: AtomicBool,
}

struct Active {
    stop_flag: Arc<AtomicBool>,
    gen: u64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SessionInner {
                current: Mutex::new(None),
                generation: AtomicU64::new(0),
                api_key: Mutex::new(String::new()),
                asr_url: Mutex::new(
                    "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async".to_string(),
                ),
                resource_id: Mutex::new("volc.seedasr.sauc.duration".to_string()),
                mode: Mutex::new(TriggerMode::Hold),
                mic_source: Mutex::new(MicSource::Auto),
                config_path: Mutex::new(None),
                overlay_enabled: AtomicBool::new(true),
                keyboard_audio: Mutex::new(None),
                voice_edit: Mutex::new(None),
                edit_phase: Mutex::new(EditPhase::Idle),
                wifi_ssid: Mutex::new(String::new()),
                wifi_password: Mutex::new(String::new()),
                ble_enabled: AtomicBool::new(false),
            }),
        }
    }

    /// 设定配置文件的落盘路径，并立即加载已保存的配置。
    pub fn init_config_file(&self, path: PathBuf) {
        *self.inner.config_path.lock().unwrap() = Some(path.clone());
        self.load_from_disk();
    }

    /// 从磁盘加载配置到内存（仅覆盖「文件里实际存在」的字段）。
    fn load_from_disk(&self) {
        let path = match self.inner.config_path.lock().unwrap().as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let Ok(data) = std::fs::read_to_string(&path) else {
            return;
        };
        // 兼容带 UTF-8 BOM 的文本编辑器写入结果（serde_json 不接受前置 BOM）。
        let data = data.trim_start_matches('\u{feff}');
        let Ok(cfg) = serde_json::from_str::<PersistConfig>(data) else {
            return;
        };
        if let Some(v) = cfg.asr_url.filter(|v| !v.is_empty()) {
            *self.inner.asr_url.lock().unwrap() = v;
        }
        if let Some(v) = cfg.resource_id.filter(|v| !v.is_empty()) {
            *self.inner.resource_id.lock().unwrap() = v;
        }
        if let Some(v) = cfg.api_key {
            *self.inner.api_key.lock().unwrap() = v.trim().to_string();
        }
        if let Some(v) = cfg.trigger_mode.as_deref() {
            match v {
                "call" => *self.inner.mode.lock().unwrap() = TriggerMode::Call,
                _ => *self.inner.mode.lock().unwrap() = TriggerMode::Hold,
            }
        }
        if let Some(v) = cfg.mic_source.as_deref() {
            match v {
                "pc" => *self.inner.mic_source.lock().unwrap() = MicSource::Pc,
                "wifi" => *self.inner.mic_source.lock().unwrap() = MicSource::Wifi,
                "bluetooth" => *self.inner.mic_source.lock().unwrap() = MicSource::Bluetooth,
                _ => *self.inner.mic_source.lock().unwrap() = MicSource::Auto,
            }
        }
        if let Some(v) = cfg.overlay_enabled {
            self.inner.overlay_enabled.store(v, Ordering::Relaxed);
        }
        // LLM 配置分发到 VoiceEditManager
        if let Some(ve) = self.inner.voice_edit.lock().unwrap().as_ref() {
            let llm_cfg = ve.llm_client().config();
            let updated = crate::llm_client::LlmConfig {
                backend: cfg.llm_backend.as_deref().and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok()).unwrap_or(llm_cfg.backend),
                endpoint: cfg.llm_endpoint.clone().unwrap_or(llm_cfg.endpoint),
                model: cfg.llm_model.clone().unwrap_or(llm_cfg.model),
                api_key: cfg.llm_api_key.clone().unwrap_or(llm_cfg.api_key),
                timeout_secs: cfg.llm_timeout_secs.unwrap_or(llm_cfg.timeout_secs),
            };
            crate::log_buffer::info(format!(
                "从磁盘加载 LLM 配置: backend={:?} model={} endpoint={} key_len={} timeout={}s",
                updated.backend, updated.model, updated.endpoint, updated.api_key.len(), updated.timeout_secs
            ));
            ve.llm_client().set_config(updated);
        } else {
            crate::log_buffer::warn("load_from_disk: voice_edit 未设置，LLM 配置未加载");
        }
        // PTT 键分发到 ptt_hook
        if let Some(spec) = cfg.ptt_spec.as_deref() {
            if let Ok(key) = crate::ptt_hook::PttKey::parse(spec) {
                let _ = crate::ptt_hook::set_ptt_key(key);
            }
        }
        if let Some(spec) = cfg.edit_ptt_spec.as_deref() {
            if let Ok(key) = crate::ptt_hook::PttKey::parse(spec) {
                let _ = crate::ptt_hook::set_edit_ptt_key(Some(key));
            }
        }
        // Wi-Fi 配置
        if let Some(v) = cfg.wifi_ssid {
            *self.inner.wifi_ssid.lock().unwrap() = v;
        }
        if let Some(v) = cfg.wifi_password {
            *self.inner.wifi_password.lock().unwrap() = v;
        }
        if let Some(v) = cfg.ble_enabled {
            self.inner.ble_enabled.store(v, Ordering::Relaxed);
        }
    }

    /// 将当前所有配置写入磁盘（仅在已设置路径时生效）。
    fn persist(&self) {
        let path = match self.inner.config_path.lock().unwrap().as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let mut cfg = PersistConfig {
            asr_url: Some(self.asr_url()),
            resource_id: Some(self.resource_id()),
            api_key: Some(self.api_key()),
            trigger_mode: Some(match self.mode() {
                TriggerMode::Hold => "hold".to_string(),
                TriggerMode::Call => "call".to_string(),
            }),
            mic_source: Some(match self.mic_source() {
                MicSource::Auto => "auto".to_string(),
                MicSource::Pc => "pc".to_string(),
                MicSource::Wifi => "wifi".to_string(),
                MicSource::Bluetooth => "bluetooth".to_string(),
            }),
            overlay_enabled: Some(self.overlay_enabled()),
            ptt_spec: crate::ptt_hook::ptt_key().map(|k| k.label),
            edit_ptt_spec: crate::ptt_hook::edit_ptt_key().map(|k| k.label),
            wifi_ssid: Some(self.inner.wifi_ssid.lock().unwrap().clone()),
            wifi_password: Some(self.inner.wifi_password.lock().unwrap().clone()),
            ble_enabled: Some(self.inner.ble_enabled.load(Ordering::Relaxed)),
            ..Default::default()
        };
        // LLM 配置
        if let Some(ve) = self.inner.voice_edit.lock().unwrap().as_ref() {
            let llm = ve.llm_client().config();
            crate::log_buffer::info(format!(
                "持久化 LLM 配置: backend={:?} model={} key_len={} timeout={}s",
                llm.backend, llm.model, llm.api_key.len(), llm.timeout_secs
            ));
            cfg.llm_backend = Some(serde_json::to_string(&llm.backend).unwrap_or_default().trim_matches('"').to_string());
            cfg.llm_endpoint = Some(llm.endpoint);
            cfg.llm_model = Some(llm.model);
            cfg.llm_api_key = Some(llm.api_key);
            cfg.llm_timeout_secs = Some(llm.timeout_secs);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// 持久化全部配置（公开接口，供 lib.rs 命令调用）。
    pub fn persist_all(&self) {
        self.persist();
    }

    /// 重新从磁盘加载配置并分发到各模块（供 lib.rs 在所有模块初始化后调用）。
    pub fn reload_config(&self) {
        self.load_from_disk();
    }

    /// 设置 Wi-Fi 配置并持久化。
    pub fn set_wifi_config(&self, ssid: &str, password: &str) {
        *self.inner.wifi_ssid.lock().unwrap() = ssid.to_string();
        *self.inner.wifi_password.lock().unwrap() = password.to_string();
        self.persist();
    }

    /// 获取 Wi-Fi 配置 (ssid, password)。
    pub fn get_wifi_config(&self) -> (String, String) {
        (
            self.inner.wifi_ssid.lock().unwrap().clone(),
            self.inner.wifi_password.lock().unwrap().clone(),
        )
    }

    /// 设置蓝牙启用状态并持久化。
    pub fn set_ble_enabled(&self, enabled: bool) {
        self.inner.ble_enabled.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    /// 获取蓝牙启用状态。
    pub fn ble_enabled(&self) -> bool {
        self.inner.ble_enabled.load(Ordering::Relaxed)
    }

    pub fn set_api_key(&self, key: &str) {
        *self.inner.api_key.lock().unwrap() = key.to_string();
        self.persist();
    }

    pub fn set_asr_url(&self, url: &str) {
        *self.inner.asr_url.lock().unwrap() = url.to_string();
        self.persist();
    }

    pub fn set_resource_id(&self, id: &str) {
        *self.inner.resource_id.lock().unwrap() = id.to_string();
        self.persist();
    }

    pub fn set_mode(&self, mode: TriggerMode) {
        *self.inner.mode.lock().unwrap() = mode;
        self.persist();
    }

    pub fn mode(&self) -> TriggerMode {
        *self.inner.mode.lock().unwrap()
    }

    pub fn set_mic_source(&self, source: MicSource) {
        *self.inner.mic_source.lock().unwrap() = source;
        self.persist();
    }

    pub fn mic_source(&self) -> MicSource {
        *self.inner.mic_source.lock().unwrap()
    }

    pub fn set_overlay_enabled(&self, enabled: bool) {
        self.inner.overlay_enabled.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    pub fn overlay_enabled(&self) -> bool {
        self.inner.overlay_enabled.load(Ordering::Relaxed)
    }

    /// 设置键盘音频管理器（APP 启动时调用，UDP 绑定成功后传入）。
    pub fn set_keyboard_audio(&self, ka: keyboard_audio::KeyboardAudioManager) {
        *self.inner.keyboard_audio.lock().unwrap() = Some(ka);
    }

    /// 开发板 Wi-Fi 音频是否在线（最近 5 秒内收到过心跳）。
    pub fn keyboard_audio_online(&self) -> bool {
        self.inner
            .keyboard_audio
            .lock()
            .unwrap()
            .as_ref()
            .map(|k| k.is_online())
            .unwrap_or(false)
    }

    /// 尝试获取键盘音频管理器克隆（供监听线程使用）。
    fn keyboard_audio_clone(&self) -> Option<keyboard_audio::KeyboardAudioManager> {
        self.inner.keyboard_audio.lock().unwrap().clone()
    }

    /// 设置语音编辑管理器（APP 启动时调用）。
    pub fn set_voice_edit(&self, ve: crate::voice_edit::VoiceEditManager) {
        *self.inner.voice_edit.lock().unwrap() = Some(ve);
    }

    /// 当前编辑阶段。
    pub fn edit_phase(&self) -> EditPhase {
        *self.inner.edit_phase.lock().unwrap()
    }

    /// 编辑 PTT toggle：第一次按开始语音识别，第二次按确认并提交 LLM 编辑。
    pub fn edit_ptt_down(&self, app: tauri::AppHandle) {
        let mut phase = self.inner.edit_phase.lock().unwrap();
        match *phase {
            EditPhase::Idle => {
                *phase = EditPhase::Listening;
                drop(phase);
                self.start_edit_listening(app);
            }
            EditPhase::Listening => {
                *phase = EditPhase::Processing;
                drop(phase);
                self.stop_edit_listening(app);
            }
            EditPhase::Processing => {}
        }
    }

    pub fn api_key(&self) -> String {
        self.inner.api_key.lock().unwrap().clone()
    }

    pub fn asr_url(&self) -> String {
        self.inner.asr_url.lock().unwrap().clone()
    }

    pub fn resource_id(&self) -> String {
        self.inner.resource_id.lock().unwrap().clone()
    }

    fn has_active(&self) -> bool {
        self.inner.current.lock().unwrap().is_some()
    }

    fn register(&self, stop_flag: Arc<AtomicBool>) -> u64 {
        let gen = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.inner.current.lock().unwrap() = Some(Arc::new(Active {
            stop_flag,
            gen,
        }));
        gen
    }

    fn finish(&self, gen: u64) {
        let mut cur = self.inner.current.lock().unwrap();
        if cur.as_ref().map(|a| a.gen) == Some(gen) {
            *cur = None;
        }
    }

    /// 置位当前会话的停止标志（若有）。
    fn stop_active(&self) {
        if let Some(a) = self.inner.current.lock().unwrap().as_ref() {
            a.stop_flag.store(true, Ordering::Relaxed);
        }
    }

    /// PTT 按下：单次按键 toggle 模式。
    /// 首次按下启动录音识别并弹出字幕浮层；再次按下结束录音并注入文本。
    pub fn ptt_down(&self, app: tauri::AppHandle) {
        match self.mode() {
            TriggerMode::Hold => {
                if self.has_active() {
                    self.stop_active();
                } else {
                    self.start_hold(app);
                }
            }
            TriggerMode::Call => {
                // 打电话：按一下开，再按一下关。
                if self.has_active() {
                    self.stop_active();
                } else {
                    self.start_call(app);
                }
            }
        }
    }

    /// PTT 松开：单次按键模式下不再停止录音，需再次按下才结束。
    pub fn ptt_up(&self) {}

    /// 手动停止：不限模式，强制结束当前会话。
    pub fn force_stop(&self) {
        self.stop_active();
    }

    /// Hold：一次录音识别 + 注入。
    fn start_hold(&self, app: tauri::AppHandle) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("asr-state", "请先在设置里填写 API Key");
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone());

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        let ka = self.keyboard_audio_clone();
        let use_keyboard = match self.mic_source() {
            MicSource::Auto => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Pc => false,
            MicSource::Wifi => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Bluetooth => false,
        };

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("asr-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("asr-state", "正在听，请说话…");
            if session.overlay_enabled() {
                show_overlay(&app_asr);
            }
            let result = asr::run_recognition(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
            )
            .await;
            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            match result {
                Ok(text) => session.inject_one(app_asr.clone(), text).await,
                Err(e) => {
                    let _ = app_asr.emit("asr-state", format!("识别失败: {e}"));
                }
            }
            hide_overlay(&app_asr);
        });
    }

    /// Call：持续通话，每句注入，再按一次挂断。
    fn start_call(&self, app: tauri::AppHandle) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("asr-state", "请先在设置里填写 API Key");
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone());

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        // 定稿句队列：识别任务产出 → 注入 worker 串行上屏。
        let (inj_tx, inj_rx) = tokio::sync::mpsc::channel::<String>(16);

        let ka = self.keyboard_audio_clone();
        let use_keyboard = match self.mic_source() {
            MicSource::Auto => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Pc => false,
            MicSource::Wifi => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Bluetooth => false,
        };

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_CALL_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("asr-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_CALL_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        // 注入 worker：串行消费定稿句，逐句上屏。
        let app_inj = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = inj_rx;
            while let Some(text) = rx.recv().await {
                let app2 = app_inj.clone();
                let text2 = text.clone();
                let injected =
                    tauri::async_runtime::spawn_blocking(move || text_delivery::deliver(&text2))
                        .await;
                match injected {
                    Ok(Ok(())) => {
                        let _ = app2.emit("asr-final", &text);
                        let _ = app2.emit("asr-state", "通话中：自动断句上屏…");
                    }
                    Ok(Err(e)) => {
                        let _ = app2.emit("asr-state", format!("上屏失败: {e}"));
                    }
                    Err(e) => {
                        let _ = app2.emit("asr-state", format!("上屏任务失败: {e}"));
                    }
                }
            }
        });

        // 识别任务：持续流式识别，定稿句送注入队列。
        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("asr-state", "通话中：说完一句自动上屏，再按一次挂断");
            if session.overlay_enabled() {
                show_overlay(&app_asr);
            }
            let result = asr::run_call(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
                inj_tx,
            )
            .await;
            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            if let Err(e) = result {
                let _ = app_asr.emit("asr-state", format!("识别失败: {e}"));
            } else {
                let _ = app_asr.emit("asr-state", "通话结束");
            }
            hide_overlay(&app_asr);
        });
    }

    /// 注入单句并更新状态。
    async fn inject_one(&self, app: tauri::AppHandle, raw: String) {
        let text = raw.trim().to_string();
        if text.is_empty() {
            let _ = app.emit("asr-state", "未识别到内容");
            return;
        }
        let _ = app.emit("asr-state", "正在上屏…");
        let text_for_inject = text.clone();
        let injected =
            tauri::async_runtime::spawn_blocking(move || text_delivery::deliver(&text_for_inject))
                .await;
        match injected {
            Ok(Ok(())) => {
                let _ = app.emit("asr-final", &text);
                let _ = app.emit("asr-state", "已上屏");
            }
            Ok(Err(e)) => {
                let _ = app.emit("asr-state", format!("上屏失败: {e}"));
            }
            Err(e) => {
                let _ = app.emit("asr-state", format!("上屏任务失败: {e}"));
            }
        }
    }

    /// 启动编辑模式语音识别（第一次按编辑键）。
    fn start_edit_listening(&self, app: tauri::AppHandle) {
        let api_key = self.api_key();
        let asr_url = self.asr_url();
        let resource_id = self.resource_id();
        if api_key.trim().is_empty() {
            let _ = app.emit("edit-state", "请先在设置里填写 API Key");
            *self.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let gen = self.register(stop_flag.clone());

        let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        let ka = self.keyboard_audio_clone();
        let use_keyboard = match self.mic_source() {
            MicSource::Auto => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Pc => false,
            MicSource::Wifi => ka.as_ref().map(|k| k.is_online()).unwrap_or(false),
            MicSource::Bluetooth => false,
        };

        if use_keyboard {
            let ka = ka.unwrap();
            ka.start_streaming(gen as u64, audio_tx);
            let flag_watch = stop_flag.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                while !flag_watch.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                ka.stop_streaming();
            });
        } else {
            let flag_rec = stop_flag.clone();
            let app_rec = app.clone();
            std::thread::spawn(move || {
                let capture = match audio::AudioCapture::start(audio_tx) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_rec.emit("edit-state", format!("录音失败: {e}"));
                        return;
                    }
                };
                let started = Instant::now();
                while !flag_rec.load(Ordering::Relaxed) && started.elapsed() < MAX_DURATION {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(capture);
            });
        }

        let app_asr = app.clone();
        let session = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_asr.emit("edit-state", "编辑聆听中，请说出修改要求…");
            hide_overlay(&app_asr);
            show_edit_overlay(&app_asr);

            let result = asr::run_recognition(
                app_asr.clone(),
                api_key,
                asr_url,
                resource_id,
                audio_rx,
                stop_flag.clone(),
            )
            .await;

            stop_flag.store(true, Ordering::Relaxed);
            session.finish(gen);

            match result {
                Ok(text) => {
                    let text = text.trim().to_string();
                    if text.is_empty() {
                        let _ = app_asr.emit("edit-state", "未识别到内容");
                        *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                        hide_edit_overlay(&app_asr);
                        return;
                    }

                    let _ = app_asr.emit("edit-state", format!("识别完成: {text}"));

                    let ve = session.inner.voice_edit.lock().unwrap().clone();
                    if let Some(ve) = ve {
                        // execute_edit 负责浮层的显示/隐藏（含错误 2 秒延迟关闭）。
                        let _ = ve.execute_edit(text, app_asr.clone()).await;
                    } else {
                        let _ = app_asr.emit("edit-state", "编辑管理器未初始化");
                        hide_edit_overlay(&app_asr);
                    }

                    *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                }
                Err(e) => {
                    let _ = app_asr.emit("edit-state", format!("识别失败: {e}"));
                    *session.inner.edit_phase.lock().unwrap() = EditPhase::Idle;
                    hide_edit_overlay(&app_asr);
                }
            }
        });
    }

    /// 停止编辑模式语音识别（第二次按编辑键，确认提交）。
    fn stop_edit_listening(&self, _app: tauri::AppHandle) {
        self.stop_active();
    }
}
