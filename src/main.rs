#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]

mod app;
mod db;
mod diag;
mod editkeys;
mod export;
mod graph;
mod icons;
mod img;
mod layout;
mod math;
mod md;
mod probe;
mod render;
mod secret;
mod sync;
mod webdav;
mod theme;
mod update;
mod vault;

use eframe::egui;

/// 시작 때 복구·안전 모드로 생긴 알림(앱이 첫 프레임에 한 번 보여 준다).
static NOTICE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 알림을 덧붙인다(여러 개면 " · "로 이어 한 줄로 보여 준다).
pub fn add_notice(msg: &str) {
    if let Ok(mut n) = NOTICE.lock() {
        *n = Some(match n.take() { Some(old) => format!("{old} · {msg}"), None => msg.to_string() });
    }
}

pub fn take_notice() -> Option<String> {
    NOTICE.lock().ok().and_then(|mut n| n.take())
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // 렌더러 시험 전용 자식 프로세스: 저장소·설정을 열지 않고 결과만 종료 코드로 알린다.
    if let Some(i) = args.iter().position(|a| a == "--probe-renderer") {
        std::process::exit(probe::run(args.get(i + 1).map_or("", |s| s.as_str())));
    }
    let safe_mode = args.iter().any(|a| a == "--safe-mode") || probe::shift_down();
    let mut cfg = app::Config::load();
    let recovered = !cfg.renderer_pending.is_empty();
    // 지난 시작이 첫 화면을 그리기 전에 끝났다면(그래픽 드라이버 충돌 등) 그 렌더러를 믿지 않고 되돌린다.
    if !cfg.renderer_pending.is_empty() {
        let bad = std::mem::take(&mut cfg.renderer_pending);
        let to = if bad == "directx" { "opengl" } else { "directx" };
        let msg = format!("지난 시작에서 {} 렌더러가 첫 화면을 그리지 못해 {}(으)로 되돌렸습니다", kind_for(&bad).name(), kind_for(to).name());
        diag::log(&msg);
        cfg.renderer = to.to_string();
        cfg.save();
        *NOTICE.lock().unwrap() = Some(msg);
    }
    if safe_mode {
        diag::log("안전 모드: 이번 실행만 DirectX 사용");
        *NOTICE.lock().unwrap() = Some("안전 모드: 이번 실행만 DirectX로 시작했습니다".into());
    }
    if app::CONFIG_RESET.load(std::sync::atomic::Ordering::Relaxed) {
        let mut n = NOTICE.lock().unwrap();
        if n.is_none() {
            *n = Some("설정 파일을 읽을 수 없어 기본값으로 시작했습니다(원본은 config.bad.json으로 보관)".into());
        }
    }
    if app::SECRET_UNREADABLE.load(std::sync::atomic::Ordering::Relaxed) {
        let mut n = NOTICE.lock().unwrap();
        if n.is_none() {
            *n = Some("WebDAV 비밀번호·복구 문자열을 이 PC·계정에서 풀 수 없어 비웠습니다. 옵션에서 다시 입력하세요".into());
        }
    }
    let path = std::path::PathBuf::from(&cfg.db_path);
    let db = match db::Db::open(&path) {
        Ok(d) => d,
        Err(e) => {
            // 다른 아카식이 쓰는 중이거나 잠긴 것뿐이면 빈 저장소를 새로 만들어 문서가 사라진 것처럼 보이게 하지 않는다.
            let low = e.to_lowercase();
            if low.contains("locked") || low.contains("busy") {
                rfd::MessageDialog::new()
                    .set_title("아카식")
                    .set_description(format!("문서 저장소가 다른 곳에서 사용 중이거나 잠겨 있습니다.\n{}\n\n이미 실행 중인 아카식을 닫고 다시 실행하세요.\n\n{e}", path.display()))
                    .show();
                return Ok(());
            }
            let fallback = app::default_db_path();
            let ask = rfd::MessageDialog::new()
                .set_title("아카식")
                .set_description(format!("문서 저장소를 열 수 없습니다.\n{}\n\n{e}\n\n기본 위치에 새로 만들고 설정도 그 위치로 바꿀까요?\n(기존 문서 파일은 그대로 남습니다)\n{}", path.display(), fallback.display()))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if ask != rfd::MessageDialogResult::Yes {
                return Ok(());
            }
            cfg.db_path = fallback.to_string_lossy().into_owned();
            match db::Db::open(&fallback) {
                Ok(d) => d,
                Err(e) => {
                    rfd::MessageDialog::new().set_title("아카식").set_description(format!("새 저장소도 만들 수 없습니다: {e}")).show();
                    return Ok(());
                }
            }
        }
    };
    if std::env::args().any(|a| a == "--seed") {
        seed_demo(&db);
    }
    // 지난번 창 크기(옵션으로 늘린 크기)를 이어서 쓴다. 너무 작거나 비정상이면 기본값.
    let (w, h) = if cfg.win_w >= 640.0 && cfg.win_h >= 420.0 && cfg.win_w < 10000.0 && cfg.win_h < 10000.0 { (cfg.win_w, cfg.win_h) } else { (1180.0, 760.0) };
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([w, h]).with_min_inner_size([640.0, 420.0]).with_drag_and_drop(true).with_maximized(cfg.win_max);
    // 창·작업 표시줄 아이콘(파란 사각형 + 흰색 A)
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-128.png")) {
        viewport = viewport.with_icon(icon);
    }
    // 렌더러: 기본은 모든 Windows에 있는 DirectX(12). 옵션/환경변수(AKASHIC_RENDERER)로 바꿀 수 있고,
    // 시작에 실패하면(드라이버 없음·Vulkan 로더 오류 등) 순서대로 다른 렌더러를 자동으로 시도한다.
    // 지난 시작이 실패해 되돌린 직후에는 환경변수로 같은 렌더러를 다시 고르지 않는다(자동 복구가 무력화되므로).
    let env_choice = if recovered { None } else { std::env::var("AKASHIC_RENDERER").ok() };
    let choice = if safe_mode { "directx".to_string() } else { env_choice.unwrap_or_else(|| cfg.renderer.clone()).to_lowercase() };
    let chain = renderer_chain(&choice);
    let state = std::sync::Arc::new(std::sync::Mutex::new(Some((db, cfg))));
    let mut last_err = String::new();
    for kind in chain {
        let options = native_options(kind, viewport.clone());
        ACTIVE.store(kind as u8, std::sync::atomic::Ordering::Relaxed);
        // 초기화 직전에 "시도 중" 표시를 남긴다. 앱이 첫 프레임을 그리면 지운다.
        // 드라이버 충돌로 프로세스가 죽으면 표시가 남아 다음 시작에서 안전한 렌더러로 되돌린다.
        if !safe_mode {
            if let Some((_, c)) = state.lock().unwrap().as_mut() {
                c.renderer_pending = kind.key().to_string();
                c.save();
            }
        }
        let st = state.clone();
        let r = eframe::run_native(
            "아카식",
            options,
            Box::new(move |cc| {
                // 앱을 실제로 만드는 시점에 상태를 넘긴다(그 전에 실패하면 상태가 남아 있어 재시도할 수 있다).
                let (db, cfg) = st.lock().unwrap().take().expect("앱 상태가 이미 사용되었습니다");
                Ok(Box::new(app::App::new(cc, db, cfg)))
            }),
        );
        match r {
            Err(e) if state.lock().unwrap().is_some() => {
                last_err.push_str(&format!("{}: {e}\n", kind.name()));
            }
            other => return other,
        }
    }
    rfd::MessageDialog::new()
        .set_title("아카식")
        .set_description(format!("화면을 만들 수 없습니다. 그래픽 드라이버를 확인하세요.\n\n{last_err}"))
        .show();
    Ok(())
}

/// 렌더러 종류별 eframe 옵션. 선택한 백엔드만 wgpu에 넘긴다(다른 그래픽 드라이버는 읽지 않는다).
fn native_options(kind: Kind, viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    // 시스템 글꼴 전체를 훑어 대체 글꼴을 찾는 기능은 끈다(시작이 느려지고 메모리·디스크 읽기가 큼).
    // 필요한 글꼴(맑은 고딕·Segoe UI·Consolas·이모지·선택한 글꼴)은 theme.rs가 직접 읽는다.
    options.system_font_fallback = false;
    match kind {
        Kind::Gl => options.renderer = eframe::Renderer::Glow,
        Kind::Dx12 | Kind::Vulkan | Kind::Auto => {
            use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew};
            let backends = match kind {
                Kind::Dx12 => eframe::wgpu::Backends::DX12,
                Kind::Vulkan => eframe::wgpu::Backends::VULKAN,
                _ => eframe::wgpu::Backends::PRIMARY,
            };
            let mut setup = WgpuSetupCreateNew::without_display_handle();
            setup.instance_descriptor.backends = backends;
            options.renderer = eframe::Renderer::Wgpu;
            // 창 크기를 끌 때 이전 프레임이 늦게 늘어나 보여 글자가 떨리는 것을 줄인다(프레임 지연 1).
            options.wgpu_options.surface.desired_maximum_frame_latency = Some(1);
            options.wgpu_options.wgpu_setup = WgpuSetup::CreateNew(setup);
        }
    }
    options
}

/// 저장 값(directx/opengl/vulkan/auto) → 종류.
fn kind_for(key: &str) -> Kind {
    match key {
        "opengl" | "gl" | "glow" => Kind::Gl,
        "vulkan" => Kind::Vulkan,
        "auto" | "wgpu" => Kind::Auto,
        _ => Kind::Dx12,
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Dx12 = 0,
    Gl = 1,
    Vulkan = 2,
    Auto = 3,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Dx12 => "directx",
            Kind::Gl => "opengl",
            Kind::Vulkan => "vulkan",
            Kind::Auto => "auto",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Kind::Dx12 => "DirectX",
            Kind::Gl => "OpenGL",
            Kind::Vulkan => "Vulkan",
            Kind::Auto => "자동",
        }
    }
}

static ACTIVE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// 옵션에 보여 줄 렌더러 목록(저장 값, 표시 이름).
pub const RENDERERS: &[(&str, &str)] = &[
    ("directx", "DirectX (기본 · 모든 Windows)"),
    ("opengl", "OpenGL (가볍고 메모리 적음)"),
    ("vulkan", "Vulkan"),
    ("auto", "자동 (그래픽 드라이버가 고름)"),
];

pub fn render_name(key: &str) -> &'static str {
    RENDERERS.iter().find(|(k, _)| *k == key).map_or(RENDERERS[0].1, |(_, v)| *v)
}

/// 지금 실제로 쓰고 있는 렌더러 이름.
pub fn active_renderer() -> &'static str {
    match ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Kind::Gl.name(),
        2 => Kind::Vulkan.name(),
        3 => Kind::Auto.name(),
        _ => Kind::Dx12.name(),
    }
}

/// 선택한 렌더러를 먼저, 실패하면 이어서 시도할 순서.
/// Vulkan·자동(Vulkan 포함)은 사용자가 직접 골랐을 때만 쓴다(드라이버가 프로세스째 죽는 PC가 있다).
fn renderer_chain(choice: &str) -> Vec<Kind> {
    match kind_for(choice) {
        Kind::Gl => vec![Kind::Gl, Kind::Dx12],
        Kind::Vulkan => vec![Kind::Vulkan, Kind::Dx12, Kind::Gl],
        Kind::Auto => vec![Kind::Auto, Kind::Dx12, Kind::Gl],
        Kind::Dx12 => vec![Kind::Dx12, Kind::Gl],
    }
}
/// 개발용: 비어 있는 저장소에 화면 확인용 문서를 채운다(`--seed`).
fn seed_demo(db: &db::Db) {
    if let Err(e) = seed_demo_inner(db) {
        diag::log(&format!("예시 문서 만들기 실패: {e}"));
    }
}

fn seed_demo_inner(db: &db::Db) -> Result<(), String> {
    if db.counts().map(|c| c.0).unwrap_or(1) > 0 {
        return Ok(());
    }
    let q = db.add_category(None, "품질")?;
    let m = db.add_category(Some(q), "측정기")?;
    let w = db.add_category(None, "업무")?;
    // 이모지 표시 확인용
    let emo = db.add_category(None, "가 이모지")?;
    for n in ["📁 프로젝트", "🔥 긴급 🚀", "😀 웃음", "👨‍👩‍👧 가족", "🇰🇷 국기", "한글🙂뒤", "✅ 완료", "❤ 하트"] {
        let _ = db.add_category(Some(emo), n);
    }
    // 깊은 한글 분류 확인용(3~4단계)
    let k = db.add_category(Some(m), "교정 이력")?;
    let _ = db.add_category(Some(k), "2026년 정기 점검")?;
    let _ = db.add_category(Some(k), "가나다라마바사")?;
    // 목차 확인용 긴 문서(제목 여러 단계 + 스크롤이 생길 만큼의 본문)
    let mut long_doc = String::from("# 목차 시험 문서\n\n이 문서는 오른쪽 사이드바의 목차 기능을 확인하기 위한 것입니다.\n");
    for (i, sec) in ["시작하기", "설치와 설정", "기본 사용법", "고급 기능", "문제 해결", "자주 묻는 질문"].iter().enumerate() {
        long_doc.push_str(&format!("\n## {}. {}\n", i + 1, sec));
        for l in 0..14 {
            long_doc.push_str(&format!("{} 구역의 본문 {}번째 줄입니다. 목차를 눌러 이 구역으로 이동해 보세요.\n", sec, l + 1));
        }
        for sub in 1..=2 {
            long_doc.push_str(&format!("\n### {}-{} 세부 항목\n", i + 1, sub));
            for l in 0..5 {
                long_doc.push_str(&format!("세부 내용 {} 입니다.\n", l + 1));
            }
        }
    }
    long_doc.push_str("\n#### 마지막 참고\n끝.\n");
    let docs = [
        ("목차 시험 문서", Some(q), long_doc.as_str()),
        ("마크다운 서식 모음", Some(q), "# 큰 제목\n\n정렬 표\n\n| 이름 | 가운데 | 오른쪽 |\n|:---|:---:|---:|\n| 사과 | a \\| b | 1,200 |\n| 두 줄 셀 | 첫째 줄<br>둘째 줄 | 7 |\n| 아주 긴 이름의 항목입니다 | x | 3 |\n| 한 칸만 |\n\na | b\n--|--\n1 | 2\n\n| 구분선 없음 | 모노 |\n| x | y |\n\n```\n언어 없는 코드 | ---\n  들여쓰기 **굵게 아님**\n```\n\n---\n\n끝 문단.\n\n수식(한 줄에 $$...$$만):\n\n$$\\text{추가해야 하는 양 }x=\\frac{\\text{목표 비율} \\times \\text{기존 무게}}{1 - \\text{목표 비율}}$$\n\n$$E = mc^2 \\qquad \\sqrt{a^2+b^2} \\qquad \\sum_{i=1}^{n} x_i \\approx \\alpha\\pi$$\n\n여러 줄 수식(위아래에 $$만 있는 줄):\n\n$$\n\\begin{aligned}\n(a+b)^2 &= a^2 + 2ab + b^2 \\\\\n&= \\frac{a}{b}\n\\end{aligned}\n$$\n\n$$\nx = \\frac{a \\times b}\n         {1 - c}\n\\\\\ny = 2\n$$\n\n구조 명령:\n\n$$\\dfrac{1}{2} + \\binom{n}{k} + \\sqrt[3]{x^2+1} + \\left( \\frac{a}{b} \\right)^2 + \\left\\{ \\frac{a}{\\frac{b}{c}} \\right.$$\n\n$$\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2} \\qquad \\lim_{x \\to 0} \\frac{\\sin x}{x} = 1 \\qquad \\int\\limits_0^1 x\\,dx \\qquad \\int_0^1 x\\,dx$$\n\n$$\\hat{x} \\ \\bar{y} \\ \\vec{v} \\ \\dot{a} \\ \\ddot{a} \\ \\tilde{z} \\ \\overline{AB} \\ \\underline{CD} \\ \\boxed{E=mc^2} \\ \\cancel{k} \\ \\overset{!}{=} \\ \\underset{a}{b}$$\n\n본문에 **굵게**, *기울임*, ~~취소선~~, `inline code` 를 쓸 수 있습니다.\n\n> 인용문은 이렇게 보입니다\n\n- 목록 항목\n- [ ] 해야 할 일\n- [x] 끝낸 일\n\n1. 번호 목록\n2. 두 번째\n\n[[측정기 점검]] 문서로 이동하려면 Ctrl+클릭\n\n---\n\n| 시료 | 입도 |\n|---|---|\n| A | 12 |\n\n```rust\nfn main() {\n    println!(\"안녕\");\n}\n```\n"),
        ("측정기 점검", Some(m), "# 측정기 점검\n\n매일 아침 점검합니다. 한글과 English가 섞여 있어도 잘 보입니다.\n\n돌아가기: [[마크다운 서식 모음]]\n"),
        ("주간 회의록", Some(w), "## 이번 주\n- 회의 내용 정리\n- 다음 주 계획: 글자가 겹치는 걸 막기 위해 문서의 이름이 겹치지 않게 하라고 했는데 번갯불처럼 튀어 오르는 모습이 보기 안좋다는 의견이 있어서 줄바꿈된 행도 같은 간격으로 들여써서 보여야 한다\n1. 숫자 목록도 마찬가지로 아주 긴 문장이 줄바꿈되어 두 번째 줄이 첫 줄의 글자 위치에 맞춰져야 하며 번호 바로 아래로 파고들면 안 된다\n- [ ] 체크박스 항목도 긴 문장이 줄바꿈되면 체크박스 뒤 글자 위치에 맞춰져야 한다는 것을 확인하는 줄입니다\n\n참고: [[측정기 점검]], [[마크다운 서식 모음]], [[새 아이디어]]\n"),
    ];
    for (t, c, b) in docs {
        let id = db.new_doc(c)?;
        db.save_doc(id, t, b)?;
        let _ = db.add_tag(id, "예시");
    }
    Ok(())
}
