use crate::db::*;
mod about_ui;
mod col_ui;
mod extras_ui;
mod transfer_ui;
mod attach_ui;
mod export_ui;
mod find_ui;
mod graph_ui;
mod docsync_ui;
mod help_ui;
mod image_ui;
mod ref_window;
mod status_ui;
mod sync_ui;
mod toc_ui;
mod update_ui;
mod theme_ui;

use crate::graph::{Graph, GraphAct};
use crate::md;
use crate::theme::{self, Theme};
use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- 설정
/// 상태 표시줄 한 칸의 설정.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct StatusItem {
    pub id: String,
    pub show: bool,
    /// true = 오른쪽 묶음, false = 왼쪽 묶음
    pub right: bool,
}

/// (id, 이름, 기본 표시, 기본 오른쪽) — 목록 순서가 기본 순서.
pub const STATUS_DEFS: &[(&str, &str, bool, bool)] = &[
    ("msg", "알림 메시지 (저장했습니다 등, 있을 때만)", true, false),
    ("pos", "커서 위치 (줄·칸)", true, false),
    ("stats", "글자 수 · 줄 수 · 단어 수", true, false),
    ("zoom", "글자 배율 (클릭하면 100%)", true, false),
    ("num", "NumLock", true, false),
    ("caps", "CapsLock", true, false),
    ("scroll", "ScrollLock", true, false),
    ("save", "저장 상태 + 저장 막대", true, true),
    ("merge", "로그 병합 진행 + 막대", true, true),
    ("db", "DB 크기 · 문서 수", true, true),
    ("backup", "백업", true, true),
    ("sync", "동기화", true, true),
    ("mem", "메모리 사용량", true, true),
];

pub fn default_status_items() -> Vec<StatusItem> {
    STATUS_DEFS.iter().map(|(id, _, show, right)| StatusItem { id: id.to_string(), show: *show, right: *right }).collect()
}

/// 모르는 항목은 버리고, 빠진 항목은 기본값으로 채운다(저장 파일이 옛 버전이어도 안전).
pub fn normalize_status(v: &mut Vec<StatusItem>) {
    let ok = |id: &str| STATUS_DEFS.iter().any(|d| d.0 == id);
    let before = v.len();
    v.retain(|i| ok(&i.id));
    let mut seen = std::collections::HashSet::new();
    v.retain(|i| seen.insert(i.id.clone()));
    for (id, _, show, right) in STATUS_DEFS {
        if !v.iter().any(|i| i.id == *id) {
            v.push(StatusItem { id: id.to_string(), show: *show, right: *right });
        }
    }
    let _ = before;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub db_path: String,
    pub theme: String,
    pub font_size: f32,
    /// 화면 글자(메뉴·단추·상태바 등) 크기. 본문 크기와 따로 정한다.
    pub ui_font_size: f32,
    /// 분류 창 글자 크기(10~24)
    pub cat_font_size: f32,
    /// 분류 목록 높이(px, 0 = 자동). 목록 아래 경계를 끌어 조절한다.
    pub cat_height: f32,
    /// 마지막 창 크기(논리 픽셀, 0 = 기본)와 최대화 여부
    pub win_w: f32,
    pub win_h: f32,
    pub win_max: bool,
    /// 본문 줄 간격 / 목록 항목 간격 / 문단 간격(빈 줄 높이) 배율
    pub line_spacing: f32,
    pub list_spacing: f32,
    pub para_spacing: f32,
    pub wrap: bool,
    pub hide_syntax: bool,
    /// 숫자 목록(1. 2. 3.)을 고치면 번호를 자동으로 다시 매긴다
    pub auto_renumber: bool,
    pub sidebar: bool,
    pub links_panel: bool,
    /// 오른쪽 사이드바 모드: 0 = 연결 그래프, 1 = 목차, 2 = 함께(그래프 위·목차 아래, 기본)
    pub right_mode: u8,
    /// "함께"를 기본으로 바꾼 뒤 한 번만 모드를 "함께"로 옮겼는지(옛 설정에 남은 0을 기본값과 구별할 수 없어서)
    pub right_default_v2: bool,
    /// 목차에 보일 제목 단계(1~6)
    pub toc_depth: u8,
    pub sort: u8,
    /// 자동 저장 간격(분, 1~20): 첫 변경 후 이 시간이 지나면 한 번에 기록한다
    pub autosave_min: u32,
    /// 쓰기 로그(WAL)를 본 파일에 병합하는 간격(분)
    pub merge_min: u32,
    /// 그래픽 렌더러: directx(기본) / opengl / vulkan / auto
    pub renderer: String,
    /// 렌더러를 시작하는 중이라는 표시(첫 화면을 그리면 지운다). 남아 있으면 지난 시작이 실패한 것이다.
    pub renderer_pending: String,
    /// DB 메모리 캐시 상한(MB)
    pub cache_mb: u32,
    /// 상태 표시줄 항목: 순서·표시 여부·왼쪽/오른쪽 위치
    pub status_items: Vec<StatusItem>,    pub trash_days: i64,
    pub backup_keep: usize,
    /// 연결 그래프 설정(우클릭 메뉴, 자동 저장)
    pub graph: crate::graph::GraphSettings,
    // --- 동기화 저장소 ---
    pub sync_dir: String,
    pub sync_auto: bool,
    pub sync_hours: u32,
    /// 문서 단위 동기화의 확인 주기(분)
    pub sync_mins: u32,
    pub last_sync: i64,
    /// 마지막으로 열었던 문서 id(다시 시작할 때 그 문서를 연다)
    pub last_doc: i64,
    /// 마지막으로 동기화 폴더에 올렸을 때의 DB 변경 카운터(지금 값과 같으면 올릴 것이 없다)
    pub sync_rev: i64,
    /// 닫을 때 마지막 동기화 이후 바뀐 것이 있으면 한 번 올린다(자동 동기화가 켜져 있을 때)
    pub sync_on_close: bool,
    /// 이미지를 넣을 때 줄이는 긴 변 최대 크기(px)와 WebP 품질(60~95)
    pub img_max: u32,
    pub img_quality: u32,
    /// 마지막 동기화 시도의 결과(성공/실패 한 줄)와 그 시각
    pub sync_result: String,
    pub sync_result_at: i64,
    pub sync_failed: bool,
    /// 자동 동기화를 건너뛴 이유(비어 있으면 정상). 다음에 올리거나 불러오면 지워진다.
    pub sync_skip: String,
    // --- WebDAV 암호화 동기화(sync_mode 가 "webdav" 일 때). 비밀번호·복구 문자열은 파일에 Windows 계정 암호화(DPAPI)로 저장 ---
    /// 동기화 방식: "folder"(기본, 폴더) / "webdav"(전체 스냅숏) / "webdoc"(문서 단위)
    pub sync_mode: String,
    pub wd_url: String,
    pub wd_user: String,
    pub wd_pass: String,
    /// 저장소 복구 문자열(AKS1-…). 비어 있으면 아직 저장소를 만들거나 연결하지 않은 것
    pub wd_key: String,
    /// 마지막으로 올리거나 받은 manifest 의 ETag(서버가 그 뒤 바뀌었는지 알아내는 기준)
    pub wd_etag: String,
    // --- 자동 업데이트(GitHub 릴리스) ---
    pub update_check: bool,
    pub update_auto: bool,
    pub update_last: i64,
    // --- 사용자 지정 테마(15색, 0xRRGGBB; 비어 있으면 없음) / 글꼴 이름(비어 있으면 기본) ---
    pub custom_theme: Vec<u32>,
    pub font_latin: String,
    pub font_korean: String,
    /// 선택한 글꼴의 파일 위치(시작할 때 이 파일만 읽는다; ttc는 face 번호 포함)
    pub font_latin_loc: theme::FontPaths,
    pub font_korean_loc: theme::FontPaths,
    /// 사용자 템플릿 (이름, 본문)
    pub templates: Vec<(String, String)>,
    /// 스니펫 (이름=약어, 내용)
    pub snippets: Vec<(String, String)>,
    /// 저장된 검색어
    pub saved_searches: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            db_path: default_db_path().to_string_lossy().into_owned(),
            theme: "라이트".into(),
            font_size: 15.0,
            ui_font_size: theme::UI_SIZE_DEFAULT,
            cat_font_size: 14.0,
            cat_height: 0.0,
            win_w: 0.0,
            win_h: 0.0,
            win_max: false,
            line_spacing: 1.0,
            list_spacing: 1.3,
            para_spacing: 1.0,
            wrap: true,
            hide_syntax: true,
            auto_renumber: true,
            sidebar: true,
            links_panel: false,
            right_mode: 2,
            // 설정 파일에 이 항목이 없으면(옛 설정) false 로 읽혀 한 번 "함께"로 옮겨진다. 새 설정은 load()에서 true 로 시작한다.
            right_default_v2: false,
            toc_depth: 6,
            sort: 0,
            autosave_min: 2,
            merge_min: 60,
            renderer: "directx".into(),
            renderer_pending: String::new(),
            cache_mb: 64,
            status_items: default_status_items(),
            trash_days: 30,
            backup_keep: 7,
            graph: Default::default(),
            sync_dir: String::new(),
            sync_auto: false,
            sync_hours: 24,
            sync_mins: 5,
            last_sync: 0,
            last_doc: 0,
            sync_rev: 0,
            sync_on_close: true,
            img_max: 1600,
            img_quality: 80,
            sync_result: String::new(),
            sync_result_at: 0,
            sync_failed: false,
            sync_skip: String::new(),
            sync_mode: "folder".into(),
            wd_url: String::new(),
            wd_user: String::new(),
            wd_pass: String::new(),
            wd_key: String::new(),
            wd_etag: String::new(),
            update_check: true,
            update_auto: true,
            update_last: 0,
            custom_theme: Vec::new(),
            font_latin: String::new(),
            font_korean: String::new(),
            font_latin_loc: Default::default(),
            font_korean_loc: Default::default(),
            templates: vec![],
            snippets: vec![],
            saved_searches: vec![],
        }
    }
}

pub fn app_dir() -> PathBuf {
    // 개발·시험용: AKASHIC_DATA로 설정/DB 폴더를 따로 지정할 수 있다.
    if let Some(d) = std::env::var_os("AKASHIC_DATA") {
        return PathBuf::from(d);
    }
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Akashic")
}
pub fn default_db_path() -> PathBuf {
    app_dir().join("vault.db")
}
/// 설정 파일을 읽지 못해 기본값으로 시작했는지(시작 알림용)
pub static CONFIG_RESET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 설정의 WebDAV 비밀번호·복구 문자열이 보호된 채로 있는데 이 PC·계정에서 풀 수 없었는지(시작 알림용)
pub static SECRET_UNREADABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn config_path() -> PathBuf {
    app_dir().join("config.json")
}
impl Config {
    pub fn load() -> Config {
        let path = config_path();
        let Ok(raw) = std::fs::read_to_string(&path) else { return Config { right_default_v2: true, ..Config::default() } };
        // 메모장 등으로 저장한 BOM 있는 파일도 읽는다.
        match serde_json::from_str::<Config>(raw.trim_start_matches('\u{feff}')) {
            Ok(mut c) => {
                if !c.right_default_v2 {
                    c.right_mode = 2;
                    c.right_default_v2 = true;
                }
                // 비밀값은 파일에서 Windows 계정에 묶여 암호화돼 있다(표식 없는 옛 평문은 그대로 읽고 다음 저장 때 보호된다).
                for v in [&mut c.wd_pass, &mut c.wd_key] {
                    match crate::secret::unprotect(v) {
                        Some(p) => *v = p,
                        None => {
                            v.clear();
                            SECRET_UNREADABLE.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
                c
            }
            Err(_) => {
                // 손상되었거나 알 수 없는 형식: 기본값으로 시작하되, 이후 저장이 원본을 덮어쓰기 전에 사본을 남긴다
                // (옮겨 둔 DB 위치·동기화 폴더 같은 설정을 사용자가 되살릴 수 있도록).
                let _ = std::fs::copy(&path, path.with_extension("bad.json"));
                CONFIG_RESET.store(true, std::sync::atomic::Ordering::Relaxed);
                Config { right_default_v2: true, ..Config::default() }
            }
        }
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(app_dir());
        // 비밀번호·복구 문자열은 암호화해서 쓴다. 암호화에 실패하면 평문으로 쓰지 않고 이번 저장을 건너뛴다.
        let mut c = self.clone();
        match (crate::secret::protect(&self.wd_pass), crate::secret::protect(&self.wd_key)) {
            (Some(p), Some(k)) => {
                c.wd_pass = p;
                c.wd_key = k;
            }
            _ => {
                crate::diag::log("설정 저장 건너뜀: 비밀값을 암호화하지 못했습니다");
                return;
            }
        }
        if let Ok(s) = serde_json::to_string_pretty(&c) {
            let tmp = config_path().with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, config_path());
            }
        }
    }
}

// ---------------------------------------------------------------- 상태
#[derive(Clone)]
struct DocDrag(i64);
#[derive(Clone)]
struct CatDrag(i64);

enum ListRow {
    Header(String),
    Doc(usize),
}

/// 편집창 우클릭·… 메뉴의 편집 요청
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditReq {
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// 감싸기 토글(왼쪽, 오른쪽)
    Wrap(&'static str, &'static str),
    Link,
    Respace(crate::editkeys::SpaceMode),
    Lines(crate::editkeys::LineOp),
    /// cfg.snippets[번호]를 선택 자리에 넣는다
    Snippet(usize),
    /// 캐럿 앞의 약어를 스니펫으로 펼친다(Ctrl+J)
    Abbrev,
    /// 첨부 링크(`attach_text`)를 선택 자리에 넣는다
    AttachText,
    /// 열 자르기(단위, 범위, 남기기 여부)
    Cols(crate::editkeys::ColUnit, crate::editkeys::ColRange, bool),
}

enum Act {
    NewDoc,
    Today,
    FromTemplate(usize),
    SaveTemplate,
    ExtrasWindow,
    Open(i64),
    SetView(View),
    ToggleTag(String),
    Pin(i64),
    Trash(i64),
    Restore(i64),
    Purge(i64),
    MoveDoc(i64, Option<i64>),
    TagDoc(i64, String),
    PromptNewCat(Option<i64>),
    PromptRename(i64),
    DelCat(i64),
    MoveCat(i64, Option<i64>),
    ToggleCollapse(i64),
    Import,
    ImportDir,
    /// 파일 첨부(선택 범위: 우클릭 때 저장해 둔 것, 없으면 마지막 선택)
    Attach(Option<(usize, usize)>),
    /// 첨부 파일 열기(해시, 이름)
    OpenFile(String, String),
    ExportHtml,
    ExportPdf,
    Export,
    InsertImage,
    Edit(EditReq),
    /// 우클릭 순간의 선택(작은 쪽, 큰 쪽)과 함께
    EditAt((usize, usize), EditReq),
    ColDialog,
    Back,
    Forward,
    /// 문서를 읽기 전용 참고 창으로 연다
    OpenRef(i64),
    Follow(String),
    AddTag,
    RemoveTag(String),
    EmptyTrash,
    /// 직접 정렬: (옮길 문서, 이 문서 앞으로; None=맨 끝)
    Reorder(i64, Option<i64>),
    /// 상태바 안내
    Say(String),
}

struct Prompt {
    rename: Option<i64>,
    parent: Option<i64>,
    text: String,
    seq: u32,        // 열 때마다 달라지는 번호(입력 상태 재사용 방지)
    focused: bool,   // 첫 포커스를 이미 요청했는지
    composing: bool, // 한글 등 IME 조합 중인지
}

impl Prompt {
    fn new(rename: Option<i64>, parent: Option<i64>, text: String, seq: u32) -> Prompt {
        Prompt { rename, parent, text, seq, focused: false, composing: false }
    }
}

pub struct App {
    db: Db,
    cfg: Config,
    theme: Theme,
    rows: Vec<DocRow>,
    list_rows: Vec<ListRow>,
    cats: Vec<Category>,
    tags: Vec<String>,
    counts: (i64, i64, i64, i64),
    filter: Filter,
    search: String,
    list_dirty: bool,
    collapsed: HashSet<i64>,

    cur: Option<i64>,
    title: String,
    body: String,
    doc_tags: Vec<String>,
    new_tag: String,
    cur_trashed: bool,
    cur_pinned: bool,
    cur_cat: Option<i64>,
    last_edit: Option<Instant>,
    /// 본문 편집창에서 한글 등 IME 조합 중인지(조합을 끝내는 Enter가 목록 이어쓰기를 일으키지 않게)
    body_composing: bool,
    back: Vec<i64>,
    fwd: Vec<i64>,
    out_links: Vec<(String, Option<i64>)>,
    in_links: Vec<(i64, String)>,
    graph: Option<Graph>,
    graph_sig: u64,
    graph_cur: Option<i64>,
    graph_stale: bool,
    graph_cfg_dirty: bool,
    toc: Vec<md::Heading>,
    show_help: bool,
    /// 연결 그래프를 별도 창으로 분리해 보는 중
    graph_window: bool,
    help_topic: usize,
    help_filter: String,
    toc_sig: u64,
    goto_line: Option<usize>,
    /// Ctrl+G 줄 번호 막대
    goto_open: bool,
    goto_text: String,
    goto_focus: bool,
    /// PageUp/PageDown 요청(아래로?, Shift?)과, 다음 그리기에 맞출 스크롤 위치
    page_req: Option<(bool, bool)>,
    /// 우클릭·… 메뉴에서 고른 편집(다음에 편집창을 그리기 전에 적용)
    edit_req: Option<EditReq>,
    /// 우클릭 메뉴에서 고른 편집이 쓸 선택(메뉴를 누르면 편집창이 포커스를 잃어 egui가 선택을 풀기 때문에 우클릭 순간의 것을 쓴다)
    edit_sel: Option<(usize, usize)>,
    /// 마지막 우클릭 순간의 선택
    menu_sel: (usize, usize),
    /// 편집창이 포커스를 가졌던 마지막 프레임의 선택(툴바 메뉴·대화상자를 누르면 포커스와 함께 선택이 풀리므로)
    last_sel: (usize, usize),
    /// 공백 정리가 마크다운 구조(목록 기호·표·코드 블록)를 건드리지 않게 함
    respace_md: bool,
    col: col_ui::ColCut,
    extras_open: bool,
    extras_tab: u8,
    /// 백그라운드로 만드는 PDF의 결과 수신기
    pdf_rx: Option<std::sync::mpsc::Receiver<String>>,
    /// 파일 첨부 직후 넣을 링크 글자
    attach_text: Option<String>,
    page_scroll: Option<Vec2>,

    cursor_line: Option<usize>,
    cursor_idx: usize,
    galley_cache: Option<(u64, Arc<egui::Galley>)>,
    /// 글꼴을 바꾼 패스 번호. egui는 새 글꼴을 다음 패스부터 쓰므로, 그 뒤에 한 번 더 글자 배치 캐시를 비운다.
    font_swap_pass: Option<u64>,
    /// 문서 안 이미지의 화면용 텍스처 캐시
    img: crate::img::ImgCache<egui::TextureHandle>,
    stats: status_ui::DocStats,
    storage: status_ui::StorageInfo,
    last_saved_ts: i64,
    /// 마지막으로 쓰기 로그를 본 파일에 병합한 시각
    last_merge: Instant,
    /// 마지막 병합 시각(표시용, epoch 초)
    last_merge_ts: i64,
    pos_cache: (usize, usize, usize, usize), // (커서, 본문 길이, 줄, 칸)

    status: String,
    status_at: Instant,
    show_options: bool,
    prompt: Option<Prompt>,
    prompt_seq: u32,
    opt_tab: u8,
    focus_search: bool,
    last_title: String,
    last_theme: String,
    /// 글자 그리기 환경 지문(바뀌면 galley 캐시를 비운다)
    text_env: theme::TextEnv,
    /// 문서 안 찾기(Ctrl+F)
    find: find_ui::Find,
    /// 렌더러 시험 결과(저장 값 → 상태)와 진행 중인 시험
    probes: std::collections::HashMap<String, crate::probe::Probe>,
    probe_rx: Vec<(String, std::sync::mpsc::Receiver<crate::probe::Probe>)>,
    boot_done: bool,
    win_seen: Option<((f32, f32), bool)>,
    win_changed: Option<Instant>,
    /// 참고 창(읽기 전용 새 창) 목록과 창별 그리기 캐시
    refs: ref_window::RefWins,
    ref_views: std::collections::HashMap<u64, ref_window::RefView>,
    /// 다음 프레임에 앞으로 가져올 참고 창
    ref_focus: Option<u64>,
    /// 저장·목록 갱신 때마다 올라가는 번호(참고 창이 DB 저장본을 다시 읽을 때를 안다)
    ref_rev: u64,
}

/// 수정키가 정확히 `m`일 때만 `k`를 소비한다(egui의 `consume_key`는 Shift·Alt가 더 눌려도 맞다고 본다).
fn key_exact(ctx: &egui::Context, m: Modifiers, k: Key) -> bool {
    ctx.input_mut(|i| {
        let c = i.modifiers;
        c.alt == m.alt && c.shift == m.shift && c.command == m.command && i.consume_key(m, k)
    })
}

fn byte_of(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

fn fmt_size(b: u64) -> String {
    if b >= 1 << 20 { format!("{:.1} MB", b as f64 / 1048576.0) } else { format!("{:.0} KB", b as f64 / 1024.0) }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, db: Db, mut cfg: Config) -> App {
        theme::set_ui_size(cfg.ui_font_size);
        md::set_spacing(cfg.line_spacing, cfg.list_spacing, cfg.para_spacing);
        let theme = theme_ui::startup_theme(&cc.egui_ctx, &mut cfg);
        theme::apply(&cc.egui_ctx, &theme);
        let sort = match cfg.sort { 1 => Sort::Created, 2 => Sort::Title, 3 => Sort::Manual, _ => Sort::Updated };
        let mut a = App {
            db,
            theme,
            rows: vec![],
            list_rows: vec![],
            cats: vec![],
            tags: vec![],
            counts: (0, 0, 0, 0),
            filter: Filter { view: View::All, tags: vec![], query: String::new(), sort },
            search: String::new(),
            list_dirty: true,
            collapsed: HashSet::new(),
            cur: None,
            title: String::new(),
            body: String::new(),
            doc_tags: vec![],
            new_tag: String::new(),
            cur_trashed: false,
            cur_pinned: false,
            cur_cat: None,
            last_edit: None,
            body_composing: false,
            back: vec![],
            fwd: vec![],
            out_links: vec![],
            in_links: vec![],
            graph: None,
            graph_sig: 0,
            graph_cur: None,
            graph_stale: false,
            graph_cfg_dirty: false,
            toc: Vec::new(),
            show_help: false,
            graph_window: false,
            help_topic: 0,
            help_filter: String::new(),
            toc_sig: 0,
            goto_line: None,
            goto_open: false,
            goto_text: String::new(),
            goto_focus: false,
            page_req: None,
            edit_req: None,
            edit_sel: None,
            menu_sel: (0, 0),
            last_sel: (0, 0),
            respace_md: true,
            col: Default::default(),
            extras_open: false,
            extras_tab: 0,
            pdf_rx: None,
            attach_text: None,
            page_scroll: None,
            cursor_line: None,
            cursor_idx: 0,
            galley_cache: None,
            font_swap_pass: None,
            img: crate::img::ImgCache::new(),
            stats: status_ui::DocStats::unset(),
            storage: Default::default(),
            last_saved_ts: 0,
            last_merge: Instant::now(),
            last_merge_ts: 0,
            pos_cache: (usize::MAX, 0, 1, 1),
            status: "준비됨".into(),
            status_at: Instant::now(),
            show_options: false,
            prompt: None,
            prompt_seq: 0,
            opt_tab: 0,
            focus_search: false,
            last_title: String::new(),
            last_theme: String::new(),
            text_env: Default::default(),
            find: Default::default(),
            probes: Default::default(),
            probe_rx: Vec::new(),
            boot_done: false,
            win_seen: None,
            win_changed: None,
            refs: Default::default(),
            ref_views: Default::default(),
            ref_focus: None,
            ref_rev: 0,
            cfg,
        };
        let days = a.cfg.trash_days;
        if let Ok(n) = a.db.purge_old(days) {
            if n > 0 {
                let _ = a.db.prune_images(); // 영구 삭제된 문서가 있을 때만 쓰지 않는 그림을 정리한다
            }
        }
        match a.db.daily_backup(a.cfg.backup_keep) {
            Ok(Some(_)) => a.say("오늘의 자동 백업을 만들었습니다"),
            Err(x) => a.say(&format!("자동 백업 실패: {x}")),
            _ => {}
        }
        a.db.set_cache_mb(a.cfg.cache_mb);
        a.refresh();
        a.sync_on_start(&cc.egui_ctx);
        a.update_on_start(&cc.egui_ctx);
        // 개발용: 도움말 창을 시작 때 열고(AKASHIC_OPEN_HELP=항목번호) 해당 항목을 고른다.
        if let Some(v) = std::env::var_os("AKASHIC_OPEN_HELP") {
            a.show_help = true;
            a.help_topic = v.to_string_lossy().parse().unwrap_or(0);
        }
        if let Some(v) = std::env::var_os("AKASHIC_OPEN_OPT") {
            a.show_options = true; // 개발용: 옵션 창을 해당 탭으로 연다
            a.opt_tab = v.to_string_lossy().parse().unwrap_or(0);
        }
        if std::env::var_os("AKASHIC_OPEN_GRAPH").is_some() {
            a.graph_window = true; // 개발용: 분리된 그래프 창을 시작 때 연다
            a.cfg.links_panel = true;
        }
        let want = std::env::var("AKASHIC_OPEN_TITLE").ok(); // 개발용: 제목에 이 글자가 있는 문서를 연다
        let pick = want.and_then(|w| a.rows.iter().find(|r| r.title.contains(&w)).map(|r| r.id));
        // 마지막으로 열었던 문서(있고 휴지통이 아니면) → 없으면 목록의 첫 문서
        let last = (a.cfg.last_doc > 0)
            .then(|| a.db.get_doc(a.cfg.last_doc).ok().flatten())
            .flatten()
            .filter(|d| !d.trashed)
            .map(|d| d.id);
        if let Some(first) = pick.or(last).or_else(|| a.rows.first().map(|r| r.id)) {
            a.open_doc(first, false);
        }
        a
    }

    fn say(&mut self, s: &str) {
        self.status = s.to_string();
        self.status_at = Instant::now();
    }

    fn refresh(&mut self) {
        self.filter.query = self.search.clone();
        match self.db.list_docs(&self.filter) {
            Ok(r) => self.rows = r,
            Err(x) => self.say(&format!("목록 오류: {x}")),
        }
        self.cats = self.db.categories().unwrap_or_default();
        self.tags = self.db.all_tags().unwrap_or_default();
        self.counts = self.db.counts().unwrap_or_default();
        self.build_list_rows();
        self.list_dirty = false;
    }

    fn build_list_rows(&mut self) {
        use chrono::{Datelike, Local, TimeZone};
        self.list_rows.clear();
        let today = Local::now().date_naive();
        let mut last = String::new();
        for (i, r) in self.rows.iter().enumerate() {
            let head = if r.pinned {
                "★ 고정됨".to_string()
            } else if self.filter.sort == Sort::Title || self.filter.sort == Sort::Manual || self.filter.view == View::Trash {
                String::new()
            } else {
                let ts = if self.filter.sort == Sort::Created { r.created } else { r.updated };
                let d = Local.timestamp_opt(ts, 0).single().map(|x| x.date_naive()).unwrap_or(today);
                let days = (today - d).num_days();
                if days <= 0 { "오늘".into() } else if days == 1 { "어제".into() } else if days < 7 { "지난 7일".into() }
                else { format!("{}년 {}월", d.year(), d.month()) }
            };
            if head != last {
                if !head.is_empty() {
                    self.list_rows.push(ListRow::Header(head.clone()));
                }
                last = head;
            }
            self.list_rows.push(ListRow::Doc(i));
        }
    }

    // ------------------------------------------------------------ 문서
    /// 편집 중인 문서를 저장한다. 저장하지 못했으면 `false`: 편집 내용과 "수정됨" 표시를 지우지 않으므로
    /// 호출한 쪽은 다른 문서를 여는 등 내용을 덮어쓰는 동작을 멈춰야 한다.
    fn flush(&mut self) -> bool {
        if let Some(id) = self.cur {
            if self.cur_trashed {
                self.last_edit = None;
                return true;
            }
            match self.db.save_doc(id, &self.title, &self.body) {
                Ok(true) => {
                    self.list_dirty = true;
                    self.ref_rev += 1;
                    self.last_saved_ts = now();
                }
                Ok(false) => {}
                Err(x) => {
                    self.say(&format!("저장 실패: {x} (편집 내용은 그대로 두었습니다)"));
                    return false;
                }
            }
        }
        self.last_edit = None;
        true
    }

    /// 쓰기 로그(WAL)를 본 파일에 병합할 때가 되었는지 확인한다(병합 주기 경과 또는 로그가 64MB 초과).
    fn merge_tick(&mut self, ctx: &egui::Context) {
        let every = Duration::from_secs(self.cfg.merge_min.clamp(5, 720) as u64 * 60);
        let big = self.storage.wal > 64 * 1024 * 1024;
        if self.last_merge.elapsed() >= every || big {
            self.merge_now();
        }
        // 막대가 천천히 차오르므로 10초에 한 번만 깨운다
        ctx.request_repaint_after(Duration::from_secs(10));
    }

    fn merge_now(&mut self) {
        match self.db.checkpoint() {
            Ok(()) => self.last_merge_ts = now(),
            Err(x) => self.say(&format!("병합 실패: {x}")),
        }
        self.last_merge = Instant::now();
        self.storage.at = None;
    }

    fn open_doc(&mut self, id: i64, record: bool) {
        if !self.flush() {
            return; // 저장하지 못한 채 다른 문서로 가면 편집 내용이 사라진다
        }
        self.find_reset_for_doc();
        if let Some(old) = self.cur {
            if old != id {
                let _ = self.db.delete_empty(old); // 쓰다 만 빈 문서 정리
                self.list_dirty = true;
            }
        }
        match self.db.get_doc(id) {
            Ok(Some(d)) => {
                if record {
                    if let Some(old) = self.cur {
                        if old != id {
                            self.back.push(old);
                        }
                    }
                    self.fwd.clear();
                }
                self.cur = Some(d.id);
                self.cfg.last_doc = d.id; // 다음에 시작할 때 이 문서를 다시 연다(설정은 닫을 때 저장)
                self.title = d.title;
                self.body = d.body;
                self.cur_trashed = d.trashed;
                self.cur_pinned = d.pinned;
                self.cur_cat = d.category;
                self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
                self.cursor_line = None;
                self.last_edit = None;
                self.compute_links();
            }
            _ => self.say("문서를 열 수 없습니다"),
        }
    }

    fn compute_links(&mut self) {
        self.out_links = md::wiki_links(&self.body)
            .into_iter()
            .map(|t| {
                let id = self.db.find_by_title(&t).ok().flatten();
                (t, id)
            })
            .collect();
        self.in_links = match self.cur {
            Some(id) => self.db.backlinks(id, &self.title).unwrap_or_default(),
            None => vec![],
        };
    }

    /// 현재 문서의 (제목, 링크 목록) 지문 — 이게 바뀔 때만 그래프를 다시 만든다.
    fn graph_signature(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.cur.hash(&mut h);
        self.title.trim().to_lowercase().hash(&mut h);
        md::wiki_links(&self.body).hash(&mut h);
        h.finish()
    }

    fn rebuild_graph(&mut self) {
        match self.db.all_docs() {
            Ok(docs) => {
                let cats = self.db.doc_categories().unwrap_or_default();
                let g = Graph::build(&docs, &cats, self.cur, self.graph.as_ref(), &self.cfg.graph);
                self.graph = Some(g);
                if self.cur.is_some() {
                    let t = self.title.clone();
                    if let Some(g) = self.graph.as_mut() {
                        g.focus(self.cur, &t);
                    }
                }
            }
            Err(x) => self.say(&format!("그래프 오류: {x}")),
        }
        self.graph_cur = self.cur;
        self.graph_sig = self.graph_signature();
        self.graph_stale = false;
    }

    fn new_doc(&mut self) {
        self.flush();
        let cat = match self.filter.view { View::Category(c) => Some(c), _ => None };
        match self.db.new_doc(cat) {
            Ok(id) => {
                if self.filter.view == View::Trash {
                    self.filter.view = View::All;
                }
                self.list_dirty = true;
                self.open_doc(id, true);
                self.refresh();
            }
            Err(x) => self.say(&format!("새 문서 실패: {x}")),
        }
    }

    /// "분류로 이동" 메뉴: 대분류 → 소분류 순서로 단계마다 하위 메뉴를 펼친다.
    /// 형제는 이름순(왼쪽 분류 트리와 같은 순서)이고, 하위 분류가 있는 분류는 하위 메뉴 맨 위의 "~에 넣기"로 그 분류 자체에도 옮길 수 있다.
    fn move_menu(&self, ui: &mut egui::Ui, parent: Option<i64>, doc: i64, depth: usize, acts: &mut Vec<Act>) {
        for c in self.cats.iter().filter(|c| c.parent == parent) {
            let has_kids = depth < 8 && self.cats.iter().any(|k| k.parent == Some(c.id));
            if has_kids {
                ui.menu_button(c.name.as_str(), |ui| {
                    if ui.button(format!("{} 에 넣기", c.name)).clicked() {
                        acts.push(Act::MoveDoc(doc, Some(c.id)));
                        ui.close();
                    }
                    ui.separator();
                    self.move_menu(ui, Some(c.id), doc, depth + 1, acts);
                });
            } else if ui.button(c.name.as_str()).clicked() {
                acts.push(Act::MoveDoc(doc, Some(c.id)));
                ui.close();
            }
        }
    }

    fn cat_path(&self, id: Option<i64>) -> String {
        let mut parts = vec![];
        let mut cur = id;
        while let Some(c) = cur {
            match self.cats.iter().find(|x| x.id == c) {
                Some(x) => {
                    parts.push(x.name.clone());
                    cur = x.parent;
                }
                None => break,
            }
            if parts.len() > 8 {
                break;
            }
        }
        parts.reverse();
        if parts.is_empty() { "미분류".into() } else { parts.join(" / ") }
    }

    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.flush();
        let cat = match self.filter.view { View::Category(c) => Some(c), _ => None };
        let (mut ok, mut cp, mut failed) = (0, 0, 0);
        let mut last = None;
        for p in paths {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "md" && ext != "txt" {
                continue;
            }
            if let Ok(b) = std::fs::read(&p) {
                let (text, was_cp949) = decode_text(&b);
                let title = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if let Ok(id) = self.db.new_doc(cat) {
                    match self.db.save_doc(id, &title, &text) {
                        Ok(_) => {
                            ok += 1;
                            cp += was_cp949 as usize;
                            last = Some(id);
                        }
                        Err(_) => {
                            failed += 1;
                            let _ = self.db.delete_empty(id); // 내용 없이 남은 빈 문서를 치운다
                        }
                    }
                } else {
                    failed += 1;
                }
            }
        }
        self.list_dirty = true;
        self.refresh();
        if let Some(id) = last {
            self.open_doc(id, true);
        }
        self.say(&format!(
            "{ok}개 가져옴{}{}",
            if cp > 0 { format!(" (CP949 {cp}개는 UTF-8로 변환)") } else { String::new() },
            if failed > 0 { format!(" · {failed}개는 저장하지 못했습니다") } else { String::new() }
        ));
    }

    // ------------------------------------------------------------ 동작 처리
    fn apply(&mut self, acts: Vec<Act>) {
        self.poll_export();
        for a in acts {
            match a {
                Act::Say(s) => self.say(&s),
                Act::Reorder(id, before) => {
                    match self.db.reorder(id, before) {
                        Ok(()) => self.say("순서를 바꿨습니다"),
                        Err(x) => self.say(&x),
                    }
                    self.list_dirty = true;
                }
                Act::NewDoc => self.new_doc(),
                Act::Today => self.today_note(),
                Act::ImportDir => self.import_dir(),
                Act::Attach(sel) => self.attach_file(sel),
                Act::OpenFile(h, n) => self.open_attachment(&h, &n),
                Act::ExportHtml => self.export_html(),
                Act::ExportPdf => self.export_pdf(),
                Act::FromTemplate(i) => self.new_from_template(i),
                Act::SaveTemplate => self.save_as_template(),
                Act::ExtrasWindow => self.extras_open = true,
                Act::Open(id) => {
                    if self.cur != Some(id) {
                        self.open_doc(id, true)
                    }
                }
                Act::SetView(v) => {
                    self.filter.view = v;
                    if self.filter.view != View::All {
                        // 분류·특수 보기를 고르면 태그 필터는 풀어 준다.
                    }
                    self.filter.tags.clear();
                    self.list_dirty = true;
                }
                Act::ToggleTag(t) => {
                    if let Some(p) = self.filter.tags.iter().position(|x| *x == t) {
                        self.filter.tags.remove(p);
                    } else {
                        self.filter.tags.push(t);
                    }
                    self.list_dirty = true;
                }
                Act::Pin(id) => {
                    let v = self.db.get_doc(id).ok().flatten().map_or(false, |d| !d.pinned);
                    if let Err(x) = self.db.set_pinned(id, v) {
                        self.say(&format!("고정하지 못했습니다: {x}"));
                        continue;
                    }
                    if self.cur == Some(id) {
                        self.cur_pinned = v;
                    }
                    self.list_dirty = true;
                }
                Act::Trash(id) => {
                    if self.cur == Some(id) {
                        self.flush();
                    }
                    if let Err(x) = self.db.trash(id) {
                        self.say(&format!("휴지통으로 옮기지 못했습니다: {x}"));
                        continue;
                    }
                    if self.cur == Some(id) {
                        self.cur_trashed = true;
                    }
                    self.list_dirty = true;
                    self.say("휴지통으로 옮겼습니다");self.graph_stale = true;
                }
                Act::Restore(id) => {
                    if let Err(x) = self.db.restore(id) {
                        self.say(&format!("복원하지 못했습니다: {x}"));
                        continue;
                    }
                    self.graph_stale = true;
                    if self.cur == Some(id) {
                        self.cur_trashed = false;
                    }
                    self.list_dirty = true;
                }
                Act::Purge(id) => {
                    if let Err(x) = self.db.purge(id) {
                        self.say(&format!("영구 삭제하지 못했습니다: {x}"));
                        continue;
                    }
                    if self.cur == Some(id) {
                        self.cur = None;
                        self.title.clear();
                        self.body.clear();
                        self.cur_trashed = false;
                        self.last_edit = None;
                    }
                    let _ = self.prune_images_now(); // 어느 문서도 쓰지 않는 그림을 정리한다(편집 중인 내용을 먼저 저장)
                    self.list_dirty = true;
                    self.say("영구 삭제했습니다");self.graph_stale = true;
                }
                Act::EmptyTrash => {
                    let ids: Vec<i64> = self.db.list_docs(&Filter { view: View::Trash, tags: vec![], query: String::new(), sort: Sort::Updated }).unwrap_or_default().iter().map(|r| r.id).collect();
                    // 화면에는 검색·태그 필터로 일부만 보일 수 있으므로, 실제로 지울 전체 개수를 보여 주고 확인받는다.
                    let yes = rfd::MessageDialog::new()
                        .set_title("휴지통 비우기")
                        .set_description(format!("휴지통의 문서 {}개를 모두 영구 삭제합니다(검색·태그 필터와 상관없이 전체).\n되돌릴 수 없습니다. 계속할까요?", ids.len()))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::Yes;
                    if yes {
                        let mut failed = 0;
                        for id in &ids {
                            if self.db.purge(*id).is_err() {
                                failed += 1;
                            }
                        }
                        if self.cur.map_or(false, |c| ids.contains(&c)) {
                            self.cur = None;
                            self.title.clear();
                            self.body.clear();
                            self.cur_trashed = false;
                            self.last_edit = None;
                        }
                        let _ = self.prune_images_now(); // 어느 문서도 쓰지 않는 그림을 정리한다(편집 중인 내용을 먼저 저장)
                        self.list_dirty = true;
                        if failed == 0 {
                            self.say(&format!("휴지통을 비웠습니다 ({}개)", ids.len()));
                        } else {
                            self.say(&format!("휴지통을 비웠습니다 ({}개 중 {failed}개는 삭제하지 못했습니다)", ids.len()));
                        }
                        self.graph_stale = true;
                    }
                }
                Act::MoveDoc(id, cat) => {
                    if self.cur == Some(id) {
                        self.flush();
                        self.cur_cat = cat;
                    }
                    if let Err(x) = self.db.set_doc_category(id, cat) {
                        self.say(&format!("분류를 옮기지 못했습니다: {x}"));
                    }
                    self.list_dirty = true;
                }
                Act::TagDoc(id, t) => {
                    if let Err(x) = self.db.add_tag(id, &t) {
                        self.say(&format!("태그를 달지 못했습니다: {x}"));
                    }
                    if self.cur == Some(id) {
                        self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
                    }
                    self.list_dirty = true;
                }
                Act::PromptNewCat(p) => {
                    self.prompt_seq += 1;
                    self.prompt = Some(Prompt::new(None, p, String::new(), self.prompt_seq));
                }
                Act::PromptRename(id) => {
                    let name = self.cats.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
                    self.prompt_seq += 1;
                    self.prompt = Some(Prompt::new(Some(id), None, name, self.prompt_seq));
                }
                Act::DelCat(id) => {
                    match self.db.delete_category(id) {
                        Ok(()) => {
                            if self.filter.view == View::Category(id) {
                                self.filter.view = View::All;
                            }
                            self.say("분류를 삭제했습니다(하위 분류와 문서는 한 단계 위로)");
                        }
                        Err(x) => self.say(&x),
                    }
                    self.list_dirty = true;
                }
                Act::MoveCat(id, parent) => {
                    if let Err(x) = self.db.move_category(id, parent) {
                        self.say(&x);
                    }
                    self.list_dirty = true;
                }
                Act::ToggleCollapse(id) => {
                    if !self.collapsed.remove(&id) {
                        self.collapsed.insert(id);
                    }
                }
                Act::InsertImage => self.insert_image_dialog(),
                Act::ColDialog => self.col.open = true,
                Act::Edit(r) => {
                    self.edit_sel = None;
                    self.edit_req = Some(r);
                }
                Act::EditAt(sel, r) => {
                    self.edit_sel = Some(sel);
                    self.edit_req = Some(r);
                }
                Act::Import => {
                    if let Some(p) = rfd::FileDialog::new().add_filter("마크다운/텍스트", &["md", "txt"]).pick_files() {
                        self.import_paths(p);
                    }
                }
                Act::Export => {
                    if self.cur.is_some() {
                        let name = if self.title.trim().is_empty() { "문서".to_string() } else { self.title.clone() };
                        let safe: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
                        if let Some(p) = rfd::FileDialog::new().add_filter("마크다운", &["md"]).set_file_name(format!("{safe}.md")).save_file() {
                            // 이미지 줄은 파일 옆 images 폴더의 상대경로 링크로 바꾸고 그림 파일을 함께 쓴다.
                            let (text, used) = crate::export::rewrite_images(&self.body, 0);
                            match std::fs::write(&p, &text) {
                                Ok(()) => {
                                    let mut miss = 0;
                                    if !used.is_empty() {
                                        let dir = p.parent().unwrap_or(std::path::Path::new(".")).join("images");
                                        let _ = std::fs::create_dir_all(&dir);
                                        for (h, _) in &used {
                                            match self.db.get_image(h) {
                                                Ok(Some((b, _, _))) if std::fs::write(dir.join(format!("{h}.webp")), &b).is_ok() => {}
                                                _ => miss += 1,
                                            }
                                        }
                                    }
                                    if miss == 0 {
                                        self.say(&format!("내보냄: {}", p.display()));
                                    } else {
                                        self.say(&format!("내보냄: {} (이미지 {miss}개는 저장하지 못했습니다)", p.display()));
                                    }
                                }
                                Err(x) => self.say(&format!("내보내기 실패: {x}")),
                            }
                        }
                    }
                }
                Act::Back => {
                    while let Some(prev) = self.back.pop() {
                        if self.db.get_doc(prev).ok().flatten().is_some() {
                            if let Some(c) = self.cur {
                                self.fwd.push(c);
                            }
                            self.open_doc(prev, false);
                            break;
                        }
                    }
                }
                Act::Forward => {
                    while let Some(next) = self.fwd.pop() {
                        if self.db.get_doc(next).ok().flatten().is_some() {
                            if let Some(c) = self.cur {
                                self.back.push(c);
                            }
                            self.open_doc(next, false);
                            break;
                        }
                    }
                }
                Act::OpenRef(id) => {
                    self.flush();
                    self.open_ref_window(id);
                }
                Act::Follow(title) => {
                    self.flush();
                    let id = match self.db.find_by_title(&title) {
                        Ok(Some(id)) => id,
                        Err(x) => {
                            self.say(&format!("문서를 찾지 못했습니다: {x}"));
                            continue;
                        }
                        Ok(None) => {
                            let cat = self.cur_cat;
                            match self.db.new_doc(cat) {
                                Ok(id) => {
                                    let _ = self.db.save_doc(id, &title, "");
                                    self.say(&format!("새 문서 \"{title}\"를 만들었습니다"));self.graph_stale = true;
                                    id
                                }
                                Err(_) => continue,
                            }
                        }
                    };
                    self.list_dirty = true;
                    self.open_doc(id, true);
                }
                Act::AddTag => {
                    if let Some(id) = self.cur {
                        let raw = std::mem::take(&mut self.new_tag);
                        for part in raw.split(|c: char| c == ',' || c.is_whitespace()) {
                            if part.trim().is_empty() {
                                continue;
                            }
                            let _ = self.db.add_tag(id, part);
                        }
                        self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
                        self.list_dirty = true;
                    }
                }
                Act::RemoveTag(t) => {
                    if let Some(id) = self.cur {
                        let _ = self.db.remove_tag(id, &t);
                        self.filter.tags.retain(|x| *x != t);
                        self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
                        self.list_dirty = true;
                    }
                }
            }
        }
    }

    /// 편집 키로 본문을 바꾼 뒤 숫자 목록 번호를 고친다(선택이 없을 때만; 커서 위치도 맞춰 돌려준다).
    fn renumber_after(&mut self, sel: (usize, usize)) -> (usize, usize) {
        if self.cfg.auto_renumber && sel.0 == sel.1 {
            if let Some((nt, nc)) = md::renumber(&self.body, sel.0) {
                self.body = nt;
                return (nc, nc);
            }
        }
        sel
    }

    /// 편집창의 선택 범위(작은 쪽, 큰 쪽). 상태가 없으면 (0, 0).
    fn sel_range(ctx: &egui::Context, id: egui::Id) -> (usize, usize) {
        egui::TextEdit::load_state(ctx, id).and_then(|st| st.cursor.char_range()).map_or((0, 0), |r| {
            let (p, s): (usize, usize) = (r.primary.index.into(), r.secondary.index.into());
            (p.min(s), p.max(s))
        })
    }

    /// 우클릭·… 메뉴에서 고른 편집을 적용한다. 편집창을 그리기 전에 본문을 바꿔 Ctrl+Z 한 번으로 되돌아간다.
    fn apply_edit_req(&mut self, ctx: &egui::Context, id: egui::Id) {
        use egui::text::{CCursor, CCursorRange};
        let Some(req) = self.edit_req.take() else { return };
        if self.cur_trashed && !matches!(req, EditReq::Copy | EditReq::SelectAll) {
            return;
        }
        ctx.memory_mut(|m| m.request_focus(id)); // 메뉴를 누르면 편집창이 포커스를 잃으므로 되돌린다
        let (lo, hi) = match self.edit_sel.take() {
            Some((a, b)) => {
                // 우클릭 순간의 선택을 편집창 상태에 되돌려 놓는다(복사·잘라내기 입력이 이 선택을 보게)
                let n = self.body.chars().count();
                let (a, b) = (a.min(n), b.min(n));
                let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                st.store(ctx, id);
                (a, b)
            }
            None => Self::sel_range(ctx, id),
        };
        match req {
            EditReq::Copy | EditReq::Cut => {
                if lo == hi {
                    return;
                }
                // Ctrl+C / Ctrl+X와 같은 길로 보낸다: 편집창이 이번 프레임에 이 입력을 받아 복사(잘라내기는 삭제까지) 한다.
                // 직접 `copy_text`를 부르면 클립보드가 바뀌지 않는 일이 있었다.
                let ev = if req == EditReq::Cut { egui::Event::Cut } else { egui::Event::Copy };
                ctx.input_mut(|i| i.events.push(ev));
                ctx.request_repaint();
            }
            EditReq::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            EditReq::SelectAll => {
                let n = self.body.chars().count();
                let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(n))));
                st.store(ctx, id);
                ctx.request_repaint();
            }
            EditReq::Wrap(l, r) => {
                let e = crate::editkeys::toggle_wrap(&self.body, lo, hi, l, r);
                self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
            }
            EditReq::Link => {
                let (ba, bb) = (byte_of(&self.body, lo), byte_of(&self.body, hi));
                self.body.insert_str(bb, "](주소)");
                self.body.insert_str(ba, "[");
                let t = std::mem::take(&mut self.body);
                self.apply_body_edit(ctx, id, t, (lo + 1, hi + 1));
            }
            EditReq::Cols(unit, range, keep) => match crate::editkeys::cut_columns(&self.body, lo, hi, unit, range, keep) {
                Some(e) => {
                    let scope = if lo == hi { "문서 전체" } else { "선택한 줄" };
                    self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
                    self.say(&format!("{scope}에 열 자르기를 적용했습니다"));
                }
                None => self.say("바뀌는 내용이 없습니다"),
            },
            EditReq::Snippet(i) => {
                if let Some(t) = self.snippet_text(i) {
                    let e = crate::editkeys::replace_range(&self.body, lo, hi, &t);
                    self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
                }
            }
            EditReq::AttachText => {
                if let Some(t) = self.attach_text.take() {
                    let e = crate::editkeys::replace_range(&self.body, lo, hi, &t);
                    self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
                }
            }
            EditReq::Abbrev => match crate::editkeys::expand_abbrev(&self.body, hi, &self.filled_snippets()) {
                Some(e) => self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi)),
                None => self.say("펼칠 약어가 없습니다 (이름 바로 뒤에서 Ctrl+J)"),
            },
            EditReq::Lines(op) => match crate::editkeys::line_op(&self.body, lo, hi, op) {
                Some(e) => {
                    let scope = if lo == hi { "문서 전체" } else { "선택한 줄" };
                    self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
                    self.say(&format!("{scope}: {}", match op { crate::editkeys::LineOp::SortAsc => "오름차순으로 정렬했습니다", crate::editkeys::LineOp::SortDesc => "내림차순으로 정렬했습니다", crate::editkeys::LineOp::Dedup => "겹치는 줄을 지웠습니다", crate::editkeys::LineOp::Number => "번호를 붙였습니다" }));
                }
                None => self.say("바뀌는 내용이 없습니다"),
            },
            EditReq::Respace(mode) => match crate::editkeys::respace(&self.body, lo, hi, mode, self.respace_md) {
                Some(e) => {
                    let scope = if lo == hi { "문서 전체" } else { "선택한 줄" };
                    self.apply_body_edit(ctx, id, e.text, (e.lo, e.hi));
                    self.say(&format!("{scope}의 공백을 {}", if mode == crate::editkeys::SpaceMode::ToTab { "탭으로 바꿨습니다" } else { "지웠습니다" }));
                }
                None => self.say("바꿀 공백이 없습니다"),
            },
        }
    }

    /// 편집창 밖(찾아 바꾸기 등)에서 본문을 바꾼다: 커서·저장 예약·줄 번호를 갱신한다.
    /// 편집창을 그리기 전에 부르면 egui의 실행 취소 기록에 한 번의 변경으로 들어간다.
    fn apply_body_edit(&mut self, ctx: &egui::Context, id: egui::Id, text: String, sel: (usize, usize)) {
        use egui::text::{CCursor, CCursorRange};
        self.body = text;
        let sel = self.renumber_after(sel);
        let n = self.body.chars().count();
        let (x, y) = (sel.0.min(n), sel.1.min(n));
        let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(x), CCursor::new(y))));
        st.store(ctx, id);
        self.cursor_idx = y;
        self.cursor_line = Some(md::line_of_char(&self.body, y));
        self.last_edit.get_or_insert_with(Instant::now);
        ctx.request_repaint();
    }

    // ------------------------------------------------------------ 편집기 키
    fn editor_keys(&mut self, ctx: &egui::Context, id: egui::Id) {
        use egui::text::{CCursor, CCursorRange};
        if self.cur_trashed || !ctx.memory(|m| m.has_focus(id)) {
            return;
        }
        self.paste_image_if_any(ctx); // 클립보드에 그림만 있으면 이미지로 넣는다
        let Some(mut state) = egui::TextEdit::load_state(ctx, id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        let (pi, si): (usize, usize) = (range.primary.index.into(), range.secondary.index.into());
        let (a, b) = (pi.min(si), pi.max(si));
        let mut changed = false;
        let mut new_cursor: Option<(usize, usize)> = None;

        let rev = pi < si; // 선택을 오른쪽→왼쪽으로 했으면 방향을 유지한다
        let sel = |lo: usize, hi: usize| if rev { (hi, lo) } else { (lo, hi) };
        let pairs: [(Key, Modifiers, &str, &str); 4] = [
            (Key::B, Modifiers::COMMAND, "**", "**"),
            (Key::I, Modifiers::COMMAND, "*", "*"),
            (Key::X, Modifiers::COMMAND | Modifiers::SHIFT, "~~", "~~"),
            (Key::Backtick, Modifiers::COMMAND, "`", "`"),
        ];
        for (k, m, l, r) in pairs {
            if key_exact(ctx, m, k) {
                // 이미 감싸져 있으면 기호를 걷어내고, 아니면 감싼다
                let e = crate::editkeys::toggle_wrap(&self.body, a, b, l, r);
                self.body = e.text;
                new_cursor = Some(sel(e.lo, e.hi));
                changed = true;
                break;
            }
        }
        if key_exact(ctx, Modifiers::COMMAND, Key::J) {
            self.edit_req = Some(EditReq::Abbrev);
        }
        if !changed && key_exact(ctx, Modifiers::COMMAND, Key::K) {
            let (ba, bb) = (byte_of(&self.body, a), byte_of(&self.body, b));
            self.body.insert_str(bb, "](주소)");
            self.body.insert_str(ba, "[");
            new_cursor = Some((a + 1, b + 1));
            changed = true;
        }
        // 한글 조합 상태 추적: 조합 중이거나 이번 프레임에 조합이 끝났다면 Enter는 조합 확정용이다.
        let composing_before = self.body_composing;
        let mut ime_this_frame = false;
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Ime(ev) = e {
                    match ev {
                        egui::ImeEvent::Preedit { text, .. } => self.body_composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => {
                            self.body_composing = false;
                            ime_this_frame = true;
                        }
                        _ => {}
                    }
                }
            }
        });
        let ime_busy = composing_before || self.body_composing || ime_this_frame;
        // 편집 키 묶음: 들여쓰기·줄 이동/복제/삭제·페이지 이동 (한글 조합 중에는 건드리지 않는다)
        if !changed && !ime_busy {
            use crate::editkeys as ek;
            let edit: Option<Option<ek::Edit>> = if key_exact(ctx, Modifiers::NONE, Key::Tab) {
                Some(ek::indent(&self.body, a, b, false))
            } else if key_exact(ctx, Modifiers::SHIFT, Key::Tab) {
                Some(ek::indent(&self.body, a, b, true))
            } else if key_exact(ctx, Modifiers::ALT, Key::ArrowUp) {
                Some(ek::move_lines(&self.body, a, b, true))
            } else if key_exact(ctx, Modifiers::ALT, Key::ArrowDown) {
                Some(ek::move_lines(&self.body, a, b, false))
            } else if key_exact(ctx, Modifiers::COMMAND, Key::D) {
                Some(Some(ek::duplicate_lines(&self.body, a, b)))
            } else if key_exact(ctx, Modifiers::COMMAND | Modifiers::SHIFT, Key::K) {
                Some(Some(ek::delete_lines(&self.body, a, b)))
            } else {
                None
            };
            if let Some(e) = edit {
                // 바꿀 것이 없어도(None) 키는 소비했으므로 Tab이 글자를 지우거나 포커스를 옮기지 않는다.
                if let Some(e) = e {
                    self.body = e.text;
                    new_cursor = Some(sel(e.lo, e.hi));
                    changed = true;
                }
            } else if key_exact(ctx, Modifiers::NONE, Key::PageDown) {
                self.page_req = Some((true, false));
            } else if key_exact(ctx, Modifiers::SHIFT, Key::PageDown) {
                self.page_req = Some((true, true));
            } else if key_exact(ctx, Modifiers::NONE, Key::PageUp) {
                self.page_req = Some((false, false));
            } else if key_exact(ctx, Modifiers::SHIFT, Key::PageUp) {
                self.page_req = Some((false, true));
            }
            if self.page_req.is_some() {
                ctx.request_repaint();
            }
        }
        // Enter: 목록·인용·체크박스 이어쓰기
        if !changed && !ime_busy && a == b && ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any()) {
            let bi = byte_of(&self.body, a);
            let ls = self.body[..bi].rfind('\n').map_or(0, |p| p + 1);
            let le = self.body[bi..].find('\n').map_or(self.body.len(), |p| bi + p);
            let line = self.body[ls..le].trim_end_matches('\r').to_string();
            let in_fence = self.body[..ls].lines().filter(|l| l.trim_start().starts_with("```")).count() % 2 == 1;
            if !in_fence && bi == ls + line.len() {
                match md::continue_list(&line) {
                    Some(md::Enter::Insert(p)) => {
                        ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                        // 줄 끝이 CRLF인 본문이면 CRLF로 이어 줄 바꿈이 섞이지 않게 한다.
                        let nl = if self.body[bi..].starts_with('\r') { "\r\n" } else { "\n" };
                        self.body.insert_str(bi, &format!("{nl}{p}"));
                        let c = a + nl.chars().count() + p.chars().count();
                        new_cursor = Some((c, c));
                        changed = true;
                    }
                    Some(md::Enter::Clear) => {
                        ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                        self.body.replace_range(ls..ls + line.len(), "");
                        let c = self.body[..ls].chars().count();
                        new_cursor = Some((c, c));
                        changed = true;
                    }
                    None => {}
                }
            }
        }
        if changed {
            if let Some((x, y)) = new_cursor {
                new_cursor = Some(self.renumber_after((x, y)));
            }
            if let Some((x, y)) = new_cursor {
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(x), CCursor::new(y))));
            }
            state.store(ctx, id);
            self.last_edit.get_or_insert_with(Instant::now);
        }
    }

    // ------------------------------------------------------------ 왼쪽 패널
    fn row_widget(ui: &mut egui::Ui, h: f32, selected: bool, t: &Theme) -> (egui::Response, Rect) {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
        let bg = if selected { t.sel } else if resp.hovered() { t.hover } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
        (resp, rect)
    }

    fn label_in(ui: &mut egui::Ui, rect: Rect, x: f32, text: &str, size: f32, color: Color32) {
        let width = (rect.width() - x - 6.0).max(10.0);
        let mut job = LayoutJob::simple(text.to_owned(), FontId::proportional(size), color, width);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let pos = Pos2::new(rect.left() + x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }

    fn drop_frame(ui: &mut egui::Ui, rect: Rect, resp: &egui::Response, accent: Color32) {
        if resp.dnd_hover_payload::<DocDrag>().is_some() || resp.dnd_hover_payload::<CatDrag>().is_some() {
            ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.5, accent), StrokeKind::Inside);
        }
    }

    /// 보기 막대(전체 · 미분류 · ★ · 휴지통을 한 줄에). 그리기와 동작은 `view_bar_ui`.
    fn view_bar(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        view_bar_ui(ui, &self.theme, self.counts, &self.filter.view, self.filter.tags.is_empty(), acts);
    }
    fn cat_tree(&self, ui: &mut egui::Ui, parent: Option<i64>, depth: usize, acts: &mut Vec<Act>) {
        for c in self.cats.iter().filter(|c| c.parent == parent) {
            let has_kids = self.cats.iter().any(|k| k.parent == Some(c.id));
            let open = !self.collapsed.contains(&c.id);
            let selected = self.filter.view == View::Category(c.id) && self.filter.tags.is_empty();
            let (resp, rect) = Self::row_widget(ui, (self.cfg.cat_font_size + 10.0).max(24.0), selected, &self.theme);
            let x0 = 8.0 + depth as f32 * 14.0;
            if has_kids {
                let m = Pos2::new(rect.left() + x0 + 5.0, rect.center().y);
                let pts = if open {
                    vec![Pos2::new(m.x - 4.0, m.y - 2.0), Pos2::new(m.x + 4.0, m.y - 2.0), Pos2::new(m.x, m.y + 3.0)]
                } else {
                    vec![Pos2::new(m.x - 2.0, m.y - 4.0), Pos2::new(m.x - 2.0, m.y + 4.0), Pos2::new(m.x + 3.0, m.y)]
                };
                ui.painter().add(egui::Shape::convex_polygon(pts, self.theme.dim, Stroke::NONE));
                // 화살표 칸을 누르면 접기/펴기
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        if p.x < rect.left() + x0 + 12.0 {
                            acts.push(Act::ToggleCollapse(c.id));
                        }
                    }
                }
            }
            Self::label_in(ui, rect, x0 + 14.0, &c.name, self.cfg.cat_font_size, self.theme.text);
            ui.painter().text(Pos2::new(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, c.count.to_string(), FontId::proportional((self.cfg.cat_font_size - 1.0).max(10.0)), self.theme.dim);
            Self::drop_frame(ui, rect, &resp, self.theme.accent);
            if resp.clicked() {
                let on_arrow = has_kids && resp.interact_pointer_pos().map_or(false, |p| p.x < rect.left() + x0 + 12.0);
                if !on_arrow {
                    acts.push(Act::SetView(View::Category(c.id)));
                    if !open {
                        acts.push(Act::ToggleCollapse(c.id));
                    }
                }
            }
            if resp.drag_started() {
                resp.dnd_set_drag_payload(CatDrag(c.id));
            }
            if let Some(p) = resp.dnd_release_payload::<DocDrag>() {
                acts.push(Act::MoveDoc(p.0, Some(c.id)));
            }
            if let Some(p) = resp.dnd_release_payload::<CatDrag>() {
                if p.0 != c.id {
                    acts.push(Act::MoveCat(p.0, Some(c.id)));
                }
            }
            resp.context_menu(|ui| {
                if crate::icons::btn(ui, crate::icons::Icon::Plus, "하위 분류 만들기").clicked() { acts.push(Act::PromptNewCat(Some(c.id))); ui.close(); }
                if crate::icons::btn(ui, crate::icons::Icon::Edit, "이름 바꾸기").clicked() { acts.push(Act::PromptRename(c.id)); ui.close(); }
                if c.parent.is_some() && crate::icons::btn(ui, crate::icons::Icon::Up, "최상위로 옮기기").clicked() { acts.push(Act::MoveCat(c.id, None)); ui.close(); }
                if crate::icons::btn(ui, crate::icons::Icon::Trash, "삭제 (하위·문서는 한 단계 위로)").clicked() { acts.push(Act::DelCat(c.id)); ui.close(); }
            });
            if has_kids && open {
                self.cat_tree(ui, Some(c.id), depth + 1, acts);
            }
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let t = self.theme.clone();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if crate::icons::btn_ex(ui, crate::icons::Icon::NewDoc, "새 문서  (Ctrl+N)", true, Some(egui::vec2((ui.available_width() - 32.0).max(60.0), 28.0))).clicked() {
                acts.push(Act::NewDoc);
            }
            ui.menu_button("▾", |ui| self.new_doc_menu(ui, acts)).response.on_hover_text("오늘의 노트 · 템플릿 · 서식·스니펫 관리");
        });
        let r = ui
            .horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("검색 (Ctrl+E)").desired_width((ui.available_width() - 32.0).max(60.0)));
                ui.menu_button("★", |ui| self.saved_search_menu(ui)).response.on_hover_text("저장된 검색");
                r
            })
            .inner;
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
        if r.changed() {
            self.list_dirty = true;
        }
        ui.add_space(4.0);
        // 보기 막대 오른쪽 끝에 "+"(최상위 분류 만들기)를 둔다. 분류를 끌어 이 단추에 놓으면 최상위로 옮겨진다.
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 34.0).max(60.0);
            ui.allocate_ui(Vec2::new(w, 26.0), |ui| self.view_bar(ui, acts));
            let r = ui.add_sized([28.0, 26.0], egui::Button::new("+")).on_hover_text("최상위 분류 만들기 (분류를 끌어 놓으면 최상위로)");
            Self::drop_frame(ui, r.rect, &r, t.accent);
            if let Some(p) = r.dnd_release_payload::<CatDrag>() {
                acts.push(Act::MoveCat(p.0, None));
            }
            if r.clicked() {
                acts.push(Act::PromptNewCat(None));
            }
        });
        ui.add_space(4.0);

        // 분류 목록 높이: 저장된 값이 있으면 그 높이로 고정, 없으면 자동(남는 높이의 45%까지). 아래 경계를 끌어 바꾼다.
        let free = ui.available_height();
        let fixed = (self.cfg.cat_height > 0.0).then(|| self.cfg.cat_height.clamp(60.0, (free - 140.0).max(60.0)));
        let mut sa = egui::ScrollArea::vertical().id_salt("cats").auto_shrink([false, fixed.is_none()]);
        sa = match fixed {
            Some(h) => sa.min_scrolled_height(h).max_height(h),
            None => sa.max_height((free * 0.45).max(120.0)),
        };
        let cat_out = sa.show(ui, |ui| {
            self.cat_tree(ui, None, 0, acts);
        });
        let (hrect, hresp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 7.0), Sense::drag());
        let hot = hresp.hovered() || hresp.dragged();
        ui.painter().hline(hrect.x_range(), hrect.center().y, Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { t.accent } else { t.border }));
        let hresp = hresp.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("끌어서 분류 목록 높이 조절 (더블클릭: 자동)");
        if hresp.dragged() {
            let cur = cat_out.inner_rect.height();
            self.cfg.cat_height = (cur + hresp.drag_delta().y).clamp(60.0, (free - 140.0).max(60.0));
            self.graph_cfg_dirty = true; // 마우스를 놓은 뒤 저장
        }
        if hresp.double_clicked() {
            self.cfg.cat_height = 0.0;
            self.graph_cfg_dirty = true;
        }

        if !self.tags.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("태그").size(crate::theme::ui(0.0)).color(t.dim));
            egui::ScrollArea::vertical().id_salt("tags").max_height(80.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for tag in &self.tags {
                        let sel = self.filter.tags.contains(tag);
                        let resp = ui.selectable_label(sel, tag);
                        if resp.clicked() {
                            acts.push(Act::ToggleTag(tag.clone()));
                        }
                        if let Some(p) = resp.dnd_release_payload::<DocDrag>() {
                            acts.push(Act::TagDoc(p.0, tag.clone()));
                        }
                    }
                });
            });
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let names = ["수정순", "만든순", "제목순", "직접 정렬"];
            let cur = self.cfg.sort.min(3) as usize;
            egui::ComboBox::from_id_salt("sort").selected_text(names[cur]).width(90.0).show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(cur == i, *n).clicked() {
                        self.cfg.sort = i as u8;
                        self.filter.sort = [Sort::Updated, Sort::Created, Sort::Title, Sort::Manual][i];
                        self.list_dirty = true;
                    }
                }
            });
            ui.label(egui::RichText::new(format!("{}개", self.rows.len())).size(crate::theme::ui(0.0)).color(t.dim));
            if self.filter.view == View::Trash && !self.rows.is_empty() && crate::icons::btn(ui, crate::icons::Icon::Trash, "휴지통 비우기").clicked() {
                acts.push(Act::EmptyTrash);
            }
        });

        // 문서 목록(가상 스크롤: 보이는 줄만 그린다)
        let row_h = 26.0;
        let n = self.list_rows.len();
        egui::ScrollArea::vertical().id_salt("docs").auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, range| {
            for i in range {
                match &self.list_rows[i] {
                    ListRow::Header(h) => {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                        ui.painter().text(Pos2::new(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, h, FontId::proportional(crate::theme::ui(0.0)), t.accent);
                    }
                    ListRow::Doc(ix) => {
                        let r = &self.rows[*ix];
                        let selected = self.cur == Some(r.id);
                        let (resp, rect) = Self::row_widget(ui, row_h, selected, &t);
                        let title = if r.title.trim().is_empty() { "(제목 없음)" } else { &r.title };
                        let color = if r.title.trim().is_empty() { t.dim } else { t.text };
                        Self::label_in(ui, rect, 8.0, title, 14.0, color);
                        let tip = format!("{}\n수정 {}", self.cat_path(r.category), fmt_time(r.updated));
                        let resp = resp.on_hover_text(tip);
                        if resp.clicked() {
                            acts.push(Act::Open(r.id));
                        }
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(DocDrag(r.id));
                        }
                        // 직접 정렬: 다른 문서 행 위에 놓으면 순서 변경(위쪽 절반=앞, 아래쪽 절반=뒤)
                        let trashed = self.filter.view == View::Trash;
                        let why_not = if self.filter.sort != Sort::Manual {
                            Some("순서를 바꾸려면 정렬을 '직접 정렬'로 바꾸세요")
                        } else if trashed {
                            Some("휴지통에서는 순서를 바꿀 수 없습니다")
                        } else if !self.search.trim().is_empty() {
                            Some("검색 중에는 순서를 바꿀 수 없습니다")
                        } else if !self.filter.tags.is_empty() {
                            Some("태그 필터 중에는 순서를 바꿀 수 없습니다")
                        } else {
                            None
                        };
                        let after_half = ui.input(|i| i.pointer.interact_pos()).map_or(false, |p| p.y > rect.center().y);
                        if let Some(p) = resp.dnd_hover_payload::<DocDrag>() {
                            if p.0 != r.id && why_not.is_none() {
                                let y = if after_half { rect.bottom() - 1.0 } else { rect.top() + 1.0 };
                                ui.painter().hline(rect.x_range(), y, Stroke::new(2.0, t.accent));
                            }
                        }
                        if let Some(p) = resp.dnd_release_payload::<DocDrag>() {
                            if p.0 != r.id {
                                match why_not {
                                    Some(m) => acts.push(Act::Say(m.to_string())),
                                    None => match Db::plan_reorder(&self.rows, p.0, r.id, after_half) {
                                        Ok((id, before)) => acts.push(Act::Reorder(id, before)),
                                        Err(m) => acts.push(Act::Say(m)),
                                    },
                                }
                            }
                        }
                        resp.context_menu(|ui| {
                            if trashed {
                                if crate::icons::btn(ui, crate::icons::Icon::Restore, "복원").clicked() { acts.push(Act::Restore(r.id)); ui.close(); }
                                if crate::icons::btn(ui, crate::icons::Icon::Cross, "영구 삭제").clicked() { acts.push(Act::Purge(r.id)); ui.close(); }
                            } else {
                                if crate::icons::btn(ui, crate::icons::Icon::Star, if r.pinned { "즐겨찾기 해제" } else { "즐겨찾기" }).clicked() { acts.push(Act::Pin(r.id)); ui.close(); }
                                if crate::icons::btn(ui, crate::icons::Icon::Copy, "문서 링크 복사").on_hover_text("[[제목]] 형태로 클립보드에 복사합니다. 다른 문서에 붙여넣으면 이 문서로 연결됩니다").clicked() {
                                    match md::wiki_tag(&r.title) {
                                        Ok(tag) => {
                                            ui.ctx().copy_text(tag.clone());
                                            acts.push(Act::Say(format!("복사했습니다: {tag}")));
                                        }
                                        Err(m) => acts.push(Act::Say(m)),
                                    }
                                    ui.close();
                                }
                                if crate::icons::btn(ui, crate::icons::Icon::Detach, "새 창으로 열기").on_hover_text("읽기 전용 참고 창으로 엽니다").clicked() { acts.push(Act::OpenRef(r.id)); ui.close(); }
                                ui.menu_button("분류로 이동", |ui| {
                                    if ui.button("미분류").clicked() { acts.push(Act::MoveDoc(r.id, None)); ui.close(); }
                                    ui.separator();
                                    self.move_menu(ui, None, r.id, 0, acts);
                                });
                                if crate::icons::btn(ui, crate::icons::Icon::Trash, "휴지통").clicked() { acts.push(Act::Trash(r.id)); ui.close(); }
                            }
                        });
                    }
                }
            }
        });
    }

    // ------------------------------------------------------------ 가운데
    fn central(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let t = self.theme.clone();
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if crate::icons::btn(ui, crate::icons::Icon::Sidebar, if self.cfg.sidebar { "목록 접기" } else { "목록 펴기" }).on_hover_text("Ctrl+\\").clicked() {
                self.cfg.sidebar = !self.cfg.sidebar;
            }
            if crate::icons::btn_ex(ui, crate::icons::Icon::Back, "뒤로", !self.back.is_empty(), None).on_hover_text("Alt+←").clicked() {
                acts.push(Act::Back);
            }
            if crate::icons::btn_ex(ui, crate::icons::Icon::Forward, "앞으로", !self.fwd.is_empty(), None).on_hover_text("Alt+→").clicked() {
                acts.push(Act::Forward);
            }
            if crate::icons::btn_ex(ui, crate::icons::Icon::Detach, "새 창", self.cur.is_some(), None).on_hover_text("지금 문서를 읽기 전용 참고 창으로 엽니다(여러 개 가능)").clicked() {
                if let Some(c) = self.cur {
                    acts.push(Act::OpenRef(c));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if crate::icons::btn(ui, crate::icons::Icon::Options, "옵션").on_hover_text("Ctrl+,").clicked() {
                    self.show_options = true;
                }
                if crate::icons::btn(ui, crate::icons::Icon::Help, "도움말").on_hover_text("F1 — 마크다운 문법·단축키를 새 창으로").clicked() {
                    self.show_help = true;
                }
                let n = self.out_links.len() + self.in_links.len();
                // 오른쪽 사이드바 단추 하나: 누를 때마다 열고 닫는다. 연결 그래프/목차 전환은 사이드바 안쪽 탭에서 한다.
                let label = if n > 0 { format!("사이드바 {n}") } else { "사이드바".to_string() };
                if ui.selectable_label(self.cfg.links_panel, label).on_hover_text("오른쪽 사이드바: 연결 그래프·목차 (숫자는 연결된 문서 수)").clicked() {
                    self.cfg.links_panel = !self.cfg.links_panel;
                    self.graph_cfg_dirty = true; // 설정 저장
                }
                if self.cur.is_some() {
                    ui.menu_button("…", |ui| {
                        if crate::icons::btn(ui, crate::icons::Icon::Star, if self.cur_pinned { "즐겨찾기 해제" } else { "즐겨찾기 (Ctrl+Shift+D)" }).clicked() { acts.push(Act::Pin(self.cur.unwrap())); ui.close(); }
                        if crate::icons::btn(ui, crate::icons::Icon::Export, ".md로 내보내기").clicked() { acts.push(Act::Export); ui.close(); }
                        if crate::icons::btn(ui, crate::icons::Icon::Import, ".md/.txt 가져오기").clicked() { acts.push(Act::Import); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Attach, "파일 첨부…", self.cur.is_some() && !self.cur_trashed, None).on_hover_text("파일(15MB까지)을 DB에 넣고 본문에 링크를 둡니다. 링크를 Ctrl+클릭하면 엽니다").clicked() { acts.push(Act::Attach(None)); ui.close(); }
                        if crate::icons::btn(ui, crate::icons::Icon::Import, "폴더 가져오기…").on_hover_text("Obsidian·Notion·일반 마크다운 폴더나 아카식 내보내기 폴더를 읽습니다. 하위 폴더는 분류가 되고, 머리말(YAML)의 제목·태그를 씁니다").clicked() { acts.push(Act::ImportDir); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Export, "HTML로 저장…", self.cur.is_some(), None).on_hover_text("지금 문서를 그림이 들어간 HTML 파일 하나로 저장합니다").clicked() { acts.push(Act::ExportHtml); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Export, "PDF로 저장…", self.cur.is_some() && self.pdf_rx.is_none(), None).on_hover_text("지금 문서를 PDF로 저장합니다(Microsoft Edge를 화면 없이 사용)").clicked() { acts.push(Act::ExportPdf); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Import, "이미지 삽입…", !self.cur_trashed, None).on_hover_text("그림을 WebP로 줄여 문서에 넣습니다(붙여넣기·끌어놓기도 됩니다)").clicked() { acts.push(Act::InsertImage); ui.close(); }
                        ui.separator();
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Import, "공백을 탭으로", !self.cur_trashed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)의 글자 사이 공백을 탭 하나로 바꿉니다. 줄 앞 들여쓰기는 그대로 둡니다").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Respace(crate::editkeys::SpaceMode::ToTab))); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Import, "공백 제거", !self.cur_trashed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)의 글자 사이 공백을 모두 지웁니다. 줄 앞 들여쓰기·줄바꿈은 그대로 둡니다").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Respace(crate::editkeys::SpaceMode::Remove))); ui.close(); }
                        if let Some(i) = self.snippet_menu(ui) { acts.push(Act::EditAt(self.last_sel, EditReq::Snippet(i))); ui.close(); }
                        ui.checkbox(&mut self.respace_md, "마크다운 구조 보존").on_hover_text("목록 기호·체크박스·제목 #·인용 >, 표 줄, 코드 블록은 바꾸지 않습니다");
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Edit, "줄 오름차순 정렬", !self.cur_trashed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)을 글자 순서로 정렬합니다").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Lines(crate::editkeys::LineOp::SortAsc))); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Edit, "줄 내림차순 정렬", !self.cur_trashed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)을 거꾸로 정렬합니다").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Lines(crate::editkeys::LineOp::SortDesc))); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Edit, "겹치는 줄 지우기", !self.cur_trashed, None).on_hover_text("같은 줄이 또 나오면 뒤의 것을 지웁니다(빈 줄은 그대로)").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Lines(crate::editkeys::LineOp::Dedup))); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Edit, "줄 번호 매기기", !self.cur_trashed, None).on_hover_text("빈 줄이 아닌 줄 앞에 1. 2. 3. 번호를 붙입니다").clicked() { acts.push(Act::EditAt(self.last_sel, EditReq::Lines(crate::editkeys::LineOp::Number))); ui.close(); }
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Edit, "열 자르기…", !self.cur_trashed, None).on_hover_text("각 줄에서 A~B번째·처음 N개·끝 N개만 남기거나 지웁니다(글자 위치 또는 탭 열 기준)").clicked() { acts.push(Act::ColDialog); ui.close(); }
                        ui.separator();
                        if self.cur_trashed {
                            if crate::icons::btn(ui, crate::icons::Icon::Restore, "복원").clicked() { acts.push(Act::Restore(self.cur.unwrap())); ui.close(); }
                        } else if crate::icons::btn(ui, crate::icons::Icon::Trash, "휴지통으로").clicked() { acts.push(Act::Trash(self.cur.unwrap())); ui.close(); }
                    });
                    let star = ui.selectable_label(self.cur_pinned, "    ").on_hover_text("즐겨찾기 (Ctrl+Shift+D)");
                    crate::icons::paint_in(ui, &star, crate::icons::Icon::Star, crate::icons::side(ui));
                    if star.clicked() {
                        acts.push(Act::Pin(self.cur.unwrap()));
                    }
                }
            });
        });
        ui.separator();

        let Some(cur) = self.cur else {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("문서가 선택되지 않았습니다").size(crate::theme::ui(0.0)).color(t.dim));
                ui.add_space(8.0);
                if crate::icons::btn(ui, crate::icons::Icon::NewDoc, "새 문서 만들기").clicked() {
                    acts.push(Act::NewDoc);
                }
            });
            return;
        };

        if self.cur_trashed {
            ui.horizontal(|ui| {
                ui.colored_label(t.accent, "휴지통에 있는 문서입니다 (읽기 전용)");
                if crate::icons::btn(ui, crate::icons::Icon::Restore, "복원").clicked() { acts.push(Act::Restore(cur)); }
            });
        }

        // 제목
        let tr = ui.add_enabled(
            !self.cur_trashed,
            egui::TextEdit::singleline(&mut self.title)
                .hint_text("제목 (비우면 본문 첫 줄이 제목)")
                .font(FontId::new(self.cfg.font_size * 1.5, egui::FontFamily::Name("bold".into())))
                .desired_width(f32::INFINITY),
        );
        if tr.changed() {
            self.last_edit.get_or_insert_with(Instant::now);
        }

        // 태그 + 분류 경로
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(self.cat_path(self.cur_cat)).size(crate::theme::ui(0.0)).color(t.dim));
            for tag in self.doc_tags.clone() {
                if ui.add_enabled(!self.cur_trashed, egui::Button::new(format!("{tag}  ×"))).on_hover_text("태그 지우기").clicked() {
                    acts.push(Act::RemoveTag(tag));
                }
            }
            let r = ui.add_enabled(!self.cur_trashed, egui::TextEdit::singleline(&mut self.new_tag).hint_text("태그 추가").desired_width(90.0));
            if r.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) && !self.new_tag.trim().is_empty() {
                acts.push(Act::AddTag);
                r.request_focus();
            }
        });
        ui.separator();

        self.body_editor(ui, acts);
    }

    fn body_editor(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let ctx = ui.ctx().clone();
        #[allow(deprecated)]
        let id = egui::Id::new("body_edit");
        // Ctrl+G: 줄 번호로 이동 막대 열기(현재 줄 번호를 미리 채워 전체 선택)
        if !self.cur_trashed && key_exact(&ctx, Modifiers::COMMAND, Key::G) {
            self.goto_open = true;
            self.goto_focus = true;
            self.goto_text = (md::line_of_char(&self.body, self.cursor_idx) + 1).to_string();
        }
        self.editor_keys(&ctx, id);
        self.apply_edit_req(&ctx, id);
        self.find_keys(&ctx, id);
        self.find_bar(ui, id);
        self.goto_bar(ui, id);
        let find_jump = self.find_apply_jump(&ctx, id);
        // 목차에서 고른 줄로 커서를 옮긴다(스크롤은 아래 편집창을 그린 뒤).
        let mut scroll_to: Option<usize> = None;
        if let Some(line) = self.goto_line.take() {
            use egui::text::{CCursor, CCursorRange};
            let idx = md::char_of_line(&self.body, line);
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(idx))));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            self.cursor_idx = idx;
            self.cursor_line = Some(line);
            scroll_to = Some(idx);
        }
        // Ctrl+마우스 휠: 본문 글자 크기(포인터가 본문 위에 있을 때만)
        if ui.rect_contains_pointer(ui.max_rect()) {
            let z = ctx.input(|i| i.zoom_delta());
            if (z - 1.0).abs() > 0.001 {
                let ns = (self.cfg.font_size + if z > 1.0 { 1.0 } else { -1.0 }).clamp(8.0, 60.0);
                if ns != self.cfg.font_size {
                    self.cfg.font_size = ns;
                    self.graph_cfg_dirty = true; // 마우스를 놓은 뒤 저장
                }
            }
        }
        let t = self.theme.clone();
        // 아주 큰 문서는 줄마다 서식 기호를 숨겼다 보이는 방식이 전체 글자 배치를 다시 하게 만들어 느리므로 끈다.
        let big = self.body.len() > 300_000;
        let (size, hide, wrap_on, cl) = (self.cfg.font_size, self.cfg.hide_syntax && !big, self.cfg.wrap, self.cursor_line);
        // 글꼴을 방금 바꿨으면: 바꾼 패스에서는 옛 글꼴로 배치된 글자가 캐시에 들어갔을 수 있어, 새 글꼴이 적용된 패스에서 한 번 비운다.
        if self.font_swap_pass.is_some_and(|p| ctx.cumulative_pass_nr() > p) {
            self.font_swap_pass = None;
            self.galley_cache = None;
        }
        let cache = &mut self.galley_cache;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, width: f32| -> Arc<egui::Galley> {
            let text = buf.as_str();
            let w = if wrap_on { width } else { f32::INFINITY };
            let mut h = DefaultHasher::new();
            crate::render::cache_key(text).hash(&mut h);
            // 서식 기호를 숨기지 않으면 커서 줄이 배치에 영향이 없다 → 줄을 옮겨도 다시 배치하지 않는다.
            (if hide { cl } else { None }, w.to_bits(), size.to_bits(), t.name, hide).hash(&mut h);
            { let (a, b, c) = md::spacing(); (a.to_bits(), b.to_bits(), c.to_bits()).hash(&mut h); }
            let key = h.finish();
            if let Some((k, g)) = cache.as_ref() {
                if *k == key {
                    return g.clone();
                }
            }
            // 목록 줄이 줄바꿈되면 이어지는 행을 기호 뒤에 맞춰 들여쓴다: 밀려나는 폭만큼 미리 줄 폭을 줄여 둔다.
            let (hangs, maxm) = if w.is_finite() { md::hang_info(text) } else { (Vec::new(), 0) };
            let reserve = if maxm > 0 { size * (md::LIST_INDENT + 0.6 * maxm as f32) } else { 0.0 };
            let job = md::build(text, cl, size, &t, (w - reserve).max(60.0), hide);
            let mut g = ui.ctx().fonts_mut(|f| f.layout_job(job));
            if maxm > 0 {
                // 예상보다 많이 밀려나면(넓은 글자·체크박스) 실제 폭만큼 줄 폭을 줄여 다시 배치한다.
                let actual = md::max_hang_shift(&g, &hangs);
                if actual > reserve + 0.5 {
                    let job = md::build(text, cl, size, &t, (w - actual).max(60.0), hide);
                    g = ui.ctx().fonts_mut(|f| f.layout_job(job));
                }
            }
            if maxm > 0 {
                g = Arc::new(md::apply_hang(&g, &hangs));
            }
            *cache = Some((key, g.clone()));
            g
        };
        let editable = !self.cur_trashed;
        let mut area = if wrap_on { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        if let Some(o) = self.page_scroll.take() {
            area = area.scroll_offset(o); // PageUp/PageDown으로 정한 스크롤 위치
        }
        let pre_sel = Self::sel_range(&ctx, id); // 우클릭이 선택을 풀어 버리므로(egui는 버튼 종류를 가리지 않음) 그리기 전 선택을 기억해 둔다
        let blocks = crate::render::blocks_cached(&ctx, &self.body);
        let find_view: Option<(&[(usize, usize)], usize)> = if self.find.open { Some((&self.find.matches[..], self.find.cur)) } else { None };
        let bg = ui.visuals().extreme_bg_color;
        let avail_h = ui.available_height();
        self.img.begin_frame();
        let (img_cache, db_ref) = (&mut self.img, &self.db);
        let sa = area
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 표·코드 박스는 글자 뒤에 깔아야 하므로 자리를 먼저 잡고, 편집창 바탕은 투명하게 한다.
                let slot = ui.painter().add(egui::Shape::Noop);
                let out = egui::TextEdit::multiline(&mut self.body)
                    .id(id)
                    .interactive(editable)
                    .desired_width(f32::INFINITY)
                    .desired_rows(1)
                    .min_size(egui::vec2(0.0, (avail_h - 6.0).max(60.0)))
                    .lock_focus(true)
                    .background_color(egui::Color32::TRANSPARENT)
                    .layouter(&mut layouter)
                    .show(ui);
                let mut imgs = |h: &str| img_cache.texture(&ctx, db_ref, h);
                Self::paint_blocks_behind(ui, slot, &out, &blocks, bg, &t, size, hide, cl, find_view, &mut imgs);
                if let Some(idx) = find_jump {
                    // 찾은 곳이 보이는 영역 가운데쯤 오도록 스크롤한다.
                    let r = out.galley.pos_from_cursor(egui::text::CCursor::new(idx)).translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(r.expand2(egui::vec2(0.0, 40.0)), Some(egui::Align::Center));
                }
                if let Some(idx) = scroll_to {
                    // 고른 제목이 보이는 영역 맨 위쪽에 오도록 스크롤한다.
                    let r = out.galley.pos_from_cursor(egui::text::CCursor::new(idx));
                    let rect = r.translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect.expand2(egui::vec2(0.0, 28.0)), Some(egui::Align::TOP));
                }
                out
            });
        let (scroll_off, view_h) = (sa.state.clamped_offset(), sa.inner_rect.height());
        let out = sa.inner;
        if ctx.memory(|m| m.has_focus(id)) {
            self.last_sel = Self::sel_range(&ctx, id);
        }
        // 선택 안쪽을 우클릭하면 선택을 그대로 둔다(바깥을 우클릭하면 캐럿만 그 자리로 옮겨지는 기본 동작).
        if pre_sel.0 != pre_sel.1 && ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary)) {
            if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
                let at: usize = out.galley.cursor_from_pos(p - out.galley_pos).index.into();
                if out.response.rect.contains(p) && at >= pre_sel.0 && at <= pre_sel.1 {
                    let mut st = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(pre_sel.0), egui::text::CCursor::new(pre_sel.1))));
                    st.store(&ctx, id);
                    ctx.request_repaint();
                }
            }
        }
        {
            let ed = !self.cur_trashed;
            if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary)) {
                self.menu_sel = Self::sel_range(&ctx, id); // 우클릭 순간의 선택(복원 뒤)
            }
            let snap = self.menu_sel;
            let has_sel = snap.0 != snap.1;
            out.response.context_menu(|ui| {
                use crate::icons::{btn_ex, Icon};
                if btn_ex(ui, Icon::Export, "잘라내기", ed && has_sel, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Cut)); ui.close(); }
                if btn_ex(ui, Icon::Copy, "복사", has_sel, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Copy)); ui.close(); }
                if btn_ex(ui, Icon::Import, "붙여넣기", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Paste)); ui.close(); }
                if btn_ex(ui, Icon::Check, "전체 선택", true, None).clicked() { acts.push(Act::EditAt(snap, EditReq::SelectAll)); ui.close(); }
                ui.separator();
                if btn_ex(ui, Icon::Edit, "굵게 (Ctrl+B)", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Wrap("**", "**"))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "기울임 (Ctrl+I)", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Wrap("*", "*"))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "취소선 (Ctrl+Shift+X)", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Wrap("~~", "~~"))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "인라인 코드 (Ctrl+`)", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Wrap("`", "`"))); ui.close(); }
                if btn_ex(ui, Icon::Attach, "링크 (Ctrl+K)", ed, None).clicked() { acts.push(Act::EditAt(snap, EditReq::Link)); ui.close(); }
                if btn_ex(ui, Icon::Attach, "파일 첨부… (15MB까지)", ed, None).clicked() { acts.push(Act::Attach(Some(snap))); ui.close(); }
                ui.separator();
                if btn_ex(ui, Icon::Edit, "공백을 탭으로", ed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)의 글자 사이 공백을 탭 하나로 바꿉니다").clicked() { acts.push(Act::EditAt(snap, EditReq::Respace(crate::editkeys::SpaceMode::ToTab))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "공백 제거", ed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)의 글자 사이 공백을 모두 지웁니다").clicked() { acts.push(Act::EditAt(snap, EditReq::Respace(crate::editkeys::SpaceMode::Remove))); ui.close(); }
                if let Some(i) = self.snippet_menu(ui) { acts.push(Act::EditAt(snap, EditReq::Snippet(i))); ui.close(); }
                ui.checkbox(&mut self.respace_md, "마크다운 구조 보존").on_hover_text("목록 기호·체크박스·제목 #·인용 >, 표 줄, 코드 블록은 바꾸지 않습니다");
                if btn_ex(ui, Icon::Edit, "줄 오름차순 정렬", ed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)을 글자 순서로 정렬합니다").clicked() { acts.push(Act::EditAt(snap, EditReq::Lines(crate::editkeys::LineOp::SortAsc))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "줄 내림차순 정렬", ed, None).on_hover_text("선택한 줄(선택이 없으면 문서 전체)을 거꾸로 정렬합니다").clicked() { acts.push(Act::EditAt(snap, EditReq::Lines(crate::editkeys::LineOp::SortDesc))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "겹치는 줄 지우기", ed, None).on_hover_text("같은 줄이 또 나오면 뒤의 것을 지웁니다(빈 줄은 그대로)").clicked() { acts.push(Act::EditAt(snap, EditReq::Lines(crate::editkeys::LineOp::Dedup))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "줄 번호 매기기", ed, None).on_hover_text("빈 줄이 아닌 줄 앞에 1. 2. 3. 번호를 붙입니다").clicked() { acts.push(Act::EditAt(snap, EditReq::Lines(crate::editkeys::LineOp::Number))); ui.close(); }
                if btn_ex(ui, Icon::Edit, "열 자르기…", ed, None).on_hover_text("각 줄에서 A~B번째·처음 N개·끝 N개만 남기거나 지웁니다").clicked() { acts.push(Act::ColDialog); ui.close(); }
                ui.separator();
                if btn_ex(ui, Icon::Import, "이미지 삽입…", ed, None).clicked() { acts.push(Act::InsertImage); ui.close(); }
            });
        }
        if let Some((down, shift)) = self.page_req.take() {
            self.page_move_caret(&ctx, id, &out, scroll_off, view_h, down, shift);
        }
        if out.response.changed() {
            self.last_edit.get_or_insert_with(Instant::now);
            // 숫자 목록 번호 자동 고침(실행 취소 직후에는 되돌린 모양을 존중해 건드리지 않는다)
            let undo = ctx.input(|i| i.modifiers.command && (i.key_pressed(Key::Z) || i.key_pressed(Key::Y)));
            if self.cfg.auto_renumber && editable && !undo {
                let cur = out.cursor_range.map_or(self.cursor_idx, |r| usize::from(r.primary.index));
                if let Some((nt, nc)) = md::renumber(&self.body, cur) {
                    self.body = nt;
                    let mut st = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(nc))));
                    st.store(&ctx, id);
                    self.cursor_idx = nc;
                    ctx.request_repaint();
                }
            }
        }
        if let Some(r) = out.cursor_range {
            let idx: usize = r.primary.index.into();
            self.cursor_idx = idx;
            let line = md::line_of_char(&self.body, idx);
            if self.cursor_line != Some(line) {
                self.cursor_line = Some(line);
                ctx.request_repaint();
            }
        }
        // 마우스를 올린 곳이 누를 수 있는 곳이면 알려 준다: 체크박스는 손가락 모양, [[문서 링크]]·각주는 Ctrl을 누른 동안 손가락 모양
        // (Ctrl 없이 누르면 동작하지 않으므로, 누르지 않은 때는 "Ctrl+클릭" 안내 말풍선만 띄운다).
        if editable && out.response.hovered() {
            if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                let hidx: usize = out.galley.cursor_from_pos(p - out.galley_pos).index.into();
                if md::is_checkbox_at(&self.body, hidx) {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                } else if md::wiki_at(&self.body, hidx).is_some() || md::footnote_jump(&self.body, hidx).is_some() || md::file_at(&self.body, hidx).is_some() {
                    if ctx.input(|i| i.modifiers.command) {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    } else {
                        let _ = egui::Response::clone(&out.response).on_hover_text("Ctrl+클릭: 이동");
                    }
                }
            }
        }
        if out.response.clicked() && editable {
            let ctrl = ctx.input(|i| i.modifiers.command);
            if ctrl {
                if let Some(title) = md::wiki_at(&self.body, self.cursor_idx) {
                    acts.push(Act::Follow(title));
                } else if let Some((h, n)) = md::file_at(&self.body, self.cursor_idx) {
                    acts.push(Act::OpenFile(h, n));
                } else if let Some(line) = md::footnote_jump(&self.body, self.cursor_idx) {
                    self.goto_line = Some(line);
                    ctx.request_repaint();
                }
            } else if let Some(nb) = md::toggle_checkbox_at(&self.body, self.cursor_idx) {
                self.body = nb;
                self.last_edit.get_or_insert_with(Instant::now);
            }
        }
    }

    /// PageUp/PageDown: 캐럿과 화면을 한 화면 높이만큼 같이 옮긴다(캐럿의 화면상 위치 유지).
    /// 줄 높이가 제각각(표·그림·수식)이라 줄 수가 아니라 글 배치(galley)의 실제 y 좌표로 계산한다.
    #[allow(clippy::too_many_arguments)]
    fn page_move_caret(&mut self, ctx: &egui::Context, id: egui::Id, out: &egui::text_edit::TextEditOutput, off: Vec2, view_h: f32, down: bool, shift: bool) {
        use egui::text::CCursorRange;
        let Some(r) = out.cursor_range else { return };
        let caret = out.galley.pos_from_cursor(r.primary);
        let content_h = out.galley.rect.bottom();
        let (ny, ns) = crate::editkeys::page_move(caret.top(), off.y, view_h, content_h, down);
        let target = out.galley.cursor_from_pos(Vec2::new(caret.min.x, ny));
        let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        let range = if shift { CCursorRange::two(r.secondary, target) } else { CCursorRange::one(target) };
        st.cursor.set_char_range(Some(range));
        st.store(ctx, id);
        let idx: usize = target.index.into();
        self.cursor_idx = idx;
        self.cursor_line = Some(md::line_of_char(&self.body, idx));
        self.page_scroll = Some(Vec2::new(off.x, ns));
        ctx.request_repaint();
    }

    /// Ctrl+G 줄 번호 막대. Enter로 이동(범위를 넘으면 맨 끝 줄), 숫자가 아니면 안내, Esc로 닫기.
    fn goto_bar(&mut self, ui: &mut egui::Ui, body_id: egui::Id) {
        if !self.goto_open {
            return;
        }
        let ctx = ui.ctx().clone();
        #[allow(deprecated)]
        let eid = egui::Id::new("goto_edit");
        if self.goto_focus {
            let mut st = egui::TextEdit::load_state(&ctx, eid).unwrap_or_default();
            let len = self.goto_text.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(len))));
            st.store(&ctx, eid);
        }
        let (enter, esc) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        let total = crate::editkeys::line_count(&self.body);
        let dim = self.theme.dim;
        let mut go = false;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label("줄 번호로 이동");
            let r = ui.add(egui::TextEdit::singleline(&mut self.goto_text).id(eid).desired_width(80.0).hint_text("번호"));
            if self.goto_focus {
                r.request_focus();
                self.goto_focus = false;
            }
            if r.lost_focus() && enter {
                go = true;
            }
            if r.has_focus() && esc {
                close = true;
            }
            ui.label(egui::RichText::new(format!("(1~{total})")).color(dim));
            if ui.button("이동").clicked() {
                go = true;
            }
            if ui.button("닫기").on_hover_text("Esc").clicked() {
                close = true;
            }
        });
        ui.add_space(2.0);
        if go {
            match crate::editkeys::parse_goto(&self.goto_text, total) {
                Some(line) => {
                    let asked: Option<usize> = self.goto_text.trim().parse().ok();
                    if asked.is_none_or(|n| n > total) {
                        self.say(&format!("{total}줄이 끝입니다 — 맨 끝 줄로 이동했습니다"));
                    }
                    self.goto_line = Some(line);
                    close = true;
                }
                None => {
                    self.say("줄 번호는 숫자로 입력하세요");
                    self.goto_focus = true;
                }
            }
        }
        if close {
            self.goto_open = false;
            ctx.memory_mut(|m| m.request_focus(body_id));
        }
    }

    /// 편집창 뒤(자리 `slot`)에 바탕과 표·코드 박스·가로선을 깐다. 글자가 아닌 그림만 더한다.
    #[allow(clippy::too_many_arguments)]
    fn paint_blocks_behind(ui: &egui::Ui, slot: egui::layers::ShapeIdx, out: &egui::text_edit::TextEditOutput, blocks: &[md::Block], bg: egui::Color32, t: &theme::Theme, size: f32, hide: bool, cl: Option<usize>, find: Option<(&[(usize, usize)], usize)>, imgs: &mut dyn FnMut(&str) -> crate::img::Tex<egui::TextureHandle>) {
        let origin = out.galley_pos - egui::vec2(out.galley.rect.left(), 0.0);
        let mut shapes = vec![egui::Shape::rect_filled(out.response.rect, egui::CornerRadius::ZERO, bg)];
        let text_rect = egui::Rect::from_min_max(out.text_clip_rect.min, egui::pos2(out.response.rect.right() - 8.0, out.text_clip_rect.max.y));
        shapes.extend(crate::render::paint(ui, blocks, &out.galley, origin, text_rect, cl, t, size, hide, imgs));
        // 찾은 글자 표시는 표·코드 상자 바탕 위, 글자 뒤에 깐다.
        if let Some((m, cur)) = find {
            shapes.extend(Self::find_highlights(out, m, cur, t));
        }
        ui.painter().set(slot, egui::Shape::Vec(shapes));
    }

    fn links_panel(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let t = self.theme.clone();
        ui.add_space(4.0);
        // 모드 전환 탭: 연결 그래프 / 목차 / 함께(그래프를 위, 목차를 아래에 한 화면으로)
        ui.horizontal(|ui| {
            for (mode, name, tip) in [(2u8, "함께", "연결 그래프(위)와 목차(아래)를 한 화면에 봅니다"), (0u8, "그래프", "연결 그래프를 크게, 연결된 문서 목록과 함께 자세히 봅니다"), (1u8, "목차", "목차를 전체 높이로 자세히 봅니다")] {
                if ui.selectable_label(self.cfg.right_mode == mode, name).on_hover_text(tip).clicked() && self.cfg.right_mode != mode {
                    self.cfg.right_mode = mode;
                    self.graph_cfg_dirty = true; // 설정 저장
                }
            }
            if self.cfg.right_mode != 1 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (txt, tip) = if self.graph_window { ("붙이기", "분리된 그래프 창을 닫고 여기에 다시 표시합니다") } else { ("분리", "연결 그래프를 별도 창으로 띄워 크게 보고 문서를 찾습니다") };
                    if crate::icons::btn(ui, if self.graph_window { crate::icons::Icon::Attach } else { crate::icons::Icon::Detach }, txt).on_hover_text(tip).clicked() {
                        self.graph_window = !self.graph_window;
                        if let Some(g) = self.graph.as_mut() {
                            g.request_fit();
                        }
                    }
                });
            }
        });
        ui.separator();
        if self.cfg.right_mode == 1 {
            self.toc_panel(ui, acts);
            return;
        }
        if self.cfg.right_mode == 2 {
            // 함께: 그래프는 위쪽 45%, 나머지는 목차(연결된 문서 목록은 "연결 그래프" 탭에서 본다)
            let gh = (ui.available_height() * 0.45).max(150.0);
            self.graph_view(ui, gh, acts);
            ui.separator();
            self.toc_panel(ui, acts);
            return;
        }
        let gh = (ui.available_height() * 0.5).max(180.0);
        self.graph_view(ui, gh, acts);
        ui.add_space(4.0);
        ui.label(egui::RichText::new("이 문서가 가리키는 문서").size(crate::theme::ui(0.0)).color(t.dim));
        egui::ScrollArea::vertical().id_salt("outl").max_height(ui.available_height() / 2.0 - 20.0).show(ui, |ui| {
            if self.out_links.is_empty() {
                ui.label(egui::RichText::new("없음").color(t.dim));
            }
            for (title, id) in &self.out_links {
                let label = if id.is_some() { title.clone() } else { format!("{title}  (없는 문서 · 눌러서 만들기)") };
                if ui.selectable_label(false, label).clicked() {
                    acts.push(match id { Some(i) => Act::Open(*i), None => Act::Follow(title.clone()) });
                }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("이 문서를 가리키는 문서").size(crate::theme::ui(0.0)).color(t.dim));
        egui::ScrollArea::vertical().id_salt("inl").show(ui, |ui| {
            if self.in_links.is_empty() {
                ui.label(egui::RichText::new("없음").color(t.dim));
            }
            for (id, title) in &self.in_links {
                let label = if title.trim().is_empty() { "(제목 없음)" } else { title };
                if ui.selectable_label(false, label).clicked() {
                    acts.push(Act::Open(*id));
                }
            }
        });
    }

    /// 연결 그래프 상자(높이 gh). 별도 창으로 분리돼 있으면 안내만 그린다.
    fn graph_view(&mut self, ui: &mut egui::Ui, gh: f32, acts: &mut Vec<Act>) {
        let t = self.theme.clone();
        ui.add_space(2.0);
        let gsize = Vec2::new(ui.available_width(), gh);
        if self.graph_window {
            let (rect, _) = ui.allocate_exact_size(gsize, Sense::hover());
            ui.painter().rect_stroke(rect, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "연결 그래프를\n별도 창에서 보는 중", egui::FontId::proportional(crate::theme::ui(0.0)), t.dim);
        } else if let Some(g) = self.graph.as_mut() {
            let out = g.show(ui, gsize, &t, &mut self.cfg.graph);
            match out.act {
                Some(GraphAct::Open(id)) => acts.push(Act::Open(id)),
                Some(GraphAct::Create(title)) => acts.push(Act::Follow(title)),
                None => {}
            }
            if out.rebuild {
                self.graph_stale = true; // 표시 조건이 바뀌면 다시 만든다
            }
            if out.changed {
                self.graph_cfg_dirty = true; // 슬라이더를 놓은 뒤 저장
            }
        }
    }

    // ------------------------------------------------------------ 옵션
    fn options_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_options;
        let mut changed_theme = false;
        let mut notes: Vec<String> = vec![];
        egui::Window::new("옵션").open(&mut open).collapsible(false).resizable(true).default_size([560.0, 560.0]).min_size([480.0, 320.0]).show(ctx, |ui| {
            // 탭: 화면·편집 / 문서 저장소 / 백업·동기화 / 정보·라이선스
            ui.horizontal(|ui| {
                for (i, name) in [("화면·편집", 0usize), ("상태 표시줄", 4), ("문서 저장소", 1), ("백업·동기화", 2), ("정보·라이선스", 3)].iter().map(|(n, i)| (*i, *n)) {
                    if ui.selectable_label(self.opt_tab == i as u8, name).clicked() {
                        self.opt_tab = i as u8;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("opt_scroll").auto_shrink([false, false]).show(ui, |ui| match self.opt_tab {
                0 => {
                    // 테마·글꼴 UI는 theme_ui.rs (바뀌면 true)
                    if self.theme_options(ui) {
                        changed_theme = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label("본문 글자 크기");
                        ui.add(egui::Slider::new(&mut self.cfg.font_size, 8.0..=60.0).step_by(1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("화면 글자 크기");
                        if ui.add(egui::Slider::new(&mut self.cfg.ui_font_size, 10.0..=16.0).step_by(1.0)).changed() {
                            theme::set_ui_size(self.cfg.ui_font_size);
                            changed_theme = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("분류창 글자 크기");
                        ui.add(egui::Slider::new(&mut self.cfg.cat_font_size, 10.0..=24.0).step_by(1.0));
                    });
                    let mut sp_changed = false;
                    for (label, v, lo, hi) in [("줄 간격 (본문)", &mut self.cfg.line_spacing, 0.8, 2.5), ("목록 항목 간격", &mut self.cfg.list_spacing, 0.8, 2.5), ("문단 간격 (빈 줄 높이)", &mut self.cfg.para_spacing, 0.2, 3.0)] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            sp_changed |= ui.add(egui::Slider::new(v, lo..=hi).step_by(0.05).fixed_decimals(2)).changed();
                        });
                    }
                    if sp_changed {
                        md::set_spacing(self.cfg.line_spacing, self.cfg.list_spacing, self.cfg.para_spacing);
                    }
                    ui.checkbox(&mut self.cfg.wrap, "줄바꿈");
                    ui.checkbox(&mut self.cfg.auto_renumber, "숫자 목록 번호 자동 고침");
                    ui.checkbox(&mut self.cfg.hide_syntax, "커서가 없는 줄의 문법 기호 숨기기");
                    ui.separator();
                    ui.label(egui::RichText::new("저장 방식").strong());
                    ui.horizontal(|ui| {
                        ui.label("자동 저장 간격");
                        ui.add(egui::Slider::new(&mut self.cfg.autosave_min, 1..=20).suffix("분"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("로그 병합 간격");
                        ui.add(egui::Slider::new(&mut self.cfg.merge_min, 5..=240).suffix("분"));
                    });
                    if crate::icons::btn(ui, crate::icons::Icon::Merge, "지금 병합").clicked() {
                        self.merge_now();
                        notes.push("쓰기 로그를 본 파일에 병합했습니다".into());
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("그래픽 렌더러").strong());
                    let cur = self.cfg.renderer.clone();
                    let mut start_probe = false;
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("renderer").selected_text(crate::render_name(&cur)).show_ui(ui, |ui| {
                            for (k, v) in crate::RENDERERS {
                                if ui.selectable_value(&mut self.cfg.renderer, k.to_string(), *v).clicked() && *k != cur {
                                    start_probe = true;
                                }
                            }
                        });
                        if crate::icons::btn(ui, crate::icons::Icon::Sync, "시험").clicked() {
                            start_probe = true;
                        }
                    });
                    let sel = self.cfg.renderer.clone();
                    let running = self.probe_rx.iter().any(|(k, _)| *k == sel);
                    if start_probe && !running {
                        self.probes.insert(sel.clone(), crate::probe::Probe::Running);
                        self.probe_rx.push((sel.clone(), crate::probe::spawn(&sel, Duration::from_secs(10))));
                    }
                    ui.label(egui::RichText::new(format!("현재 사용 중: {} (바꾸면 다시 시작 후 적용)", crate::active_renderer())).size(crate::theme::ui(0.0)).color(self.theme.dim));
                    match self.probes.get(&sel) {
                        Some(crate::probe::Probe::Running) => {
                            ui.label(egui::RichText::new("시험 중…").size(crate::theme::ui(0.0)).color(self.theme.dim));
                        }
                        Some(crate::probe::Probe::Ok) => {
                            ui.label(egui::RichText::new("시험 결과: 사용 가능").size(crate::theme::ui(0.0)).color(self.theme.accent));
                        }
                        Some(crate::probe::Probe::Fail(why)) => {
                            ui.label(egui::RichText::new(format!("시험 결과: 사용 불가 — {why}")).size(crate::theme::ui(0.0)).color(egui::Color32::from_rgb(0xC0, 0x39, 0x2B)));
                        }
                        None => {
                            ui.label(egui::RichText::new("시험하지 않음 (시험 단추로 확인)").size(crate::theme::ui(0.0)).color(self.theme.dim));
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("메모리 캐시").strong());
                    ui.horizontal(|ui| {
                        ui.label("DB 캐시 최대");
                        if ui.add(egui::Slider::new(&mut self.cfg.cache_mb, 4..=1024).logarithmic(true).suffix(" MB")).changed() {
                            self.db.set_cache_mb(self.cfg.cache_mb);
                        }
                    });
                    ui.label(egui::RichText::new("메모리가 넉넉한 PC는 크게, 부족한 PC는 작게 (상한값이며 실제로 쓴 만큼만 차지)").size(crate::theme::ui(0.0)).color(self.theme.dim));
                    ui.add_space(4.0);
                    if crate::icons::btn(ui, crate::icons::Icon::Help, "저장 방식·렌더러·캐시 자세히 (도움말)").clicked() {
                        self.show_help = true;
                        self.help_topic = help_ui::topic_index("저장 방식과 SSD");
                    }                }
                1 => {
                    ui.label(egui::RichText::new("문서 저장소").strong());
                    ui.label(format!("{}  ({})", self.db.path.display(), fmt_size(self.storage.db)));
                    ui.horizontal(|ui| {
                        ui.label("휴지통 보관(일)");
                        ui.add(egui::DragValue::new(&mut self.cfg.trash_days).range(1..=365));
                    });
                    ui.horizontal_wrapped(|ui| {
                        if crate::icons::btn(ui, crate::icons::Icon::Database, "DB 정리").on_hover_text("빈 공간을 없애고 검색 색인을 최적화합니다").clicked() {
                            self.fs_invalidate(false);
                            notes.push(match self.db.compact() { Ok((a, b)) => format!("DB 정리: {} → {}", fmt_size(a), fmt_size(b)), Err(x) => format!("정리 실패: {x}") });
                        }
                        if crate::icons::btn(ui, crate::icons::Icon::Search, "검색 색인 다시 만들기").clicked() {
                            notes.push(match self.db.rebuild_index() { Ok(()) => "검색 색인을 다시 만들었습니다".into(), Err(x) => format!("실패: {x}") });
                        }
                        if crate::icons::btn(ui, crate::icons::Icon::Folder, "위치 바꾸기…").on_hover_text("복사·검증 후 전환합니다. 이전 파일은 그대로 남습니다").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                let dest = dir.join("vault.db");
                                if dest.exists() {
                                    notes.push("그 폴더에 이미 vault.db가 있습니다. 다른 폴더를 고르세요".into());
                                } else {
                                    let r = self.db.snapshot_to(&dest).and_then(|_| Db::open(&dest));
                                    match r {
                                        Ok(nd) if nd.integrity_ok() => {
                                            self.db = nd;
                                            self.db.set_cache_mb(self.cfg.cache_mb);
                                            self.cfg.db_path = dest.to_string_lossy().into_owned();
                                            self.list_dirty = true;
                                            notes.push(format!("저장소를 옮겼습니다: {}", dest.display()));
                                        }
                                        Ok(_) => notes.push("복사본 검사에 실패해 전환하지 않았습니다".into()),
                                        Err(x) => notes.push(format!("전환 실패: {x}")),
                                    }
                                }
                            }
                        }
                    });
                    // 내보내기 → export_ui.rs
                    self.export_options(ui, &mut notes);
                    // 이미지 → image_ui.rs
                    self.image_options(ui, &mut notes);
                }
                2 => {
                    ui.label(egui::RichText::new("자동 백업").strong());
                    ui.horizontal(|ui| {
                        ui.label("자동 백업 보관 개수");
                        ui.add(egui::DragValue::new(&mut self.cfg.backup_keep).range(1..=60));
                    });
                    ui.horizontal_wrapped(|ui| {
                        if crate::icons::btn(ui, crate::icons::Icon::Save, "지금 백업").clicked() {
                            let p = self.db.path.parent().unwrap_or(std::path::Path::new(".")).join("backup").join(format!("vault-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                            notes.push(match self.db.snapshot_to(&p) { Ok(()) => format!("백업 완료: {}", p.display()), Err(x) => format!("백업 실패: {x}") });
                            self.fs_invalidate(false);
                        }
                        if crate::icons::btn(ui, crate::icons::Icon::Folder, "백업 폴더 열기").clicked() {
                            let d = self.db.path.parent().unwrap_or(std::path::Path::new(".")).join("backup");
                            let _ = std::fs::create_dir_all(&d);
                            let _ = std::process::Command::new("explorer").arg(d).spawn();
                        }
                    });
                    // 동기화 저장소 → sync_ui.rs
                    self.sync_options(ui, &mut notes);
                }
                4 => self.status_tab(ui),
            _ => self.about_tab(ui),
            });
        });
        self.show_options = open;
        for n in notes {
            self.say(&n);
        }
        if changed_theme {
            self.theme = self.resolve_theme();
            theme::apply(ctx, &self.theme);
            self.galley_cache = None;
        }
        if !open {
            self.cfg.save();
        }
    }

    fn prompt_window(&mut self, ctx: &egui::Context) {
        let Some(p) = self.prompt.as_mut() else { return };
        let done = prompt_ui(ctx, p);
        if let Some(ok) = done {
            let p = self.prompt.take().unwrap();
            if ok {
                let res = match p.rename {
                    Some(id) => self.db.rename_category(id, &p.text),
                    None => self.db.add_category(p.parent, &p.text).map(|id| {
                        if let Some(par) = p.parent {
                            self.collapsed.remove(&par);
                        }
                        let _ = id;
                    }),
                };
                if let Err(x) = res {
                    self.say(&x);
                }
                self.list_dirty = true;
            }
        }
    }
}

/// 보기 막대: 전체 · 미분류 · ★ · 휴지통을 한 줄에 가로로 놓는다.
/// 문서를 끌어 놓으면 미분류(분류 해제)·휴지통(삭제)으로 보낼 수 있다.
fn view_bar_ui(ui: &mut egui::Ui, t: &Theme, c: (i64, i64, i64, i64), current: &View, tags_empty: bool, acts: &mut Vec<Act>) {
    // (표시 글자, 설명, 개수, 보기, 끌어 놓기 동작, 폭 비율)
    let cells: [(&str, &str, i64, View, u8, f32); 4] = [
        ("전체", "모든 문서", c.0, View::All, 0, 1.1),
        ("미분류", "분류가 없는 문서 (문서를 끌어 놓으면 분류 해제)", c.1, View::Uncategorized, 1, 1.5),
        ("", "즐겨찾기", c.2, View::Favorites, 0, 0.75),
        ("", "휴지통 (문서를 끌어 놓으면 휴지통으로)", c.3, View::Trash, 2, 0.75),
    ];
    let total: f32 = cells.iter().map(|x| x.5).sum();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let mut x = row.left();
    for (i, (label, tip, count, view, drop, w)) in cells.iter().enumerate() {
        let cw = row.width() * w / total;
        let rect = Rect::from_min_size(Pos2::new(x, row.top()), Vec2::new(cw, row.height()));
        x += cw;
        let resp = ui.interact(rect, ui.scope_id().with(("view_bar", i)), Sense::click());
        let selected = current == view && tags_empty;
        let bg = if selected { t.sel } else if resp.hovered() { t.hover } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
        if i > 0 {
            ui.painter().line_segment([rect.left_top() + Vec2::new(0.0, 4.0), rect.left_bottom() - Vec2::new(0.0, 4.0)], Stroke::new(1.0, t.border));
        }
        match i {
            // 즐겨찾기·휴지통은 글자 대신 테마 색을 따르는 선 아이콘
            2 | 3 => {
                let side = 16.0;
                let r = Rect::from_min_size(Pos2::new(rect.left() + 7.0, rect.center().y - side / 2.0), Vec2::splat(side));
                crate::icons::paint(ui.painter(), r, if i == 2 { crate::icons::Icon::Star } else { crate::icons::Icon::Trash }, t.text);
            }
            _ => {
                ui.painter().text(Pos2::new(rect.left() + 7.0, rect.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(crate::theme::ui(0.0)), t.text);
            }
        }
        // ★·🗑는 개수가 0이면 숫자를 생략해 칸을 깔끔하게 둔다.
        if i < 2 || *count > 0 {
            ui.painter().text(Pos2::new(rect.right() - 6.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(crate::theme::ui(-1.0)), t.dim);
        }
        App::drop_frame(ui, rect, &resp, t.accent);
        if resp.clicked() {
            acts.push(Act::SetView(view.clone()));
        }
        if let Some(p) = resp.dnd_release_payload::<DocDrag>() {
            match drop {
                1 => acts.push(Act::MoveDoc(p.0, None)),
                2 => acts.push(Act::Trash(p.0)),
                _ => {}
            }
        }
        resp.on_hover_text(*tip);
    }
}
/// 분류 이름 입력 창. 확인=Some(true), 취소=Some(false), 계속=None.
/// - 창·입력칸 id를 열 때마다 새로 만들어 이전 입력(특히 한글 조합) 상태가 남지 않게 한다.
/// - 포커스는 처음 한 번만 요청한다(매 프레임 요청하면 한글 조합이 끊길 수 있음).
/// - 한글 조합 중에 누른 Enter는 조합을 끝내는 키이므로 "확인"으로 처리하지 않는다.
fn prompt_ui(ctx: &egui::Context, p: &mut Prompt) -> Option<bool> {
    let mut done: Option<bool> = None;
    let title = if p.rename.is_some() { "분류 이름 바꾸기" } else if p.parent.is_some() { "하위 분류 만들기" } else { "분류 만들기" };
    #[allow(deprecated)]
    let win_id = egui::Id::new(("cat_prompt", p.seq));
    let edit_id = win_id.with("text");

    // 이 프레임 이전부터 조합 중이었는지(이번 프레임의 이벤트로 바뀌기 전 값)
    let composing_before = p.composing;
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Ime(ev) = e {
                match ev {
                    egui::ImeEvent::Preedit { text, .. } => p.composing = !text.is_empty(),
                    egui::ImeEvent::Commit(_) => p.composing = false,
                    _ => {}
                }
            }
        }
    });
    let enter = ctx.input(|i| i.key_pressed(Key::Enter));

    egui::Window::new(title).id(win_id).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.text).id(edit_id).desired_width(260.0).hint_text("분류 이름"));
        if !p.focused {
            r.request_focus();
            p.focused = true;
        }
        ui.horizontal(|ui| {
            let enter_confirm = enter && !composing_before && !p.composing && !p.text.trim().is_empty();
            let empty = p.text.trim().is_empty();
            let ok_clicked = ui.add_enabled_ui(!empty, |ui| crate::icons::btn(ui, crate::icons::Icon::Check, "확인").clicked()).inner;
            if ok_clicked || enter_confirm {
                done = Some(true);
            }
            if crate::icons::btn(ui, crate::icons::Icon::Cross, "취소").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(false);
            }
        });
    });
    done
}

fn fmt_time(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(ts, 0).single().map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 글자 그리기 환경(색 변환·배율·아틀라스)이 바뀌었으면 앱이 따로 보관한 배치 캐시를 비운다.
        if self.text_env.changed(&ctx) {
            self.galley_cache = None;
        }
        // 첫 화면을 그렸다: 렌더러 "시도 중" 표시를 지우고, 시작 때 생긴 알림을 보여 준다.
        if !self.boot_done {
            self.boot_done = true;
            if !self.cfg.renderer_pending.is_empty() {
                self.cfg.renderer_pending.clear();
                self.cfg.save();
            }
            if let Some(n) = crate::take_notice() {
                self.say(&n);
            }
        }
        // 창 크기 저장: 크기가 바뀌고 1초 안정되면 기록한다(최대화 중에는 최대화 여부만).
        {
            let (inner, maxed) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized.unwrap_or(false)));
            if let Some(r) = inner {
                let sz = (r.width().round(), r.height().round());
                if self.win_seen != Some((sz, maxed)) {
                    self.win_seen = Some((sz, maxed));
                    self.win_changed = Some(Instant::now());
                    ctx.request_repaint_after(Duration::from_millis(1100));
                }
            }
            if let (Some(t0), Some((sz, maxed))) = (self.win_changed, self.win_seen) {
                if t0.elapsed() > Duration::from_secs(1) && !ctx.input(|i| i.pointer.any_down()) {
                    self.win_changed = None;
                    if !maxed && sz.0 >= 640.0 && sz.1 >= 420.0 {
                        self.cfg.win_w = sz.0;
                        self.cfg.win_h = sz.1;
                    }
                    self.cfg.win_max = maxed;
                    self.cfg.save();
                }
            }
        }
        // 렌더러 시험 결과 수집
        if !self.probe_rx.is_empty() {
            let mut i = 0;
            while i < self.probe_rx.len() {
                match self.probe_rx[i].1.try_recv() {
                    Ok(p) => {
                        let (k, _) = self.probe_rx.remove(i);
                        self.probes.insert(k, p);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                    Err(_) => {
                        let (k, _) = self.probe_rx.remove(i);
                        self.probes.insert(k, crate::probe::Probe::Fail("시험 중단".into()));
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        let mut acts: Vec<Act> = Vec::new();

        // 전역 단축키
        let (new, find, save, opts, side, pin, back, fwd) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && i.key_pressed(Key::N),
                c && i.key_pressed(Key::E),
                c && !i.modifiers.shift && i.key_pressed(Key::S),
                c && i.key_pressed(Key::Comma),
                c && i.key_pressed(Key::Backslash),
                c && i.modifiers.shift && i.key_pressed(Key::D),
                i.modifiers.alt && i.key_pressed(Key::ArrowLeft),
                i.modifiers.alt && i.key_pressed(Key::ArrowRight),
            )
        });
        if ctx.input(|i| i.key_pressed(Key::F1)) {
            self.show_help = true; // 도움말 새 창
        }
        if new { acts.push(Act::NewDoc); }
        if ctx.input(|i| i.modifiers.command && i.modifiers.alt && i.key_pressed(Key::T)) { acts.push(Act::Today); }
        if find { self.cfg.sidebar = true; self.focus_search = true; }
        if save && self.flush() { self.say("저장했습니다"); }
        if opts { self.show_options = true; }
        if side { self.cfg.sidebar = !self.cfg.sidebar; }
        if pin { if let Some(id) = self.cur { acts.push(Act::Pin(id)); } }
        if back { acts.push(Act::Back); }
        if fwd { acts.push(Act::Forward); }

        // 파일 끌어 놓기 가져오기
        self.poll_image(&ctx);
        self.poll_pdf(&ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            // 그림 파일은 이미지로 넣고, 나머지(.md/.txt)는 문서로 가져온다.
            let (pics, docs): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(|p| image_ui::is_image_path(p));
            for p in pics {
                self.start_image_job(&ctx, image_ui::ImageSrc::File(p));
            }
            if !docs.is_empty() {
                self.import_paths(docs);
            }
        }

        // 자동 저장
        // 첫 변경 후 N분(1~20)이 지나면 한 번에 기록한다. 그 사이의 입력은 메모리에만 쌓인다.
        if let Some(t) = self.last_edit {
            let wait = Duration::from_secs(self.cfg.autosave_min.clamp(1, 20) as u64 * 60);
            if t.elapsed() >= wait {
                self.flush();
                if self.cfg.links_panel {
                    self.compute_links();
                    // 이 문서의 제목·링크가 바뀐 경우에만 그래프를 다시 계산한다.
                    if self.graph_signature() != self.graph_sig {
                        self.graph_stale = true;
                    }
                }
            } else {
                // 상태바 막대가 줄어들도록 1초마다 갱신(분 단위라 자주 그릴 필요 없음)
                ctx.request_repaint_after((wait - t.elapsed()).min(Duration::from_secs(1)));
            }
        }
        self.merge_tick(&ctx);
        // 닫을 때 저장
        if ctx.input(|i| i.viewport().close_requested()) {
            self.flush();
            if let Some(c) = self.cur {
                let _ = self.db.delete_empty(c);
            }
            self.merge_now();
            self.sync_on_close();
            self.cfg.save();
            self.update_on_close();
        }
        if self.list_dirty {
            self.ref_rev += 1;
            self.refresh();
        }
        self.sync_tick();
        self.update_tick();
        crate::diag::tick();
        // 그래프 설정은 마우스를 놓은 뒤(슬라이더 조작이 끝난 뒤) 한 번만 저장한다.
        if self.graph_cfg_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.cfg.save();
            self.graph_cfg_dirty = false;
        }
        // 연결 그래프: 사이드바가 열려 있을 때만 유지한다(레이아웃은 저장하지 않음).
        if !((self.cfg.links_panel && self.cfg.right_mode != 1) || self.graph_window) {
            self.graph = None;
        } else if self.graph.is_none() || self.graph_stale {
            self.rebuild_graph();
        } else if self.graph_cur != self.cur {
            self.graph_cur = self.cur;
            let t = self.title.clone();
            if let Some(g) = self.graph.as_mut() {
                g.focus(self.cur, &t);
            }
            self.graph_sig = self.graph_signature();
        }
        if self.status_at.elapsed() > Duration::from_secs(8) && self.status != "준비됨" {
            self.status = "준비됨".into();
        }

        // 창 제목
        let shown = if self.title.trim().is_empty() { "새 문서" } else { self.title.trim() };
        let title = format!("{}아카식 — {shown}", if self.last_edit.is_some() { "● " } else { "" });
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        self.theme_tick(&ctx); // Ctrl+Alt+D 빠른 전환·사용자 지정 테마 편집 창
        if self.last_theme != self.theme.name {
            theme::apply(&ctx, &self.theme);
            self.galley_cache = None;
            self.last_theme = self.theme.name.to_string();
        }

        // 하단 상태바 (status_ui.rs)
        self.status_bar(ui);

        if self.cfg.sidebar {
            egui::Panel::left("side").default_size(290.0).size_range(220.0..=480.0).resizable(true).show(ui, |ui| {
                self.side_panel(ui, &mut acts);
            });
        }
        if self.cfg.links_panel {
            egui::Panel::right("links").default_size(320.0).size_range(240.0..=640.0).resizable(true).show(ui, |ui| {
                self.links_panel(ui, &mut acts);
            });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            self.central(ui, &mut acts);
        });

        if self.show_options {
            self.options_window(&ctx);
        }
        if self.show_help {
            self.help_window(&ctx);
        }
        if self.extras_open {
            self.extras_window(&ctx);
        }
        if self.col.open {
            self.col_window(&ctx);
        }
        if self.graph_window {
            let mut a2 = Vec::new();
            self.graph_window(&ctx, &mut a2);
            acts.extend(a2);
        }
        self.ref_windows(&ctx);
        self.prompt_window(&ctx);
        let nrefs = self.refs.wins.len();
        self.apply(acts);
        if self.refs.wins.len() != nrefs || self.ref_focus.is_some() {
            ctx.request_repaint(); // 새 참고 창을 바로 만들도록 다음 프레임을 요청
        }
    }
}

#[cfg(test)]
mod prompt_tests {
    use super::*;
    use egui::{Event, ImeEvent, Modifiers, Pos2, RawInput, Rect};

    fn frame(ctx: &egui::Context, t: &mut f64, events: Vec<Event>, p: &mut Prompt) -> Option<bool> {
        *t += 0.05;
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)));
        raw.time = Some(*t);
        raw.focused = true;
        raw.events = events;
        let mut out = None;
        let mut full = ctx.run_pass(raw, |ctx| {
            if out.is_none() {
                out = prompt_ui(ctx, p);
            }
        });
        full.textures_delta.clear(); // 그리지 않으므로 버린다
        out
    }
    fn pre(s: &str) -> Event {
        Event::Ime(ImeEvent::Preedit { text: s.into(), active_range_chars: None })
    }
    fn commit(s: &str) -> Event {
        Event::Ime(ImeEvent::Commit(s.into()))
    }
    fn enter() -> Event {
        Event::Key { key: egui::Key::Enter, physical_key: Some(egui::Key::Enter), pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// 한글 "하위"를 조합해 입력하고 Enter로 확정하는 과정을 같은 컨텍스트에서 연속 3번 반복한다.
    #[test]
    fn korean_ime_typing_in_repeated_prompts() {
        let ctx = egui::Context::default();
        let mut t = 0.0;
        for round in 1..=3u32 {
            let mut p = Prompt::new(None, Some(1), String::new(), round);
            for _ in 0..3 {
                assert!(frame(&ctx, &mut t, vec![], &mut p).is_none());
            }
            // ㅎ → 하 → (확정 하) ㅇ → 위 → 확정
            for ev in [vec![pre("ㅎ")], vec![pre("하")], vec![commit("하"), pre("ㅇ")], vec![pre("위")], vec![commit("위")]] {
                assert!(frame(&ctx, &mut t, ev, &mut p).is_none(), "round {round}: 조합 중에는 확인되면 안 됨");
            }
            assert_eq!(p.text, "하위", "round {round}: 입력 글자");
            assert_eq!(frame(&ctx, &mut t, vec![enter()], &mut p), Some(true), "round {round}: Enter로 확인");
            assert_eq!(p.text, "하위");
        }
    }

    /// 마지막 글자를 아직 조합 중일 때 Enter가 오면 확인하지 않고(조합 종료), 그다음 Enter에서 확인한다.
    #[test]
    fn enter_during_composition_does_not_confirm_early() {
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut p = Prompt::new(None, None, String::new(), 1);
        for _ in 0..3 {
            frame(&ctx, &mut t, vec![], &mut p);
        }
        frame(&ctx, &mut t, vec![pre("분")], &mut p);
        assert_eq!(frame(&ctx, &mut t, vec![enter()], &mut p), None, "조합 중 Enter는 확인이 아님");
        frame(&ctx, &mut t, vec![commit("분")], &mut p);
        assert!(p.text.contains('분'));
        assert_eq!(frame(&ctx, &mut t, vec![enter()], &mut p), Some(true));
    }
}

#[cfg(test)]
mod view_bar_tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};

    fn frame(ctx: &egui::Context, t: &mut f64, events: Vec<Event>, cur: &View, acts: &mut Vec<Act>) {
        *t += 0.05;
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 200.0)));
        raw.time = Some(*t);
        raw.focused = true;
        raw.events = events;
        let theme = theme::themes()[0].clone();
        let mut full = ctx.run_ui(raw, |ui| view_bar_ui(ui, &theme, (3, 0, 0, 2), cur, true, acts));
        full.textures_delta.clear();
    }
    fn click(ctx: &egui::Context, t: &mut f64, pos: Pos2, cur: &View) -> Vec<Act> {
        let mut acts = vec![];
        frame(ctx, t, vec![Event::PointerMoved(pos)], cur, &mut acts);
        let b = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(ctx, t, vec![b(true)], cur, &mut acts);
        frame(ctx, t, vec![b(false)], cur, &mut acts);
        acts
    }

    /// 네 칸을 각각 눌렀을 때 알맞은 보기로 바뀌어야 한다(폭 300 기준 칸 경계: 0·80·~190·~245·300).
    #[test]
    fn each_cell_selects_its_view() {
        let ctx = egui::Context::default();
        let mut t = 0.0;
        for _ in 0..2 {
            frame(&ctx, &mut t, vec![], &View::All, &mut vec![]);
        }
        let want = [(30.0, "전체"), (130.0, "미분류"), (215.0, "즐겨찾기"), (275.0, "휴지통")];
        for (x, name) in want {
            let acts = click(&ctx, &mut t, Pos2::new(x, 13.0), &View::All);
            let got = acts.iter().find_map(|a| if let Act::SetView(v) = a { Some(v.clone()) } else { None });
            let expect = match name {
                "전체" => View::All,
                "미분류" => View::Uncategorized,
                "즐겨찾기" => View::Favorites,
                _ => View::Trash,
            };
            assert_eq!(got, Some(expect), "{name} 칸을 눌렀을 때");
        }
    }
}
