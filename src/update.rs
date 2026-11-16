//! GitHub Releases 기반 자동 업데이트(새 크레이트 없이 Windows 기본 curl.exe 사용).
//! 순서: 최신 릴리스 조회 → 새 버전이면 Akashic.exe + SHA256SUMS 내려받기 → 해시 확인 → 실행 중인 exe를 .old로 바꾸고 새 파일을 제자리에 둔다.
//! 실행 중인 exe는 덮어쓸 수 없지만 이름 변경은 되므로, 다음 시작부터 새 버전이 쓰인다(지금 실행 중인 프로그램은 건드리지 않는다).
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 릴리스를 올리는 GitHub 저장소("사용자/저장소"). 비어 있으면 업데이트 기능이 꺼진다.
pub const REPO: &str = match option_env!("AKASHIC_REPO") {
    Some(r) => r,
    None => "",
};
/// 릴리스에 올리는 실행 파일 이름과 해시 목록 이름.
pub const ASSET_EXE: &str = "Akashic.exe";
pub const ASSET_SUMS: &str = "SHA256SUMS";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub version: (u32, u32, u32),
    pub exe_url: String,
    pub sums_url: String,
}

/// `v2.0.1` / `2.0.1` / `2.0` → (2,0,1). 숫자가 아닌 꼬리(`-beta`)는 버린다.
pub fn parse_ver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().map_or(Some(0), |x| x.parse().ok())?;
    let c = it.next().map_or(Some(0), |x| x.parse().ok())?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

pub fn current_version() -> (u32, u32, u32) {
    parse_ver(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

pub fn fmt_ver(v: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// GitHub API의 releases/latest 응답에서 필요한 값만 뽑는다. 시험판(prerelease)·초안은 거른다.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("응답을 읽을 수 없음: {e}"))?;
    if let Some(m) = v.get("message").and_then(|m| m.as_str()) {
        return Err(format!("GitHub: {m}"));
    }
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Err("정식 릴리스가 아닙니다".into());
    }
    let tag = v["tag_name"].as_str().ok_or("tag_name 없음")?.to_string();
    let version = parse_ver(&tag).ok_or_else(|| format!("버전을 읽을 수 없음: {tag}"))?;
    let find = |name: &str| {
        v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(name))).and_then(|x| x["browser_download_url"].as_str()).map(String::from)
    };
    let exe_url = find(ASSET_EXE).ok_or_else(|| format!("릴리스에 {ASSET_EXE}가 없음"))?;
    let sums_url = find(ASSET_SUMS).ok_or_else(|| format!("릴리스에 {ASSET_SUMS}가 없음"))?;
    Ok(Release { tag, version, exe_url, sums_url })
}

/// `sha256sum` 형식(`<64자리 해시>  <파일명>`)에서 파일의 해시를 찾는다(소문자).
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let h = it.next()?;
        let f = it.next()?.trim_start_matches('*');
        (f == name && h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
    })
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn curl() -> Command {
    let sys = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("curl.exe")).filter(|p| p.exists());
    let mut c = Command::new(sys.unwrap_or_else(|| PathBuf::from("curl")));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.args(["-fsSL", "-A", "Akashic-updater", "--proto", "=https", "--max-time"]);
    c
}

fn run_curl(secs: u32, extra: &[&str], url: &str) -> Result<Vec<u8>, String> {
    let out = curl().arg(secs.to_string()).args(extra).arg(url).output().map_err(|e| format!("curl.exe를 실행할 수 없음: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        return Err(if err.is_empty() { format!("내려받기 실패(코드 {:?})", out.status.code()) } else { err.chars().take(160).collect() });
    }
    Ok(out.stdout)
}

/// 최신 릴리스를 조회한다. 저장소가 설정되지 않았으면 Err.
pub fn fetch_latest() -> Result<Release, String> {
    if REPO.is_empty() {
        return Err("업데이트 저장소가 설정되지 않았습니다".into());
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = run_curl(20, &["-H", "Accept: application/vnd.github+json"], &url)?;
    parse_release(&String::from_utf8_lossy(&body))
}

pub fn is_newer(r: &Release) -> bool {
    r.version > current_version()
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// 새 파일을 내려받아 해시를 확인하고 `exe` 자리에 놓는다. 성공하면 `exe`(새 버전)의 경로를 돌려준다.
pub fn install(rel: &Release, exe: &Path) -> Result<PathBuf, String> {
    let sums = run_curl(30, &[], &rel.sums_url)?;
    let want = parse_sums(&String::from_utf8_lossy(&sums), ASSET_EXE).ok_or("SHA256SUMS에 해시가 없음")?;
    let new = with_suffix(exe, ".new");
    let _ = std::fs::remove_file(&new);
    let r = run_curl(600, &["-o", &new.to_string_lossy()], &rel.exe_url).map(|_| ());
    if let Err(e) = r {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    let got = sha256_file(&new).map_err(|e| format!("내려받은 파일을 읽을 수 없음: {e}"))?;
    if got != want {
        let _ = std::fs::remove_file(&new);
        return Err("내려받은 파일의 해시가 맞지 않아 설치하지 않았습니다".into());
    }
    if std::fs::metadata(&new).map_or(true, |m| m.len() < 1_000_000) {
        let _ = std::fs::remove_file(&new);
        return Err("내려받은 파일이 너무 작아 설치하지 않았습니다".into());
    }
    swap(exe, &new)?;
    Ok(exe.to_path_buf())
}

/// 실행 중일 수 있는 `exe`를 `.old`로 비켜 두고 `new`를 그 자리에 놓는다. 실패하면 원래대로 되돌린다.
pub fn swap(exe: &Path, new: &Path) -> Result<(), String> {
    let old = with_suffix(exe, ".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("현재 프로그램 이름을 바꿀 수 없음(쓰기 권한 확인): {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("새 프로그램을 놓을 수 없음: {e}"));
    }
    Ok(())
}

/// 지난 업데이트가 남긴 `.old`·`.new` 찌꺼기를 지운다(시작할 때 호출, 실패해도 무시).
pub fn cleanup(exe: &Path) {
    let _ = std::fs::remove_file(with_suffix(exe, ".old"));
    let _ = std::fs::remove_file(with_suffix(exe, ".new"));
}

/// 잠시 뒤(지금 프로그램이 끝난 다음) `exe`를 다시 실행한다.
pub fn relaunch_later(exe: &Path) {
    let mut c = Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.args(["/C", "ping -n 3 127.0.0.1 >nul & start \"\" "]).arg(format!("\"{}\"", exe.display()));
    let _ = c.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_ver("v2.0.1"), Some((2, 0, 1)));
        assert_eq!(parse_ver("2.1"), Some((2, 1, 0)));
        assert_eq!(parse_ver("v3.0.0-beta.1"), Some((3, 0, 0)));
        assert_eq!(parse_ver("1.2.3.4"), None);
        assert_eq!(parse_ver("abc"), None);
        assert!(parse_ver("v2.0.10") > parse_ver("v2.0.9"));
        assert!(parse_ver("v2.10.0") > parse_ver("v2.9.9"));
        assert!(current_version() > (0, 0, 0));
    }

    #[test]
    fn release_json() {
        let j = r#"{"tag_name":"v2.1.0","draft":false,"prerelease":false,"assets":[
            {"name":"SHA256SUMS","browser_download_url":"https://x/s"},
            {"name":"Akashic.exe","browser_download_url":"https://x/e"}]}"#;
        let r = parse_release(j).unwrap();
        assert_eq!((r.version, r.exe_url.as_str(), r.sums_url.as_str()), ((2, 1, 0), "https://x/e", "https://x/s"));
        assert!(parse_release(r#"{"message":"Not Found"}"#).unwrap_err().contains("Not Found"));
        assert!(parse_release(r#"{"tag_name":"v9.0.0","prerelease":true,"assets":[]}"#).is_err());
        assert!(parse_release(r#"{"tag_name":"v9.0.0","assets":[]}"#).is_err());
        assert!(parse_release("not json").is_err());
    }

    #[test]
    fn sums_lines() {
        let h = "a".repeat(64);
        let t = format!("{h}  Akashic.exe\n{}  other.zip\n", "b".repeat(64));
        assert_eq!(parse_sums(&t, "Akashic.exe"), Some(h.clone()));
        assert_eq!(parse_sums(&format!("{}  *Akashic.exe", h.to_uppercase()), "Akashic.exe"), Some(h));
        assert_eq!(parse_sums("short  Akashic.exe", "Akashic.exe"), None);
        assert_eq!(parse_sums(&t, "none.exe"), None);
    }

    #[test]
    fn hash_and_swap() {
        let d = std::env::temp_dir().join(format!("akashic-upd-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (exe, new) = (d.join("A.exe"), d.join("A.exe.new"));
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&new, b"abc").unwrap();
        assert_eq!(sha256_file(&new).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"abc");
        assert_eq!(std::fs::read(with_suffix(&exe, ".old")).unwrap(), b"old");
        assert!(!new.exists());
        // 새 파일이 없으면 원래대로 돌아온다
        assert!(swap(&exe, &new).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"abc");
        cleanup(&exe);
        assert!(!with_suffix(&exe, ".old").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
