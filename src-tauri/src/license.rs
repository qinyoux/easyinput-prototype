//! 本地激活码（授权码）校验——无需云端服务器。
//!
//! 设计：Ed25519 非对称签名。私钥由作者离线保管（`scripts/.license_private.key`，gitignored），
//! 公钥硬编码在此处。激活码 = `base64url(载荷JSON) + "~" + base64url(Ed25519签名)`。
//! 任何人都能用公钥验证，但**无法伪造**（没有私钥）。载荷含版本/版次/过期时间/备注/机器绑定。

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// 内置 Ed25519 公钥（32 字节）。由 `node scripts/gen_license.mjs genkey` 生成，私钥离线保存。
/// **改私钥会使所有已发激活码失效——务必谨慎。**
pub const LICENSE_PUBLIC_KEY: [u8; 32] = [
    0x75, 0xb3, 0xcf, 0x5a, 0xfd, 0xf3, 0x3c, 0x8a, 0x3b, 0xc1, 0xa0, 0x15, 0x6b, 0x0b, 0xcc, 0xeb,
    0x4a, 0xb5, 0x3d, 0x89, 0x39, 0x82, 0xcc, 0x22, 0xbd, 0x34, 0xad, 0x9c, 0xcc, 0x92, 0x73, 0xd5,
];

/// 激活码载荷。字段顺序**必须与** `scripts/gen_license.mjs` 的 `JSON.stringify` 完全一致，
/// 否则两端序列化出的字节不同，签名对不上。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LicensePayload {
    /// 协议版本，当前固定为 1。
    pub v: u8,
    /// 版次标识，如 `trial` / `pro` / `team`。
    pub edition: String,
    /// 过期时间（Unix 毫秒）；`None` = 永不过期。
    pub exp: Option<i64>,
    /// 备注（如授权对象、订单号）。
    pub note: String,
    /// 绑定的机器 ID；`None` = 不绑定机器。
    pub mid: Option<String>,
}

/// 校验后的授权信息（返回给前端展示）。
#[derive(Serialize, Clone, Debug)]
pub struct LicenseInfo {
    pub valid: bool,
    pub edition: String,
    pub exp: Option<i64>,
    pub note: String,
    pub machine_bound: bool,
    pub message: String,
}

impl LicenseInfo {
    /// 未注册状态（试用版）。
    pub fn unregistered() -> Self {
        Self {
            valid: false,
            edition: String::new(),
            exp: None,
            note: String::new(),
            machine_bound: false,
            message: "未注册（试用版）".to_string(),
        }
    }

    /// 注册但无效（如过期/篡改）。
    pub fn invalid(message: String) -> Self {
        Self {
            valid: false,
            edition: String::new(),
            exp: None,
            note: String::new(),
            machine_bound: false,
            message,
        }
    }
}

/// 紧凑序列化载荷（与 Node 端 `JSON.stringify` 对齐：无空格、字段按声明顺序）。
fn canonical_payload(payload: &LicensePayload) -> Vec<u8> {
    serde_json::to_vec(payload).expect("LicensePayload 可序列化")
}

/// 稳定的机器指纹（用于可选的机器绑定）。当前取计算机名，足够区分不同设备。
pub fn machine_id() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// 用指定公钥校验激活码（便于单测使用测试公钥）。
pub fn verify_license_with(pubkey: &[u8; 32], code: &str, mid: &str) -> Result<LicenseInfo, String> {
    let normalized = code.replace([' ', '\n', '\r', '\t'], "");
    let (payload_b64, sig_b64) = normalized
        .split_once('~')
        .ok_or_else(|| "激活码格式错误：缺少分隔符（~）".to_string())?;
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| format!("激活码载荷解码失败：{e}"))?;
    let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|e| format!("激活码签名解码失败：{e}"))?;
    if sig_bytes.len() != 64 {
        return Err(format!("签名长度错误：应为 64 字节，实为 {}", sig_bytes.len()));
    }
    let vk = VerifyingKey::from_bytes(pubkey).map_err(|e| format!("内置公钥非法：{e}"))?;
    let sig = Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());
    vk.verify(&payload_bytes, &sig)
        .map_err(|_| "签名校验失败：激活码无效或被篡改".to_string())?;
    let payload: LicensePayload = serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("载荷解析失败：{e}"))?;
    if payload.v != 1 {
        return Err(format!("不支持的激活码版本：{}", payload.v));
    }
    if let Some(exp) = payload.exp {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as i64;
        if now > exp {
            return Err("激活码已过期".to_string());
        }
    }
    let machine_bound = payload.mid.is_some();
    if let Some(bound) = &payload.mid {
        if bound != mid {
            return Err("激活码与本机不匹配（已绑定其他设备）".to_string());
        }
    }
    Ok(LicenseInfo {
        valid: true,
        edition: payload.edition,
        exp: payload.exp,
        note: payload.note,
        machine_bound,
        message: "已注册".to_string(),
    })
}

/// 用内置公钥校验激活码。
pub fn verify_license(code: &str, mid: &str) -> Result<LicenseInfo, String> {
    verify_license_with(&LICENSE_PUBLIC_KEY, code, mid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// 测试用固定种子（**非生产密钥**）。Ed25519 任意 32 字节都是合法种子。
    const TEST_SEED: [u8; 32] = [7u8; 32];

    fn sign_for_test(payload: &LicensePayload) -> String {
        let sk = SigningKey::from_bytes(&TEST_SEED);
        let msg = canonical_payload(payload);
        let sig = sk.sign(&msg);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(msg)
            + "~"
            + &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes())
    }

    fn test_pubkey() -> [u8; 32] {
        SigningKey::from_bytes(&TEST_SEED)
            .verifying_key()
            .to_bytes()
    }

    fn sample_payload() -> LicensePayload {
        LicensePayload {
            v: 1,
            edition: "pro".to_string(),
            exp: None,
            note: "测试专业版".to_string(),
            mid: None,
        }
    }

    #[test]
    fn verify_roundtrip_valid_code() {
        let code = sign_for_test(&sample_payload());
        let info = verify_license_with(&test_pubkey(), &code, "any").unwrap();
        assert!(info.valid);
        assert_eq!(info.edition, "pro");
        assert!(!info.machine_bound);
    }

    #[test]
    fn reject_wrong_signature() {
        let mut code = sign_for_test(&sample_payload());
        let mut bytes = code.into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        code = String::from_utf8(bytes).unwrap();
        assert!(verify_license_with(&test_pubkey(), &code, "any").is_err());
    }

    #[test]
    fn reject_tampered_payload() {
        let code = sign_for_test(&sample_payload());
        let normalized = code.replace([' ', '\n', '\r', '\t'], "");
        let (pl, sg) = normalized.split_once('~').unwrap();
        let mut payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(pl).unwrap();
        // 把 "pro" 改成 "prx"——仍是合法 JSON，但签名对不上
        let idx = payload_bytes.windows(3).position(|w| w == b"pro").unwrap();
        payload_bytes[idx + 2] = b'x';
        let bad_code =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload_bytes) + "~" + sg;
        assert!(verify_license_with(&test_pubkey(), &bad_code, "any").is_err());
    }

    #[test]
    fn reject_expired() {
        let p = LicensePayload {
            v: 1,
            edition: "pro".to_string(),
            exp: Some(1_000_000),
            note: String::new(),
            mid: None,
        };
        let code = sign_for_test(&p);
        let err = verify_license_with(&test_pubkey(), &code, "any").unwrap_err();
        assert!(err.contains("过期"), "实际：{err}");
    }

    #[test]
    fn reject_machine_mismatch() {
        let p = LicensePayload {
            v: 1,
            edition: "pro".to_string(),
            exp: None,
            note: String::new(),
            mid: Some("OTHER-PC".to_string()),
        };
        let code = sign_for_test(&p);
        let err = verify_license_with(&test_pubkey(), &code, "THIS-PC").unwrap_err();
        assert!(err.contains("不匹配"), "实际：{err}");
    }

    #[test]
    fn accept_machine_match() {
        let p = LicensePayload {
            v: 1,
            edition: "pro".to_string(),
            exp: None,
            note: String::new(),
            mid: Some("THIS-PC".to_string()),
        };
        let code = sign_for_test(&p);
        assert!(verify_license_with(&test_pubkey(), &code, "THIS-PC").is_ok());
    }

    #[test]
    fn reject_unknown_version() {
        let mut p = sample_payload();
        p.v = 2;
        let code = sign_for_test(&p);
        let err = verify_license_with(&test_pubkey(), &code, "any").unwrap_err();
        assert!(err.contains("版本"), "实际：{err}");
    }

    #[test]
    fn reject_malformed_code() {
        assert!(verify_license_with(&test_pubkey(), "not-a-valid-code", "any").is_err());
    }
}
