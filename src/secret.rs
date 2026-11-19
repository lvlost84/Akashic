//! 설정 파일에 넣는 비밀값(WebDAV 비밀번호·복구 문자열) 보호.
//! Windows 사용자 계정에 묶인 DPAPI로 암호화한다: 같은 사용자 계정으로 이 PC에서만 풀린다.
//! config.json 을 복사해 가거나 클라우드 폴더에서 새어 나가도 그대로는 읽을 수 없다.
//! (같은 사용자 권한으로 실행되는 악성 프로그램은 막지 못한다.)

/// 보호된 값 앞에 붙는 표식. 표식이 없으면 옛 평문 설정으로 보고 그대로 쓴다.
pub const PREFIX: &str = "dpapi1:";
const ENTROPY: &[u8] = b"akashic-config-secret-v1";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct Blob {
        pub cb: u32,
        pub pb: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(pin: *const Blob, desc: *const u16, entropy: *const Blob, res: *mut c_void, prompt: *mut c_void, flags: u32, pout: *mut Blob) -> i32;
        fn CryptUnprotectData(pin: *const Blob, desc: *mut *mut u16, entropy: *const Blob, res: *mut c_void, prompt: *mut c_void, flags: u32, pout: *mut Blob) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(h: *mut c_void) -> *mut c_void;
    }
    const UI_FORBIDDEN: u32 = 1;

    fn call(data: &[u8], entropy: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = Blob { cb: data.len() as u32, pb: data.as_ptr() as *mut u8 };
        let ent = Blob { cb: entropy.len() as u32, pb: entropy.as_ptr() as *mut u8 };
        let mut out = Blob { cb: 0, pb: std::ptr::null_mut() };
        // SAFETY: 입력·엔트로피 블롭은 호출이 끝날 때까지 살아 있는 슬라이스를 가리키고(읽기만 함),
        // 출력 블롭은 시스템이 LocalAlloc 으로 채우므로 복사한 뒤 LocalFree 로 돌려준다.
        unsafe {
            let ok = if protect {
                CryptProtectData(&input, std::ptr::null(), &ent, std::ptr::null_mut(), std::ptr::null_mut(), UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), &ent, std::ptr::null_mut(), std::ptr::null_mut(), UI_FORBIDDEN, &mut out)
            };
            if ok == 0 || out.pb.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(out.pb, out.cb as usize).to_vec();
            LocalFree(out.pb as *mut c_void);
            Some(v)
        }
    }
    pub fn protect(data: &[u8], entropy: &[u8]) -> Option<Vec<u8>> {
        call(data, entropy, true)
    }
    pub fn unprotect(data: &[u8], entropy: &[u8]) -> Option<Vec<u8>> {
        call(data, entropy, false)
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn protect(_: &[u8], _: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_: &[u8], _: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// 설정 파일에 쓸 값. 빈 값은 그대로 둔다. 암호화에 실패하면 `None`(호출하는 쪽이 저장하지 않고 건너뛴다).
pub fn protect(plain: &str) -> Option<String> {
    if plain.is_empty() {
        return Some(String::new());
    }
    sys::protect(plain.as_bytes(), ENTROPY).map(|b| format!("{PREFIX}{}", hex(&b)))
}

/// 설정 파일에서 읽은 값을 푼다. 표식이 없으면 옛 평문이므로 그대로 돌려준다.
/// 표식이 있는데 풀 수 없으면(다른 PC·다른 사용자 계정, 깨진 값) `None`.
pub fn unprotect(stored: &str) -> Option<String> {
    let Some(h) = stored.strip_prefix(PREFIX) else { return Some(stored.to_string()) };
    let raw = unhex(h)?;
    String::from_utf8(sys::unprotect(&raw, ENTROPY)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stays_empty() {
        assert_eq!(protect(""), Some(String::new()));
        assert_eq!(unprotect(""), Some(String::new()));
    }

    #[test]
    fn legacy_plaintext_passes_through() {
        assert_eq!(unprotect("옛 비밀번호 pw!"), Some("옛 비밀번호 pw!".to_string()));
    }

    #[cfg(windows)]
    #[test]
    fn round_trip_hides_the_secret() {
        let secret = "AKS1-ABCD-비밀번호/한글 pw";
        let p = protect(secret).unwrap();
        assert!(p.starts_with(PREFIX));
        assert!(!p.contains("ABCD") && !p.contains("pw"), "평문이 그대로 보이면 안 된다: {p}");
        assert_eq!(unprotect(&p).as_deref(), Some(secret));
        // 같은 값도 매번 다른 암호문이 나온다(소금)
        assert_ne!(p, protect(secret).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn tampered_or_garbage_blob_is_rejected() {
        let p = protect("secret").unwrap();
        let mut t = p.clone();
        let last = t.pop().unwrap();
        t.push(if last == '0' { '1' } else { '0' });
        assert_eq!(unprotect(&t), None);
        assert_eq!(unprotect(&format!("{PREFIX}zz")), None);
        assert_eq!(unprotect(&format!("{PREFIX}abc")), None);
        assert_eq!(unprotect(&format!("{PREFIX}00112233")), None);
    }
}
