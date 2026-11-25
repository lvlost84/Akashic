//! 하단 상태바: 저장까지 남은 시간, 데이터·캐시, 백업, 동기화, 문서 통계, 커서 위치.
use super::*;
use super::sync_ui::SyncBar;
use std::time::SystemTime;

/// 본문 통계(길이가 바뀔 때만 다시 센다).
#[derive(Clone, Default)]
pub(super) struct DocStats {
    pub bytes: usize,
    /// 본문 지문(같은 길이의 편집도 알아내기 위함)
    pub key: u64,
    pub chars: usize,
    pub lines: usize,
    pub words: usize,
}

impl DocStats {
    /// 아직 센 적 없음(첫 프레임에 반드시 계산되도록).
    pub fn unset() -> DocStats {
        DocStats { bytes: usize::MAX, ..Default::default() }
    }
    pub fn of(s: &str) -> DocStats {
        // 줄 수는 위치 표시(줄 N)와 같은 기준(개행 수 + 1)으로 센다. 끝의 빈 줄도 한 줄이다.
        let lines = s.bytes().filter(|&b| b == b'\n').count() + 1;
        DocStats { bytes: s.len(), key: crate::render::cache_key(s), chars: s.chars().count(), lines, words: s.split_whitespace().count() }
    }
}

/// 백그라운드에서 읽은 파일 시스템 상태.
pub(super) struct FsSnap {
    db: u64,
    wal: u64,
    backups: usize,
    last_backup: Option<SystemTime>,
}

fn collect_fs(db_path: &std::path::Path) -> FsSnap {
    let len = |p: &std::path::Path| {
        crate::diag::count("fs_stat");
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    };
    let db = len(db_path);
    let wal = len(std::path::Path::new(&format!("{}-wal", db_path.display())));
    let dir = db_path.parent().unwrap_or(std::path::Path::new(".")).join("backup");
    let (mut n, mut newest): (usize, Option<SystemTime>) = (0, None);
    crate::diag::count("backup_dir_read");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for f in rd.flatten() {
            if f.file_name().to_string_lossy().starts_with("vault-") {
                n += 1;
                if let Ok(m) = f.metadata().and_then(|m| m.modified()) {
                    newest = Some(newest.map_or(m, |o| o.max(m)));
                }
            }
        }
    }
    FsSnap { db, wal, backups: n, last_backup: newest }
}

/// 동기화 폴더 확인(접근 가능 여부, 그 폴더의 저장본). 끊어진 네트워크 폴더에서는 오래 멈출 수 있으므로
/// 로컬 정보와 따로, 시간 제한을 두고 읽는다.
fn collect_sync(dir: &str) -> (bool, Option<(i64, String)>) {
    crate::diag::count("sync_dir_check");
    let p = std::path::Path::new(dir);
    if p.is_dir() {
        let remote = crate::sync::read_manifest(p).ok().flatten().map(|m| (m.saved_at, m.device));
        (true, remote)
    } else {
        (false, None)
    }
}

/// WebDAV 서버 확인(접속·키가 맞는지, 서버의 저장본). 실패하면 접근 불가로 본다.
fn collect_wd(url: &str, user: &str, pass: &str, key: &str, doc: bool) -> (bool, Option<(i64, String)>) {
    crate::diag::count("sync_wd_check");
    let r = (|| {
        let k = crate::vault::VaultKey::from_recovery(key).ok()?;
        // 문서 단위 동기화는 스냅숏 방식과 다른 폴더를 쓴다.
        let folder = if doc { super::docsync_ui::doc_folder(&k) } else { k.folder_name() };
        let remote = crate::webdav::WebDavRemote::new(url, user.trim(), pass, &folder).ok()?;
        crate::vault::read_state(&remote, &k).ok()
    })();
    match r {
        Some(st) => (true, st.map(|s| (s.manifest.saved_at, s.manifest.device))),
        None => (false, None),
    }
}

/// 동기화 폴더 확인을 이 시간(초) 안에 끝내지 못하면 "접근 불가"로 본다.
const SYNC_CHECK_LIMIT_SECS: u64 = 15;
/// 저장소 정보(파일 시스템을 매 프레임 읽지 않도록 5초마다 갱신).
#[derive(Default)]
pub(super) struct StorageInfo {
    pub db: u64,
    pub wal: u64,
    pub backups: usize,
    pub last_backup: Option<SystemTime>,
    pub at: Option<Instant>,
    /// 백그라운드 읽기 결과를 기다리는 채널
    pub rx: Option<std::sync::mpsc::Receiver<FsSnap>>,
    /// 동기화 폴더 확인 결과를 기다리는 채널과 시작 시각(시간 제한용)
    pub sync_rx: Option<(std::sync::mpsc::Receiver<(bool, Option<(i64, String)>)>, Instant)>,
    /// 동기화 폴더를 마지막으로 확인한 시각(None이면 다음에 확인)
    pub sync_at: Option<Instant>,
    /// 동기화 폴더 접근 가능 여부(폴더 미설정·아직 모름이면 None)
    pub sync_ok: Option<bool>,
    /// 동기화 폴더에 있는 저장본(시각, 기기)
    pub remote: Option<(i64, String)>,
    /// 프로세스 메모리(작업 집합, 비공개) 바이트와 측정 시각
    pub mem: (u64, u64),
    pub mem_at: Option<Instant>,
}

/// 이 프로세스의 메모리: (작업 집합, 비공개 사용량) 바이트. Windows가 아니면 0.
#[cfg(windows)]
fn process_memory() -> (u64, u64) {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private_usage: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(h: *mut core::ffi::c_void, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, page_fault_count: 0, peak_working_set: 0, working_set: 0, quota_peak_paged: 0, quota_paged: 0, quota_peak_nonpaged: 0, quota_nonpaged: 0, pagefile: 0, peak_pagefile: 0, private_usage: 0 };
    // SAFETY: 크기를 맞춘 구조체 포인터와 현재 프로세스 핸들(의사 핸들)만 넘긴다.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok != 0 { (c.working_set as u64, c.private_usage as u64) } else { (0, 0) }
}

#[cfg(not(windows))]
fn process_memory() -> (u64, u64) {
    (0, 0)
}
/// 키보드 잠금 키 상태: (NumLock, CapsLock, ScrollLock). Windows가 아니면 모두 꺼짐.
#[cfg(windows)]
fn lock_keys() -> (bool, bool, bool) {
    unsafe extern "system" {
        fn GetKeyState(vkey: i32) -> i16;
    }
    // SAFETY: 가상 키 코드만 넘기는 읽기 전용 호출.
    unsafe { (GetKeyState(0x90) & 1 != 0, GetKeyState(0x14) & 1 != 0, GetKeyState(0x91) & 1 != 0) }
}

#[cfg(not(windows))]
fn lock_keys() -> (bool, bool, bool) {
    (false, false, false)
}

/// 잠금 키 표시 칸: 켜지면 강조색 굵게, 꺼지면 흐리게.
fn lock_chip(ui: &mut egui::Ui, name: &str, on: bool, accent: Color32, dim: Color32, tip: &str) {
    let txt = egui::RichText::new(name).size(crate::theme::ui(0.0));
    let txt = if on { txt.strong().color(accent) } else { txt.color(dim) };
    ui.add(egui::Label::new(txt).sense(Sense::hover())).on_hover_text(format!("{tip}: {}", if on { "켜짐" } else { "꺼짐" }));
}
fn clock(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default()
}

fn backup_text(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    let days = (Local::now().date_naive() - dt.date_naive()).num_days();
    match days {
        0 => format!("오늘 {}", dt.format("%H:%M")),
        1 => "어제".into(),
        n if n > 1 => format!("{n}일 전"),
        _ => dt.format("%m-%d").to_string(),
    }
}

/// 상태바 한 칸. 클릭할 수 있으면 true를 돌려준다.
fn seg(ui: &mut egui::Ui, text: String, tip: String, color: Color32, clickable: bool) -> bool {
    let sense = if clickable { Sense::click() } else { Sense::hover() };
    let r = ui.add(egui::Label::new(egui::RichText::new(text).size(crate::theme::ui(0.0)).color(color)).sense(sense));
    if clickable && r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked = r.clicked();
    r.on_hover_text(tip);
    clicked
}

/// 각진 진행 막대(테두리 + 채움).
fn bar(ui: &mut egui::Ui, frac: f32, fill: Color32, line: Color32, tip: String) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(70.0, 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, line), egui::StrokeKind::Inside);
    let mut f = rect.shrink(2.0);
    f.set_width(f.width() * frac.clamp(0.0, 1.0));
    p.rect_filled(f, 0.0, fill);
    r.on_hover_text(tip);
}

/// 끝을 모르는 진행 막대: 조각이 좌우로 오간다(움직이는 동안 화면을 계속 다시 그린다).
fn sweep(ui: &mut egui::Ui, fill: Color32, line: Color32, tip: String) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(70.0, 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, line), egui::StrokeKind::Inside);
    let inner = rect.shrink(2.0);
    let w = inner.width() * 0.3;
    let t = (ui.input(|i| i.time) / 1.2) % 2.0;
    let pos = if t < 1.0 { t } else { 2.0 - t } as f32;
    let x = inner.left() + (inner.width() - w) * pos;
    p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, inner.top()), egui::vec2(w, inner.height())), 0.0, fill);
    ui.ctx().request_repaint_after(Duration::from_millis(50));
    r.on_hover_text(tip);
}

#[derive(Clone, Copy, PartialEq)]
enum Click {
    None,
    Zoom,
    Goto(u8),
}

#[derive(Clone)]
enum Piece {
    Seg { txt: String, tip: String, col: Color32, act: Click },
    Bar { frac: f32, fill: Color32, line: Color32, tip: String },
    /// 끝을 모르는 진행: 막대 안에서 좌우로 움직이는 조각
    Sweep { fill: Color32, line: Color32, tip: String },
    Lock { name: &'static str, on: bool, tip: &'static str },
}

/// 조각이 차지할 가로 폭(그리기 전에 어림한다).
fn piece_width(ui: &egui::Ui, p: &Piece) -> f32 {
    let text_w = |s: &str| ui.ctx().fonts_mut(|f| f.layout_no_wrap(s.to_string(), egui::FontId::proportional(crate::theme::ui(0.0)), Color32::WHITE)).size().x;
    match p {
        Piece::Seg { txt, .. } => text_w(txt),
        Piece::Bar { .. } | Piece::Sweep { .. } => 70.0,
        Piece::Lock { name, .. } => text_w(name),
    }
}
fn draw_piece(ui: &mut egui::Ui, p: &Piece, accent: Color32, dim: Color32, click: &mut Click) {
    match p {
        Piece::Seg { txt, tip, col, act } => {
            if seg(ui, txt.clone(), tip.clone(), *col, *act != Click::None) {
                *click = *act;
            }
        }
        Piece::Bar { frac, fill, line, tip } => bar(ui, *frac, *fill, *line, tip.clone()),
        Piece::Sweep { fill, line, tip } => sweep(ui, *fill, *line, tip.clone()),
        Piece::Lock { name, on, tip } => lock_chip(ui, name, *on, accent, dim, tip),
    }
}
impl App {
    /// 파일 시스템 결과를 받는다(받기만 하므로 가볍다). 새로 읽어야 할 때만 백그라운드 스레드를 띄운다.
    /// UI 프레임에서는 디스크·네트워크 폴더에 직접 접근하지 않는다.
    fn refresh_storage_info(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;
        if let Some(rx) = &self.storage.rx {
            match rx.try_recv() {
                Ok(s) => {
                    self.storage.db = s.db;
                    self.storage.wal = s.wal;
                    self.storage.backups = s.backups;
                    self.storage.last_backup = s.last_backup;
                    self.storage.rx = None;
                    self.storage.at = Some(Instant::now());
                }
                Err(TryRecvError::Disconnected) => self.storage.rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // 동기화 폴더 확인 결과: 시간 안에 오면 반영, 너무 오래 걸리면 접근 불가로 보고 기다림을 포기한다(멈춘 스레드는 버린다).
        if let Some((rx, started)) = &self.storage.sync_rx {
            let mut done = true;
            match rx.try_recv() {
                Ok((ok, remote)) => {
                    self.storage.sync_ok = Some(ok);
                    self.storage.remote = remote;
                }
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => {
                    if started.elapsed() >= Duration::from_secs(SYNC_CHECK_LIMIT_SECS) {
                        self.storage.sync_ok = Some(false);
                        self.storage.remote = None;
                        crate::diag::log("동기화 폴더 확인이 시간 안에 끝나지 않아 접근 불가로 처리");
                    } else {
                        done = false;
                        ctx.request_repaint_after(Duration::from_secs(1));
                    }
                }
            }
            if done {
                self.storage.sync_rx = None;
                self.storage.sync_at = Some(Instant::now());
            }
        }
        // 크기·백업은 변경 신호(at=None)나 60초마다, 동기화 폴더는 시작·동기화·설정 변경이나 30분마다 확인한다.
        let local_due = self.storage.rx.is_none() && self.storage.at.map_or(true, |t| t.elapsed() >= Duration::from_secs(60));
        let sync_due = self.storage.sync_rx.is_none() && self.storage.sync_at.map_or(true, |t| t.elapsed() >= Duration::from_secs(1800));
        if local_due {
            let db_path = self.db.path.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(collect_fs(&db_path));
                ctx2.request_repaint();
            });
            self.storage.rx = Some(rx);
        }
        if sync_due {
            let wd = matches!(self.cfg.sync_mode.as_str(), "webdav" | "webdoc");
            let doc = self.cfg.sync_mode == "webdoc";
            let empty = if wd { self.cfg.wd_url.trim().is_empty() || self.cfg.wd_key.trim().is_empty() } else { self.cfg.sync_dir.trim().is_empty() };
            if empty {
                // 설정이 없으면 확인할 것이 없으므로 시각만 갱신한다.
                self.storage.sync_ok = None;
                self.storage.remote = None;
                self.storage.sync_at = Some(Instant::now());
            } else {
                let dir = self.cfg.sync_dir.clone();
                let (url, user, pass, key) = (self.cfg.wd_url.clone(), self.cfg.wd_user.clone(), self.cfg.wd_pass.clone(), self.cfg.wd_key.clone());
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(if wd { collect_wd(&url, &user, &pass, &key, doc) } else { collect_sync(&dir) });
                    ctx2.request_repaint();
                });
                self.storage.sync_rx = Some((rx, Instant::now()));
            }
        }
    }

    /// 다음 프레임에 파일 시스템 상태를 다시 읽게 한다(`sync`가 참이면 동기화 폴더도).
    pub(super) fn fs_invalidate(&mut self, sync: bool) {
        self.storage.at = None;
        if sync {
            self.storage.sync_at = None;
        }
    }
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        // 길이가 같은 편집(글자 바꾸기)도 반영되도록 빠른 해시로 변경을 알아낸다.
        let body_key = crate::render::cache_key(&self.body);
        if self.stats.bytes != self.body.len() || self.stats.key != body_key {
            self.stats = DocStats::of(&self.body);
        }
        self.refresh_storage_info(&ui.ctx().clone());
        normalize_status(&mut self.cfg.status_items);
        if self.storage.mem_at.map_or(true, |t| t.elapsed() >= Duration::from_secs(300)) {
            self.storage.mem = process_memory();
            self.storage.mem_at = Some(Instant::now());
        }
        // 커서의 줄·칸(커서나 길이가 바뀔 때만 다시 계산)
        if self.pos_cache.0 != self.cursor_idx || self.pos_cache.1 != body_key as usize {
            let upto = &self.body[..byte_of(&self.body, self.cursor_idx)];
            let line = upto.matches('\n').count() + 1;
            let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            self.pos_cache = (self.cursor_idx, body_key as usize, line, col);
        }

        let dim = self.theme.dim;
        let warn = self.theme.code_fg;
        let accent = self.theme.accent;

        // 저장 상태
        let (save_txt, save_col, save_tip) = if self.cur.is_none() {
            ("저장할 문서 없음".to_string(), dim, "문서를 선택하거나 새로 만드세요".to_string())
        } else if self.cur_trashed {
            ("휴지통 문서 · 읽기 전용".to_string(), warn, "복원하면 다시 편집할 수 있습니다".to_string())
        } else if let Some(t) = self.last_edit {
            let total = self.cfg.autosave_min.clamp(1, 20) as u64 * 60;
            let left = total.saturating_sub(t.elapsed().as_secs());
            (format!("저장까지 {}:{:02}", left / 60, left % 60), accent, format!("첫 변경 후 {}분이 지나면 작은 쓰기 로그에 기록합니다(그 전까지는 메모리에 보관).\nCtrl+S·문서 이동·종료 시에는 즉시 저장합니다.", total / 60))
        } else if self.last_saved_ts > 0 {
            (format!("저장됨 {}", clock(self.last_saved_ts)), dim, "마지막으로 DB에 기록한 시각입니다.\n내용이 바뀌지 않으면 쓰지 않습니다.".to_string())
        } else {
            ("저장됨".to_string(), dim, "변경 사항이 없습니다".to_string())
        };

        // 데이터·캐시
        let c = self.counts;
        let data_txt = format!("DB {} · 문서 {}", fmt_size(self.storage.db + self.storage.wal), c.0);
        let layout_cache = if self.galley_cache.is_some() { "사용 중" } else { "비어 있음" };
        let graph_info = match &self.graph {
            Some(g) => format!("노드 {}개", g.nodes.len()),
            None => "닫힘".to_string(),
        };
        let data_tip = format!(
            "저장소: {}\nDB 파일 {} (쓰기 로그 WAL {})\n문서 {} · 휴지통 {} · 분류 {} · 태그 {}\n검색 색인: FTS5 trigram — 저장할 때 함께 갱신되어 항상 최신\n메모리 캐시: DB 페이지 최대 {}MB · 본문 레이아웃 {} · 연결 그래프 {}\n자동 백업 {}개 보관 중(최대 {}개)\n\n클릭: 옵션 > 문서 저장소",
            self.db.path.display(),
            fmt_size(self.storage.db),
            fmt_size(self.storage.wal),
            c.0,
            c.3,
            self.cats.len(),
            self.tags.len(),
            self.cfg.cache_mb,
            layout_cache,
            graph_info,
            self.storage.backups,
            self.cfg.backup_keep
        );

        // 백업
        let (bak_txt, bak_col) = match self.storage.last_backup {
            Some(t) => (format!("백업 {}", backup_text(t)), dim),
            None => ("백업 없음".to_string(), warn),
        };
        let bak_tip = "하루에 한 번 자동으로 백업합니다.\n클릭: 옵션 > 백업·동기화".to_string();

        // 동기화
        let (sync_txt, sync_tip, sync_warn) = self.sync_status();

        // 쓰기 로그 안내(로그가 1MB 이상일 때만 표시)
        let merge_total = self.cfg.merge_min.clamp(5, 720) as f32 * 60.0;
        let merge_left = (merge_total - self.last_merge.elapsed().as_secs_f32()).max(0.0) as u64 / 60;
        let merge_tip = format!(
            "쓰기 로그(WAL) {} — {}분 뒤 본 파일에 병합합니다(병합 간격 {}분).\n{}\n로그가 64MB를 넘거나 프로그램을 시작·종료할 때도 병합합니다.",
            fmt_size(self.storage.wal),
            merge_left,
            self.cfg.merge_min,
            if self.last_merge_ts > 0 { format!("마지막 병합 {}", clock(self.last_merge_ts)) } else { "이번 실행에서는 아직 병합 전".into() }
        );
        let mem_txt = format!("메모리 {}", fmt_size(self.storage.mem.1.max(self.storage.mem.0)));
        let mem_tip = format!(
            "이 프로그램이 쓰는 메모리 (5분마다 갱신)\n비공개 사용량 {} · 작업 집합 {}\n그래픽 렌더러: {} — 대부분 이 값이 좌우합니다(옵션에서 OpenGL로 바꾸면 크게 줄어듭니다)\nDB 캐시 상한 {}MB",
            fmt_size(self.storage.mem.1),
            fmt_size(self.storage.mem.0),
            crate::active_renderer(),
            self.cfg.cache_mb
        );
        let stat_txt = format!("{}자 · {}줄 · {}단어", self.stats.chars, self.stats.lines, self.stats.words);
        let pos_txt = format!("줄 {}, 칸 {}", self.pos_cache.2, self.pos_cache.3);
        let status = self.status.clone();
        let mut goto: Option<u8> = None;
        let mut reset_zoom = false;
        let zoom_txt = format!("글자 {}%", (self.cfg.font_size / 15.0 * 100.0).round() as i32);
        let zoom_tip = format!("본문 글자 크기 {}pt\nCtrl+마우스 휠로 본문 확대·축소, 클릭하면 100%(15pt)로", self.cfg.font_size);

        // 항목별 조각 만들기(보이는 항목만 그린다)
        let (num, caps, scroll) = lock_keys();
        let sync_bar = match self.sync_bar() {
            SyncBar::Hidden => None,
            SyncBar::Busy => Some(Piece::Sweep { fill: accent, line: dim, tip: sync_tip.clone() }),
            SyncBar::Wait(f) => Some(Piece::Bar { frac: f, fill: dim, line: dim, tip: sync_tip.clone() }),
            SyncBar::Warn => Some(Piece::Bar { frac: 1.0, fill: warn, line: warn, tip: sync_tip.clone() }),
        };
        let pieces = |id: &str| -> Vec<Piece> {
            match id {
                "msg" if status != "준비됨" => vec![Piece::Seg { txt: status.clone(), tip: "알림".into(), col: dim, act: Click::None }],
                "pos" => vec![Piece::Seg { txt: pos_txt.clone(), tip: "커서 위치".into(), col: dim, act: Click::None }],
                "stats" => vec![Piece::Seg { txt: stat_txt.clone(), tip: "현재 문서의 글자 수 · 줄 수 · 단어 수".into(), col: dim, act: Click::None }],
                "zoom" => vec![Piece::Seg { txt: zoom_txt.clone(), tip: zoom_tip.clone(), col: dim, act: Click::Zoom }],
                "num" => vec![Piece::Lock { name: "Num", on: num, tip: "NumLock" }],
                "caps" => vec![Piece::Lock { name: "Caps", on: caps, tip: "CapsLock" }],
                "scroll" => vec![Piece::Lock { name: "Scroll", on: scroll, tip: "ScrollLock" }],
                "save" => vec![Piece::Seg { txt: save_txt.clone(), tip: save_tip.clone(), col: save_col, act: Click::None }],
                "merge" if self.storage.wal >= 1 << 20 => vec![Piece::Seg { txt: format!("로그 {}", fmt_size(self.storage.wal)), tip: merge_tip.clone(), col: dim, act: Click::None }],
                "db" => vec![Piece::Seg { txt: data_txt.clone(), tip: data_tip.clone(), col: dim, act: Click::Goto(1) }],
                "backup" => vec![Piece::Seg { txt: bak_txt.clone(), tip: bak_tip.clone(), col: bak_col, act: Click::Goto(2) }],
                "sync" => {
                    let mut v = vec![Piece::Seg { txt: sync_txt.clone(), tip: sync_tip.clone(), col: if sync_warn { warn } else { dim }, act: Click::Goto(2) }];
                    v.extend(sync_bar.clone());
                    v
                }
                "mem" => vec![Piece::Seg { txt: mem_txt.clone(), tip: mem_tip.clone(), col: dim, act: Click::None }],
                _ => vec![],
            }
        };
        let items = self.cfg.status_items.clone();
        let left: Vec<Vec<Piece>> = items.iter().filter(|i| i.show && !i.right).map(|i| pieces(&i.id)).filter(|p| !p.is_empty()).collect();
        let right: Vec<Vec<Piece>> = items.iter().filter(|i| i.show && i.right).map(|i| pieces(&i.id)).filter(|p| !p.is_empty()).collect();
        let mut click: Click = Click::None;

        egui::Panel::bottom("status").show_separator_line(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (n, group) in left.iter().enumerate() {
                    if n > 0 {
                        ui.separator();
                    }
                    for p in group {
                        draw_piece(ui, p, accent, dim, &mut click);
                    }
                }
                // 왼쪽은 항상 남기고, 오른쪽 묶음은 남는 폭에 들어가는 만큼만(오른쪽 끝부터) 보인다.
                let free = (ui.max_rect().right() - ui.cursor().left() - 14.0).max(0.0);
                let sp = ui.spacing().item_spacing.x;
                let mut used = 0.0f32;
                let mut fit = 0usize;
                for group in right.iter().rev() {
                    let w: f32 = group.iter().map(|p| piece_width(ui, p) + sp).sum::<f32>() + if fit > 0 { sp * 2.0 + 1.0 } else { 0.0 };
                    if used + w > free {
                        break;
                    }
                    used += w;
                    fit += 1;
                }
                let skip = right.len() - fit;
                let right: Vec<Vec<Piece>> = right.into_iter().skip(skip).collect();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // 오른쪽 묶음은 오른쪽 끝부터 채우므로 거꾸로 그려야 목록 순서대로 왼쪽→오른쪽으로 읽힌다.
                    for (n, group) in right.iter().rev().enumerate() {
                        if n > 0 {
                            ui.separator();
                        }
                        for p in group.iter().rev() {
                            draw_piece(ui, p, accent, dim, &mut click);
                        }
                    }
                });
            });
        });
        match click {
            Click::Zoom => reset_zoom = true,
            Click::Goto(t) => goto = Some(t),
            Click::None => {}
        }        if reset_zoom {
            self.cfg.font_size = 15.0;
            self.graph_cfg_dirty = true;
        }
        if let Some(tab) = goto {
            self.show_options = true;
            self.opt_tab = tab;
        }
    }

    /// 옵션 "상태 표시줄" 탭: 항목 표시 여부·순서·왼쪽/오른쪽 위치.
    pub(super) fn status_tab(&mut self, ui: &mut egui::Ui) {
        let dim = self.theme.dim;
        normalize_status(&mut self.cfg.status_items);
        ui.label(egui::RichText::new("상태 표시줄").strong());
        ui.label(egui::RichText::new("항목마다 보이기/숨기기, 순서(↑↓), 왼쪽/오른쪽 묶음을 정합니다. 바꾸면 바로 적용됩니다.").size(crate::theme::ui(0.0)).color(dim));
        ui.add_space(4.0);
        // 동작은 목록을 다 그린 뒤 한 번만 적용한다.
        enum Op {
            Up(usize),
            Down(usize),
            Flip(usize),
        }
        let mut op: Option<Op> = None;
        let mut changed = false;
        for (side_right, title) in [(false, "왼쪽 묶음"), (true, "오른쪽 묶음")] {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(title).strong());
            let idxs: Vec<usize> = (0..self.cfg.status_items.len()).filter(|i| self.cfg.status_items[*i].right == side_right).collect();
            if idxs.is_empty() {
                ui.label(egui::RichText::new("(비어 있음)").color(dim));
            }
            egui::Grid::new(("status_grid", side_right)).num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
                for (k, &i) in idxs.iter().enumerate() {
                    let id = self.cfg.status_items[i].id.clone();
                    let name = STATUS_DEFS.iter().find(|d| d.0 == id).map_or("", |d| d.1);
                    if ui.checkbox(&mut self.cfg.status_items[i].show, name).changed() {
                        changed = true;
                    }
                    if crate::icons::btn_ex(ui, crate::icons::Icon::Up, "", k > 0, None).on_hover_text("위로(앞으로)").clicked() {
                        op = Some(Op::Up(i));
                    }
                    if crate::icons::btn_ex(ui, crate::icons::Icon::Down, "", k + 1 < idxs.len(), None).on_hover_text("아래로(뒤로)").clicked() {
                        op = Some(Op::Down(i));
                    }
                    if ui.button(if side_right { "← 왼쪽으로" } else { "오른쪽으로 →" }).clicked() {
                        op = Some(Op::Flip(i));
                    }
                    ui.end_row();
                }
            });
        }
        ui.add_space(6.0);
        if crate::icons::btn(ui, crate::icons::Icon::Restore, "기본값으로").clicked() {
            self.cfg.status_items = default_status_items();
            changed = true;
        }
        let v = &mut self.cfg.status_items;
        match op {
            Some(Op::Up(i)) => {
                if let Some(j) = (0..i).rev().find(|j| v[*j].right == v[i].right) {
                    v.swap(i, j);
                    changed = true;
                }
            }
            Some(Op::Down(i)) => {
                if let Some(j) = ((i + 1)..v.len()).find(|j| v[*j].right == v[i].right) {
                    v.swap(i, j);
                    changed = true;
                }
            }
            Some(Op::Flip(i)) => {
                let mut it = v.remove(i);
                it.right = !it.right;
                v.push(it);
                changed = true;
            }
            None => {}
        }
        if changed {
            self.graph_cfg_dirty = true; // 마우스를 놓은 뒤 저장
        }
    }
}
