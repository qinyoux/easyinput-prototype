//! 下发前的本地校验，严格镜像固件 `config_payload.cpp` / `hid_keycode.cpp` /
//! `host_action_protocol.cpp` 的判定，避免把固件会拒绝的配置发下去。
//!
//! 注意：校验通过并不保证固件一定接受（固件是最终真源），这里只是前置兜底。

use crate::constants::*;
use crate::model::{ActionSpec, AppTarget, Profile, Prompt};

/// 校验一个可打开目标是否填写完整（用于应用库录入/编辑）。
pub fn is_valid_app_target(target: &AppTarget) -> bool {
    match target {
        AppTarget::App { path, .. } => !path.trim().is_empty(),
        AppTarget::File { path } => !path.trim().is_empty(),
        AppTarget::Url { url } => url.starts_with("http://") || url.starts_with("https://"),
        AppTarget::Folder { path } => !path.trim().is_empty(),
    }
}

/// 校验一个动作值；返回 `Ok(())` 或带中文说明的 `Err`。
pub fn validate_action(action: &ActionSpec, input: &str) -> Result<(), String> {
    match action {
        ActionSpec::Disabled => Ok(()),
        ActionSpec::Named(name) => {
            if !NAMED_ACTIONS.contains(&name.as_str())
                && !crate::plugin_registry::is_plugin_action(&name)
            {
                return Err(format!(
                    "「{input}」的具名动作「{name}」不在固件词表中"
                ));
            }
            Ok(())
        }
        ActionSpec::Hotkey(hotkey) => validate_hotkey(hotkey, input),
        ActionSpec::Text(text) => validate_text(text, input),
        ActionSpec::HostAction(value) => validate_host_action(value, input),
        ActionSpec::TaskDesk(_) => {
            // 槽位号只可能是 UI 里那四个；出现别的值说明配置被改坏，
            // 宁可拒绝下发也不猜一个槽位（猜错 = 语音进错槽，用户无从察觉）。
            if action.desk_slot().is_none() {
                return Err(format!(
                    "「{input}」的任务台槽位非法（应为 1–4，实际 {:?}）",
                    match action {
                        ActionSpec::TaskDesk(v) => v.as_str(),
                        _ => "",
                    }
                ));
            }
            Ok(())
        }
        ActionSpec::Studio(v) => {
            match v.as_str() {
                "prev" | "next" | "confirm" => Ok(()),
                _ => Err(format!(
                    "「{input}」的内容台动作非法（应为 prev / next / confirm，实际 {v:?}）"
                )),
            }
        }
    }
}

/// 校验热键字符串（token 词表 + normalize + 至少一个可识别 token）。
pub fn validate_hotkey(hotkey: &str, input: &str) -> Result<(), String> {
    let trimmed = hotkey.trim();
    if trimmed.is_empty() {
        return Err(format!("「{input}」的组合键不能为空"));
    }
    let mut saw_token = false;
    for part in trimmed.split('+') {
        let token = normalize_token(part);
        if token.is_empty() {
            continue;
        }
        saw_token = true;
        if token == "fn" || token == "function" {
            return Err(format!("「{input}」的组合键不支持 Fn 键"));
        }
        if HOTKEY_MODIFIER_TOKENS.contains(&token.as_str()) {
            continue;
        }
        if !is_named_key(&token) && !is_plain_key(&token) {
            return Err(format!(
                "「{input}」的组合键包含固件不支持的键「{part}」（支持 a-z、0-9、Enter/Escape/Backspace/Tab/Space、方向键、F1-F12）"
            ));
        }
    }
    if !saw_token {
        return Err(format!("「{input}」的组合键不能只含分隔符"));
    }
    Ok(())
}

/// 校验固定文本：非空、UTF-8 字节数 ≤ `FIXED_TEXT_MAX_UTF8_BYTES`。
pub fn validate_text(text: &str, input: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err(format!("「{input}」的固定文本不能为空"));
    }
    let bytes = text.len();
    if bytes > FIXED_TEXT_MAX_UTF8_BYTES {
        return Err(format!(
            "「{input}」的固定文本过长：{bytes} 字节，上限 {FIXED_TEXT_MAX_UTF8_BYTES} 字节"
        ));
    }
    Ok(())
}

/// 校验 host_action 值：必须以 `host_action:` 开头且为规范小写 hex UUID。
pub fn validate_host_action(value: &str, input: &str) -> Result<(), String> {
    if !value.starts_with(HOST_ACTION_PREFIX) {
        return Err(format!("「{input}」的打开应用值必须以 {HOST_ACTION_PREFIX} 开头"));
    }
    let uuid = &value[HOST_ACTION_PREFIX_LEN..];
    if !is_canonical_uuid(uuid) {
        return Err(format!("「{input}」的打开应用 UUID 格式非法（需规范小写 hex UUID）"));
    }
    Ok(())
}

/// 校验方案的提示词库：条数上限 + 每条非空且不超长。
///
/// 提示词不属于 `ALL_INPUTS` 里的任何一个输入，因此 `validate_action` 那套
/// 固件词表校验天然扫不到它 —— 它本来也不下发固件，只需要一组成员自己用得上
/// 的上限（见 `PROMPT_MAX_UTF8_BYTES` / `PROMPT_MAX_COUNT` 的说明）。
pub fn validate_prompts(prompts: &[Prompt]) -> Result<(), String> {
    if prompts.len() > PROMPT_MAX_COUNT {
        return Err(format!("提示词太多（{} 条，上限 {PROMPT_MAX_COUNT} 条）", prompts.len()));
    }
    for prompt in prompts {
        if prompt.text.trim().is_empty() {
            return Err("提示词内容不能为空".into());
        }
        if prompt.text.len() > PROMPT_MAX_UTF8_BYTES {
            return Err(format!(
                "提示词过长（{} 字节，上限 {PROMPT_MAX_UTF8_BYTES} 字节）",
                prompt.text.len()
            ));
        }
    }
    Ok(())
}

/// 校验整个方案：所有 11 个输入均在位且动作合法，提示词库合规。
pub fn validate_profile(profile: &Profile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("方案名不能为空".into());
    }
    for input in crate::model::ALL_INPUTS {
        let action = profile.action_for(input);
        validate_action(&action, input)?;
    }
    // text_caret_select / mouse_drag_select 仅允许绑定到编码器按下。
    for input in crate::model::KEY_INPUTS {
        if let Some(ActionSpec::Named(name)) = profile.keys.get(*input) {
            if name == "text_caret_select" || name == "mouse_drag_select" {
                return Err(format!("「{input}」不支持 text_caret_select（仅编码器按下可用）"));
            }
        }
    }
    validate_prompts(&profile.prompts)?;
    Ok(())
}

/// 校验规范小写 hex UUID：36 字符，连字符位 8/13/18/23，其余为 0-9a-f。
pub fn is_canonical_uuid(uuid: &str) -> bool {
    if uuid.len() != 36 {
        return false;
    }
    let bytes = uuid.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() || (b as char).is_ascii_uppercase() {
                    return false;
                }
            }
        }
    }
    true
}

/// 去掉空格/下划线/连字符并转小写，与固件 `normalize` 一致。
pub fn normalize_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| *c != ' ' && *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 是否为一个合法的「具名键」（经 normalize 后）。
fn is_named_key(token: &str) -> bool {
    HOTKEY_NAMED_KEYS.contains(&token)
}

/// 是否为一个合法的「普通键」（单字符 a-z / 0-9，或 F1-F12，均经 normalize）。
fn is_plain_key(token: &str) -> bool {
    if token.len() == 1 {
        let ch = token.chars().next().unwrap();
        if ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
            return true;
        }
    }
    // F1-F12
    if token.len() >= 2 && token.starts_with('f') {
        if let Ok(n) = token[1..].parse::<u32>() {
            return (1..=12).contains(&n);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_canonical_ok() {
        assert!(is_canonical_uuid("12345678-1234-4abc-8def-1234567890ab"));
        assert!(!is_canonical_uuid("12345678-1234-4ABC-8DEF-1234567890AB"));
        assert!(!is_canonical_uuid("1234567812344abc8def1234567890ab"));
        assert!(!is_canonical_uuid("12345678-1234-4abc-8def-1234567890abx"));
    }

    #[test]
    fn hotkey_tokens() {
        assert!(validate_hotkey("Ctrl+Alt+A", "KEY1").is_ok());
        assert!(validate_hotkey("Foo+Bar", "KEY1").is_err());
        assert!(validate_hotkey("Fn", "KEY1").is_err());
        assert!(validate_hotkey("Home", "KEY1").is_err());
        assert!(validate_hotkey("Return", "KEY1").is_ok());
        assert!(validate_hotkey("F12", "KEY1").is_ok());
    }

    #[test]
    fn text_limit() {
        assert!(validate_text("你好", "KEY1").is_ok());
        let big = "a".repeat(FIXED_TEXT_MAX_UTF8_BYTES + 1);
        assert!(validate_text(&big, "KEY1").is_err());
    }
}
