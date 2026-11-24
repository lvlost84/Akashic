//! 테마와 "각진 디자인" 규칙.
//! 둥근 모서리·그림자·번짐(feathering)을 모두 끄고, 1px 선과 직각 사각형만 쓴다.
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Vec2};
use eframe::egui::epaint::Shadow;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,
    pub bg: Color32,      // 편집창·입력칸 바탕
    pub panel: Color32,   // 패널 바탕
    pub text: Color32,
    pub dim: Color32,     // 문법 기호·보조 글자
    pub accent: Color32,
    pub code_bg: Color32,
    pub code_fg: Color32,
    pub quote: Color32,
    pub link: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub sel: Color32,
}

fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn themes() -> Vec<Theme> {
    vec![
        Theme {
            name: "라이트", dark: false, bg: c(0xffffff), panel: c(0xf3f4f6), text: c(0x1f2328), dim: c(0x9aa1ab),
            accent: c(0x1a6fd6), code_bg: c(0xeceff3), code_fg: c(0xb3261e), quote: c(0x5b6573), link: c(0x1a6fd6),
            border: c(0xc9ced6), hover: c(0xe3e7ed), sel: c(0xbcd7f7),
        },
        Theme {
            name: "다크", dark: true, bg: c(0x1b1d21), panel: c(0x25282d), text: c(0xe3e6ea), dim: c(0x6b7380),
            accent: c(0x5aa2f0), code_bg: c(0x2d3137), code_fg: c(0xf08d80), quote: c(0x9aa3b0), link: c(0x6fb1ff),
            border: c(0x3a3f47), hover: c(0x30343b), sel: c(0x274b78),
        },
        Theme {
            name: "레이저", dark: true, bg: c(0x050505), panel: c(0x0e0e0e), text: c(0xf2f2f2), dim: c(0x5c5c5c),
            accent: c(0x44d62c), code_bg: c(0x151515), code_fg: c(0x9be88c), quote: c(0x9a9a9a), link: c(0x44d62c),
            border: c(0x2a2a2a), hover: c(0x1a1a1a), sel: c(0x1d4a14),
        },
    ]
}

pub fn by_name(name: &str) -> Theme {
    let all = themes();
    all.iter().find(|t| t.name == name).cloned().unwrap_or_else(|| all[0].clone())
}

/// 글자 그리기 환경의 지문. 바뀌면 egui가 글리프 아틀라스를 다시 만들어 이전 글자 배치(galley)가 엉뚱한 글리프를 가리키므로,
/// 앱이 따로 보관한 글자 배치 캐시도 함께 비워야 한다(시작 시 밝은 시스템 모드에서 어두운 테마로 켤 때 글자가 깨지던 원인).
#[derive(Default)]
pub struct TextEnv {
    opts: Option<egui::epaint::text::TextOptions>,
    ppp: f32,
    fill: f32,
    init: bool,
}

impl TextEnv {
    /// 이번 프레임에서 글자 환경이 바뀌었으면 true(첫 호출도 true).
    pub fn changed(&mut self, ctx: &egui::Context) -> bool {
        let opts = ctx.global_style().visuals.text_options;
        let ppp = ctx.pixels_per_point();
        let fill = ctx.fonts(|f| f.font_atlas_fill_ratio());
        let changed = !self.init || self.opts != Some(opts) || self.ppp != ppp || fill < self.fill - 1e-4;
        self.init = true;
        self.opts = Some(opts);
        self.ppp = ppp;
        self.fill = fill;
        changed
    }
}
/// 화면 글자 기본 크기(pt). 옵션에서 10~16으로 바꾼다.
pub const UI_SIZE_DEFAULT: f32 = 12.0;
static UI_SIZE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4140_0000); // 12.0f32

pub fn set_ui_size(v: f32) {
    UI_SIZE.store(v.clamp(10.0, 16.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 화면 글자 크기 + `delta`(작은 글자는 음수).
pub fn ui(delta: f32) -> f32 {
    (f32::from_bits(UI_SIZE.load(std::sync::atomic::Ordering::Relaxed)) + delta).max(9.0)
}

/// 모든 위젯을 직각으로 만들고 번지는 효과를 끈다.
pub fn apply(ctx: &egui::Context, t: &Theme) {
    // 시스템(Windows) 밝음/어두움 모드를 따라가지 않고 앱 테마의 밝기로 고정한다. 둘이 다르면 시작 직후
    // egui가 다른 쪽 스타일로 바꾸며 글자 아틀라스를 다시 만든다. (창 제목 표시줄도 같이 맞춰진다.)
    ctx.set_theme(if t.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.bg;
    v.faint_bg_color = t.hover;
    v.code_bg_color = t.code_bg;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.link;
    v.selection.bg_fill = t.sel;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_stroke = Stroke::new(1.0, t.border);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::ZERO;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    // 슬라이더 막대·체크박스 바탕이 패널 색과 같으면 보이지 않으므로 테두리 색으로 구분한다.
    v.widgets.inactive.bg_fill = t.border;
    v.widgets.inactive.weak_bg_fill = t.hover;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.active.bg_fill = t.sel;
    v.widgets.active.weak_bg_fill = t.sel;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.open.bg_fill = t.hover;
    v.widgets.open.weak_bg_fill = t.hover;
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        // 화면 글자(메뉴·단추·상태바·목록 등)는 기본 12pt 이하. 문서 본문 크기와 따로 정한다.
        use egui::TextStyle::{Body, Button, Heading, Monospace, Small};
        let fam = FontFamily::Proportional;
        s.text_styles.insert(Body, egui::FontId::new(ui(0.0), fam.clone()));
        s.text_styles.insert(Button, egui::FontId::new(ui(0.0), fam.clone()));
        s.text_styles.insert(Heading, egui::FontId::new(ui(0.0), FontFamily::Name("bold".into())));
        s.text_styles.insert(Small, egui::FontId::new(ui(-1.0), fam));
        s.text_styles.insert(Monospace, egui::FontId::new(ui(0.0), FontFamily::Monospace));
        s.spacing.item_spacing = Vec2::new(6.0, 4.0);
        s.spacing.button_padding = Vec2::new(8.0, 3.0);
    });
    // 가장자리 번짐(안티앨리어싱 feathering)을 끄면 선이 또렷한 직각으로 그려진다.
    ctx.tessellation_options_mut(|o| o.feathering = false);
}

// ---------------------------------------------------------------- 사용자 지정 테마
pub const CUSTOM_NAME: &str = "사용자 지정";
pub const COLOR_COUNT: usize = 12;
/// 편집 창에 보이는 색 이름(Theme의 12색 순서와 같다).
pub const COLOR_LABELS: [&str; COLOR_COUNT] = [
    "배경", "패널", "글자", "보조 글자", "강조색", "코드 배경", "코드 글자", "인용", "링크", "테두리", "호버", "선택",
];

pub fn to_u32(c: Color32) -> u32 {
    ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32
}

/// "#RRGGBB"(또는 "RRGGBB") → 0xRRGGBB. 잘못된 입력은 None.
pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

pub fn to_hex(v: u32) -> String {
    format!("#{:06X}", v & 0xFFFFFF)
}

/// 배경색 휘도로 어두운 테마인지 판정한다.
pub fn is_dark_color(v: u32) -> bool {
    let (r, g, b) = (((v >> 16) & 0xff) as f32, ((v >> 8) & 0xff) as f32, (v & 0xff) as f32);
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0 < 0.5
}

impl Theme {
    pub fn colors(&self) -> [u32; COLOR_COUNT] {
        [self.bg, self.panel, self.text, self.dim, self.accent, self.code_bg, self.code_fg, self.quote, self.link, self.border, self.hover, self.sel]
            .map(to_u32)
    }

    /// 12색(0xRRGGBB)에서 "사용자 지정" 테마를 만든다. 길이가 12가 아니면 None.
    pub fn from_colors(v: &[u32]) -> Option<Theme> {
        if v.len() != COLOR_COUNT {
            return None;
        }
        Some(Theme {
            name: CUSTOM_NAME,
            dark: is_dark_color(v[0]),
            bg: c(v[0]), panel: c(v[1]), text: c(v[2]), dim: c(v[3]), accent: c(v[4]), code_bg: c(v[5]),
            code_fg: c(v[6]), quote: c(v[7]), link: c(v[8]), border: c(v[9]), hover: c(v[10]), sel: c(v[11]),
        })
    }
}

/// 설정의 테마 이름과 사용자 지정 색으로 테마를 정한다. 사용자 지정인데 색이 없거나 길이가 틀리면 라이트.
pub fn resolve(name: &str, custom: &[u32]) -> Theme {
    if name == CUSTOM_NAME {
        if let Some(t) = Theme::from_colors(custom) {
            return t;
        }
        return by_name("라이트");
    }
    by_name(name)
}

/// 기본 테마 이름(라이트/다크/레이저)에서 12색을 가져온다.
pub fn base_colors(name: &str) -> [u32; COLOR_COUNT] {
    by_name(name).colors()
}

// ---------------------------------------------------------------- 글꼴
const DEFAULT_LATIN_FILE: &str = "segoeui.ttf";
const DEFAULT_LATIN_BOLD: &str = "segoeuib.ttf";
const DEFAULT_KOREAN_FILE: &str = "malgun.ttf";
const DEFAULT_KOREAN_BOLD: &str = "malgunbd.ttf";
const MONO_FILE: &str = "consola.ttf";

#[derive(Clone, Debug, PartialEq)]
pub struct FontLoc {
    pub path: PathBuf,
    pub index: u32,
}

/// 글꼴 패밀리 하나(파일 위치와 굵은 변형 포함).
#[derive(Clone, Debug)]
pub struct FamilyInfo {
    pub name: String,
    pub latin: bool,  // 'A' 글리프가 있음
    pub hangul: bool, // '가' 글리프가 있음
    pub regular: FontLoc,
    pub bold: Option<FontLoc>,
}

/// 파일 하나에서 읽은 face 정보.
#[derive(Clone, Debug)]
pub struct FaceInfo {
    pub family: String,
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
    pub latin: bool,
    pub hangul: bool,
}

fn face_family(face: &ttf_parser::Face) -> Option<String> {
    names_family(face.names())
}

fn names_family(names: ttf_parser::name::Names) -> Option<String> {
    use ttf_parser::{PlatformId, name_id};
    for id in [name_id::TYPOGRAPHIC_FAMILY, name_id::FAMILY] {
        let mut any: Option<String> = None;
        for n in names {
            if n.name_id != id || n.platform_id == PlatformId::Macintosh {
                continue;
            }
            if let Some(s) = n.to_string() {
                let s = s.trim().to_string();
                if s.is_empty() {
                    continue;
                }
                if n.language_id == 0x409 {
                    return Some(s);
                }
                any.get_or_insert(s);
            }
        }
        if any.is_some() {
            return any;
        }
    }
    None
}

/// 글꼴 파일 바이트(ttc면 여러 face)에서 그릴 수 있는 face만 골라 읽는다. 깨진 바이트는 빈 목록.
pub fn read_faces(data: &[u8]) -> Vec<FaceInfo> {
    let n = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut out = vec![];
    for index in 0..n.min(64) {
        let Ok(face) = ttf_parser::Face::parse(data, index) else { continue };
        if face.number_of_glyphs() == 0 {
            continue;
        }
        let latin = face.glyph_index('A').is_some();
        let hangul = face.glyph_index('가').is_some();
        if !latin && !hangul {
            continue; // 기호 글꼴 등은 글자를 그릴 수 없다
        }
        let Some(family) = face_family(&face) else { continue };
        out.push(FaceInfo { family, index, weight: face.weight().to_number(), italic: face.is_italic() || face.is_oblique(), latin, hangul });
    }
    out
}

pub fn font_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(r"C:\Windows\Fonts")];
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(l).join(r"Microsoft\Windows\Fonts"));
    }
    v
}

/// 폴더들의 글꼴 파일을 훑어 패밀리 목록(이름순)을 만든다. `only`가 있으면 그 이름의 패밀리만.
pub fn scan_families(dirs: &[PathBuf], only: Option<&[&str]>) -> Vec<FamilyInfo> {
    // 소문자 이름 → (표시 이름, face 목록)
    let mut map: FaceMap = FaceMap::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()).unwrap_or_default();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue }; // 없는/읽을 수 없는 파일은 제외
            for f in read_faces(&data) {
                if let Some(names) = only {
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&f.family)) {
                        continue;
                    }
                }
                let ent = map.entry(f.family.to_lowercase()).or_insert_with(|| (f.family.clone(), vec![]));
                ent.1.push((f, p.clone()));
            }
        }
    }
    group_families(map)
}

type FaceMap = std::collections::BTreeMap<String, (String, Vec<(FaceInfo, PathBuf)>)>;

/// 글꼴 파일 헤더만 읽어(name·OS/2 표를 찾아 그 부분만 읽음) 패밀리 이름·굵기를 얻는다. 파일 전체를 읽지 않는다.
/// ttc(여러 글꼴 묶음)는 face 번호별로 돌려준다. 읽을 수 없는 파일은 빈 목록.
pub fn peek_faces(path: &std::path::Path) -> Vec<FaceInfo> {
    use std::io::{Read, Seek, SeekFrom};
    let mut out = vec![];
    let Ok(mut f) = std::fs::File::open(path) else { return out };
    crate::diag::count("font_peek");
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let be16 = |b: &[u8]| u16::from_be_bytes([b[0], b[1]]);
    let mut hdr = [0u8; 12];
    if f.read_exact(&mut hdr).is_err() {
        return out;
    }
    let offsets: Vec<u64> = if &hdr[0..4] == b"ttcf" {
        let n = be32(&hdr[8..12]).min(64) as usize;
        let mut buf = vec![0u8; n * 4];
        if f.read_exact(&mut buf).is_err() {
            return out;
        }
        (0..n).map(|i| be32(&buf[i * 4..]) as u64).collect()
    } else {
        vec![0]
    };
    for (index, off) in offsets.into_iter().enumerate() {
        let mut h = [0u8; 12];
        if f.seek(SeekFrom::Start(off)).is_err() || f.read_exact(&mut h).is_err() {
            continue;
        }
        if !(h[0..4] == [0, 1, 0, 0] || &h[0..4] == b"OTTO" || &h[0..4] == b"true") {
            continue;
        }
        let nt = be16(&h[4..6]) as usize;
        if nt == 0 || nt > 128 {
            continue;
        }
        let mut dir = vec![0u8; nt * 16];
        if f.read_exact(&mut dir).is_err() {
            continue;
        }
        let find = |t: &[u8; 4]| (0..nt).find(|i| &dir[i * 16..i * 16 + 4] == t).map(|i| (be32(&dir[i * 16 + 8..]) as u64, be32(&dir[i * 16 + 12..]) as usize));
        let Some((no, nl)) = find(b"name") else { continue };
        if nl == 0 || nl > 1 << 20 {
            continue;
        }
        let mut nb = vec![0u8; nl];
        if f.seek(SeekFrom::Start(no)).is_err() || f.read_exact(&mut nb).is_err() {
            continue;
        }
        let Some(fam) = ttf_parser::name::Table::parse(&nb).and_then(|t| names_family(t.names)) else { continue };
        let (mut weight, mut italic) = (400u16, false);
        if let Some((oo, ol)) = find(b"OS/2") {
            let mut ob = [0u8; 64];
            if ol >= 64 && f.seek(SeekFrom::Start(oo)).is_ok() && f.read_exact(&mut ob).is_ok() {
                weight = be16(&ob[4..6]);
                italic = be16(&ob[62..64]) & 1 != 0;
            }
        }
        // 글리프 유무는 헤더만으로 알 수 없어, 이미 고른 글꼴을 다시 찾는 용도로만 쓴다(둘 다 true).
        out.push(FaceInfo { family: fam, index: index as u32, weight, italic, latin: true, hangul: true });
    }
    out
}

/// 폴더들의 글꼴 파일 헤더만 읽어 이름이 `wanted`인 패밀리를 찾는다(전체 목록 스캔보다 훨씬 가볍다).
pub fn scan_names(dirs: &[PathBuf], wanted: &[&str]) -> Vec<FamilyInfo> {
    let mut map: FaceMap = FaceMap::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()).unwrap_or_default();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                continue;
            }
            for fi in peek_faces(&p) {
                if !wanted.iter().any(|n| n.eq_ignore_ascii_case(&fi.family)) {
                    continue;
                }
                let ent = map.entry(fi.family.to_lowercase()).or_insert_with(|| (fi.family.clone(), vec![]));
                ent.1.push((fi, p.clone()));
            }
        }
    }
    group_families(map)
}

/// 같은 이름의 face들을 패밀리 하나로 묶고 일반체·굵은체를 고른다.
fn group_families(map: FaceMap) -> Vec<FamilyInfo> {
    let mut out = vec![];
    for (_, (name, faces)) in map {
        let pick = |bold: bool| {
            faces
                .iter()
                .filter(|(f, _)| !f.italic && ((f.weight >= 600) == bold))
                .min_by_key(|(f, _)| (f.weight as i32 - if bold { 700 } else { 400 }).abs())
        };
        let Some(reg) = pick(false).or_else(|| faces.iter().find(|(f, _)| !f.italic)).or_else(|| faces.first()) else { continue };
        let bold = pick(true).filter(|b| b.1 != reg.1 || b.0.index != reg.0.index);
        out.push(FamilyInfo {
            name,
            latin: faces.iter().any(|(f, _)| f.latin),
            hangul: faces.iter().any(|(f, _)| f.hangul),
            regular: FontLoc { path: reg.1.clone(), index: reg.0.index },
            bold: bold.map(|b| FontLoc { path: b.1.clone(), index: b.0.index }),
        });
    }
    out
}

#[derive(Clone)]
pub struct FontFace {
    pub data: Vec<u8>,
    pub index: u32,
}

/// 한 글꼴 선택(일반체 + 굵은체).
#[derive(Clone, Default)]
pub struct FontSel {
    pub regular: Option<FontFace>,
    pub bold: Option<FontFace>,
}

fn data_of(f: &FontFace) -> Arc<FontData> {
    let mut d = FontData::from_owned(f.data.clone());
    d.index = f.index;
    Arc::new(d)
}

/// 영문·한글 선택으로 FontDefinitions를 만든다(UI·디스크와 무관한 순수 함수).
/// Proportional = [영문, 한글, egui 기본…], bold = 같은 방식, Monospace = [Consolas, 맑은 고딕(code_ko 없으면 한글 선택), 기본…].
pub fn build_fonts(latin: &FontSel, korean: &FontSel, consolas: Option<&FontFace>, code_ko: Option<&FontFace>) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, key: &str, f: &Option<FontFace>| -> Option<String> {
        f.as_ref().map(|f| {
            fonts.font_data.insert(key.into(), data_of(f));
            key.to_string()
        })
    };
    let lr = add(&mut fonts, "latin", &latin.regular);
    let lb = add(&mut fonts, "latin-b", &latin.bold);
    let kr = add(&mut fonts, "korean", &korean.regular);
    let kb = add(&mut fonts, "korean-b", &korean.bold);
    let cons = add(&mut fonts, "consolas", &consolas.cloned());
    let mal = add(&mut fonts, "malgun-code", &code_ko.cloned()).or_else(|| kr.clone());

    // 선택한 글꼴에 없는 기호(★ 등)는 맑은 고딕이 받는다.
    let prop: Vec<String> = [lr.clone(), kr.clone(), mal.clone()].into_iter().flatten().collect();
    let bold: Vec<String> = [lb.or(lr), kb.or(kr.clone()), mal.clone()].into_iter().flatten().collect();
    let mono: Vec<String> = [cons, mal].into_iter().flatten().collect();

    let old_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (fam, mut list) in [
        (FontFamily::Proportional, prop),
        (FontFamily::Monospace, mono),
        (FontFamily::Name("bold".into()), bold),
    ] {
        let base = if fam == FontFamily::Monospace {
            fonts.families.get(&fam).cloned().unwrap_or_default()
        } else {
            old_prop.clone()
        };
        for b in base {
            if !list.contains(&b) {
                list.push(b);
            }
        }
        fonts.families.insert(fam, list);
    }
    fonts
}

fn load_loc(l: &FontLoc) -> Option<FontFace> {
    let data = std::fs::read(&l.path).ok()?;
    crate::diag::add("font_files_read", 1);
    crate::diag::add("font_bytes_read", data.len() as u64);
    Some(FontFace { data, index: l.index })
}

fn load_default(dir: &str, file: &str) -> Option<FontFace> {
    load_loc(&FontLoc { path: PathBuf::from(format!("{dir}{file}")), index: 0 })
}

fn sel_from_family(f: &FamilyInfo) -> FontSel {
    FontSel { regular: load_loc(&f.regular), bold: f.bold.as_ref().and_then(load_loc) }
}

/// 설정에 저장하는 글꼴 파일 위치(ttc는 face 번호 포함). 비어 있으면 아직 모름.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FontPaths {
    pub path: String,
    pub index: u32,
    pub bold_path: String,
    pub bold_index: u32,
}

impl FontPaths {
    pub fn from_family(f: &FamilyInfo) -> Self {
        FontPaths {
            path: f.regular.path.to_string_lossy().into_owned(),
            index: f.regular.index,
            bold_path: f.bold.as_ref().map(|b| b.path.to_string_lossy().into_owned()).unwrap_or_default(),
            bold_index: f.bold.as_ref().map_or(0, |b| b.index),
        }
    }
}

/// 저장된 위치가 아직 그 이름의 글꼴인지 파일 헤더만 읽어 확인하고, 맞으면 패밀리 정보로 만든다.
fn family_from_paths(name: &str, p: &FontPaths) -> Option<FamilyInfo> {
    if p.path.is_empty() {
        return None;
    }
    let path = PathBuf::from(&p.path);
    let ok = peek_faces(&path).iter().any(|f| f.index == p.index && f.family.eq_ignore_ascii_case(name));
    if !ok {
        return None;
    }
    let bold = (!p.bold_path.is_empty() && std::path::Path::new(&p.bold_path).is_file()).then(|| FontLoc { path: PathBuf::from(&p.bold_path), index: p.bold_index });
    Some(FamilyInfo { name: name.to_string(), latin: true, hangul: true, regular: FontLoc { path, index: p.index }, bold })
}

/// 시작 때 글꼴 적용 결과: 새로 알게 된 위치(설정에 저장)와 알릴 말.
#[derive(Default)]
pub struct FontReport {
    pub latin: FontPaths,
    pub korean: FontPaths,
    pub notes: Vec<String>,
}

/// 설정의 글꼴 이름(빈 문자열 = 기본값)과 저장된 파일 위치로 글꼴을 등록한다.
/// 시스템 글꼴 전체를 읽지 않는다: 저장된 위치의 파일만 읽고, 위치가 틀렸을 때만 파일 헤더로 이름을 찾는다.
/// 끝내 못 찾으면 기본 글꼴로 대체하고 알린다.
pub fn install_fonts(ctx: &egui::Context, latin: (&str, &FontPaths), korean: (&str, &FontPaths)) -> FontReport {
    let mut rep = FontReport::default();
    let mut dirs_scanned: Option<Vec<FamilyInfo>> = None;
    let mut resolve = |name: &str, paths: &FontPaths, out: &mut FontPaths, notes: &mut Vec<String>| -> Option<FamilyInfo> {
        if name.is_empty() {
            return None;
        }
        if let Some(f) = family_from_paths(name, paths) {
            *out = paths.clone();
            return Some(f);
        }
        // 위치를 모르거나(옛 설정) 파일이 바뀌었다: 헤더만 읽어 한 번 찾는다(둘째 호출은 결과 재사용).
        let found = dirs_scanned.get_or_insert_with(|| scan_names(&font_dirs(), &[latin.0, korean.0].iter().copied().filter(|s| !s.is_empty()).collect::<Vec<_>>()));
        match found.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            Some(f) => {
                *out = FontPaths::from_family(f);
                Some(f.clone())
            }
            None => {
                notes.push(format!("글꼴 '{name}'을 찾을 수 없어 기본 글꼴을 사용합니다"));
                None
            }
        }
    };
    let l = resolve(latin.0, latin.1, &mut rep.latin, &mut rep.notes);
    let k = resolve(korean.0, korean.1, &mut rep.korean, &mut rep.notes);
    install_with(ctx, l.as_ref(), k.as_ref());
    rep
}

/// 이미 찾은 패밀리 정보로 글꼴을 등록한다(None = 기본값).
pub fn install_with(ctx: &egui::Context, latin: Option<&FamilyInfo>, korean: Option<&FamilyInfo>) {
    let dir = r"C:\Windows\Fonts\";
    let latin = latin.map(sel_from_family).filter(|s| s.regular.is_some()).unwrap_or_else(|| FontSel {
        regular: load_default(dir, DEFAULT_LATIN_FILE),
        bold: load_default(dir, DEFAULT_LATIN_BOLD),
    });
    let ko_custom = korean.map(sel_from_family).filter(|s| s.regular.is_some());
    let korean_default = ko_custom.is_none();
    let korean = ko_custom.unwrap_or_else(|| FontSel {
        regular: load_default(dir, DEFAULT_KOREAN_FILE),
        bold: load_default(dir, DEFAULT_KOREAN_BOLD),
    });
    // 코드용 맑은 고딕: 기본 한글이면 같은 글꼴을 재사용, 아니면 따로 읽는다.
    let code_ko = if korean_default { None } else { load_default(dir, DEFAULT_KOREAN_FILE) };
    let cons = load_default(dir, MONO_FILE);
    let mut fonts = build_fonts(&latin, &korean, cons.as_ref(), code_ko.as_ref());
    add_emoji_font(&mut fonts, dir);
    ctx.set_fonts(fonts);
}

/// 이모지 글꼴(Segoe UI Emoji)을 내 글꼴들 바로 뒤, egui 기본 글꼴 앞에 끼워 넣는다.
/// 시스템 대체 글꼴로 그려지면 본문보다 작고 위로 떠 보이므로, 크기·세로 위치를 보정한다.
/// (보정은 모양만 바꾸고 줄 배치에는 영향이 없다.)
pub const EMOJI_SCALE: f32 = 1.25;
pub const EMOJI_Y_OFFSET: f32 = 0.09;

pub fn add_emoji_font(fonts: &mut FontDefinitions, dir: &str) {
    let Ok(bytes) = std::fs::read(format!("{dir}seguiemj.ttf")) else { return };
    crate::diag::add("font_files_read", 1);
    crate::diag::add("font_bytes_read", bytes.len() as u64);
    // 개발용 튜닝: AKASHIC_EMOJI_TWEAK="배율,세로이동"
    let (mut scale, mut dy) = (EMOJI_SCALE, EMOJI_Y_OFFSET);
    if let Ok(v) = std::env::var("AKASHIC_EMOJI_TWEAK") {
        let mut it = v.split(',').map(|x| x.trim().parse::<f32>());
        if let (Some(Ok(a)), Some(Ok(b))) = (it.next(), it.next()) {
            scale = a;
            dy = b;
        }
    }
    let data = FontData::from_owned(bytes).tweak(egui::FontTweak { scale, y_offset_factor: dy, ..Default::default() });
    fonts.font_data.insert("emoji".into(), Arc::new(data));
    const MINE: [&str; 6] = ["latin", "latin-b", "korean", "korean-b", "malgun-code", "consolas"];
    for fam in [FontFamily::Proportional, FontFamily::Monospace, FontFamily::Name("bold".into())] {
        let list = fonts.families.entry(fam).or_default();
        list.retain(|k| k != "emoji");
        let at = list.iter().rposition(|k| MINE.contains(&k.as_str())).map_or(0, |i| i + 1);
        list.insert(at, "emoji".into());
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_env_detects_theme_switch() {
        let ctx = egui::Context::default();
        let mut env = TextEnv::default();
        let pass = |ctx: &egui::Context| {
            // 텍스처 변경분은 쓰지 않으니 버리기 전에 비운다(안 비우면 egui가 패닉).
            let mut out = ctx.run_ui(Default::default(), |_| {});
            out.textures_delta.clear();
        };
        pass(&ctx);
        assert!(env.changed(&ctx), "첫 호출은 항상 true");
        pass(&ctx);
        assert!(!env.changed(&ctx), "아무 것도 안 바뀌면 false");
        // 밝은 테마 ↔ 어두운 테마는 글자 색 변환(text_options)이 달라 아틀라스가 다시 만들어진다.
        ctx.set_visuals(egui::Visuals::light());
        pass(&ctx);
        let a = env.changed(&ctx);
        ctx.set_visuals(egui::Visuals::dark());
        pass(&ctx);
        let b = env.changed(&ctx);
        assert!(a || b, "테마 전환을 감지해야 한다");
    }

    #[test]
    fn peek_reads_header_names_without_loading_file() {
        let p = std::path::Path::new(r"C:\Windows\Fonts\malgun.ttf");
        if !p.exists() {
            return;
        }
        let f = peek_faces(p);
        assert!(f.iter().any(|x| x.family == "Malgun Gothic" && x.index == 0), "{f:?}");
        // 같은 글꼴을 전체 파싱한 결과와 이름·굵기가 같아야 한다
        let full = read_faces(&std::fs::read(p).unwrap());
        assert_eq!(f[0].family, full[0].family);
        assert_eq!(f[0].weight, full[0].weight);
        assert!(peek_faces(std::path::Path::new(r"C:\no\such\file.ttf")).is_empty());
        assert!(peek_faces(std::path::Path::new(r"C:\Windows\win.ini")).is_empty());
    }

    #[test]
    fn ttc_face_index_is_kept() {
        let p = std::path::Path::new(r"C:\Windows\Fonts\msgothic.ttc");
        if !p.exists() {
            return;
        }
        let faces = peek_faces(p);
        assert!(faces.len() >= 2, "{faces:?}");
        let found = scan_names(&[PathBuf::from(r"C:\Windows\Fonts")], &["MS PGothic"]);
        let f = found.first().expect("MS PGothic");
        assert!(f.regular.path.ends_with("msgothic.ttc"));
        assert!(f.regular.index >= 1, "ttc 안의 두 번째 이후 face여야 한다: {:?}", f.regular);
        // 저장된 위치로 다시 찾아 읽을 수 있다
        let paths = FontPaths::from_family(f);
        assert!(family_from_paths("MS PGothic", &paths).is_some());
        assert!(family_from_paths("MS Gothic", &paths).is_none(), "다른 face 이름이면 거부");
        assert!(load_loc(&f.regular).is_some());
    }

    /// 시스템 글꼴 대체 기능을 끈 뒤에도 자주 쓰는 글자가 두부(□)로 나오지 않는지: 우리가 읽는 글꼴 + egui 기본 글꼴에 글리프가 있어야 한다.
    #[test]
    fn common_chars_have_glyphs_without_system_fallback() {
        let dir = r"C:\Windows\Fonts\";
        if !std::path::Path::new(&format!("{dir}malgun.ttf")).exists() {
            return;
        }
        let mut datas: Vec<Vec<u8>> = ["malgun.ttf", "segoeui.ttf", "seguiemj.ttf", "consola.ttf"].iter().filter_map(|f| std::fs::read(format!("{dir}{f}")).ok()).collect();
        for d in FontDefinitions::default().font_data.values() {
            datas.push((*d.font).as_ref().to_vec());
        }
        let faces: Vec<ttf_parser::Face> = datas.iter().filter_map(|d| ttf_parser::Face::parse(d, 0).ok()).collect();
        let sample = "漢字かなカナ★☆→←↑↓✓✔♥♡①②㈜©®™€₩“”‘’…―㎏㎡℃±×÷≤≥≠∞√°·•※◎○●□■▲▼◆◇♪😀🚀❤✅🔥📁🙂";
        let missing: Vec<char> = sample.chars().filter(|&c| !faces.iter().any(|f| f.glyph_index(c).is_some())).collect();
        assert!(missing.is_empty(), "글리프가 없는 글자: {missing:?}");
    }

    #[test]
    fn missing_font_falls_back_to_default() {
        let ctx = egui::Context::default();
        let rep = install_fonts(&ctx, ("없는글꼴XYZ", &FontPaths::default()), ("", &FontPaths::default()));
        assert_eq!(rep.notes.len(), 1);
        assert!(rep.latin.path.is_empty());
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(parse_hex("#1A6FD6"), Some(0x1a6fd6));
        assert_eq!(parse_hex(" 1a6fd6 "), Some(0x1a6fd6));
        assert_eq!(to_hex(0x0a0b0c), "#0A0B0C");
        for bad in ["", "#12345", "#1234567", "#GGGGGG", "#12 456", "##123456"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn colors_theme_conversion() {
        let l = by_name("라이트");
        let v = l.colors();
        let t = Theme::from_colors(&v).unwrap();
        assert_eq!(t.name, CUSTOM_NAME);
        assert_eq!(t.colors(), v);
        assert!(!t.dark);
        assert!(Theme::from_colors(&v[..11]).is_none());
        assert!(Theme::from_colors(&[0; 13]).is_none());
        assert!(Theme::from_colors(&[]).is_none());
    }

    #[test]
    fn resolve_fallbacks() {
        assert_eq!(resolve(CUSTOM_NAME, &[]).name, "라이트");
        assert_eq!(resolve(CUSTOM_NAME, &[1, 2, 3]).name, "라이트");
        assert_eq!(resolve("다크", &[]).name, "다크");
        let mut v = by_name("레이저").colors().to_vec();
        v[0] = 0xffffff;
        let t = resolve(CUSTOM_NAME, &v);
        assert_eq!(t.name, CUSTOM_NAME);
        assert!(!t.dark);
    }

    #[test]
    fn luminance() {
        assert!(is_dark_color(0x000000));
        assert!(is_dark_color(0x1b1d21));
        assert!(!is_dark_color(0xffffff));
        assert!(!is_dark_color(0xf3f4f6));
    }

    #[test]
    fn broken_font_bytes_are_excluded() {
        assert!(read_faces(&[]).is_empty());
        assert!(read_faces(b"not a font at all, just bytes").is_empty());
        let mut junk = vec![0u8, 1, 0, 0];
        junk.extend(std::iter::repeat(0xAB).take(200));
        assert!(read_faces(&junk).is_empty());
    }

    #[test]
    fn scan_skips_broken_files() {
        let d = std::env::temp_dir().join(format!("akashic-fonttest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("broken.ttf"), b"garbage").unwrap();
        std::fs::write(d.join("empty.otf"), b"").unwrap();
        let r = scan_families(&[d.clone(), d.join("nonexistent")], None);
        let _ = std::fs::remove_dir_all(&d);
        assert!(r.is_empty());
    }

    #[test]
    fn reads_malgun_family_name() {
        let p = r"C:\Windows\Fonts\malgun.ttf";
        let Ok(data) = std::fs::read(p) else { return };
        let faces = read_faces(&data);
        assert!(!faces.is_empty());
        assert!(faces[0].family.contains("Malgun"), "{}", faces[0].family);
        assert!(faces[0].hangul && faces[0].latin);
    }

    #[test]
    fn scan_finds_malgun_with_bold() {
        if !std::path::Path::new(r"C:\Windows\Fonts\malgun.ttf").exists() {
            return;
        }
        let r = scan_families(&font_dirs(), Some(&["Malgun Gothic"]));
        assert_eq!(r.len(), 1);
        assert!(r[0].bold.is_some());
    }

    fn face(b: &[u8]) -> FontFace {
        FontFace { data: b.to_vec(), index: 0 }
    }

    #[test]
    fn build_fonts_order_and_bold() {
        let latin = FontSel { regular: Some(face(b"L")), bold: None };
        let korean = FontSel { regular: Some(face(b"K")), bold: Some(face(b"KB")) };
        let f = build_fonts(&latin, &korean, Some(&face(b"C")), None);
        let prop = &f.families[&FontFamily::Proportional];
        assert_eq!(&prop[..2], &["latin".to_string(), "korean".to_string()]);
        assert!(prop.len() > 2); // egui 기본 대체 글꼴
        let bold = &f.families[&FontFamily::Name("bold".into())];
        assert!(!bold.is_empty());
        assert_eq!(&bold[..2], &["latin".to_string(), "korean-b".to_string()]); // 영문 굵은체 없으면 일반체
        let mono = &f.families[&FontFamily::Monospace];
        assert_eq!(&mono[..2], &["consolas".to_string(), "korean".to_string()]);
        // 코드 글꼴은 영문·한글 선택과 무관
        let f2 = build_fonts(&FontSel::default(), &FontSel::default(), Some(&face(b"C")), Some(&face(b"M")));
        assert_eq!(&f2.families[&FontFamily::Monospace][..2], &["consolas".to_string(), "malgun-code".to_string()]);
        assert!(!f2.families[&FontFamily::Name("bold".into())].is_empty());
    }
}
