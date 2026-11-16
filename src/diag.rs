//! 개발용 진단: `AKASHIC_DIAG=1`이면 파일 시스템 접근 같은 지점의 횟수를 세어 시작 10초 뒤 로그에 적는다.
use std::collections::BTreeMap;
use std::sync::Mutex;

static COUNTS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("AKASHIC_DIAG").is_some())
}

/// 이름 붙은 지점을 한 번 지났다고 센다(꺼져 있으면 아무것도 안 함).
pub fn count(name: &'static str) {
    add(name, 1);
}

/// 이름 붙은 합계에 `n`을 더한다(읽은 바이트 수 등).
pub fn add(name: &'static str, n: u64) {
    if enabled() {
        if let Ok(mut m) = COUNTS.lock() {
            *m.entry(name).or_insert(0) += n;
        }
    }
}

/// 지금까지의 횟수를 로그 파일에 덧붙인다.
pub fn dump(tag: &str) {
    if !enabled() {
        return;
    }
    let mut s = format!("[{tag}] ");
    if let Ok(m) = COUNTS.lock() {
        for (k, v) in m.iter() {
            s.push_str(&format!("{k}={v} "));
        }
    }
    log(&s);
}

/// 로그 한 줄(%AppData%\Akashic\akashic.log). 실패해도 조용히 넘어간다.
pub fn log(line: &str) {
    use std::io::Write;
    let p = crate::app::app_dir().join("akashic.log");
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new(".")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), line);
    }
}

/// 매 프레임 호출: 시작 10초 뒤 한 번 횟수를 기록한다(진단이 꺼져 있으면 바로 끝).
pub fn tick() {
    if !enabled() {
        return;
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    static DONE: AtomicBool = AtomicBool::new(false);
    let t0 = START.get_or_init(std::time::Instant::now);
    if t0.elapsed().as_secs() >= 10 && !DONE.swap(true, Ordering::Relaxed) {
        dump("시작 10초");
    }
}
