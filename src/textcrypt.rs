//! 본문 일부 자동 암호화. 편집창에서는 `🔒⟪글⟫`로 읽히고, DB에는 `🔒⟪enc:키번호:암호문⟫`로만 저장된다.
//! 열 때 풀고(`reveal`) 저장할 때 잠근다(`conceal`). 같은 글은 같은 암호문을 다시 써서(캐시) 저장할 때마다 본문이 바뀌지 않게 한다.
//! 키를 모르면(다른 PC 등) 암호문 표식이 그대로 보이고 저장해도 그대로 유지된다.
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const OPEN: &str = "🔒⟪";
pub const CLOSE: &str = "⟫";
const AAD: &[u8] = b"akashic-text-v1";
pub type Key = [u8; 32];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// 키 문자열(64자리 16진수) → 키
pub fn parse_key(s: &str) -> Option<Key> {
    let b = unhex(s.trim())?;
    b.try_into().ok()
}

pub fn key_string(k: &Key) -> String {
    hex(k)
}

pub fn new_key() -> Result<Key, String> {
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).map_err(|e| format!("난수를 얻을 수 없음: {e}"))?;
    Ok(k)
}

/// 키를 구별하는 짧은 번호(키 자체는 알 수 없다)
fn kid(k: &Key) -> String {
    hex(&Sha256::digest([b"akashic-text-kid".as_slice(), k].concat())[..4])
}

/// 글 하나를 잠근다: `enc:키번호:16진수(nonce 24바이트 + 암호문)`
pub fn seal(key: &Key, plain: &str) -> Result<String, String> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|e| format!("난수를 얻을 수 없음: {e}"))?;
    let ct = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| e.to_string())?
        .encrypt((&nonce).into(), Payload { msg: plain.as_bytes(), aad: AAD })
        .map_err(|_| "암호화에 실패했습니다".to_string())?;
    let mut raw = nonce.to_vec();
    raw.extend(ct);
    Ok(format!("enc:{}:{}", kid(key), hex(&raw)))
}

/// `enc:…`를 푼다. 맞는 키가 없거나 깨졌으면 None.
pub fn open(ring: &[Key], token: &str) -> Option<String> {
    let rest = token.strip_prefix("enc:")?;
    let (id, data) = rest.split_once(':')?;
    let raw = unhex(data)?;
    if raw.len() < 24 + 16 {
        return None;
    }
    let key = ring.iter().find(|k| kid(k) == id)?;
    let nonce: [u8; 24] = raw[..24].try_into().ok()?;
    let pt = XChaCha20Poly1305::new_from_slice(key).ok()?.decrypt((&nonce).into(), Payload { msg: &raw[24..], aad: AAD }).ok()?;
    String::from_utf8(pt).ok()
}

/// `🔒⟪…⟫` 구간들을 (시작, 안쪽 시작, 안쪽 끝, 끝)으로 찾는다(바이트 위치).
fn spans(s: &str) -> Vec<(usize, usize, usize, usize)> {
    let mut v = Vec::new();
    let mut from = 0;
    while let Some(a) = s[from..].find(OPEN) {
        let a = from + a;
        let inner = a + OPEN.len();
        let Some(b) = s[inner..].find(CLOSE) else { break };
        v.push((a, inner, inner + b, inner + b + CLOSE.len()));
        from = inner + b + CLOSE.len();
    }
    v
}

pub fn has_markers(s: &str) -> bool {
    s.contains(OPEN)
}

/// DB 본문 → 편집창 본문: 풀 수 있는 암호문을 글로 바꾼다. 풀린 (글 → 암호문)은 `cache`에 기억한다.
pub fn reveal(body: &str, ring: &[Key], cache: &mut HashMap<String, String>) -> String {
    if !has_markers(body) {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for (a, i, j, b) in spans(body) {
        out.push_str(&body[last..a]);
        let inner = &body[i..j];
        match open(ring, inner) {
            Some(plain) => {
                out.push_str(&format!("{OPEN}{plain}{CLOSE}"));
                cache.insert(plain, inner.to_string());
            }
            None => out.push_str(&body[a..b]),
        }
        last = b;
    }
    out.push_str(&body[last..]);
    out
}

/// 편집창 본문 → DB 본문: 글을 암호문으로 바꾼다. 이미 `enc:`인 것(못 푼 암호문)은 그대로 둔다. 빈 표식은 그대로 둔다.
pub fn conceal(body: &str, key: &Key, cache: &mut HashMap<String, String>) -> Result<String, String> {
    if !has_markers(body) {
        return Ok(body.to_string());
    }
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for (a, i, j, b) in spans(body) {
        out.push_str(&body[last..a]);
        let inner = &body[i..j];
        if inner.is_empty() || inner.starts_with("enc:") {
            out.push_str(&body[a..b]);
        } else {
            let token = match cache.get(inner) {
                Some(t) if open(&[*key], t).as_deref() == Some(inner) => t.clone(), // 지금 키로 풀리는 기존 암호문만 재사용
                _ => {
                    let t = seal(key, inner)?;
                    cache.insert(inner.to_string(), t.clone());
                    t
                }
            };
            out.push_str(&format!("{OPEN}{token}{CLOSE}"));
        }
        last = b;
    }
    out.push_str(&body[last..]);
    Ok(out)
}

/// 이 글을 암호화 표식 안에 넣어도 되는가(`⟫`가 있으면 닫는 표식과 헷갈리고, 그림·첨부 링크는 정리 때 놓칠 수 있다).
pub fn can_wrap(sel: &str) -> Result<(), &'static str> {
    if sel.contains(CLOSE) || sel.contains(OPEN) {
        return Err("선택에 `⟫`나 `🔒⟪`가 있어 암호화할 수 없습니다");
    }
    if sel.contains("img:") || sel.contains("[[file:") {
        return Err("그림·첨부 파일 링크가 든 부분은 암호화할 수 없습니다");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(n: u8) -> Key {
        [n; 32]
    }

    #[test]
    fn seal_open_roundtrip_and_wrong_key() {
        let t = seal(&k(1), "비밀 글 🔑").unwrap();
        assert!(t.starts_with("enc:") && !t.contains("비밀"));
        assert_eq!(open(&[k(2), k(1)], &t).as_deref(), Some("비밀 글 🔑"));
        assert_eq!(open(&[k(2)], &t), None);
        assert_ne!(seal(&k(1), "x").unwrap(), seal(&k(1), "x").unwrap()); // nonce가 달라 매번 다름
        let mut bad = t.clone();
        bad.push_str("00");
        assert_eq!(open(&[k(1)], &bad), None);
    }

    #[test]
    fn conceal_then_reveal_is_stable() {
        let key = k(7);
        let mut cache = HashMap::new();
        let body = format!("앞 {OPEN}카드 1234{CLOSE} 뒤\n둘째 {OPEN}한 줄\n두 줄{CLOSE}\n끝");
        let stored = conceal(&body, &key, &mut cache).unwrap();
        assert!(!stored.contains("1234") && !stored.contains("두 줄"));
        assert!(stored.starts_with("앞 🔒⟪enc:") && stored.ends_with("\n끝"));
        // 같은 글을 다시 저장해도 암호문이 바뀌지 않는다
        assert_eq!(conceal(&body, &key, &mut cache).unwrap(), stored);
        // 열면 원래 글, 새 캐시로도 다시 잠가도 같은 결과(열 때 캐시가 채워진다)
        let mut c2 = HashMap::new();
        let shown = reveal(&stored, &[key], &mut c2);
        assert_eq!(shown, body);
        assert_eq!(conceal(&shown, &key, &mut c2).unwrap(), stored);
    }

    #[test]
    fn unknown_key_keeps_token_untouched() {
        let mut cache = HashMap::new();
        let stored = conceal(&format!("{OPEN}남의 비밀{CLOSE}"), &k(1), &mut cache).unwrap();
        let mut c2 = HashMap::new();
        let shown = reveal(&stored, &[k(9)], &mut c2); // 키 없음
        assert_eq!(shown, stored);
        assert_eq!(conceal(&shown, &k(9), &mut c2).unwrap(), stored); // 저장해도 그대로
    }

    #[test]
    fn plain_text_and_empty_markers_pass_through() {
        let mut c = HashMap::new();
        assert_eq!(conceal("일반 글", &k(1), &mut c).unwrap(), "일반 글");
        assert_eq!(conceal("a 🔒⟪⟫ b", &k(1), &mut c).unwrap(), "a 🔒⟪⟫ b");
        assert_eq!(reveal("안 닫힘 🔒⟪abc", &[k(1)], &mut c), "안 닫힘 🔒⟪abc");
        assert!(can_wrap("보통 글").is_ok() && can_wrap("a⟫b").is_err() && can_wrap("![](img:0123456789abcdef 1x1)").is_err());
    }

    #[test]
    fn key_strings_roundtrip() {
        let key = new_key().unwrap();
        assert_eq!(parse_key(&key_string(&key)), Some(key));
        assert_eq!(parse_key("zz"), None);
        assert_eq!(parse_key(&"a".repeat(62)), None);
    }
}
