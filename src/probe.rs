//! 렌더러 시험: 그래픽 드라이버가 프로세스째 죽는 PC(예: Vulkan 드라이버 접근 위반)에서도
//! 앱이 같이 죽지 않도록, 별도 프로세스(`--probe-renderer <이름>`)로 먼저 시험해 본다.
use eframe::egui;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    Running,
    Ok,
    /// 사용 불가: 사유(짧게)
    Fail(String),
}

static FRAMES: AtomicU32 = AtomicU32::new(0);

struct ProbeApp;

impl eframe::App for ProbeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        let ctx = ui.ctx().clone();
        if n >= 3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }
}

/// 자식 프로세스 본체: 화면 밖의 작은 창으로 몇 프레임을 그려 보고 종료 코드로 결과를 알린다.
/// 0 = 정상, 2 = 시작 실패(오류 문구는 표준 출력), 3 = 프레임을 그리지 못함.
pub fn run(name: &str) -> i32 {
    let kind = crate::kind_for(name);
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([64.0, 64.0])
        .with_position([-32000.0, -32000.0])
        .with_decorations(false)
        .with_taskbar(false);
    let options = crate::native_options(kind, viewport);
    let r = eframe::run_native("probe", options, Box::new(|_| Ok(Box::new(ProbeApp))));
    match r {
        Ok(()) if FRAMES.load(Ordering::Relaxed) >= 2 => 0,
        Ok(()) => 3,
        Err(e) => {
            println!("{e}");
            2
        }
    }
}

/// 부모 쪽: 시험 프로세스를 띄우고 최대 `limit` 동안 기다린다. 결과는 채널로 받는다.
pub fn spawn(name: &str, limit: Duration) -> Receiver<Probe> {
    let (tx, rx) = channel();
    let name = name.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(probe_blocking(&name, limit));
    });
    rx
}

fn probe_blocking(name: &str, limit: Duration) -> Probe {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return Probe::Fail(format!("실행 파일 확인 실패: {e}")),
    };
    // CREATE_NO_WINDOW
    let mut child = match Command::new(exe).arg("--probe-renderer").arg(name).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).creation_flags(0x0800_0000).spawn() {
        Ok(c) => c,
        Err(e) => return Probe::Fail(format!("시험 프로세스를 시작하지 못함: {e}")),
    };
    let t0 = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if t0.elapsed() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Probe::Fail(format!("{}초 안에 응답 없음", limit.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Probe::Fail(format!("시험 확인 실패: {e}")),
        }
    };
    let st = result.unwrap();
    let mut out = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_string(&mut out);
    }
    let out = out.lines().next().unwrap_or("").trim().to_string();
    let p = match st.code() {
        Some(0) => Probe::Ok,
        Some(2) => Probe::Fail(if out.is_empty() { "시작 실패".into() } else { out }),
        Some(3) => Probe::Fail("화면을 그리지 못함".into()),
        Some(c) => Probe::Fail(format!("비정상 종료 (코드 0x{:08X})", c as u32)),
        None => Probe::Fail("비정상 종료".into()),
    };
    crate::diag::log(&format!("렌더러 시험 {name}: {p:?}"));
    p
}

/// Shift 키가 눌려 있는지(시작 시 안전 모드 진입용).
pub fn shift_down() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(vk: i32) -> i16;
    }
    // SAFETY: 인자가 정수 하나뿐인 단순 Win32 호출.
    unsafe { (GetAsyncKeyState(0x10) as u16 & 0x8000) != 0 }
}
