//! 쓰는 즉시 서식: 마크다운 줄을 egui LayoutJob으로 바꾼다.
//! 글자는 원문 그대로 유지하고(커서·선택이 어긋나지 않게) 서식 기호만 숨기거나 흐리게 그린다.
use crate::theme::Theme;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontFamily, FontId, Stroke};

#[derive(Clone, Copy, Default)]
struct St {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: bool,
    block: bool, // 박스 안의 코드: 글자 배경은 박스가 그린다
    dim: bool,
    quote: bool,
    sup: bool, // 각주 번호: 작게, 윗첨자 위치
    mul: f32, // 0이면 1.0
    lh: f32,  // 0이 아니면 줄 높이(제목 위아래 여백용)
}

struct Cx<'a> {
    size: f32,
    t: &'a Theme,
}

fn fmt(st: &St, hidden: bool, cx: &Cx) -> TextFormat {
    let mut f = TextFormat::default();
    if hidden {
        f.font_id = FontId::new(1.0, FontFamily::Proportional);
        f.color = Color32::TRANSPARENT;
        return f;
    }
    let mul = if st.mul == 0.0 { 1.0 } else { st.mul };
    let fam = if st.code {
        FontFamily::Monospace
    } else if st.bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    let size = if st.code { cx.size * 0.94 * mul } else { cx.size * mul };
    let size = if st.sup { size * 0.72 } else { size };
    if st.sup {
        f.valign = eframe::egui::Align::TOP;
    }
    f.font_id = FontId::new(size, fam);
    f.color = if st.code {
        cx.t.code_fg
    } else if st.dim {
        cx.t.dim
    } else if st.link {
        cx.t.link
    } else if st.quote {
        cx.t.quote
    } else {
        cx.t.text
    };
    if st.code && !st.block {
        f.background = cx.t.code_bg;
    }
    if st.lh > 0.0 {
        f.line_height = Some(st.lh);
    }
    f.italics = st.italic;
    if st.strike {
        f.strikethrough = Stroke::new(1.0, f.color);
    }
    if st.link {
        f.underline = Stroke::new(1.0, cx.t.link);
    }
    f
}

fn push(job: &mut LayoutJob, s: &str, st: &St, cx: &Cx) {
    if !s.is_empty() {
        job.append(s, 0.0, fmt(st, false, cx));
    }
}

/// 서식 기호: 커서 줄이 아니면 숨기고, 커서 줄이면 흐리게 보인다.
fn marker(job: &mut LayoutJob, s: &str, hide: bool, cx: &Cx) {
    if s.is_empty() {
        return;
    }
    if hide {
        job.append(s, 0.0, fmt(&St::default(), true, cx));
    } else {
        job.append(s, 0.0, fmt(&St { dim: true, ..Default::default() }, false, cx));
    }
}

fn is_word(c: Option<char>) -> bool {
    c.map_or(false, |c| c.is_alphanumeric())
}

fn inline(job: &mut LayoutJob, s: &str, mut st: St, hide: bool, cx: &Cx) {
    let mut i = 0;
    let mut start = 0;
    macro_rules! flush {
        () => {
            push(job, &s[start..i], &st, cx);
        };
    }
    while i < s.len() {
        let rest = &s[i..];
        let ch = rest.chars().next().unwrap();
        // 이스케이프
        if ch == '\\' {
            if let Some(n) = rest[1..].chars().next() {
                if "\\`*_{}[]()#+-.!~|>".contains(n) {
                    flush!();
                    marker(job, "\\", hide, cx);
                    let l = n.len_utf8();
                    push(job, &rest[1..1 + l], &st, cx);
                    i += 1 + l;
                    start = i;
                    continue;
                }
            }
        }
        // 인라인 코드
        if ch == '`' {
            if let Some(end) = rest[1..].find('`') {
                flush!();
                marker(job, "`", hide, cx);
                push(job, &rest[1..1 + end], &St { code: true, ..st }, cx);
                marker(job, "`", hide, cx);
                i += end + 2;
                start = i;
                continue;
            }
        }
        // 각주 참조 [^이름]
        if rest.starts_with("[^") {
            if let Some(end) = rest[2..].find(']') {
                let id = &rest[2..2 + end];
                if is_fn_id(id) {
                    flush!();
                    marker(job, "[^", hide, cx);
                    push(job, id, &St { link: true, sup: true, ..st }, cx);
                    marker(job, "]", hide, cx);
                    i += end + 3;
                    start = i;
                    continue;
                }
            }
        }
        // [[문서 링크]]
        if rest.starts_with("[[") {
            if let Some(end) = rest[2..].find("]]") {
                flush!();
                let inner = &rest[2..2 + end];
                marker(job, "[[", hide, cx);
                if let Some(bar) = inner.find('|') {
                    marker(job, &inner[..bar + 1], hide, cx);
                    push(job, &inner[bar + 1..], &St { link: true, ..st }, cx);
                } else {
                    push(job, inner, &St { link: true, ..st }, cx);
                }
                marker(job, "]]", hide, cx);
                i += end + 4;
                start = i;
                continue;
            }
        }
        // [글자](주소)
        if ch == '[' {
            if let Some(mid) = rest.find("](") {
                if let Some(close) = rest[mid..].find(')') {
                    if !rest[1..mid].contains('[') {
                        flush!();
                        marker(job, "[", hide, cx);
                        push(job, &rest[1..mid], &St { link: true, ..st }, cx);
                        marker(job, &rest[mid..mid + close + 1], hide, cx);
                        i += mid + close + 1;
                        start = i;
                        continue;
                    }
                }
            }
        }
        if rest.starts_with("**") {
            flush!();
            marker(job, "**", hide, cx);
            st.bold = !st.bold;
            i += 2;
            start = i;
            continue;
        }
        if rest.starts_with("~~") {
            flush!();
            marker(job, "~~", hide, cx);
            st.strike = !st.strike;
            i += 2;
            start = i;
            continue;
        }
        if ch == '*' || ch == '_' {
            let prev = s[..i].chars().next_back();
            let next = rest[1..].chars().next();
            let ok = if !st.italic {
                // 여는 기호: 뒤에 공백이 아니어야 하고, `_`는 낱말 중간(snake_case)이면 안 됨
                next.map_or(false, |n| !n.is_whitespace() && n != ch) && !(ch == '_' && is_word(prev))
            } else {
                prev.map_or(false, |p| !p.is_whitespace()) && !(ch == '_' && is_word(next))
            };
            if ok {
                flush!();
                marker(job, &rest[..1], hide, cx);
                st.italic = !st.italic;
                i += 1;
                start = i;
                continue;
            }
        }
        i += ch.len_utf8();
    }
    flush!();
}

/// 각주 이름: 비어 있지 않고 공백·`[`·`]`·`^`가 없어야 한다.
fn is_fn_id(id: &str) -> bool {
    !id.is_empty() && !id.chars().any(|c| c.is_whitespace() || matches!(c, '[' | ']' | '^'))
}

/// `[^이름]: 설명` 줄이면 (이름, 설명이 시작하는 바이트 위치)를 돌려준다. 들여쓰기 없는 줄만.
pub fn footnote_def(line: &str) -> Option<(&str, usize)> {
    let r = line.strip_prefix("[^")?;
    let end = r.find("]:")?;
    let id = &r[..end];
    if !is_fn_id(id) {
        return None;
    }
    let after = 2 + end + 2;
    let sp = line[after..].len() - line[after..].trim_start_matches([' ', '\t']).len();
    Some((id, after + sp))
}

/// 문자 위치가 각주 `[^이름]`(참조 또는 정의 머리) 위면 이동할 줄 번호를 돌려준다.
/// 참조에서는 정의 줄로, 정의 줄에서는 첫 참조 줄로 간다. 코드 블록 안은 제외한다.
pub fn footnote_jump(text: &str, char_idx: usize) -> Option<usize> {
    let at = text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len());
    let ls = text[..at].rfind('\n').map_or(0, |p| p + 1);
    let le = text[at..].find('\n').map_or(text.len(), |p| at + p);
    let line = text[ls..le].trim_end_matches('\r');
    let col = at - ls;
    let mut from = 0;
    let mut id = None;
    while let Some(a) = line[from..].find("[^") {
        let a = from + a;
        let Some(b) = line[a + 2..].find(']') else { break };
        let end = a + 2 + b + 1;
        let cand = &line[a + 2..a + 2 + b];
        if is_fn_id(cand) && col >= a && col <= end {
            id = Some((cand.to_string(), a == 0 && line[end..].starts_with(':')));
            break;
        }
        from = end;
    }
    let (id, is_def) = id?;
    let key = format!("[^{id}]");
    let mut in_fence = false;
    for (n, l) in text.lines().enumerate() {
        let l = l.trim_end_matches('\r');
        if l.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let def_here = footnote_def(l).map_or(false, |(d, _)| d == id);
        if is_def {
            if !def_here && l.contains(&key) {
                return Some(n);
            }
        } else if def_here {
            return Some(n);
        }
    }
    None
}

fn heading_level(line: &str) -> Option<usize> {
    let n = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&n) { Some(n) } else { None }
}

fn is_hr(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && ["-", "*", "_"].iter().any(|c| t.chars().all(|x| c.starts_with(x) || x == ' ') && t.chars().filter(|x| *x != ' ').count() >= 3)
}

/// 목록 기호 길이(들여쓰기 포함)를 돌려준다.
fn list_marker_len(line: &str) -> Option<usize> {
    let ind = line.len() - line.trim_start().len();
    let r = &line[ind..];
    if r.starts_with("- ") || r.starts_with("* ") || r.starts_with("+ ") {
        return Some(ind + 2);
    }
    let digits = r.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 && r[digits..].starts_with(". ") {
        return Some(ind + digits + 2);
    }
    None
}

/// 표 줄(투명 글자)의 글자 크기 배율: 격자 칸의 위아래 여백이 된다.
pub const TABLE_ROW_MUL: f32 = 1.25;
/// 접힌 표 행의 투명 원문 글자 크기(pt). 아주 작아야 한 줄이 좁은 창에서 여러 줄로 감기지 않는다.
const TABLE_TEXT_PT: f32 = 2.0;

/// `s`가 `<br>`·`<br/>`·`<br />`(대소문자 무시, 안쪽 공백 허용)로 시작하면 그 태그의 바이트 길이.
fn br_tag_len(s: &[u8]) -> Option<usize> {
    if s.len() < 4 || s[0] != b'<' || !s[1].eq_ignore_ascii_case(&b'b') || !s[2].eq_ignore_ascii_case(&b'r') {
        return None;
    }
    let mut j = 3;
    let skip = |j: &mut usize| {
        while *j < s.len() && s[*j] == b' ' {
            *j += 1;
        }
    };
    skip(&mut j);
    if j < s.len() && s[j] == b'/' {
        j += 1;
        skip(&mut j);
    }
    (j < s.len() && s[j] == b'>').then_some(j + 1)
}

/// 표 셀 글을 `<br>` 기준으로 줄별로 나눈다. 줄마다 앞뒤 공백을 다듬고, 맨앞·맨뒤 `<br>`은 빈 줄이 된다.
pub fn split_br(cell: &str) -> Vec<&str> {
    let b = cell.as_bytes();
    let (mut out, mut start, mut i) = (Vec::new(), 0, 0);
    while i < b.len() {
        if b[i] == b'<' {
            if let Some(len) = br_tag_len(&b[i..]) {
                out.push(cell[start..i].trim());
                i += len;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    out.push(cell[start..].trim());
    out
}

/// 표 한 행이 차지하는 줄 수(셀 중 `<br>`로 나뉜 줄이 가장 많은 것). 항상 1 이상.
pub fn row_lines(row: &[String]) -> usize {
    row.iter().map(|c| split_br(c).len()).max().unwrap_or(1).max(1)
}
/// 제목 줄 높이(글자 크기 대비): 제목과 바로 뒤 본문·목록 사이에 숨 쉴 여백을 둔다.
pub const HEADING_LINE: f32 = 1.55;

static SPACING: [std::sync::atomic::AtomicU32; 3] = [std::sync::atomic::AtomicU32::new(0x3F80_0000), std::sync::atomic::AtomicU32::new(0x3FA6_6666), std::sync::atomic::AtomicU32::new(0x3F80_0000)];
/// 줄 간격(본문 문단 안), 목록 항목 간격, 문단 간격(빈 줄 높이) 배율. 기본 1.0 / 1.3 / 1.0.
pub fn set_spacing(line: f32, list: f32, para: f32) {
    use std::sync::atomic::Ordering::Relaxed;
    SPACING[0].store(line.clamp(0.8, 2.5).to_bits(), Relaxed);
    SPACING[1].store(list.clamp(0.8, 2.5).to_bits(), Relaxed);
    SPACING[2].store(para.clamp(0.2, 3.0).to_bits(), Relaxed);
}
pub fn spacing() -> (f32, f32, f32) {
    use std::sync::atomic::Ordering::Relaxed;
    (f32::from_bits(SPACING[0].load(Relaxed)), f32::from_bits(SPACING[1].load(Relaxed)), f32::from_bits(SPACING[2].load(Relaxed)))
}
/// 목록 줄의 기본 들여쓰기(글자 크기 대비). 줄바꿈된 다음 줄은 egui 한계로 들여쓰지 않는다.
pub const LIST_INDENT: f32 = 0.9;

fn hidden_fmt(size: f32) -> TextFormat {
    let mut f = TextFormat::default();
    f.font_id = FontId::new(size, FontFamily::Proportional);
    f.color = Color32::TRANSPARENT;
    f
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    /// 코드 블록. `closed`가 거짓이면 닫는 펜스가 없어 문서 끝까지다.
    Code { lang: String, closed: bool },
    /// 표. `rows[0]`은 헤더, 나머지는 본문(구분선 줄은 뺌). 모든 줄은 열 수에 맞춰져 있다.
    Table { aligns: Vec<Align>, rows: Vec<Vec<String>> },
    /// 가로줄(한 줄).
    Hr,
    /// 체크박스 목록 줄. col은 줄 안에서 [의 글자 위치.
    Check { done: bool, col: usize },
    /// 이미지 줄(`![대체글](img:해시 가로x세로)`만 한 개 이상 있는 줄). 그림은 render.rs가 그린다.
    Image { items: Vec<ImageRef> },
    /// 수식 줄(`$$수식$$`만 있는 줄). 그림은 render.rs가 그린다.
    Math { node: crate::math::Node },
}

/// 수식 줄의 위아래 여백 합(논리 픽셀).
pub const MATH_PAD: f32 = 12.0;

/// 본문에 적힌 이미지 한 줄의 내용.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRef {
    pub alt: String,
    pub hash: String,
    pub w: u32,
    pub h: u32,
}

/// 이미지를 화면에 그릴 때의 최대 높이(논리 픽셀)와 위아래 여백 합.
pub const IMAGE_MAX_H: f32 = 700.0;
pub const IMAGE_PAD: f32 = 8.0;

/// 본문 폭 `max_w`에 맞춰 줄인 표시 크기. 키우지 않고, 높이가 상한을 넘으면 비율대로 더 줄인다. 0 크기는 만들지 않는다.
pub fn image_display_size(w: u32, h: u32, max_w: f32) -> (f32, f32) {
    let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
    let s = (max_w / wf).min(IMAGE_MAX_H / hf).min(1.0);
    ((wf * s).max(1.0), (hf * s).max(1.0))
}

const IMG_HASH_LEN: usize = 16;
const ALT_MAX: usize = 60;

fn is_img_hash(s: &str) -> bool {
    s.len() == IMG_HASH_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 대체글에서 `[ ] ( )`·줄바꿈을 빼고 60자로 자른다(이미지 줄 문법을 깨지 않도록).
pub fn clean_alt(alt: &str) -> String {
    alt.chars().filter(|c| !matches!(c, '[' | ']' | '(' | ')') && !c.is_control()).collect::<String>().trim().chars().take(ALT_MAX).collect()
}

/// 한 줄 안에서 이미지 사이의 간격(논리 픽셀).
pub const IMAGE_GAP: f32 = 8.0;

/// `rest` 맨 앞의 `![대체글](img:<16자리 해시> <가로>x<세로>)` 하나를 읽고, 그 뒤 나머지를 돌려준다.
fn parse_image_at(rest: &str) -> Option<(ImageRef, &str)> {
    let rest = rest.strip_prefix("![")?;
    let k = rest.find("](img:")?;
    let alt = &rest[..k];
    if alt.contains(['[', ']']) {
        return None;
    }
    let after = &rest[k + "](img:".len()..];
    let close = after.find(')')?;
    let (hash, dims) = after[..close].split_once(' ')?;
    if !is_img_hash(hash) {
        return None;
    }
    let (w, h) = dims.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    if w == 0 || h == 0 {
        return None;
    }
    Some((ImageRef { alt: alt.to_string(), hash: hash.to_string(), w, h }, &after[close + 1..]))
}

/// 줄 전체가 이미지(하나 이상, 공백으로 구분)로만 이루어졌을 때 그 목록을 돌려준다. 다른 글이 섞이면 None.
pub fn parse_image_row(line: &str) -> Option<Vec<ImageRef>> {
    let mut rest = line.trim();
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (r, next) = parse_image_at(rest)?;
        out.push(r);
        rest = next.trim_start();
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 줄 전체가 `![대체글](img:<16자리 해시> <가로>x<세로>)` 하나일 때만 이미지로 본다.
pub fn parse_image_line(line: &str) -> Option<ImageRef> {
    let mut v = parse_image_row(line)?;
    if v.len() == 1 { v.pop() } else { None }
}

/// 한 줄에 놓인 이미지들을 폭 `max_w`에 맞추는 공통 배율(키우지 않음, 가장 큰 높이가 상한 이하).
/// 모두 같은 배율이라 비율이 유지되고, 줄 높이 = 가장 큰 원본 높이 × 배율이다.
pub fn image_row_scale(dims: &[(u32, u32)], max_w: f32) -> f32 {
    let sum_w: f32 = dims.iter().map(|d| d.0.max(1) as f32).sum();
    let max_h = dims.iter().map(|d| d.1.max(1) as f32).fold(1.0, f32::max);
    let avail = max_w - IMAGE_GAP * dims.len().saturating_sub(1) as f32;
    (avail / sum_w).min(IMAGE_MAX_H / max_h).min(1.0).max(1.0 / max_h.max(sum_w))
}

/// 이미지 줄 한 줄(개행 없음)을 만든다.
pub fn format_image_line(alt: &str, hash: &str, w: u32, h: u32) -> String {
    format!("![{}](img:{hash} {w}x{h})", clean_alt(alt))
}

/// 본문 어디에서든(코드 블록·줄 중간 포함) `img:` 바로 뒤의 16자리 해시를 모은다(처음 나온 순서, 중복 없음).
/// 정리(prune)가 쓰는 보수적 목록이라 일부러 넓게 잡는다.
pub fn image_refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("img:") {
        rest = &rest[i + 4..];
        if let Some(h) = rest.get(..IMG_HASH_LEN) {
            if is_img_hash(h) && !out.iter().any(|x| x == h) {
                out.push(h.to_string());
            }
        }
    }
    out
}

/// `line_no`(0부터)번 줄이 이미지 줄이면 그 줄 끝에 공백 하나와 `item`을 이어 붙인 본문을 돌려준다. 아니면 None.
pub fn append_to_image_row(text: &str, line_no: usize, item: &str) -> Option<String> {
    let mut start = 0;
    for (i, piece) in text.split_inclusive('\n').enumerate() {
        if i == line_no {
            let body = piece.trim_end_matches(['\r', '\n']);
            parse_image_row(body)?;
            let mut out = String::with_capacity(text.len() + item.len() + 1);
            out.push_str(&text[..start + body.len()]);
            out.push(' ');
            out.push_str(item);
            out.push_str(&text[start + body.len()..]);
            return Some(out);
        }
        start += piece.len();
    }
    None
}

/// `after_line`(0부터, 범위를 넘으면 마지막 줄) 다음 줄에 `line`을 넣는다. 앞뒤 줄이 비어 있지 않으면 빈 줄을 하나씩 보정한다.
pub fn insert_block_line(text: &str, after_line: usize, line: &str) -> String {
    if text.is_empty() {
        return line.to_string();
    }
    let crlf = text.contains("\r\n");
    let ends_nl = text.ends_with('\n');
    let mut pieces: Vec<&str> = text.split('\n').collect();
    if ends_nl {
        pieces.pop();
    }
    let idx = after_line.min(pieces.len() - 1);
    let p = idx + 1;
    let blank = |s: &str| s.trim().is_empty();
    let eol_pad = |s: &str| if crlf { format!("{s}\r") } else { s.to_string() };
    let mut out: Vec<String> = pieces[..p].iter().map(|s| s.to_string()).collect();
    if !blank(pieces[idx]) {
        out.push(eol_pad(""));
    }
    out.push(eol_pad(line));
    if pieces.get(p).map_or(false, |s| !blank(s)) {
        out.push(eol_pad(""));
    }
    out.extend(pieces[p..].iter().map(|s| s.to_string()));
    let mut s = out.join("\n");
    if ends_nl {
        s.push('\n');
    }
    s
}

/// 줄 번호 범위 `start..=end`를 차지하는 블록.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub start: usize,
    pub end: usize,
}

/// 표 줄을 `|` 기준 칸으로 쪼갠다(앞뒤 파이프는 생략 가능, `\|`는 글자 `|`).
pub fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !(t.ends_with('|') && !t.ends_with("\\|")) {
        cells.push(cur.trim().to_string());
    }
    if t.starts_with('|') && !cells.is_empty() {
        cells.remove(0);
    }
    cells
}

/// 표 구분선(`|---|:---:|`)이면 열마다 정렬을 돌려준다. 파이프가 하나도 없으면 표 구분선이 아니다.
pub fn parse_delim(line: &str) -> Option<Vec<Align>> {
    if !line.contains('|') {
        return None;
    }
    let cells = split_cells(line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|c| {
            let l = c.starts_with(':');
            let r = c.ends_with(':') && c.len() > 1;
            let core = c.trim_start_matches(':').trim_end_matches(':');
            if core.is_empty() || !core.chars().all(|x| x == '-') {
                return None;
            }
            Some(match (l, r) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect()
}

fn fix_cols(mut row: Vec<String>, n: usize) -> Vec<String> {
    row.resize(n, String::new());
    row
}

/// 문서에서 코드 블록·표·가로줄 구간을 찾는다(줄 번호는 `build`가 세는 것과 같다).
pub fn scan_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.split_inclusive('\n').map(|r| r.trim_end_matches(['\r', '\n'])).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let tl = line.trim_start();
        if tl.starts_with("```") {
            let lang = tl[3..].split_whitespace().next().unwrap_or("").trim_matches('`').to_string();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim_start().starts_with("```") {
                j += 1;
            }
            let closed = j < lines.len();
            let end = if closed { j } else { lines.len() - 1 };
            out.push(Block { kind: BlockKind::Code { lang, closed }, start: i, end });
            i = end + 1;
            continue;
        }
        if line.contains('|') && i + 1 < lines.len() {
            if let Some(aligns) = parse_delim(lines[i + 1]) {
                let n = aligns.len();
                if split_cells(line).len() == n {
                    let mut rows = vec![fix_cols(split_cells(line), n)];
                    let mut j = i + 2;
                    while j < lines.len() && lines[j].contains('|') && !lines[j].trim_start().starts_with("```") {
                        rows.push(fix_cols(split_cells(lines[j]), n));
                        j += 1;
                    }
                    out.push(Block { kind: BlockKind::Table { aligns, rows }, start: i, end: j - 1 });
                    i = j;
                    continue;
                }
            }
        }
        if line.trim() == "$$" {
            // 여러 줄 수식: 다음 `$$`만 있는 줄까지. 빈 줄·코드 펜스를 만나면 짝이 없는 것으로 보고(문서 뒤쪽을 삼키지 않는다) 그냥 글로 둔다.
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim() != "$$" && !lines[j].trim().is_empty() && !lines[j].trim_start().starts_with("```") {
                j += 1;
            }
            if j < lines.len() && lines[j].trim() == "$$" {
                if let Some(node) = crate::math::parse_multi(&lines[i + 1..j].join(" ")) {
                    out.push(Block { kind: BlockKind::Math { node }, start: i, end: j });
                }
                i = j + 1; // 틀린 수식이어도 닫는 `$$`가 다음 수식의 여는 줄이 되지 않게 한다
                continue;
            }
        }
        if heading_level(line).is_none() && is_hr(line) {
            out.push(Block { kind: BlockKind::Hr, start: i, end: i });
        } else if let Some(items) = parse_image_row(line) {
            out.push(Block { kind: BlockKind::Image { items }, start: i, end: i });
        } else if let Some(node) = crate::math::parse_block(line) {
            out.push(Block { kind: BlockKind::Math { node }, start: i, end: i });
        } else if let Some(m) = list_marker_len(line) {
            let rest = &line[m..];
            let done = rest.starts_with("[x] ") || rest.starts_with("[X] ");
            if done || rest.starts_with("[ ] ") {
                out.push(Block { kind: BlockKind::Check { done, col: line[..m].chars().count() }, start: i, end: i });
            }
        }
        i += 1;
    }
    out
}

/// `cursor_line`: 커서가 있는 줄(그 줄은 서식 기호를 그대로 보인다).
pub fn build(text: &str, cursor_line: Option<usize>, size: f32, t: &Theme, wrap: f32, hide_syntax: bool) -> LayoutJob {
    let cx = Cx { size, t };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let plain = St::default();
    // 줄 간격·문단 간격(옵션). 1.0이면 기본 모양 그대로다. 표·코드 줄은 격자 계산이 있어 건드리지 않는다.
    let (ls, lsl, pg) = spacing();
    let row = size * 1.33;
    let para = St { lh: if (ls - 1.0).abs() > 0.01 { row * ls } else { 0.0 }, ..plain };
    let lst = St { lh: if (lsl - 1.0).abs() > 0.01 { row * lsl } else { para.lh }, ..plain };
    let blocks = scan_blocks(text);
    let mut bi = 0;
    let raws: Vec<&str> = text.split_inclusive('\n').collect();
    for (n, raw) in raws.iter().copied().enumerate() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let nl = &raw[line.len()..];
        let cur = cursor_line == Some(n);
        let hide = hide_syntax && !cur;
        let mut nl_fmt: Option<TextFormat> = None;
        while bi < blocks.len() && blocks[bi].end < n {
            bi += 1;
        }
        let blk = blocks.get(bi).filter(|b| b.start <= n);
        let in_blk = blk.map_or(false, |b| cursor_line.map_or(false, |c| c >= b.start && c <= b.end));
        let collapsed = hide_syntax && !in_blk;

        if let Some(Block { kind: BlockKind::Code { lang, closed }, start, end }) = blk {
            let is_fence = n == *start || (*closed && n == *end);
            if is_fence && collapsed {
                // 펜스 줄은 거의 0 높이로 접는다(여는 줄에 언어 이름이 있으면 라벨 자리만큼 남긴다).
                let sz = if n == *start && !lang.is_empty() { 13.0 } else { 6.0 };
                let f = hidden_fmt(sz);
                job.append(line, 0.0, f.clone());
                nl_fmt = Some(f);
            } else if is_fence {
                push(&mut job, line, &St { dim: true, code: true, ..plain }, &cx);
            } else {
                push(&mut job, line, &St { code: true, block: hide_syntax, ..plain }, &cx);
                nl_fmt = Some(fmt(&St { code: true, block: hide_syntax, ..plain }, false, &cx));
            }
        } else if let Some(Block { kind: BlockKind::Table { rows, .. }, start, .. }) = blk {
            if collapsed {
                // 격자는 render.rs가 그린다. 글자는 투명하게 두되 줄 높이는 유지한다.
                if n == start + 1 {
                    let f = hidden_fmt(4.0);
                    job.append(line, 0.0, f.clone());
                    nl_fmt = Some(f);
                } else {
                    let mut f = fmt(&St { code: true, block: true, ..plain }, false, &cx);
                    // 투명한 원문 글자는 아주 작게 해서 좁은 창에서도 여러 줄로 감기지 않게 하고(감기면 행마다 높이가 부풀었다),
                    // 행 높이는 줄 높이로 직접 정한다. 셀에 <br>이 있으면 그 줄 수만큼 키운다(머리글은 n == start, 본문은 n - start - 1번째 행).
                    f.font_id = FontId::new(TABLE_TEXT_PT, FontFamily::Monospace);
                    f.color = Color32::TRANSPARENT;
                    let r = if n == *start { 0 } else { n - start - 1 };
                    let k = rows.get(r).map_or(1, |row| row_lines(row));
                    f.line_height = Some(size * TABLE_ROW_MUL * 1.2 + (k - 1) as f32 * size * 1.3);
                    job.append(line, 0.0, f.clone());
                    nl_fmt = Some(f);
                }
            } else {
                push(&mut job, line, &St { code: true, ..plain }, &cx);
            }
        } else if let Some(Block { kind: BlockKind::Image { items }, .. }) = blk {
            if hide {
                // 글자는 투명하고 아주 작게 두고, 줄 높이를 가장 큰 그림 높이로 키운다. 그림은 render.rs가 그린다.
                let dims: Vec<(u32, u32)> = items.iter().map(|r| (r.w, r.h)).collect();
                let max_h = dims.iter().map(|d| d.1.max(1) as f32).fold(1.0, f32::max);
                let dh = (max_h * image_row_scale(&dims, wrap)).max(1.0);
                let mut f = hidden_fmt(4.0);
                f.line_height = Some(dh + IMAGE_PAD);
                job.append(line, 0.0, f.clone());
                nl_fmt = Some(f);
            } else {
                push(&mut job, line, &St { dim: true, code: true, ..plain }, &cx);
            }
        } else if let Some(Block { kind: BlockKind::Math { node }, start, .. }) = blk {
            if collapsed {
                // 글자는 투명하고 아주 작게 두고, 첫 줄의 높이를 수식 높이로 키운다(나머지 줄은 거의 0). 수식은 render.rs가 그린다.
                let mut f = hidden_fmt(4.0);
                f.line_height = Some(if n == *start { crate::math::height_em(node) * size + MATH_PAD } else { 1.0 });
                job.append(line, 0.0, f.clone());
                nl_fmt = Some(f);
            } else {
                push(&mut job, line, &St { dim: true, code: true, ..plain }, &cx);
            }
        } else if let Some(lv) = heading_level(line) {
            let mul =[1.0, 1.7, 1.4, 1.2, 1.1, 1.0, 1.0][lv];
            let sp = if line[lv..].starts_with(' ') { 1 } else { 0 };
            marker(&mut job, &line[..lv + sp], hide, &cx);
            // 제목 바로 아래에 빈 줄이 없으면 본문 한 줄만큼 띄운다(HTML의 h1~h6 아래 여백처럼). 빈 줄이 있으면 그 빈 줄이 간격이다.
            let next_text = raws.get(n + 1).map_or(false, |r| !r.trim().is_empty());
            let lh = if next_text { size * mul * 1.33 + row } else { size * mul * HEADING_LINE };
            inline(&mut job, &line[lv + sp..], St { bold: true, mul, lh, ..plain }, hide, &cx);
        } else if is_hr(line) {
            if hide {
                // 글자는 투명하게 두고(줄 높이 유지) 가로선은 render.rs가 줄 폭 전체로 그린다.
                let mut f = fmt(&plain, false, &cx);
                f.color = Color32::TRANSPARENT;
                job.append(line, 0.0, f);
            } else {
                push(&mut job, line, &St { dim: true, ..plain }, &cx);
            }
        } else if let Some((id, body)) = footnote_def(line) {
            // [^1]: 설명 — 머리는 작은 번호로, 기호는 숨긴다.
            marker(&mut job, "[^", hide, &cx);
            push(&mut job, id, &St { link: true, sup: true, ..para }, &cx);
            marker(&mut job, &line[2 + id.len()..body], hide, &cx);
            inline(&mut job, &line[body..], para, hide, &cx);
        } else if line.starts_with('>') {
            let sp = if line[1..].starts_with(' ') { 2 } else { 1 };
            marker(&mut job, &line[..sp], hide, &cx);
            inline(&mut job, &line[sp..], St { quote: true, italic: false, ..para }, hide, &cx);
        } else if let Some(m) = list_marker_len(line) {
            if m > 0 {
                job.append(&line[..m], size * LIST_INDENT, fmt(&St { dim: true, ..lst }, false, &cx));
            }
            let rest = &line[m..];
            let done = rest.starts_with("[x] ") || rest.starts_with("[X] ");
            if done || rest.starts_with("[ ] ") {
                if hide_syntax {
                    // 글자는 투명하게 두고(폭 유지) 진짜 체크박스는 render.rs가 그린다.
                    let mut f = fmt(&lst, false, &cx);
                    f.color = Color32::TRANSPARENT;
                    job.append(&rest[..4], 0.0, f);
                } else {
                    push(&mut job, &rest[..4], &St { dim: !done, link: done, ..lst }, &cx);
                }
                let st = if done { St { strike: true, dim: true, ..lst } } else { lst };
                inline(&mut job, &rest[4..], st, hide, &cx);
            } else {
                inline(&mut job, rest, lst, hide, &cx);
            }
        } else {
            inline(&mut job, line, para, hide, &cx);
        }
        // 문단 사이(빈 줄)의 높이: 빈 줄 하나가 문단 구분이다.
        if blk.is_none() && line.trim().is_empty() && (pg - 1.0).abs() > 0.01 {
            nl_fmt = Some(fmt(&St { lh: row * pg, ..plain }, false, &cx));
        }
        if !nl.is_empty() {
            job.append(nl, 0.0, nl_fmt.unwrap_or_else(|| fmt(&plain, false, &cx)));
        }
    }
    if text.is_empty() || text.ends_with('\n') {
        // 마지막 빈 줄의 높이를 위해 아무 것도 추가하지 않는다(egui가 처리).
    }
    job
}

/// 본문의 제목(목차 항목).
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    /// 1~6
    pub level: usize,
    /// 서식 기호를 뺀 제목 글자
    pub text: String,
    /// 0부터 세는 줄 번호
    pub line: usize,
}

/// `#` 제목을 모은다. 코드 블록 안은 제외하고, 편집창과 같은 규칙(`###제목`처럼 붙여 쓴 것도 제목)을 따른다.
pub fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let Some(level) = heading_level(line) else { continue };
        let rest = line[level..].trim().trim_end_matches('#').trim();
        let clean = rest.replace("**", "").replace("__", "").replace("~~", "").replace('`', "");
        let clean = clean.trim();
        if !clean.is_empty() {
            out.push(Heading { level, text: clean.to_string(), line: n });
        }
    }
    out
}

/// `line`번째 줄이 시작하는 문자 위치(char index). 줄이 없으면 끝.
pub fn char_of_line(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line).map(|l| l.chars().count()).sum()
}

/// 문자 위치(char index)가 몇 번째 줄인지.
pub fn line_of_char(text: &str, char_idx: usize) -> usize {
    text.chars().take(char_idx).filter(|c| *c == '\n').count()
}

/// `[[ ]]` 안의 대상이 문서 제목이 아니라 외부 주소·파일 경로인지(`http://…`, `mailto:`, `www.…`, `C:\…`, `\서버\…`).
pub fn is_external_target(t: &str) -> bool {
    let t = t.trim();
    if t.starts_with('^') {
        return true; // 각주 표기([[^1]] 등)
    }
    let l = t.to_ascii_lowercase();
    if t.contains("://") || l.starts_with("www.") || l.starts_with("mailto:") || l.starts_with("file:") || t.starts_with("\\\\") {
        return true;
    }
    let b = t.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// 문서 제목으로 만든 링크 글자 `[[제목]]`. 링크 문법으로 그 제목을 가리킬 수 없으면 이유를 돌려준다
/// (`|`·`]]`는 링크 문법과 겹치고, 외부 주소·각주처럼 보이는 제목은 문서 링크로 읽히지 않는다).
pub fn wiki_tag(title: &str) -> Result<String, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("제목이 없는 문서는 링크로 만들 수 없습니다".into());
    }
    if t.contains('|') || t.contains("]]") || t.contains('\n') || t.contains('\r') {
        return Err("제목에 |, ]] 또는 줄바꿈이 있어 링크로 만들 수 없습니다".into());
    }
    if is_external_target(t) {
        return Err("주소나 각주처럼 보이는 제목이라 문서 링크로 쓸 수 없습니다".into());
    }
    Ok(format!("[[{t}]]"))
}

/// 한 줄에서 백틱(`)으로 감싼 인라인 코드 구간을 공백으로 바꾼다(화면에서 코드로 그리는 부분은 링크가 아니므로).
fn blank_code_spans(line: &str) -> String {
    let mut in_code = false;
    line.chars()
        .map(|c| {
            if c == '`' {
                in_code = !in_code;
                ' '
            } else if in_code {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// 본문에서 `[[제목]]` / `[[제목|글]]`의 제목들을 모은다(코드 블록·인라인 코드 안, 외부 주소·경로는 제외).
pub fn wiki_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let clean = blank_code_spans(line);
        let mut rest = clean.as_str();
        while let Some(a) = rest.find("[[") {
            let Some(b) = rest[a + 2..].find("]]") else { break };
            let inner = &rest[a + 2..a + 2 + b];
            let title = inner.split('|').next().unwrap_or("").trim();
            if !title.is_empty() && !is_external_target(title) {
                out.push(title.to_string());
            }
            rest = &rest[a + 2 + b + 2..];
        }
    }
    out
}

/// 문자 위치가 `[[...]]` 안이면 그 제목을 돌려준다.
pub fn wiki_at(text: &str, char_idx: usize) -> Option<String> {
    let line_start_byte = text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len());
    let ls = text[..line_start_byte].rfind('\n').map_or(0, |p| p + 1);
    let le = text[line_start_byte..].find('\n').map_or(text.len(), |p| line_start_byte + p);
    let line = &text[ls..le];
    let col = line_start_byte - ls;
    let mut from = 0;
    while let Some(a) = line[from..].find("[[") {
        let a = from + a;
        let Some(b) = line[a + 2..].find("]]") else { break };
        let end = a + 2 + b + 2;
        if col >= a && col <= end {
            let inner = &line[a + 2..a + 2 + b];
            let title = inner.split('|').next().unwrap_or("").trim();
            return if is_external_target(title) { None } else { Some(title.to_string()) };
        }
        from = end;
    }
    None
}

pub enum Enter {
    /// 줄바꿈 뒤에 이 접두어를 붙인다.
    Insert(String),
    /// 빈 항목에서 Enter: 그 줄의 목록 기호를 지우고 목록을 끝낸다(남길 들여쓰기 길이 0).
    Clear,
}

/// 커서가 줄 끝일 때 Enter가 목록·인용·체크박스를 이어 쓰게 할 접두어를 계산한다.
pub fn continue_list(line: &str) -> Option<Enter> {
    if let Some(rest) = line.strip_prefix('>') {
        let body = rest.strip_prefix(' ').unwrap_or(rest);
        return Some(if body.trim().is_empty() { Enter::Clear } else { Enter::Insert("> ".into()) });
    }
    let m = list_marker_len(line)?;
    let ind = &line[..line.len() - line.trim_start().len()];
    let marker = &line[ind.len()..m];
    let rest = &line[m..];
    let check_prefix = ["[ ] ", "[x] ", "[X] "].iter().find_map(|p| rest.strip_prefix(p));
    let check = check_prefix.is_some();
    let content = check_prefix.unwrap_or(rest);
    // 내용이 없는 빈 항목(기호만 있는 줄)에서 Enter를 누르면 목록을 끝낸다.
    if content.trim().is_empty() || rest.trim() == "[ ]" {
        return Some(Enter::Clear);
    }
    let next_marker = match marker.trim_end().trim_end_matches('.').parse::<u64>() {
        Ok(n) if marker.trim_end().ends_with('.') => format!("{}. ", n + 1),
        _ => marker.to_string(),
    };
    Some(Enter::Insert(format!("{ind}{next_marker}{}", if check { "[ ] " } else { "" })))
}

/// 글자 위치 `char_idx`가 목록 줄의 `[ ]`/`[x]` 위인지 본다. 맞으면 (줄 시작 바이트, 목록 기호 길이, 지금 체크됨 여부).
fn checkbox_hit(text: &str, char_idx: usize) -> Option<(usize, usize, bool)> {
    let at = text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len());
    let ls = text[..at].rfind('\n').map_or(0, |p| p + 1);
    let le = text[at..].find('\n').map_or(text.len(), |p| at + p);
    let line = text[ls..le].trim_end_matches('\r');
    let m = list_marker_len(line)?;
    let rest = &line[m..];
    if !(rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]")) {
        return None;
    }
    let col = at - ls;
    if col < m || col > m + 3 {
        return None;
    }
    // 코드 블록 안은 체크박스가 아니다.
    let fences = text[..ls].lines().filter(|l| l.trim_start().starts_with("```")).count();
    if fences % 2 == 1 {
        return None;
    }
    Some((ls, m, !rest.starts_with("[ ]")))
}

/// 글자 위치가 눌러서 켜고 끌 수 있는 체크박스 위인지(마우스를 올렸을 때 손가락 모양을 보이는 데 쓴다).
pub fn is_checkbox_at(text: &str, char_idx: usize) -> bool {
    checkbox_hit(text, char_idx).is_some()
}

/// 목록 줄의 `[ ]`/`[x]` 위에서 눌렀으면 켜고 끈 새 본문을 돌려준다.
pub fn toggle_checkbox_at(text: &str, char_idx: usize) -> Option<String> {
    let (ls, m, checked) = checkbox_hit(text, char_idx)?;
    let flip = if checked { "[ ]" } else { "[x]" };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..ls + m]);
    out.push_str(flip);
    out.push_str(&text[ls + m + 3..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::themes;

    fn same(text: &str, cur: Option<usize>) {
        let t = &themes()[0];
        for hide in [true, false] {
            let job = build(text, cur, 15.0, t, 500.0, hide);
            assert_eq!(job.text, text, "원문이 보존되어야 함: {text:?}");
        }
    }

    #[test]
    fn text_is_preserved_for_every_syntax() {
        let doc = "# 제목\n##붙여쓴 제목\n**굵게** *기울임* ~~취소~~ `코드` [[문서|보임]] [링크](http://x)\n\
> 인용 **굵게**\n- 목록\n  1. 번호\n- [ ] 할 일\n- [x] 끝남\n---\n| a | b |\n|---|---|\n```rust\nlet x = 1;\n```\n\
\\*이스케이프\\* snake_case_name 한글*붙임* 닫히지 않은 **굵게\r\n마지막 줄";
        same(doc, None);
        for l in 0..16 {
            same(doc, Some(l));
        }
        same("", None);
        same("\n\n", None);
        same("```\n안 닫힌 코드", None);
        same("**", None);
        same("[[", None);
        same("[a](", None);
        same("`", None);
        same("\\", None);
    }

    #[test]
    fn text_is_preserved_for_blocks() {
        let doc = "| a | b |\n|:--|--:|\n| 1 \\| 2 | 3 |\n\n```py\nx | y\n---\n```\n---\na | b\n--|--\n1\n```\n닫히지 않음\n> ```\n";
        same(doc, None);
        for l in 0..16 {
            same(doc, Some(l));
        }
        same("|\n|-|\n|", None);
        same("a|b\n-|-", Some(1));
    }

    fn kinds(text: &str) -> Vec<(String, usize, usize)> {
        scan_blocks(text)
            .into_iter()
            .map(|b| {
                let k = match b.kind {
                    BlockKind::Code { closed: true, .. } => "code",
                    BlockKind::Code { .. } => "open",
                    BlockKind::Table { .. } => "table",
                    BlockKind::Hr => "hr",
                    BlockKind::Check { .. } => "check",
                    BlockKind::Image { .. } => "image",
                    BlockKind::Math { .. } => "math",
                };
                (k.to_string(), b.start, b.end)
            })
            .collect()
    }

    const H16: &str = "0123456789abcdef";

    #[test]
    fn image_line_parses() {
        let r = parse_image_line("![스샷](img:0123456789abcdef 800x600)").unwrap();
        assert_eq!(r, ImageRef { alt: "스샷".into(), hash: H16.into(), w: 800, h: 600 });
        assert_eq!(parse_image_line("  ![](img:0123456789abcdef 8x6)  ").unwrap().alt, "");
    }

    #[test]
    fn image_line_rejects_mixed_or_bad() {
        for l in [
            "글 ![a](img:0123456789abcdef 8x6)",
            "![a](img:0123456789abcdef 8x6) 글",
            "![a](img:XYZ 8x6)",
            "![a](img:0123456789abcdef 0x6)",
            "![a](img:0123456789abcdef 8x0)",
            "![a](img:0123456789abcdef)",
            "![a](https://x/y.png)",
        ] {
            assert!(parse_image_line(l).is_none(), "{l}");
        }
    }

    #[test]
    fn image_row_parses_several_on_one_line() {
        let l = format!("![a](img:{H16} 8x6) ![b](img:aaaaaaaaaaaaaaaa 10x20)![](img:bbbbbbbbbbbbbbbb 3x3)");
        let v = parse_image_row(&l).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!((v[1].alt.as_str(), v[1].hash.as_str(), v[1].w, v[1].h), ("b", "aaaaaaaaaaaaaaaa", 10, 20));
        assert_eq!(parse_image_row(&format!("![a](img:{H16} 8x6)")).unwrap().len(), 1);
        for bad in [format!("![a](img:{H16} 8x6) 글 ![a](img:{H16} 8x6)"), format!("![a](img:{H16} 8x6) ![b](img:ZZ 1x1)"), "".to_string(), "  ".to_string()] {
            assert!(parse_image_row(&bad).is_none(), "{bad}");
        }
        // 한 개짜리 전용 함수는 여러 개를 받지 않는다
        assert!(parse_image_line(&l).is_none());
    }

    #[test]
    fn image_row_scale_fits_one_row_bottom_aligned() {
        // 두 장(200x100, 200x200)이 폭 1000이면 그대로(키우지 않음)
        assert_eq!(image_row_scale(&[(200, 100), (200, 200)], 1000.0), 1.0);
        // 폭 208(간격 8 제외 200)이면 같은 비율로 절반
        let s = image_row_scale(&[(200, 100), (200, 200)], 208.0);
        assert!((s - 0.5).abs() < 1e-4, "{s}");
        // 높이 상한
        assert_eq!(image_row_scale(&[(100, 2000)], 1000.0), IMAGE_MAX_H / 2000.0);
        // 한 장일 때는 image_display_size와 같다
        let (w, _) = image_display_size(800, 600, 400.0);
        assert_eq!(800.0 * image_row_scale(&[(800, 600)], 400.0), w);
        assert!(image_row_scale(&[(800, 600), (800, 600)], 0.0) > 0.0);
    }

    #[test]
    fn split_br_variants() {
        assert_eq!(split_br("a<br>b"), vec!["a", "b"]);
        assert_eq!(split_br("a<BR/>b<br />c<Br  / >d"), vec!["a", "b", "c", "d"]);
        assert_eq!(split_br(" a <br> b "), vec!["a", "b"]); // 앞뒤 공백은 줄마다 다듬는다
        assert_eq!(split_br("<br>x"), vec!["", "x"]);
        assert_eq!(split_br("x<br>"), vec!["x", ""]);
        assert_eq!(split_br("한글<br>둘째 줄<br>셋"), vec!["한글", "둘째 줄", "셋"]);
        // <br>이 아닌 것은 나누지 않는다
        for s in ["<b>굵게</b>", "<brx>", "a < br > b", "<br", "plain", ""] {
            assert_eq!(split_br(s), vec![s.trim()], "{s}");
        }
    }

    #[test]
    fn row_lines_is_max_over_cells() {
        let row = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(row_lines(&row(&["a", "b"])), 1);
        assert_eq!(row_lines(&row(&["a<br>b", "x<br>y<br>z"])), 3);
        assert_eq!(row_lines(&[]), 1);
    }

    #[test]
    fn table_row_with_br_is_taller_when_collapsed() {
        let t = &themes()[0];
        let text = "| a | b |\n|---|---|\n| 1<br>2<br>3 | x |\n| y | z |\n";
        let job = build(text, None, 15.0, t, 600.0, true);
        let lh = |needle: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(needle)).unwrap().format.line_height;
        let tall = lh("1<br>2<br>3").expect("<br>이 있는 행은 줄 높이를 직접 정한다");
        let normal = lh("| y | z |");
        assert!(normal.map_or(true, |n| tall > n), "{tall} {normal:?}");
        let two = lh("| a | b |"); // 머리글 행(<br> 없음)
        assert!(two.map_or(true, |n| n < tall));
        // 커서가 표 안에 있으면(원문 편집) 높이를 키우지 않는다
        let job = build(text, Some(2), 15.0, t, 600.0, true);
        let s = job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains("1<br>2<br>3")).unwrap();
        assert!(s.format.line_height.map_or(true, |v| v < tall));
    }

    #[test]
    fn is_checkbox_at_matches_toggle() {
        let text = "- [ ] 할 일\n- [x] 끝\n일반 글 [ ]\n```\n- [ ] 코드 안\n```\n";
        let n = text.chars().count();
        for i in 0..=n {
            assert_eq!(is_checkbox_at(text, i), toggle_checkbox_at(text, i).is_some(), "idx {i}");
        }
        assert!(is_checkbox_at(text, 3)); // `[ ]` 위
        assert!(!is_checkbox_at(text, 9)); // 글자 위
        let code = text.find("코드 안").unwrap();
        assert!(!is_checkbox_at(text, text[..code].chars().count() - 3), "코드 블록 안은 아니다");
        assert_eq!(toggle_checkbox_at("- [ ] a", 3).unwrap(), "- [x] a");
        assert_eq!(toggle_checkbox_at("- [x] a", 3).unwrap(), "- [ ] a");
    }

    #[test]
    fn chained_footnote_defs_on_one_line_do_not_panic() {
        let t = &themes()[0];
        for text in ["[^9]:[^10]: 설명\n", "[^9]:[^10]:\n", "[^9]:[^10]", "[^9]:[^10]: 설명 [^11]", "본문 [^9][^10]\n\n[^9]:[^10]: 설명\n", "[^9]:[^10]:[^11]: 가나다\n"] {
            for hide in [true, false] {
                for cur in [None, Some(0), Some(1)] {
                    let job = build(text, cur, 15.0, t, 400.0, hide);
                    assert_eq!(job.text, text, "{text:?}");
                }
            }
            for i in 0..=text.chars().count() {
                let _ = footnote_jump(text, i);
            }
        }
    }

    #[test]
    fn wiki_tag_round_trips_through_wiki_links() {
        for title in ["측정기 점검", "03. 일기 / 2026", "A [b] c", "  앞뒤 공백  "] {
            let tag = wiki_tag(title).unwrap();
            assert_eq!(wiki_links(&tag), vec![title.trim().to_string()], "{tag}");
        }
        assert_eq!(wiki_tag(" x ").unwrap(), "[[x]]");
    }

    #[test]
    fn wiki_tag_rejects_titles_that_cannot_be_linked() {
        for bad in ["", "   ", "a|b", "a]]b", "줄\n바꿈", "http://x.y", "C:\\temp", "^1", "www.a.kr"] {
            assert!(wiki_tag(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn math_line_is_a_block_and_only_when_valid() {
        let k = kinds("앞\n$$x=\\frac{a}{b}$$\n뒤\n$$\\frac{1}$$\n");
        assert_eq!(k, vec![("math".to_string(), 1, 1)]); // 틀린 수식은 블록이 아니다
        // 코드 블록 안의 $$...$$는 수식이 아니다
        assert!(kinds("```\n$$x$$\n```\n").iter().all(|(k, ..)| k != "math"));
    }

    #[test]
    fn multiline_math_block_spans_dollar_lines() {
        let k = kinds("앞\n$$\nx = \\frac{a}\n   {b}\n$$\n뒤\n");
        assert_eq!(k, vec![("math".to_string(), 1, 4)]);
        let k = kinds("  $$  \n\\begin{aligned}a&=b\\\\\n&=c\\end{aligned}\n $$\n");
        assert_eq!(k, vec![("math".to_string(), 0, 3)]);
        // 한 줄 수식은 그대로
        assert_eq!(kinds("$$x$$\n"), vec![("math".to_string(), 0, 0)]);
    }

    #[test]
    fn unclosed_or_invalid_multiline_math_is_not_a_block() {
        // 닫는 $$ 가 없으면 뒤쪽 문서를 삼키지 않는다(표·체크박스는 그대로 인식).
        let k = kinds("$$\nx = 1\n\n- [ ] 할 일\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_eq!(k.iter().map(|(k, ..)| k.as_str()).collect::<Vec<_>>(), vec!["check", "table"]);
        // 빈 줄을 넘어 먼 $$ 와 짝지어지지 않는다.
        assert!(kinds("$$\nx\n\n문단\n$$\n").is_empty());
        // 문법이 틀리면 블록이 아니고, 닫는 $$ 가 다음 수식의 여는 줄이 되지도 않는다.
        assert!(kinds("$$\n\\frac{1}\n$$\n").is_empty());
        assert_eq!(kinds("$$\n\\frac{1}\n$$\n$$\nx\n$$\n"), vec![("math".to_string(), 3, 5)]);
        // 빈 수식
        assert!(kinds("$$\n$$\n").is_empty());
    }

    #[test]
    fn multiline_math_inside_code_block_is_not_math() {
        assert!(kinds("```\n$$\nx\n$$\n```\n").iter().all(|(k, ..)| k != "math"));
        // 코드 펜스가 사이에 끼면 짝을 짓지 않는다.
        assert!(kinds("$$\nx\n```\n$$\n").iter().all(|(k, ..)| k != "math"));
    }

    #[test]
    fn multiline_math_height_on_first_line_and_cursor_shows_source() {
        let t = &themes()[0];
        let text = "a\n$$\n\\frac{a}\n{b}\n$$\nz\n";
        let lh = |job: &LayoutJob, needle: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(needle)).unwrap().format.line_height;
        let job = build(text, None, 15.0, t, 600.0, true);
        assert_eq!(job.text, text); // 글자는 원문 그대로
        let first = job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].starts_with("$$")).unwrap().format.line_height.expect("첫 줄이 수식 높이를 가진다");
        let inner = lh(&job, "frac").expect("안쪽 줄도 높이를 직접 정한다(작게)");
        assert!(first > 15.0 * 1.5 && inner < 6.0, "{first} {inner}");
        // 커서가 블록 안 어느 줄이든 모든 줄이 원문(줄 높이를 키우지 않음).
        for c in 1..=4 {
            let job = build(text, Some(c), 15.0, t, 600.0, true);
            assert_eq!(job.text, text);
            for s in &job.sections {
                assert!(s.format.line_height.map_or(true, |v| v < first), "cursor {c}");
            }
        }
        // 블록 밖 커서는 수식으로 그린다.
        let job = build(text, Some(5), 15.0, t, 600.0, true);
        assert!(lh(&job, "frac").map_or(false, |v| v < 6.0));
    }

    #[test]
    fn math_line_height_follows_formula_and_cursor_shows_source() {
        let t = &themes()[0];
        let text = "a\n$$\\frac{a}{b}$$\n$$x$$\nz\n";
        let job = build(text, None, 15.0, t, 600.0, true);
        let lh = |needle: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(needle)).unwrap().format.line_height;
        let frac = lh("frac").expect("수식 줄은 줄 높이를 직접 정한다");
        let flat = lh("$$x$$").expect("한 줄짜리 수식도 마찬가지");
        assert!(frac > flat * 1.4, "{frac} {flat}");
        // 커서가 있는 줄은 원문을 그대로(줄 높이를 키우지 않고) 보인다
        let job = build(text, Some(1), 15.0, t, 600.0, true);
        let s = job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains("frac")).unwrap();
        assert!(s.format.line_height.map_or(true, |v| v < frac));
    }

    #[test]
    fn collapsed_table_row_height_does_not_depend_on_text_length() {
        // 아주 긴 행이 좁은 창에서 감겨 높이가 부풀지 않도록, 접힌 행의 글자는 작고 높이는 줄 수로만 정해진다.
        let t = &themes()[0];
        let long = "x".repeat(400);
        let text = format!("| a | b |\n|---|---|\n| {long} | y |\n| s | t |\n");
        let job = build(&text, None, 15.0, t, 200.0, true);
        let sec = |needle: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(needle)).unwrap();
        let (a, b) = (sec(&long), sec("| s | t |"));
        assert_eq!(a.format.line_height, b.format.line_height, "행 높이는 글자 길이와 무관");
        assert!(a.format.font_id.size < 4.0, "투명 원문 글자는 감기지 않게 작다");
        assert!(a.format.line_height.unwrap() > 15.0);
    }

    #[test]
    fn real_table_with_many_br_keeps_height_and_text() {
        // 사용자가 보고한 표: 탭이 섞이고, 한 칸에 <br>이 5개(6줄)이며 한 줄이 매우 길다.
        let text = "|자료명|\t상세내역 (모든 심사 시 제출)|\n|---|---|\n|고객목록|\t(해당되는 경우 고객지정 요구사항 목록 제출)|\n|프로세스 맵|\t외주처리된 프로세스 및 원격 지원 기능 포함한 순서 및 상호작용을 보여주는 프로세스 기술<br>외주처리된 프로세스에 대한 정보*외주처리된 프로세스가 있을경우 식별(어떤 프로세스에서 관리되는지 식별)<br>외주처리된 프로세스가 없을경우, 외주처리된 프로세스가 없음을 식별<br>*원격지원기능 및 장소에 대한 정보<br>원격지원기능 주는곳의 주소, 프로세스, 기능/원격지원기능 받는곳의 주소, 프로세스, 기능)<br>*원격지원기능이 없을 경우, 원격지원기능이 없음을 식별|\n|요구사항 매트릭스\t|(CSR을 포함한) IATF 16949 요구사항이 조직의 프로세스에서 다루어졌다는 증거|\n";
        same(text, None);
        let t = &themes()[0];
        let heights = |wrap: f32| {
            let job = build(text, None, 15.0, t, wrap, true);
            let find = |needle: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(needle)).unwrap().format.line_height;
            (find("|고객목록|"), find("|프로세스 맵|"), find("|요구사항 매트릭스"))
        };
        let (a1, b1, c1) = heights(900.0);
        assert_eq!(heights(250.0), (a1, b1, c1), "창 폭과 무관");
        let (one, six) = (a1.unwrap(), b1.unwrap());
        assert_eq!(c1.unwrap(), one);
        assert!((six - (one + 5.0 * 15.0 * 1.3)).abs() < 0.01, "6줄 행 = 한 줄 높이 + 5줄: {one} {six}");
    }

    #[test]
    fn table_br_text_is_preserved() {
        let doc = "| a | b |\n|---|---|\n| 1<br>2 | x<BR/>y |\r\n| 3 | 4 |";
        same(doc, None);
        same(doc, Some(2));
    }

    #[test]
    fn append_to_image_row_only_on_image_lines() {
        let a = format!("![a](img:{H16} 8x6)");
        let b = format!("![b](img:aaaaaaaaaaaaaaaa 1x1)");
        assert_eq!(append_to_image_row(&format!("x\n{a}\ny"), 1, &b), Some(format!("x\n{a} {b}\ny")));
        assert_eq!(append_to_image_row(&format!("{a}\r\nz"), 0, &b), Some(format!("{a} {b}\r\nz"))); // CRLF 유지
        assert_eq!(append_to_image_row(&a, 0, &b), Some(format!("{a} {b}"))); // 마지막 줄(개행 없음)
        assert_eq!(append_to_image_row("x\ny", 0, &b), None); // 이미지 줄이 아니면 안 붙인다
        assert_eq!(append_to_image_row(&format!("글 {a}"), 0, &b), None);
        assert_eq!(append_to_image_row(&a, 5, &b), None); // 범위 밖
    }

    #[test]
    fn scan_blocks_finds_multi_image_row() {
        let l = format!("![a](img:{H16} 8x6) ![b](img:{H16} 8x6)");
        assert_eq!(kinds(&format!("a\n{l}\nb")), vec![("image".into(), 1, 1)]);
    }

    #[test]
    fn build_multi_image_row_height_is_tallest_scaled() {
        let t = &themes()[0];
        let text = format!("![a](img:{H16} 100x100) ![b](img:{H16} 100x300)\n");
        let job = build(&text, None, 15.0, t, 1000.0, true);
        assert_eq!(job.sections[0].format.line_height, Some(300.0 + IMAGE_PAD));
        let job = build(&text, None, 15.0, t, 108.0, true); // 간격 8 제외 폭 100 / 합 200 → 배율 0.5, 높이 150
        assert_eq!(job.sections[0].format.line_height, Some(150.0 + IMAGE_PAD));
        let job = build(&text, None, 15.0, t, 58.0, true); // 폭 50 → 배율 0.25
        assert_eq!(job.sections[0].format.line_height, Some(75.0 + IMAGE_PAD));
    }

    #[test]
    fn format_image_line_sanitizes_alt() {
        let s = format_image_line("a]b(c)\nd", H16, 8, 6);
        assert!(!s.contains('\n'));
        let r = parse_image_line(&s).unwrap();
        assert!(!r.alt.contains(['[', ']', '(', ')', '\n']));
        assert_eq!((r.hash.as_str(), r.w, r.h), (H16, 8, 6));
        let long = "가".repeat(70);
        assert_eq!(parse_image_line(&format_image_line(&long, H16, 8, 6)).unwrap().alt.chars().count(), 60);
    }

    #[test]
    fn scan_blocks_finds_image_only_on_own_line() {
        let img = format!("![x](img:{H16} 8x6)");
        assert_eq!(kinds(&format!("a\n{img}\nb")), vec![("image".into(), 1, 1)]);
        // 코드 블록·표 안은 이미지가 아니다
        assert_eq!(kinds(&format!("```\n{img}\n```")), vec![("code".into(), 0, 2)]);
        assert_eq!(kinds(&format!("| a | b |\n|---|---|\n| {img} | b |")), vec![("table".into(), 0, 2)]);
        // 줄 중간에 섞이면 블록이 아니다
        assert!(kinds(&format!("글 {img}")).is_empty());
    }

    #[test]
    fn image_refs_collects_unique_everywhere() {
        let t = format!("![a](img:{H16} 8x6)\n```\n(img:aaaaaaaaaaaaaaaa)\n```\n글 img:bbbbbbbbbbbbbbbb 끝 ![a](img:{H16} 8x6) img:zz");
        assert_eq!(image_refs(&t), vec![H16.to_string(), "aaaaaaaaaaaaaaaa".to_string(), "bbbbbbbbbbbbbbbb".to_string()]);
    }

    #[test]
    fn image_display_size_cases() {
        assert_eq!(image_display_size(800, 600, 400.0), (400.0, 300.0));
        assert_eq!(image_display_size(100, 100, 400.0), (100.0, 100.0)); // 키우지 않는다
        assert_eq!(image_display_size(500, 2000, 400.0), (175.0, 700.0)); // 높이 상한
        let (w, h) = image_display_size(800, 600, 0.0);
        assert!(w >= 1.0 && h >= 1.0);
    }

    #[test]
    fn build_gives_collapsed_image_line_its_height() {
        let t = &themes()[0];
        let text = format!("![a](img:{H16} 800x600)\n");
        let job = build(&text, None, 15.0, t, 400.0, true);
        assert_eq!(job.sections[0].format.line_height, Some(300.0 + IMAGE_PAD));
        // 커서가 있는 줄·구문 숨김 끔: 원문이 보이고 줄 높이는 그림 높이가 아니다
        for job in [build(&text, Some(0), 15.0, t, 400.0, true), build(&text, None, 15.0, t, 400.0, false)] {
            assert!(job.sections[0].format.line_height.map_or(true, |v| v < 100.0));
        }
    }

    #[test]
    fn image_line_text_is_preserved() {
        let doc = format!("a\n![x](img:{H16} 800x600)\nb\r\n![](img:{H16} 8x6)");
        same(&doc, None);
        same(&doc, Some(1));
    }

    #[test]
    fn img_scheme_is_not_external() {
        assert!(!is_external_target("img:0123456789abcdef 8x6"));
    }

    #[test]
    fn insert_block_line_pads_blank_lines() {
        assert_eq!(insert_block_line("a\nb", 0, "X"), "a\n\nX\n\nb");
        assert_eq!(insert_block_line("a\n\nb", 0, "X"), "a\n\nX\n\nb");
        assert_eq!(insert_block_line("a\n\nb", 1, "X"), "a\n\nX\n\nb");
        // 범위를 넘으면 끝에 붙는다(마지막 개행 유무 유지)
        assert_eq!(insert_block_line("a\nb", 99, "X"), "a\nb\n\nX");
        assert_eq!(insert_block_line("a\nb\n", 99, "X"), "a\nb\n\nX\n");
        assert_eq!(insert_block_line("", 0, "X"), "X");
    }

    #[test]
    fn code_block_ranges() {
        assert_eq!(kinds("a\n```rust\nlet x;\n```\nb"), vec![("code".into(), 1, 3)]);
        // 닫히지 않은 펜스는 문서 끝까지
        assert_eq!(kinds("```\nx\ny"), vec![("open".into(), 0, 2)]);
        // 따옴표 안의 펜스는 펜스가 아니다
        assert!(kinds("> ```\n> x").is_empty());
        // 코드 안의 `|`, `---`는 표·가로줄이 아니다
        assert_eq!(kinds("```\n| a |\n|---|\n---\n```"), vec![("code".into(), 0, 4)]);
        match &scan_blocks("```python  x\n```")[0].kind {
            BlockKind::Code { lang, .. } => assert_eq!(lang, "python"),
            _ => panic!(),
        }
    }

    #[test]
    fn table_detection() {
        assert_eq!(kinds("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nend"), vec![("table".into(), 0, 3)]);
        // 구분선이 없으면 표가 아니다
        assert!(kinds("| a | b |\n| 1 | 2 |").is_empty());
        // 열 수가 다르면 표가 아니다
        assert!(kinds("| a | b |\n|---|").is_empty());
        // 앞뒤 파이프 생략형
        assert_eq!(kinds("a | b\n--|--\n1 | 2"), vec![("table".into(), 0, 2)]);
        // 파이프 없는 `---`는 표 구분선이 아니라 가로줄
        assert_eq!(kinds("a | b\n---"), vec![("hr".into(), 1, 1)]);
        // 본문 줄의 칸 수가 달라도 열 수에 맞춘다
        if let BlockKind::Table { rows, .. } = &scan_blocks("| a | b |\n|---|---|\n| 1 |\n| 1 | 2 | 3 |")[0].kind {
            assert!(rows.iter().all(|r| r.len() == 2));
            assert_eq!(rows[1], vec!["1", ""]);
        } else {
            panic!();
        }
    }

    #[test]
    fn table_cells_and_alignment() {
        assert_eq!(split_cells("| a | b\\|c | d |"), vec!["a", "b|c", "d"]);
        assert_eq!(split_cells("a | b"), vec!["a", "b"]);
        assert_eq!(split_cells("| a |  |"), vec!["a", ""]);
        assert_eq!(parse_delim("|:---|:---:|---:|---|"), Some(vec![Align::Left, Align::Center, Align::Right, Align::Left]));
        assert_eq!(parse_delim("--|--"), Some(vec![Align::Left, Align::Left]));
        assert_eq!(parse_delim("---"), None);
        assert_eq!(parse_delim("| a | b |"), None);
        assert_eq!(parse_delim("|:|"), None);
    }

    #[test]
    fn hr_detection() {
        assert_eq!(kinds("a\n---\nb\n***\n___\n- - -"), vec![("hr".into(), 1, 1), ("hr".into(), 3, 3), ("hr".into(), 4, 4), ("hr".into(), 5, 5)]);
        assert!(kinds("--\n# ---\n> ---").is_empty());
    }

    #[test]
    fn list_continue_and_checkbox() {
        assert!(matches!(continue_list("- 항목"), Some(Enter::Insert(p)) if p == "- "));
        assert!(matches!(continue_list("  3. 항목"), Some(Enter::Insert(p)) if p == "  4. "));
        assert!(matches!(continue_list("- [x] 끝"), Some(Enter::Insert(p)) if p == "- [ ] "));
        assert!(matches!(continue_list("- "), Some(Enter::Clear)));
        assert!(matches!(continue_list("> "), Some(Enter::Clear)));
        assert!(matches!(continue_list("> 인용"), Some(Enter::Insert(p)) if p == "> "));
        assert!(continue_list("그냥 글").is_none());
        let t = "a\n- [ ] 할 일\n";
        let i = t.chars().position(|c| c == '[').unwrap();
        assert_eq!(toggle_checkbox_at(t, i + 1).unwrap(), "a\n- [x] 할 일\n");
        assert!(toggle_checkbox_at(t, 0).is_none());
        assert!(toggle_checkbox_at("```\n- [ ] x\n```", 7).is_none());
    }

    #[test]
    fn headings_for_toc() {
        let doc = "# 큰 제목\n본문\n## 둘째 **굵게** ##\n```\n# 코드 안은 제목 아님\n```\n###붙여쓴 제목\n####### 일곱개는 아님\n#\n### `코드` 제목\r\n마지막";
        let h = headings(doc);
        let got: Vec<(usize, &str, usize)> = h.iter().map(|x| (x.level, x.text.as_str(), x.line)).collect();
        assert_eq!(got, vec![(1, "큰 제목", 0), (2, "둘째 굵게", 2), (3, "붙여쓴 제목", 6), (3, "코드 제목", 9)]);
        assert!(headings("").is_empty());
        assert!(headings("그냥 글\n- 목록").is_empty());
        // 편집창 줄 번호와 같은 기준인지(줄 시작 문자 위치)
        assert_eq!(char_of_line(doc, 0), 0);
        assert_eq!(char_of_line(doc, 2), "# 큰 제목\n본문\n".chars().count());
        assert_eq!(line_of_char(doc, char_of_line(doc, 6)), 6);
        assert_eq!(char_of_line("a\nb", 9), 3);
    }

    #[test]
    fn wiki_link_helpers() {
        assert_eq!(wiki_links("a [[X]] b [[Y|보임]]\n```\n[[Z]]\n```"), vec!["X", "Y"]);
        let t = "앞 [[문서 A|글]] 뒤";
        let idx = t.chars().position(|c| c == 'A').unwrap();
        assert_eq!(wiki_at(t, idx).as_deref(), Some("문서 A"));
        assert_eq!(wiki_at(t, 0), None);
        // 외부 주소·경로는 문서 링크가 아니다
        assert_eq!(wiki_links("[[http://a.b/c]] [[C:\\x\\y.docx|파일]] [[www.a.kr]] [[\\\\srv\\d]] [[mailto:a@b]] [[^1]] [[진짜]]"), vec!["진짜"]);
        assert!(is_external_target("D:/a") && !is_external_target("문서: 제목") && !is_external_target("a:b"));
        let u = "[[https://x.y]]";
        assert_eq!(wiki_at(u, 5), None);
        assert_eq!(line_of_char("a\nb\nc", 4), 2);
    }
}

#[cfg(test)]
mod footnote_tests {
    use super::*;
    use crate::theme::themes;

    const DOC: &str = "본문 [^1]처럼 적고 [^긴이름] 도\n\n```\n[^1]: 코드 안\n```\n\n[^1]: 첫 설명 [^긴이름]\n[^긴이름]:   둘째\n[^ ] [^] [^a b]";

    #[test]
    fn text_is_preserved() {
        let t = &themes()[0];
        for hide in [true, false] {
            for cur in [None, Some(0), Some(7), Some(8)] {
                assert_eq!(build(DOC, cur, 15.0, t, 500.0, hide).text, DOC);
            }
        }
    }

    #[test]
    fn def_parsing() {
        assert_eq!(footnote_def("[^1]: 설명"), Some(("1", 6)));
        assert_eq!(footnote_def("[^a]:   x"), Some(("a", 8)));
        assert_eq!(footnote_def("[^a] x"), None);
        assert_eq!(footnote_def("[^]: x"), None);
        assert_eq!(footnote_def(" [^a]: x"), None);
    }

    #[test]
    fn jump_both_ways_and_skips_code() {
        let idx = |needle: &str, k: usize| DOC.char_indices().filter(|(b, _)| DOC[*b..].starts_with(needle)).nth(k).map(|(b, _)| DOC[..b].chars().count()).unwrap();
        // 참조 [^1] → 정의 줄(6), 코드 안의 정의(3)는 건너뜀
        assert_eq!(footnote_jump(DOC, idx("[^1]", 0) + 2), Some(6));
        // 정의 머리 → 첫 참조 줄(0)
        assert_eq!(footnote_jump(DOC, idx("[^1]:", 1) + 1), Some(0));
        // 이름 안쪽이 아닌 곳은 None
        assert_eq!(footnote_jump(DOC, 0), None);
        // 정의 없는 참조
        assert_eq!(footnote_jump("a [^x] b", 3), None);
    }
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn checkbox_lines_become_blocks_but_not_inside_code() {
        let b = scan_blocks("- [ ] a\n  * [x] b\n- 일반\n```\n- [ ] c\n```\n");
        let c: Vec<_> = b.iter().filter_map(|x| if let BlockKind::Check { done, col } = x.kind { Some((x.start, done, col)) } else { None }).collect();
        assert_eq!(c, vec![(0, false, 2), (1, true, 4)]);
    }
}


/// 글자 단위(char 번호) 일치 위치 목록 `(시작, 끝)`: 문서 안 찾기용. 겹치지 않게 앞에서부터 잡는다.
/// 대소문자를 구분하지 않으면 글자마다 소문자로 맞춰 비교한다(한글은 그대로).
pub fn find_all(text: &str, q: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return vec![];
    }
    let fold = |c: char| if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let hay: Vec<char> = text.chars().map(fold).collect();
    let nd: Vec<char> = q.chars().map(fold).collect();
    let mut out = vec![];
    let mut i = 0;
    while i + nd.len() <= hay.len() {
        if hay[i..i + nd.len()] == nd[..] {
            out.push((i, i + nd.len()));
            i += nd.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod find_tests {
    use super::find_all;

    #[test]
    fn finds_korean_and_ascii() {
        assert_eq!(find_all("가나다 가나 라", "가나", true), vec![(0, 2), (4, 6)]);
        assert_eq!(find_all("Hello hello HELLO", "hello", false), vec![(0, 5), (6, 11), (12, 17)]);
        assert_eq!(find_all("Hello hello HELLO", "hello", true), vec![(6, 11)]);
    }

    #[test]
    fn overlapping_matches_are_not_doubled() {
        assert_eq!(find_all("aaaa", "aa", true), vec![(0, 2), (2, 4)]);
        assert_eq!(find_all("aaa", "aa", true), vec![(0, 2)]);
    }

    #[test]
    fn empty_and_missing() {
        assert!(find_all("abc", "", true).is_empty());
        assert!(find_all("abc", "z", true).is_empty());
        assert!(find_all("", "a", true).is_empty());
    }

    #[test]
    fn indices_are_chars_not_bytes() {
        // 이모지·한글이 앞에 있어도 char 번호여야 편집창 커서 위치와 맞는다
        assert_eq!(find_all("😀한글 찾기", "찾기", true), vec![(4, 6)]);
    }
}

/// `N. ` 로 시작하는 숫자 목록 줄이면 (들여쓰기 글자 수, 번호, 번호 자릿수)를 돌려준다.
fn num_item(line: &str) -> Option<(usize, u64, usize)> {
    let ind = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let r = line.trim_start_matches([' ', '\t']);
    let digits = r.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 9 || !r[digits..].starts_with(". ") {
        return None;
    }
    Some((ind, r[..digits].parse().ok()?, digits))
}

/// 커서가 있는 숫자 목록 덩어리의 번호를 첫 항목 번호부터 1씩 늘어나게 고친다. 바꿀 것이 없으면 None.
/// - 덩어리: 같은 들여쓰기의 `N. ` 줄들. 빈 줄이나 들여쓰기가 얕은 줄·번호 없는 같은 수준 줄에서 끊기고,
///   더 깊이 들여쓴 줄(하위 목록·이어지는 문단)은 건너뛴다.
/// - 코드 블록 안은 건드리지 않는다. 첫 항목 번호는 사용자가 정한 대로 둔다(3. 부터 시작하면 3, 4, 5…).
/// - 돌려주는 값: (새 본문, 새 커서 char 번호). 커서는 같은 글자 앞에 머물도록 보정한다.
pub fn renumber(text: &str, cursor: usize) -> Option<(String, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut fence = vec![false; lines.len()];
    let mut inf = false;
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with("```") {
            inf = !inf;
            fence[i] = true;
        } else {
            fence[i] = inf;
        }
    }
    let item = |i: usize| if fence[i] { None } else { num_item(lines[i]) };
    let cl = line_of_char(text, cursor).min(lines.len() - 1);
    let target = if item(cl).is_some() { cl } else if cl > 0 && item(cl - 1).is_some() { cl - 1 } else { return None };
    let ind = item(target)?.0;
    #[derive(PartialEq)]
    enum K {
        Item,
        Sub,
        Break,
    }
    let class = |j: usize| -> K {
        let l = lines[j];
        if l.trim().is_empty() {
            return K::Break;
        }
        if let Some((i, ..)) = item(j) {
            if i == ind {
                return K::Item;
            }
        }
        let lead = l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        if lead > ind { K::Sub } else { K::Break }
    };
    let mut items = vec![target];
    let mut j = target;
    while j > 0 {
        j -= 1;
        match class(j) {
            K::Item => items.push(j),
            K::Sub => {}
            K::Break => break,
        }
    }
    items.reverse();
    let mut j = target + 1;
    while j < lines.len() {
        match class(j) {
            K::Item => items.push(j),
            K::Sub => {}
            K::Break => break,
        }
        j += 1;
    }
    let start = item(items[0])?.1;
    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut changed = false;
    for (k, &li) in items.iter().enumerate() {
        let (ind_chars, num, digits) = item(li)?;
        let want = start + k as u64;
        if num != want {
            let l = lines[li];
            let byte_ind = l.len() - l.trim_start_matches([' ', '\t']).len();
            new_lines[li] = format!("{}{}{}", &l[..byte_ind], want, &l[byte_ind + digits..]);
            let _ = ind_chars;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    // 커서 보정: 앞 줄들의 길이 변화 + (커서 줄이 바뀐 번호 줄이고 번호 뒤에 있으면) 그 줄의 길이 변화
    let mut delta: isize = 0;
    for (i, l) in lines.iter().enumerate().take(cl) {
        delta += new_lines[i].chars().count() as isize - l.chars().count() as isize;
    }
    let line_start: usize = lines.iter().take(cl).map(|l| l.chars().count() + 1).sum();
    let col = cursor.saturating_sub(line_start);
    if new_lines[cl] != lines[cl] {
        let (ic, _, d) = item(cl)?;
        let nd = new_lines[cl].trim_start_matches([' ', '\t']).bytes().take_while(|b| b.is_ascii_digit()).count();
        if col >= ic + d {
            delta += nd as isize - d as isize;
        } else if col > ic {
            delta += (col - ic).min(nd) as isize - (col - ic) as isize;
        }
    }
    let new_cursor = (cursor as isize + delta).max(0) as usize;
    Some((new_lines.join("\n"), new_cursor))
}

#[cfg(test)]
mod renumber_tests {
    use super::renumber;

    fn run(t: &str, c: usize) -> Option<(String, usize)> {
        renumber(t, c)
    }

    #[test]
    fn fixes_after_inserted_item() {
        let (t, _) = run("1. a\n2. b\n2. c\n3. d", 12).unwrap();
        assert_eq!(t, "1. a\n2. b\n3. c\n4. d");
    }

    #[test]
    fn already_correct_is_none() {
        assert!(run("1. a\n2. b\n3. c", 3).is_none());
        assert!(run("메모\n그냥 글", 0).is_none());
    }

    #[test]
    fn keeps_user_start_number() {
        let (t, _) = run("3. a\n3. b\n3. c", 6).unwrap();
        assert_eq!(t, "3. a\n4. b\n5. c");
    }

    #[test]
    fn blank_line_splits_blocks() {
        let (t, _) = run("1. a\n3. b\n\n1. x\n1. y", 6).unwrap();
        assert_eq!(t, "1. a\n2. b\n\n1. x\n1. y", "아래 덩어리는 커서가 없으므로 그대로");
    }

    #[test]
    fn nested_list_is_separate_and_skipped() {
        let src = "1. a\n   1. sub\n   1. sub2\n3. b";
        let (t, _) = run(src, 0).unwrap();
        assert_eq!(t, "1. a\n   1. sub\n   1. sub2\n2. b", "깊은 들여쓰기 줄은 건너뛰고 바깥 번호만 고친다");
        let (t2, _) = run(src, 10).unwrap();
        assert_eq!(t2, "1. a\n   1. sub\n   2. sub2\n3. b", "커서가 하위 목록에 있으면 하위 덩어리만");
    }

    #[test]
    fn code_block_untouched() {
        assert!(run("```\n1. a\n5. b\n```", 6).is_none());
    }

    #[test]
    fn cursor_follows_digit_growth() {
        // 9 → 10으로 자릿수가 늘면 커서가 같은 글자 앞에 있어야 한다
        let src = "8. a\n8. b\n8. c\n8. d";
        let cur = src.chars().count(); // 맨 끝
        let (t, c) = run(src, cur).unwrap();
        assert_eq!(t, "8. a\n9. b\n10. c\n11. d");
        assert_eq!(c, t.chars().count());
    }

    #[test]
    fn after_enter_on_last_item() {
        // "1. a\n2. b\n3. " 처럼 이어 쓴 뒤 중간 항목을 지운 상태
        let (t, _) = run("1. a\n3. ", 8).unwrap();
        assert_eq!(t, "1. a\n2. ");
    }
}


/// 문단(줄)마다 목록 줄이면 "글이 시작하는 글자 수(기호·체크박스 포함)"를, 아니면 None. 코드 블록 안은 제외.
/// 함께 가장 긴 기호 글자 수(없으면 0)를 돌려준다.
pub fn hang_info(text: &str) -> (Vec<Option<usize>>, usize) {
    let mut out = Vec::new();
    let mut maxm = 0;
    let mut in_fence = false;
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push(None);
            continue;
        }
        let m = if in_fence { None } else { list_marker_len(line) };
        let n = m.map(|m| {
            let rest = &line[m..];
            let chk = ["[ ] ", "[x] ", "[X] "].iter().any(|p| rest.starts_with(p));
            line[..m].chars().count() + if chk { 4 } else { 0 }
        });
        if let Some(n) = n {
            maxm = maxm.max(n);
        }
        out.push(n);
    }
    (out, maxm)
}

/// 이어지는 행이 밀려나는 폭 중 가장 큰 값(예상 폭과 비교해 줄 폭을 다시 정하는 데 쓴다).
pub fn max_hang_shift(g: &eframe::egui::Galley, hangs: &[Option<usize>]) -> f32 {
    let mut para = 0usize;
    let mut first_of_para = true;
    let mut best = 0.0f32;
    for pr in g.rows.iter() {
        if first_of_para {
            let s = hangs.get(para).copied().flatten().and_then(|m| pr.row.glyphs.get(m)).map_or(0.0, |gl| gl.pos.x);
            best = best.max(s);
        }
        first_of_para = pr.ends_with_newline;
        if pr.ends_with_newline {
            para += 1;
        }
    }
    best
}

/// 목록 줄이 줄바꿈될 때 다음 행을 글이 시작하는 자리(기호 뒤)에 맞춰 들여쓴다(HTML의 `<li>`처럼).
/// egui는 첫 행만 들여쓸 수 있어, 배치가 끝난 갤러리의 이어지는 행 위치를 옮긴다(글자 위치·클릭·선택은 같은 갤러리를 쓰므로 맞는다).
pub fn apply_hang(g: &eframe::egui::Galley, hangs: &[Option<usize>]) -> eframe::egui::Galley {
    let mut g = g.clone();
    let mut para = 0usize;
    let mut first_of_para = true;
    let mut shift = 0.0f32;
    for pr in g.rows.iter_mut() {
        if first_of_para {
            shift = hangs.get(para).copied().flatten().and_then(|m| pr.row.glyphs.get(m)).map_or(0.0, |gl| gl.pos.x);
        } else if shift > 0.0 {
            pr.pos.x += shift;
        }
        first_of_para = pr.ends_with_newline;
        if pr.ends_with_newline {
            para += 1;
        }
    }
    g
}
#[cfg(test)]
mod code_span_link_tests {
    use super::*;

    #[test]
    fn wiki_links_skip_inline_code() {
        let t = "본문 [[진짜]] 와 `[[코드]]` 그리고 `a` [[또 진짜|글]]\n```\n[[펜스]]\n```";
        assert_eq!(wiki_links(t), vec!["진짜".to_string(), "또 진짜".to_string()]);
    }
}
