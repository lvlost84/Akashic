//! 전체 내보내기: 분류 트리를 폴더로, 문서를 YAML front matter가 붙은 .md 파일로 쓴다.
use crate::db::{Category, ExportDoc, MAX_DEPTH};
use chrono::{Local, SecondsFormat, TimeZone};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 경로(글자 수) 상한.
pub const PATH_LIMIT: usize = 240;
const UNTITLED: &str = "제목없음";

pub struct ExportResult {
    pub dir: PathBuf,
    pub ok: usize,
    pub failed: usize,
    pub first_error: Option<String>,
}

// ---------------------------------------------------------------- 파일 이름
const RESERVED: [&str; 30] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM\u{b9}", "COM\u{b2}", "COM\u{b3}", "LPT\u{b9}", "LPT\u{b2}", "LPT\u{b3}",
    "CONIN$", "CONOUT$",
];

fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_uppercase();
    RESERVED.contains(&stem.as_str())
}

/// Windows에서 쓸 수 있는 이름 한 토막으로 만든다(확장자 없음). 한글은 그대로 둔다.
pub fn sanitize_name(s: &str) -> String {
    let mut t: String = s
        .chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) || c == '\u{2028}' || c == '\u{2029}' { '_' } else { c })
        .collect();
    t = t.trim().trim_end_matches(|c| c == '.' || c == ' ').trim_start().to_string();
    if t.is_empty() {
        t = UNTITLED.to_string();
    }
    if is_reserved(&t) {
        t.insert(0, '_');
    }
    t
}

/// 글자(유니코드 스칼라) 단위로 `max`자까지만 남긴다. 잘린 뒤 끝의 점·공백은 다시 정리.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max.max(1)).collect();
    let t = t.trim_end_matches(|c| c == '.' || c == ' ').to_string();
    if t.is_empty() { UNTITLED.chars().take(max.max(1)).collect() } else { t }
}

/// 문서의 파일 이름 바탕: 제목, 비면 본문 첫 줄, 그것도 비면 "제목없음".
pub fn doc_stem(title: &str, body: &str) -> String {
    let t = title.trim();
    if !t.is_empty() {
        return sanitize_name(t);
    }
    let first = body.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("");
    sanitize_name(first)
}

/// 한 폴더 안에서 대소문자 무시하고 겹치지 않는 이름을 고른다(`이름 (2)`…).
pub struct Namer {
    used: HashSet<String>,
}
impl Namer {
    fn new() -> Namer {
        Namer { used: HashSet::new() }
    }
    /// `stem`(최대 `budget`자)에 `ext`를 붙인 이름을 예약해 돌려준다.
    fn take(&mut self, stem: &str, ext: &str, budget: usize) -> String {
        let base = truncate_chars(stem, budget.max(1));
        let mut n = 1;
        loop {
            let cand = if n == 1 {
                base.clone()
            } else {
                let suffix = format!(" ({n})");
                let keep = budget.saturating_sub(suffix.chars().count()).max(1);
                format!("{}{}", truncate_chars(stem, keep), suffix)
            };
            let full = format!("{cand}{ext}");
            if self.used.insert(full.to_lowercase()) {
                return full;
            }
            n += 1;
        }
    }
}

// ---------------------------------------------------------------- 경로 계획
/// 분류 id → 상대 폴더 경로(최대 `MAX_DEPTH`단계). 같은 상위 안의 이름 충돌도 해결한다.
pub fn folder_paths(cats: &[Category], root_len: usize) -> (HashMap<i64, PathBuf>, HashMap<PathBuf, Namer>) {
    let by_id: HashMap<i64, &Category> = cats.iter().map(|c| (c.id, c)).collect();
    // 실제 트리 깊이로 폴더 이름 상한을 정한다.
    let depth_of = |mut id: i64| {
        let mut d = 0;
        let mut seen = HashSet::new();
        while let Some(c) = by_id.get(&id) {
            if !seen.insert(id) || d >= MAX_DEPTH {
                break;
            }
            d += 1;
            match c.parent {
                Some(p) => id = p,
                None => break,
            }
        }
        d
    };
    let max_depth = cats.iter().map(|c| depth_of(c.id)).max().unwrap_or(1).max(1);
    let cap = (PATH_LIMIT.saturating_sub(root_len + 24) / max_depth).clamp(8, 80);

    let mut paths: HashMap<i64, PathBuf> = HashMap::new();
    let mut namers: HashMap<PathBuf, Namer> = HashMap::new();
    let mut ids: Vec<i64> = cats.iter().map(|c| c.id).collect();
    ids.sort();
    fn resolve(
        id: i64,
        by_id: &HashMap<i64, &Category>,
        cap: usize,
        paths: &mut HashMap<i64, PathBuf>,
        namers: &mut HashMap<PathBuf, Namer>,
        stack: &mut Vec<i64>,
    ) -> PathBuf {
        if let Some(p) = paths.get(&id) {
            return p.clone();
        }
        let c = by_id[&id];
        stack.push(id);
        let parent_path = match c.parent {
            Some(p) if by_id.contains_key(&p) && !stack.contains(&p) && stack.len() < MAX_DEPTH => {
                resolve(p, by_id, cap, paths, namers, stack)
            }
            _ => PathBuf::new(),
        };
        stack.pop();
        let nm = namers.entry(parent_path.clone()).or_insert_with(Namer::new);
        let name = nm.take(&sanitize_name(&c.name), "", cap);
        let p = parent_path.join(name);
        paths.insert(id, p.clone());
        p
    }
    for id in ids {
        let mut stack = vec![];
        resolve(id, &by_id, cap, &mut paths, &mut namers, &mut stack);
    }
    (paths, namers)
}

/// 문서마다 상대 경로(`폴더/이름.md`)를 정한다. 결과는 `docs`와 같은 순서.
pub fn plan_paths(docs: &[ExportDoc], cats: &[Category], root_len: usize) -> Vec<PathBuf> {
    let (folders, mut namers) = folder_paths(cats, root_len);
    docs.iter()
        .map(|d| {
            let dir = d.category.and_then(|c| folders.get(&c).cloned()).unwrap_or_default();
            let dir_len: usize = dir.to_string_lossy().chars().count();
            // 루트 경로 + 구분자 + 폴더 + 구분자 + 이름 + ".md" (겹침 접미사는 이름 몫에 포함)
            let budget = PATH_LIMIT.saturating_sub(root_len + 1 + dir_len + 1 + 3).max(1);
            let nm = namers.entry(dir.clone()).or_insert_with(Namer::new);
            let file = nm.take(&doc_stem(&d.title, &d.body), ".md", budget);
            dir.join(file)
        })
        .collect()
}

// ---------------------------------------------------------------- front matter
/// YAML 한 줄 값: 안전하면 그대로, 아니면 큰따옴표와 이스케이프.
pub fn yaml_str(s: &str) -> String {
    let lower = s.to_lowercase();
    let reserved = matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "y" | "n" | "~")
        || s.parse::<f64>().is_ok();
    let first_ok = s.chars().next().map_or(false, |c| c.is_alphanumeric());
    let plain = !s.is_empty()
        && !reserved
        && first_ok
        && !s.ends_with(' ')
        && s.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '/' | ','));
    if plain {
        return s.to_string();
    }
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn iso(ts: i64) -> String {
    Local.timestamp_opt(ts, 0).single().map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, false)).unwrap_or_default()
}

/// 분류 경로 "품질 / 측정기".
pub fn category_path(cats: &[Category], id: Option<i64>) -> String {
    let by_id: HashMap<i64, &Category> = cats.iter().map(|c| (c.id, c)).collect();
    let mut names = vec![];
    let mut cur = id;
    while let Some(i) = cur {
        let Some(c) = by_id.get(&i) else { break };
        names.push(c.name.clone());
        if names.len() > MAX_DEPTH {
            break;
        }
        cur = c.parent;
    }
    names.reverse();
    names.join(" / ")
}

/// 본문의 이미지 줄(`![대체글](img:해시 가로x세로)`)을 내보낸 파일 기준 상대경로
/// `![대체글](../…/images/해시.webp)`로 바꾼다(`depth` = 문서가 내보내기 폴더 아래 몇 칸 깊이인지).
/// 코드 블록·표 안이나 줄 중간의 것은 이미지가 아니므로 건드리지 않는다. 줄바꿈은 그대로 둔다.
/// 쓰인 (해시, 대체글) 목록(중복 없음)을 함께 돌려준다.
pub fn rewrite_images(body: &str, depth: usize) -> (String, Vec<(String, String)>) {
    let imgs: HashMap<usize, (String, String)> = crate::md::scan_blocks(body)
        .into_iter()
        .filter_map(|b| match b.kind {
            crate::md::BlockKind::Image { alt, hash, .. } => Some((b.start, (alt, hash))),
            _ => None,
        })
        .collect();
    if imgs.is_empty() {
        return (body.to_string(), Vec::new());
    }
    let prefix = "../".repeat(depth);
    let mut out = String::with_capacity(body.len());
    let mut used: Vec<(String, String)> = Vec::new();
    for (i, raw) in body.split_inclusive('\n').enumerate() {
        match imgs.get(&i) {
            Some((alt, hash)) => {
                let eol = &raw[raw.trim_end_matches(['\r', '\n']).len()..];
                out.push_str(&format!("![{alt}]({prefix}images/{hash}.webp){eol}"));
                if !used.iter().any(|(h, _)| h == hash) {
                    used.push((hash.clone(), alt.clone()));
                }
            }
            None => out.push_str(raw),
        }
    }
    (out, used)
}

/// 파일 전체 내용: front matter + 본문(원문 그대로).
pub fn render(d: &ExportDoc, cats: &[Category]) -> String {
    render_body(d, cats, &d.body)
}

fn render_body(d: &ExportDoc, cats: &[Category], body: &str) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("title: {}\n", yaml_str(&d.title)));
    if d.tags.is_empty() {
        s.push_str("tags: []\n");
    } else {
        s.push_str("tags:\n");
        for t in &d.tags {
            s.push_str(&format!("  - {}\n", yaml_str(t)));
        }
    }
    let cp = category_path(cats, d.category);
    s.push_str(&format!("category: {}\n", if cp.is_empty() { "\"\"".to_string() } else { yaml_str(&cp) }));
    s.push_str(&format!("created: {}\nupdated: {}\npinned: {}\n---\n", iso(d.created), iso(d.updated), d.pinned));
    s.push_str(body);
    s
}

// ---------------------------------------------------------------- 쓰기
/// `parent` 안에 `아카식-내보내기-<stamp>` 폴더를 만들고 모든 문서를 쓴다.
/// `load_image(해시)`는 그 그림의 WebP 바이트를 돌려준다(없으면 None). 쓰인 그림은 `images/해시.webp`로 한 번씩 쓴다.
pub fn run_export(parent: &Path, stamp: &str, docs: &[ExportDoc], cats: &[Category], load_image: &dyn Fn(&str) -> Option<Vec<u8>>) -> ExportResult {
    let mut dir = parent.join(format!("아카식-내보내기-{stamp}"));
    let mut n = 2;
    while dir.exists() {
        dir = parent.join(format!("아카식-내보내기-{stamp} ({n})"));
        n += 1;
    }
    let mut res = ExportResult { dir: dir.clone(), ok: 0, failed: 0, first_error: None };
    if let Err(x) = std::fs::create_dir_all(&dir) {
        res.failed = docs.len();
        res.first_error = Some(format!("폴더를 만들 수 없습니다: {x}"));
        return res;
    }
    let root_len = dir.to_string_lossy().chars().count();
    let rels = plan_paths(docs, cats, root_len);
    let mut hashes: Vec<String> = Vec::new(); // 쓰인 그림(처음 나온 순서, 중복 없음)
    for (d, rel) in docs.iter().zip(rels) {
        let full = dir.join(&rel);
        let depth = rel.components().count().saturating_sub(1);
        let (body, used) = rewrite_images(&d.body, depth);
        for (h, _) in used {
            if !hashes.contains(&h) {
                hashes.push(h);
            }
        }
        let r = full
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&full, render_body(d, cats, &body).as_bytes()));
        match r {
            Ok(()) => res.ok += 1,
            Err(x) => {
                res.failed += 1;
                res.first_error.get_or_insert(format!("{}: {x}", rel.display()));
            }
        }
    }
    if !hashes.is_empty() {
        let imgdir = dir.join("images");
        match std::fs::create_dir_all(&imgdir) {
            Err(x) => {
                res.first_error.get_or_insert(format!("images 폴더를 만들 수 없습니다: {x}"));
            }
            Ok(()) => {
                for h in &hashes {
                    match load_image(h) {
                        Some(bytes) => {
                            if let Err(x) = std::fs::write(imgdir.join(format!("{h}.webp")), bytes) {
                                res.first_error.get_or_insert(format!("이미지 {h}: {x}"));
                            }
                        }
                        None => {
                            res.first_error.get_or_insert(format!("이미지 {h}를 DB에서 읽지 못했습니다"));
                        }
                    }
                }
            }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: i64, parent: Option<i64>, name: &str) -> Category {
        Category { id, parent, name: name.into(), count: 0 }
    }
    fn doc(id: i64, title: &str, body: &str, category: Option<i64>) -> ExportDoc {
        ExportDoc { id, title: title.into(), body: body.into(), category, tags: vec![], pinned: false, created: 1_700_000_000, updated: 1_700_000_100 }
    }

    #[test]
    fn sanitize_forbidden_reserved_trailing() {
        assert_eq!(sanitize_name("a\\b/c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_name("줄\n바꿈\t탭"), "줄_바꿈_탭");
        assert_eq!(sanitize_name("보고서. . "), "보고서");
        assert_eq!(sanitize_name("   "), "제목없음");
        assert_eq!(sanitize_name("..."), "제목없음");
        assert_eq!(sanitize_name("con"), "_con");
        assert_eq!(sanitize_name("NUL.txt"), "_NUL.txt");
        assert_eq!(sanitize_name("com9"), "_com9");
        assert_eq!(sanitize_name("LPT1"), "_LPT1");
        assert_eq!(sanitize_name("COM10"), "COM10");
        assert_eq!(sanitize_name("품질 / 측정기: 점검"), "품질 _ 측정기_ 점검");
        assert_eq!(doc_stem("", "\n# 첫 줄 제목\n본문"), "첫 줄 제목");
        assert_eq!(doc_stem("  ", " \n\n"), "제목없음");
    }

    #[test]
    fn truncate_is_char_safe() {
        let s = "가나다라마바사아자차";
        assert_eq!(truncate_chars(s, 4), "가나다라");
        assert_eq!(truncate_chars(s, 100), s);
        assert_eq!(truncate_chars("가나. . .", 4), "가나");
        let long = "한".repeat(500);
        let p = plan_paths(&[doc(1, &long, "", None)], &[], 100);
        let len = 100 + 1 + p[0].to_string_lossy().chars().count();
        assert!(len <= PATH_LIMIT, "{len}");
        assert!(p[0].to_string_lossy().ends_with(".md"));
    }

    #[test]
    fn collisions_case_insensitive() {
        let docs: Vec<_> = (0..5).map(|i| doc(i, if i % 2 == 0 { "Same" } else { "SAME" }, "", None)).collect();
        let names: Vec<String> = plan_paths(&docs, &[], 50).iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["Same.md", "SAME (2).md", "Same (3).md", "SAME (4).md", "Same (5).md"]);
        // 겹침 접미사를 붙이고도 길이 한도 안
        let long = "가".repeat(400);
        let docs: Vec<_> = (0..12).map(|i| doc(i, &long, "", None)).collect();
        let ps = plan_paths(&docs, &[], 100);
        let set: HashSet<String> = ps.iter().map(|p| p.to_string_lossy().to_lowercase()).collect();
        assert_eq!(set.len(), 12);
        assert!(ps.iter().all(|p| 101 + p.to_string_lossy().chars().count() <= PATH_LIMIT));
    }

    #[test]
    fn yaml_escaping() {
        assert_eq!(yaml_str("품질 보고서"), "품질 보고서");
        assert_eq!(yaml_str("a: b"), "\"a: b\"");
        assert_eq!(yaml_str("# 제목"), "\"# 제목\"");
        assert_eq!(yaml_str("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(yaml_str("줄\n바꿈"), "\"줄\\n바꿈\"");
        assert_eq!(yaml_str("true"), "\"true\"");
        assert_eq!(yaml_str("123"), "\"123\"");
        assert_eq!(yaml_str(""), "\"\"");
        assert_eq!(yaml_str("끝공백 "), "\"끝공백 \"");
        assert_eq!(yaml_str("a\\b"), "\"a\\\\b\"");
        assert_eq!(yaml_str("\u{1}"), "\"\\u0001\"");
        let d = ExportDoc { tags: vec!["중요".into(), "a:b".into()], pinned: true, ..doc(1, "제목: 하나", "본문\r\n둘째", None) };
        let r = render(&d, &[]);
        assert!(r.starts_with("---\ntitle: \"제목: 하나\"\ntags:\n  - 중요\n  - \"a:b\"\ncategory: \"\"\ncreated: 20"));
        assert!(r.contains("pinned: true\n---\n본문\r\n둘째"));
    }

    #[test]
    fn six_level_folders() {
        let mut cats = vec![];
        for i in 0..MAX_DEPTH as i64 {
            cats.push(cat(i + 1, if i == 0 { None } else { Some(i) }, &format!("L{}", i + 1)));
        }
        cats.push(cat(10, Some(1), "L2")); // 같은 상위, 같은 이름(대소문자 무시)이 정리 후 생기는 경우
        cats.push(cat(11, None, "a:b"));
        cats.push(cat(12, None, "a_b"));
        let (f, _) = folder_paths(&cats, 60);
        assert_eq!(f[&6], PathBuf::from("L1").join("L2").join("L3").join("L4").join("L5").join("L6"));
        assert_eq!(f[&10], PathBuf::from("L1").join("L2 (2)"));
        assert_eq!(f[&11], PathBuf::from("a_b"));
        assert_eq!(f[&12], PathBuf::from("a_b (2)"));
        let docs = vec![doc(1, "깊은 문서", "", Some(6)), doc(2, "루트 문서", "", None)];
        let p = plan_paths(&docs, &cats, 60);
        assert_eq!(p[0], f[&6].join("깊은 문서.md"));
        assert_eq!(p[1], PathBuf::from("루트 문서.md"));
        assert_eq!(category_path(&cats, Some(3)), "L1 / L2 / L3");
    }

    #[test]
    fn write_and_read_back() {
        let tmp = std::env::temp_dir().join(format!("akashic-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let cats = vec![cat(1, None, "품질"), cat(2, Some(1), "측정기")];
        let mut docs: Vec<ExportDoc> = (0..5).map(|i| doc(i, "중복 제목", &format!("본문 {i}\n둘째 줄\r\n"), Some(2))).collect();
        docs.push(doc(10, "", "# 첫 줄이 제목\n내용", None));
        docs.push(doc(11, "", "", None));
        docs.push(doc(12, "콜론: 있는 제목/슬래시", "한글 본문 ✓", Some(1)));
        docs[0].tags = vec!["예시".into(), "x y".into()];
        docs[0].pinned = true;
        let r = run_export(&tmp, "20260101-000000", &docs, &cats, &|_| None);
        assert_eq!((r.ok, r.failed), (8, 0), "{:?}", r.first_error);
        assert_eq!(r.dir.file_name().unwrap().to_string_lossy(), "아카식-내보내기-20260101-000000");
        let sub = r.dir.join("품질").join("측정기");
        let mut names: Vec<String> = std::fs::read_dir(&sub).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["중복 제목 (2).md", "중복 제목 (3).md", "중복 제목 (4).md", "중복 제목 (5).md", "중복 제목.md"]);
        // 본문 왕복(front matter 뒤가 원문 그대로, BOM 없음, CRLF 보존)
        for (i, d) in docs.iter().take(5).enumerate() {
            let fname = if i == 0 { "중복 제목.md".to_string() } else { format!("중복 제목 ({}).md", i + 1) };
            let bytes = std::fs::read(sub.join(fname)).unwrap();
            assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
            let text = String::from_utf8(bytes).unwrap();
            let rest = text.strip_prefix("---\n").unwrap();
            let end = rest.find("\n---\n").unwrap();
            let (fm, body) = (&rest[..end], &rest[end + 5..]);
            assert_eq!(body, d.body);
            assert!(fm.contains("title: 중복 제목\n"));
            assert!(fm.contains("category: 품질 / 측정기\n"));
        }
        let first = std::fs::read_to_string(sub.join("중복 제목.md")).unwrap();
        assert!(first.contains("tags:\n  - 예시\n  - x y\n"));
        assert!(first.contains("pinned: true\n"));
        assert_eq!(std::fs::read_to_string(r.dir.join("첫 줄이 제목.md")).unwrap().split("---\n").last().unwrap(), "# 첫 줄이 제목\n내용");
        assert!(r.dir.join("제목없음.md").exists());
        assert_eq!(std::fs::read_to_string(r.dir.join("품질").join("콜론_ 있는 제목_슬래시.md")).unwrap().ends_with("한글 본문 ✓"), true);
        // 같은 시각에 다시 내보내면 새 폴더
        let r2 = run_export(&tmp, "20260101-000000", &docs[..1], &cats, &|_| None);
        assert_ne!(r2.dir, r.dir);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    const H: &str = "0123456789abcdef";

    fn line() -> String {
        format!("![x](img:{H} 8x6)")
    }

    #[test]
    fn rewrite_images_depth() {
        let (s, used) = rewrite_images(&format!("{}\n", line()), 0);
        assert_eq!(s, format!("![x](images/{H}.webp)\n"));
        assert_eq!(used, vec![(H.to_string(), "x".to_string())]);
        assert_eq!(rewrite_images(&format!("{}\n", line()), 2).0, format!("![x](../../images/{H}.webp)\n"));
    }

    #[test]
    fn rewrite_images_skips_code_and_inline() {
        let body = format!("```\n{}\n```\n글 {} 끝\n", line(), line());
        let (s, used) = rewrite_images(&body, 1);
        assert_eq!(s, body);
        assert!(used.is_empty());
    }

    #[test]
    fn rewrite_images_keeps_crlf() {
        let (s, _) = rewrite_images(&format!("a\r\n{}\r\nb", line()), 0);
        assert_eq!(s, format!("a\r\n![x](images/{H}.webp)\r\nb"));
    }

    fn doc(id: i64, body: &str, category: Option<i64>) -> ExportDoc {
        ExportDoc { id, title: format!("문서{id}"), body: body.into(), category, tags: vec![], pinned: false, created: 1_700_000_000, updated: 1_700_000_100 }
    }

    #[test]
    fn run_export_writes_shared_image_once() {
        let tmp = std::env::temp_dir().join(format!("akashic-export-img-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let cats = vec![Category { id: 1, parent: None, name: "분류".into(), count: 0 }];
        let docs = vec![doc(1, &format!("{}\n", line()), None), doc(2, &format!("{}\n", line()), Some(1))];
        let calls = std::cell::Cell::new(0);
        let r = run_export(&tmp, "20260101-000000", &docs, &cats, &|h| {
            calls.set(calls.get() + 1);
            (h == H).then(|| vec![1, 2, 3])
        });
        assert_eq!((r.ok, r.failed), (2, 0), "{:?}", r.first_error);
        assert!(r.first_error.is_none());
        let imgs: Vec<_> = std::fs::read_dir(r.dir.join("images")).unwrap().collect();
        assert_eq!(imgs.len(), 1);
        assert_eq!(std::fs::read(r.dir.join("images").join(format!("{H}.webp"))).unwrap(), vec![1, 2, 3]);
        assert_eq!(calls.get(), 1); // 같은 그림은 한 번만 읽는다
        let a = std::fs::read_to_string(r.dir.join("문서1.md")).unwrap();
        let b = std::fs::read_to_string(r.dir.join("분류").join("문서2.md")).unwrap();
        assert!(a.ends_with(&format!("![x](images/{H}.webp)\n")));
        assert!(b.ends_with(&format!("![x](../images/{H}.webp)\n")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_export_missing_image_is_reported() {
        let tmp = std::env::temp_dir().join(format!("akashic-export-img2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let docs = vec![doc(1, &format!("{}\n", line()), None)];
        let r = run_export(&tmp, "20260101-000000", &docs, &[], &|_| None);
        assert_eq!((r.ok, r.failed), (1, 0));
        assert!(r.first_error.is_some());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod reserved_extra_tests {
    use super::*;

    #[test]
    fn superscript_and_console_names_are_reserved() {
        for n in ["COM\u{b9}", "lpt\u{b2}.txt", "CONIN$", "conout$"] {
            assert!(is_reserved(n), "{n}");
        }
        assert!(!is_reserved("COM10"));
    }
}
