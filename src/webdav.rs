//! WebDAV 전송(`vault::Remote` 구현). 새 크레이트 없이 Windows 기본 `curl.exe`를 쓴다.
//! - 주소는 `https://`만 허용한다(시험용으로 `http://localhost`·`127.0.0.1`·`[::1]`만 예외).
//! - 사용자·비밀번호는 명령줄이 아니라 표준입력 설정(`curl -K -`)으로 넘겨 프로세스 목록에 보이지 않게 한다.
//! - 응답 해석(상태코드·ETag·PROPFIND)은 순수 함수로 분리해 단위 테스트한다.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::vault::{Etag, PutCond, Remote, RemoteError};

/// 연결 시간 제한(초)과 요청 하나의 전체 시간 제한(초).
const CONNECT_SECS: u32 = 15;
const MAX_SECS: u32 = 180;

// ───────────────────────── 순수 함수 ─────────────────────────

/// 주소 검사. `https://`만 허용하고, 시험용으로 로컬 `http://`만 예외로 둔다. 통과하면 (끝의 `/`를 뗀 주소, curl 프로토콜 제한).
pub fn check_url(url: &str) -> Result<(String, &'static str), String> {
    let u = url.trim().trim_end_matches('/');
    if u.is_empty() {
        return Err("서버 주소가 비어 있습니다".into());
    }
    if u.chars().any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '\\') {
        return Err("서버 주소에 쓸 수 없는 글자가 있습니다".into());
    }
    let lower = u.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("https://") {
        if rest.is_empty() || rest.starts_with('/') {
            return Err("서버 주소에 호스트 이름이 없습니다".into());
        }
        return Ok((u.to_string(), "=https"));
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let name = if let Some(v6) = host.strip_prefix('[') { v6.split(']').next().map(|h| format!("[{h}]")).unwrap_or_default() } else { host.split(':').next().unwrap_or("").to_string() };
        if matches!(name.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
            return Ok((u.to_string(), "=http"));
        }
        return Err("보안을 위해 https:// 주소만 쓸 수 있습니다 (같은 PC의 시험용 서버만 http 허용)".into());
    }
    Err("서버 주소는 https:// 로 시작해야 합니다".into())
}

/// curl 설정 파일(`-K`)의 따옴표 문자열로 만든다. 줄바꿈 같은 제어 문자는 설정을 깨뜨리므로 거부한다.
pub fn cfg_quote(s: &str) -> Result<String, String> {
    if s.chars().any(|c| c.is_control()) {
        return Err("제어 문자는 쓸 수 없습니다".into());
    }
    Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
}

/// curl 이 `-D -` 로 내보낸 응답(헤더 + 본문)을 나눈다. 1xx(100 Continue 등) 블록은 건너뛴다.
#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let mut rest = raw;
    loop {
        let end = find(rest, b"\r\n\r\n").map(|p| (p, 4)).or_else(|| find(rest, b"\n\n").map(|p| (p, 2)));
        let Some((p, n)) = end else {
            return Err("서버 응답을 해석할 수 없습니다".into());
        };
        let head = String::from_utf8_lossy(&rest[..p]).into_owned();
        let body = &rest[p + n..];
        let mut lines = head.lines();
        let first = lines.next().unwrap_or("");
        if !first.starts_with("HTTP/") {
            return Err("서버 응답을 해석할 수 없습니다".into());
        }
        let status: u16 = first.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("응답 상태를 읽을 수 없습니다")?;
        if (100..200).contains(&status) {
            rest = body;
            continue;
        }
        let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
        return Ok(Response { status, headers, body: body.to_vec() });
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// 상태코드 → 사용자에게 보일 한국어 오류. 성공이면 None.
pub fn status_error(status: u16) -> Option<String> {
    let msg = match status {
        200..=299 => return None,
        401 | 403 => "인증에 실패했습니다. 사용자 이름·비밀번호와 계정의 폴더 권한을 확인하세요".to_string(),
        404 => "서버에서 경로를 찾을 수 없습니다. 주소(경로)를 확인하세요".to_string(),
        405 => "서버가 이 요청(WebDAV)을 허용하지 않습니다. 주소가 WebDAV 경로인지 확인하세요".to_string(),
        409 => "서버에 상위 폴더가 없습니다. 주소(경로)를 확인하세요".to_string(),
        412 => "조건이 맞지 않습니다(다른 기기가 먼저 바꿈)".to_string(),
        413 => "서버(프록시)가 요청 크기를 거부했습니다. 요청 크기 제한을 1MB 이상으로 늘리세요".to_string(),
        423 => "서버에서 파일이 잠겨 있습니다. 잠시 뒤에 다시 시도하세요".to_string(),
        429 => "요청이 너무 많아 서버가 제한했습니다. 잠시 뒤에 다시 시도하세요".to_string(),
        507 => "서버의 저장 공간이 부족합니다".to_string(),
        500..=599 => format!("서버 오류(HTTP {status})"),
        s => format!("예상하지 못한 응답(HTTP {s})"),
    };
    Some(msg)
}

/// curl 종료 코드·표준 오류 → 한국어 오류(전송 자체가 실패한 경우).
pub fn curl_error(code: Option<i32>, stderr: &str) -> String {
    let base = match code {
        Some(3) => "서버 주소 형식이 잘못되었습니다",
        Some(6) => "서버 이름을 찾을 수 없습니다(주소·인터넷 연결 확인)",
        Some(7) => "서버에 연결할 수 없습니다(주소·포트·방화벽·서버 실행 여부 확인)",
        Some(28) => "서버 응답 시간이 초과되었습니다(네트워크 확인)",
        Some(35) | Some(60) | Some(58) | Some(77) => "HTTPS 인증서 확인에 실패했습니다(인증서·시스템 시계 확인)",
        Some(56) | Some(52) | Some(55) => "연결이 도중에 끊겼습니다(네트워크 확인)",
        Some(1) => "이 주소의 프로토콜은 허용되지 않습니다",
        _ => "서버와 통신하지 못했습니다",
    };
    let e: String = stderr.trim().lines().last().unwrap_or("").chars().take(140).collect();
    if e.is_empty() { format!("{base} (curl 코드 {})", code.map_or("?".into(), |c| c.to_string())) } else { format!("{base} — {e}") }
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hexv = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(h), Some(l)) = (b.get(i + 1).and_then(|c| hexv(*c)), b.get(i + 2).and_then(|c| hexv(*c))) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// PROPFIND(207) 응답에서 파일 이름들을 뽑는다. 폴더(href 가 `/`로 끝남)와 빈 이름은 제외한다.
/// 네임스페이스 접두사(`D:`, `d:`, 없음)는 가리지 않는다.
pub fn parse_propfind(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let Some(gt) = rest.find('>') else { break };
        let tag = rest[..gt].trim();
        let local = tag.rsplit(':').next().unwrap_or(tag);
        if !tag.starts_with('/') && local.eq_ignore_ascii_case("href") {
            let after = &rest[gt + 1..];
            let end = after.find('<').unwrap_or(after.len());
            let href = xml_unescape(after[..end].trim());
            // 전체 주소(http://host/path)여도 경로만 본다.
            let path = href.split(['?', '#']).next().unwrap_or("");
            if !path.ends_with('/') {
                let last = path.rsplit('/').next().unwrap_or("");
                let name = percent_decode(last);
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        rest = &rest[gt + 1..];
    }
    names.sort();
    names
}

// ───────────────────────── curl ─────────────────────────

fn curl_cmd() -> Command {
    let sys = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("curl.exe")).filter(|p| p.exists());
    let mut c = Command::new(sys.unwrap_or_else(|| PathBuf::from("curl")));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 한 번의 요청. 비밀번호는 표준입력 설정으로만 넘어간다.
struct Req<'a> {
    method: &'a str,
    url: String,
    headers: Vec<String>,
    /// 올릴 본문(PUT)
    body: Option<&'a [u8]>,
}

pub struct WebDavRemote {
    /// 끝의 `/` 없는 기본 주소
    base: String,
    proto: &'static str,
    /// 기본 주소 아래의 저장소 폴더 이름(비어 있으면 기본 주소 자체)
    folder: String,
    user: String,
    pass: String,
}

impl WebDavRemote {
    pub fn new(url: &str, user: &str, pass: &str, folder: &str) -> Result<Self, String> {
        let (base, proto) = check_url(url)?;
        if user.is_empty() {
            return Err("사용자 이름이 비어 있습니다".into());
        }
        if user.contains(':') {
            return Err("사용자 이름에 ':' 를 쓸 수 없습니다".into());
        }
        cfg_quote(user)?;
        cfg_quote(pass).map_err(|_| "비밀번호에 제어 문자가 있습니다".to_string())?;
        if folder.contains(['/', '\\']) {
            return Err("잘못된 폴더 이름".into());
        }
        Ok(WebDavRemote { base, proto, folder: folder.to_string(), user: user.to_string(), pass: pass.to_string() })
    }

    fn dir_url(&self) -> String {
        if self.folder.is_empty() { format!("{}/", self.base) } else { format!("{}/{}/", self.base, self.folder) }
    }

    fn file_url(&self, name: &str) -> Result<String, RemoteError> {
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." || name.chars().any(|c| c.is_control() || c == '%' || c == '?' || c == '#' || c == '"' || c == ' ') {
            return Err(RemoteError::Other(format!("잘못된 이름: {name}")));
        }
        Ok(format!("{}{name}", self.dir_url()))
    }

    fn run(&self, req: Req) -> Result<Response, RemoteError> {
        let other = |s: String| RemoteError::Other(s);
        let q = |s: &str| cfg_quote(s).map_err(|e| other(e));
        let mut cfg = String::new();
        cfg.push_str("silent\nshow-error\nno-keepalive\n");
        cfg.push_str(&format!("proto = \"{}\"\n", self.proto));
        cfg.push_str(&format!("connect-timeout = {CONNECT_SECS}\nmax-time = {MAX_SECS}\n"));
        cfg.push_str("user-agent = \"Akashic-sync\"\n");
        cfg.push_str("dump-header = \"-\"\n");
        cfg.push_str(&format!("user = {}\n", q(&format!("{}:{}", self.user, self.pass))?));
        cfg.push_str(&format!("url = {}\n", q(&req.url)?));
        cfg.push_str("header = \"Expect:\"\n");
        let tmp = if let Some(b) = req.body {
            let p = std::env::temp_dir().join(format!("akashic-dav-{}-{}.bin", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
            std::fs::write(&p, b).map_err(|e| other(format!("임시 파일을 쓸 수 없음: {e}")))?;
            cfg.push_str(&format!("upload-file = {}\n", q(&p.to_string_lossy().replace('\\', "/"))?));
            Some(p)
        } else {
            None
        };
        if req.method == "HEAD" {
            cfg.push_str("head
");
        } else if req.body.is_some() {
            cfg.push_str("request = \"PUT\"
");
        } else {
            cfg.push_str(&format!("request = \"{}\"
", req.method));
        }
        for h in &req.headers {
            cfg.push_str(&format!("header = {}\n", q(h)?));
        }
        let out = (|| -> Result<std::process::Output, RemoteError> {
            let mut child = curl_cmd()
                .args(["-K", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| other(format!("curl.exe를 실행할 수 없음: {e}")))?;
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(cfg.as_bytes()); // 닫히면 curl 이 곧 오류를 낸다
            }
            child.wait_with_output().map_err(|e| other(format!("curl.exe 실행 실패: {e}")))
        })();
        if let Some(p) = tmp {
            let _ = std::fs::remove_file(p);
        }
        let out = out?;
        if !out.status.success() {
            return Err(other(curl_error(out.status.code(), &String::from_utf8_lossy(&out.stderr))));
        }
        parse_response(&out.stdout).map_err(other)
    }

    /// 저장소 폴더를 만든다(이미 있으면 405 라서 성공으로 본다).
    fn ensure_dir(&self) -> Result<(), RemoteError> {
        let r = self.run(Req { method: "MKCOL", url: self.dir_url(), headers: vec![], body: None })?;
        match r.status {
            200..=299 | 405 => Ok(()),
            s => Err(RemoteError::Other(status_error(s).unwrap_or_default())),
        }
    }

    fn head_etag(&self, name: &str) -> Result<Etag, RemoteError> {
        let r = self.run(Req { method: "HEAD", url: self.file_url(name)?, headers: vec![], body: None })?;
        if let Some(e) = status_error(r.status) {
            return Err(RemoteError::Other(e));
        }
        r.header("etag").map(|e| make_token(e, r.header("last-modified"))).ok_or_else(|| RemoteError::Other("서버가 ETag를 주지 않습니다".into()))
    }
}

impl Remote for WebDavRemote {
    fn get(&self, name: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError> {
        let r = self.run(Req { method: "GET", url: self.file_url(name)?, headers: vec![], body: None })?;
        if r.status == 404 {
            return Ok(None);
        }
        if let Some(e) = status_error(r.status) {
            return Err(RemoteError::Other(e));
        }
        let etag = make_token(r.header("etag").unwrap_or(""), r.header("last-modified"));
        Ok(Some((r.body, etag)))
    }

    fn put(&self, name: &str, data: &[u8], cond: PutCond) -> Result<Etag, RemoteError> {
        let url = self.file_url(name)?;
        let mut headers = vec!["Content-Type: application/octet-stream".to_string()];
        match &cond {
            PutCond::Any => {}
            PutCond::IfMatch(tok) => {
                let (etag, lm) = split_token(tok);
                if is_weak(etag) {
                    // 약한 ETag(W/"…")는 If-Match 가 받아 주지 않는다(강한 비교만 허용). 올리기 직전에 지금 값이 내가 본 값과
                    // 같은지 직접 확인하고, 서버가 지켜 주는 If-Unmodified-Since(마지막으로 본 수정 시각 + 1초)를 함께 건다.
                    // 서버의 수정 시각은 마이크로초라 같은 초를 그대로 주면 맞는 값도 거절되므로 1초를 더한다.
                    let cur = self.head_etag(name)?;
                    if split_token(&cur).0 != etag {
                        return Err(RemoteError::Conflict);
                    }
                    if let Some(l) = lm.and_then(plus_one_second) {
                        headers.push(format!("If-Unmodified-Since: {l}"));
                    }
                } else {
                    headers.push(format!("If-Match: {etag}"));
                }
            }
            PutCond::IfNoneMatch => headers.push("If-None-Match: *".to_string()),
        }
        let mut r = self.run(Req { method: "PUT", url: url.clone(), headers: headers.clone(), body: Some(data) })?;
        // 저장소 폴더가 아직 없으면 만들고 한 번 더(조건부 갱신은 "있어야 하는" 파일이므로 만들지 않는다).
        if matches!(r.status, 404 | 409) && !matches!(cond, PutCond::IfMatch(_)) {
            self.ensure_dir()?;
            r = self.run(Req { method: "PUT", url, headers, body: Some(data) })?;
        }
        if r.status == 412 {
            return Err(RemoteError::Conflict);
        }
        if let Some(e) = status_error(r.status) {
            return Err(RemoteError::Other(e));
        }
        // 조각(Any)은 ETag 를 쓰지 않으므로 응답 값을 그대로 둔다.
        if matches!(cond, PutCond::Any) {
            return Ok(r.header("etag").unwrap_or("").to_string());
        }
        // 아파치 계열(시놀로지 WebDAV 등)은 PUT 응답의 ETag 가 나중에 읽을 때의 ETag 와 다를 수 있어
        // 그 값으로 If-Match 를 걸면 맞는 값도 412 가 된다. 올린 직후 다시 읽은 값을 쓰고, 읽기에 실패할 때만 응답 값을 쓴다.
        match self.head_etag(name) {
            Ok(e) => Ok(e),
            Err(x) => r.header("etag").map(String::from).ok_or(x),
        }
    }

    fn delete(&self, name: &str) -> Result<(), RemoteError> {
        let r = self.run(Req { method: "DELETE", url: self.file_url(name)?, headers: vec![], body: None })?;
        if r.status == 404 {
            return Ok(());
        }
        match status_error(r.status) {
            None => Ok(()),
            Some(e) => Err(RemoteError::Other(e)),
        }
    }

    fn list(&self) -> Result<Vec<String>, RemoteError> {
        let r = self.run(Req { method: "PROPFIND", url: self.dir_url(), headers: vec!["Depth: 1".into()], body: None })?;
        if r.status == 404 {
            return Ok(Vec::new());
        }
        if r.status != 207 {
            return Err(RemoteError::Other(status_error(r.status).unwrap_or_else(|| format!("예상하지 못한 응답(HTTP {})", r.status))));
        }
        Ok(parse_propfind(&String::from_utf8_lossy(&r.body)))
    }
}

// ───────────────────────── ETag 토큰 ─────────────────────────
// `Remote` 가 주고받는 ETag 는 앱에게 불투명한 문자열이다. 약한 ETag(W/"…")를 주는 서버에서는 마지막 수정 시각을
// 뒤에 공백으로 붙여 `W/"3-abc" Thu, 19 Nov 2026 06:49:50 GMT` 처럼 만든다(ETag 문자에는 공백이 없으므로 처음 공백에서 나눈다).

fn is_weak(etag: &str) -> bool {
    etag.starts_with("W/")
}

fn make_token(etag: &str, last_modified: Option<&str>) -> String {
    match last_modified.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if is_weak(etag) => format!("{etag} {l}"),
        _ => etag.to_string(),
    }
}

fn split_token(token: &str) -> (&str, Option<&str>) {
    match token.split_once(' ') {
        Some((e, l)) => (e, Some(l.trim()).filter(|l| !l.is_empty())),
        None => (token, None),
    }
}

/// HTTP 날짜(`Thu, 19 Nov 2026 06:49:50 GMT`)에 1초를 더해 같은 형식으로 돌려준다. 읽을 수 없으면 None.
fn plus_one_second(http_date: &str) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc2822(http_date).ok()?;
    let t = t.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1);
    Some(t.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
}

// ───────────────────────── 연결 시험 ─────────────────────────

/// 연결 시험: 서버가 ETag 를 주고 `If-Match`·`If-None-Match: *` 를 지키는지 임시 파일로 확인한다.
/// 지키지 않으면 동기화 중 다른 기기의 내용을 덮어쓸 수 있으므로 Err(설정 거부). 시험 파일은 항상 지운다.
pub fn test_connection(url: &str, user: &str, pass: &str) -> Result<String, String> {
    let remote = WebDavRemote::new(url, user, pass, "")?;
    let name = format!("akashic-conntest-{:x}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed) + now_nanos());
    let r = run_test(&remote, &name);
    let _ = remote.delete(&name); // 실패해도 흔적을 최대한 지운다
    r
}

fn now_nanos() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as u64)
}

fn run_test(remote: &dyn Remote, name: &str) -> Result<String, String> {
    let e = |x: RemoteError| match x {
        RemoteError::Conflict => "서버가 새 파일 올리기를 거부했습니다(충돌)".to_string(),
        RemoteError::Other(s) => s,
    };
    let e1 = remote.put(name, b"one", PutCond::IfNoneMatch).map_err(e)?;
    if e1.is_empty() {
        return Err("이 서버는 ETag를 주지 않아 동기화 충돌을 감지할 수 없습니다. 다른 WebDAV 서버를 쓰세요".into());
    }
    // 이미 있는 파일에 If-None-Match: * 로 올리면 거부(412)해야 한다.
    match remote.put(name, b"x", PutCond::IfNoneMatch) {
        Err(RemoteError::Conflict) => {}
        Ok(_) => return Err("이 서버는 If-None-Match 조건을 지키지 않아 다른 기기의 저장소를 덮어쓸 수 있습니다. 다른 WebDAV 서버를 쓰세요".into()),
        Err(x) => return Err(e(x)),
    }
    let e2 = remote
        .put(name, b"two-2", PutCond::IfMatch(e1.clone()))
        .map_err(|x| match x {
            RemoteError::Conflict => "서버가 올바른 ETag의 If-Match 갱신을 412로 거부했습니다. 이 서버는 올리고 나서 읽은 ETag가 일정하지 않아 충돌 감지에 쓸 수 없습니다".to_string(),
            RemoteError::Other(s) => s,
        })?;
    if e2 == e1 {
        return Err("이 서버는 파일을 바꿔도 ETag가 바뀌지 않아 충돌을 감지할 수 없습니다. 다른 WebDAV 서버를 쓰세요".into());
    }
    // 낡은 ETag 로 올리면 거부(412)해야 한다.
    match remote.put(name, b"three", PutCond::IfMatch(e1)) {
        Err(RemoteError::Conflict) => {}
        Ok(_) => return Err("이 서버는 If-Match 조건을 지키지 않아 충돌을 감지할 수 없습니다(다른 기기의 내용을 덮어쓸 수 있음). 다른 WebDAV 서버를 쓰세요".into()),
        Err(x) => return Err(e(x)),
    }
    match remote.get(name).map_err(e)? {
        Some((d, _)) if d == b"two-2" => {}
        _ => return Err("올린 시험 파일을 다시 읽어 확인하지 못했습니다".into()),
    }
    remote.list().map_err(e)?;
    Ok("연결 성공: 충돌 감지(ETag 확인과 조건부 올리기)가 동작하는 서버입니다".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 약한_etag는_수정_시각을_붙여_토큰으로_만들고_다시_나눈다() {
        let lm = "Thu, 19 Nov 2026 06:49:50 GMT";
        let t = make_token("W/\"3-6609b829758a6\"", Some(lm));
        assert_eq!(t, format!("W/\"3-6609b829758a6\" {lm}"));
        assert_eq!(split_token(&t), ("W/\"3-6609b829758a6\"", Some(lm)));
        // 강한 ETag·시각 없음·옛 설정(시각 없이 저장된 값)은 그대로
        assert_eq!(make_token("\"abc\"", Some(lm)), "\"abc\"");
        assert_eq!(make_token("W/\"x\"", None), "W/\"x\"");
        assert_eq!(split_token("W/\"x\""), ("W/\"x\"", None));
        assert_eq!(split_token("\"abc\""), ("\"abc\"", None));
        assert!(is_weak("W/\"x\"") && !is_weak("\"x\""));
    }

    #[test]
    fn 수정_시각에_1초를_더한다() {
        assert_eq!(plus_one_second("Thu, 19 Nov 2026 06:49:50 GMT").as_deref(), Some("Thu, 19 Nov 2026 06:49:51 GMT"));
        assert_eq!(plus_one_second("Thu, 19 Nov 2026 23:59:59 GMT").as_deref(), Some("Fri, 20 Nov 2026 00:00:00 GMT"));
        assert_eq!(plus_one_second("엉터리"), None);
    }

    #[test]
    fn 주소는_https만_허용하고_로컬_http만_예외() {
        assert_eq!(check_url("https://nas.example.com/dav/").unwrap(), ("https://nas.example.com/dav".into(), "=https"));
        assert_eq!(check_url(" HTTPS://a.b ").unwrap().1, "=https");
        assert_eq!(check_url("http://localhost:8080/x").unwrap().1, "=http");
        assert_eq!(check_url("http://127.0.0.1:1/").unwrap().1, "=http");
        assert_eq!(check_url("http://[::1]:9/").unwrap().1, "=http");
        assert!(check_url("http://nas.example.com").is_err());
        assert!(check_url("http://localhost.evil.com").is_err());
        assert!(check_url("http://127.0.0.1.evil.com").is_err());
        assert!(check_url("ftp://x").is_err());
        assert!(check_url("").is_err());
        assert!(check_url("https://").is_err());
        assert!(check_url("https://a b").is_err());
    }

    #[test]
    fn 설정_문자열은_따옴표와_역슬래시를_이스케이프하고_줄바꿈을_거부() {
        assert_eq!(cfg_quote("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        assert!(cfg_quote("a\nb").is_err());
        assert!(cfg_quote("a\rb").is_err());
    }

    #[test]
    fn 응답_해석_1xx_건너뛰고_헤더_본문_분리() {
        let raw = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\nETag: \"abc\"\r\nContent-Length: 3\r\n\r\nxyz";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 201);
        assert_eq!(r.header("etag"), Some("\"abc\""));
        assert_eq!(r.body, b"xyz");
        let r2 = parse_response(b"HTTP/2 404 \r\netag: W/\"x\"\r\n\r\n").unwrap();
        assert_eq!((r2.status, r2.header("ETag"), r2.body.len()), (404, Some("W/\"x\""), 0));
        assert!(parse_response(b"garbage").is_err());
        assert!(parse_response(b"").is_err());
    }

    #[test]
    fn 본문에_빈_줄이_있어도_본문은_그대로() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\n\r\na\r\n\r\nb").unwrap();
        assert_eq!(r.body, b"a\r\n\r\nb");
    }

    #[test]
    fn 상태코드_오류_문구() {
        assert!(status_error(200).is_none() && status_error(207).is_none());
        assert!(status_error(401).unwrap().contains("인증"));
        assert!(status_error(403).unwrap().contains("인증"));
        assert!(status_error(413).unwrap().contains("크기"));
        assert!(status_error(507).unwrap().contains("공간"));
        assert!(status_error(502).unwrap().contains("502"));
        assert!(status_error(404).is_some());
    }

    #[test]
    fn curl_종료코드_문구() {
        assert!(curl_error(Some(6), "").contains("찾을 수 없"));
        assert!(curl_error(Some(7), "Failed to connect").contains("연결할 수 없"));
        assert!(curl_error(Some(28), "").contains("시간"));
        assert!(curl_error(Some(60), "").contains("인증서"));
        assert!(curl_error(Some(99), "").contains("99"));
    }

    #[test]
    fn propfind_이름_추출() {
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:">
 <D:response><D:href>/dav/akashic-ab/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
 <D:response><D:href>/dav/akashic-ab/manifest</D:href></D:response>
 <D:response><D:href>/dav/akashic-ab/1700.0.part</D:href></D:response>
 <D:response><D:href>http://h:80/dav/akashic-ab/a%20b%26c</D:href></D:response>
</D:multistatus>"#;
        assert_eq!(parse_propfind(xml), vec!["1700.0.part", "a b&c", "manifest"]);
        // 접두사 없음·소문자·네임스페이스 다른 접두사
        let x2 = "<multistatus><response><href>/a/</href></response><response><href>/a/f1</href></response></multistatus>";
        assert_eq!(parse_propfind(x2), vec!["f1"]);
        let x3 = "<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/a/x.part</d:href></d:response></d:multistatus>";
        assert_eq!(parse_propfind(x3), vec!["x.part"]);
        assert!(parse_propfind("").is_empty());
        assert!(parse_propfind("<a><b").is_empty());
    }

    #[test]
    fn percent_복호화_경계() {
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%ED%95%9C"), "한");
        assert_eq!(percent_decode("x%zz"), "x%zz");
    }

    #[test]
    fn 원격_생성_검사와_url_조립() {
        assert!(WebDavRemote::new("https://h/dav", "", "p", "f").is_err());
        assert!(WebDavRemote::new("https://h/dav", "u:v", "p", "f").is_err());
        assert!(WebDavRemote::new("https://h/dav", "u", "p\nq", "f").is_err());
        assert!(WebDavRemote::new("https://h/dav", "u", "p", "a/b").is_err());
        let r = WebDavRemote::new("https://h/dav/", "u", "p", "akashic-ab").unwrap();
        assert_eq!(r.dir_url(), "https://h/dav/akashic-ab/");
        assert_eq!(r.file_url("manifest").unwrap(), "https://h/dav/akashic-ab/manifest");
        assert!(r.file_url("../x").is_err());
        assert!(r.file_url("a b").is_err());
        assert!(r.file_url("a?b").is_err());
        let b = WebDavRemote::new("https://h/dav", "u", "p", "").unwrap();
        assert_eq!(b.dir_url(), "https://h/dav/");
    }

    /// 연결 시험 로직은 `vault::testing::MemoryRemote`(ETag·조건부 올리기를 지키는 서버)에서는 통과해야 한다.
    #[test]
    fn 연결_시험은_올바른_서버를_통과() {
        let m = crate::vault::testing::MemoryRemote::new();
        assert!(run_test(&m, "t").is_ok());
    }

    /// If-Match 를 무시하는 서버(항상 덮어씀)는 거부해야 한다.
    #[test]
    fn 연결_시험은_조건을_무시하는_서버를_거부() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        struct Lax(RefCell<HashMap<String, (Vec<u8>, u32)>>);
        impl Remote for Lax {
            fn get(&self, n: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError> {
                Ok(self.0.borrow().get(n).map(|(d, v)| (d.clone(), format!("\"{v}\""))))
            }
            fn put(&self, n: &str, d: &[u8], _c: PutCond) -> Result<Etag, RemoteError> {
                let mut m = self.0.borrow_mut();
                let v = m.get(n).map_or(1, |x| x.1 + 1);
                m.insert(n.into(), (d.to_vec(), v));
                Ok(format!("\"{v}\""))
            }
            fn delete(&self, n: &str) -> Result<(), RemoteError> {
                self.0.borrow_mut().remove(n);
                Ok(())
            }
            fn list(&self) -> Result<Vec<String>, RemoteError> {
                Ok(vec![])
            }
        }
        let e = run_test(&Lax(Default::default()), "t").unwrap_err();
        assert!(e.contains("If-None-Match") || e.contains("If-Match"), "{e}");
    }

    /// ETag 를 주지 않는 서버는 거부해야 한다.
    #[test]
    fn 연결_시험은_etag_없는_서버를_거부() {
        struct NoEtag;
        impl Remote for NoEtag {
            fn get(&self, _: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError> {
                Ok(None)
            }
            fn put(&self, _: &str, _: &[u8], _: PutCond) -> Result<Etag, RemoteError> {
                Ok(String::new())
            }
            fn delete(&self, _: &str) -> Result<(), RemoteError> {
                Ok(())
            }
            fn list(&self) -> Result<Vec<String>, RemoteError> {
                Ok(vec![])
            }
        }
        assert!(run_test(&NoEtag, "t").unwrap_err().contains("ETag"));
    }

    /// 실제 WebDAV 서버가 있어야 하는 통합 시험: `AKASHIC_TEST_DAV=url|user|pass cargo test webdav_통합 -- --ignored`
    #[test]
    #[ignore]
    fn webdav_통합() {
        use crate::vault::{self, VaultError, VaultKey};
        let spec = std::env::var("AKASHIC_TEST_DAV").expect("AKASHIC_TEST_DAV=url|user|pass");
        let mut it = spec.splitn(3, '|');
        let (url, user, pass) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        println!("{}", test_connection(url, user, pass).unwrap());
        assert!(test_connection(url, user, "틀린 비밀번호").unwrap_err().contains("인증"));
        println!("연결 불가: {}", test_connection("http://127.0.0.1:1/dav", user, pass).unwrap_err());

        let key = VaultKey::generate().unwrap();
        let remote = WebDavRemote::new(url, user, pass, &key.folder_name()).unwrap();
        assert!(remote.list().unwrap().is_empty());
        assert!(vault::read_state(&remote, &key).unwrap().is_none());
        let dir = std::env::temp_dir().join(format!("akashic-davtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("snap.db");
        let data: Vec<u8> = (0..1_300_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        std::fs::write(&snap, &data).unwrap();
        let st = vault::push_snapshot(&remote, &key, &snap, None, "A", 1000, vault::CHUNK).unwrap();
        assert_eq!((st.total, st.uploaded), (3, 3));
        // 같은 기준(None)으로 또 올리면 충돌
        assert_eq!(vault::push_snapshot(&remote, &key, &snap, None, "B", 2000, vault::CHUNK), Err(VaultError::Conflict));
        let state = vault::read_state(&remote, &key).unwrap().unwrap();
        assert_eq!(state.etag, st.etag);
        let out = dir.join("out.db");
        vault::fetch_snapshot(&remote, &key, &state.manifest, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), data);
        // 기준을 읽은 뒤 올리면 성공하고 안 바뀐 조각은 건너뛴다
        std::thread::sleep(std::time::Duration::from_millis(1200)); // 초 단위 ETag 를 쓰는 서버(wsgidav) 대비
        let st2 = vault::push_snapshot(&remote, &key, &snap, Some(&state), "A", 3000, vault::CHUNK).unwrap();
        assert_eq!((st2.uploaded, st2.skipped), (0, 3));
        // 낡은 기준으로 올리면 충돌
        assert_eq!(vault::push_snapshot(&remote, &key, &snap, Some(&state), "B", 4000, vault::CHUNK), Err(VaultError::Conflict));
        // 틀린 키
        let other = VaultKey::generate().unwrap();
        assert_eq!(vault::read_state(&remote, &other).unwrap_err(), VaultError::WrongKey);
        // 정리
        for n in remote.list().unwrap() {
            remote.delete(&n).unwrap();
        }
        assert!(remote.list().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
