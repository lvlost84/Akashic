//! 폴더 가져오기: 다른 노트 앱(Obsidian·Notion·일반 마크다운 폴더)이나 아카식 내보내기 폴더를 읽는다.
//! 하위 폴더는 분류가 되고, 맨 앞 YAML(front matter)의 title·tags·pinned를 읽는다. 이 파일은 DB를 건드리지 않는다.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
}

#[derive(Debug)]
pub struct Item {
    /// 루트 아래 폴더 이름들(분류 경로)
    pub dirs: Vec<String>,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    /// 이 문서가 있던 폴더(그림 상대 경로 기준)
    pub src_dir: PathBuf,
}

#[derive(Default)]
pub struct Scan {
    pub items: Vec<Item>,
    /// 이름(소문자) → 그림 파일(Obsidian의 ![[그림.png]]를 찾는 데 씀)
    pub images: HashMap<String, PathBuf>,
    pub failed: usize,
}

pub const IMAGE_EXT: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        let inner = &s[1..s.len() - 1];
        if s.starts_with('"') { inner.replace("\\\"", "\"").replace("\\\\", "\\") } else { inner.replace("''", "'") }
    } else {
        s.to_string()
    }
}

/// 맨 앞 `---` … `---` 블록을 읽고 나머지 본문을 돌려준다. 블록이 없으면 (기본값, 원문).
pub fn split_front_matter(text: &str) -> (Meta, &str) {
    let rest = match text.strip_prefix('\u{feff}').unwrap_or(text) {
        t if t.starts_with("---\n") => &t[4..],
        t if t.starts_with("---\r\n") => &t[5..],
        _ => return (Meta::default(), text),
    };
    // 닫는 줄 찾기
    let mut pos = 0;
    let mut close = None;
    for line in rest.split_inclusive('\n') {
        let l = line.trim_end_matches(['\r', '\n']);
        if l == "---" || l == "..." {
            close = Some((pos, pos + line.len()));
            break;
        }
        pos += line.len();
    }
    let Some((yend, bend)) = close else { return (Meta::default(), text) };
    let mut m = Meta::default();
    let mut in_tags = false;
    for line in rest[..yend].lines() {
        let raw = line.trim_end();
        if in_tags {
            if let Some(v) = raw.trim_start().strip_prefix("- ") {
                let t = unquote(v);
                if !t.is_empty() {
                    m.tags.push(t);
                }
                continue;
            }
            in_tags = false;
        }
        let Some((k, v)) = raw.split_once(':') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim());
        match k.as_str() {
            "title" if !v.is_empty() => m.title = Some(unquote(v)),
            "pinned" => m.pinned = v.eq_ignore_ascii_case("true"),
            "tags" | "tag" => {
                if v.is_empty() {
                    in_tags = true;
                } else {
                    let inner = v.strip_prefix('[').and_then(|x| x.strip_suffix(']')).unwrap_or(v);
                    m.tags.extend(inner.split(',').map(|t| unquote(t).trim_start_matches('#').to_string()).filter(|t| !t.is_empty()));
                }
            }
            _ => {}
        }
    }
    (m, &rest[bend..])
}

/// Notion이 이름 끝에 붙이는 32자리 id(` 0123…`)를 뗀다.
pub fn clean_name(s: &str) -> String {
    let t = s.trim_end();
    if t.len() > 33 {
        let (head, id) = t.split_at(t.len() - 32);
        if head.ends_with(' ') && id.chars().all(|c| c.is_ascii_hexdigit()) {
            return head.trim_end().to_string();
        }
    }
    t.to_string()
}

fn is_md(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("md" | "markdown" | "txt"))
}

pub fn is_image(p: &Path) -> bool {
    p.extension().map(|e| IMAGE_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

/// 폴더를 훑는다. `decode`는 파일 바이트를 글로 바꾼다(CP949 처리 등). 점(.)으로 시작하는 폴더(.obsidian 등)는 건너뛴다.
pub fn scan(root: &Path, decode: &dyn Fn(&[u8]) -> String) -> Scan {
    let mut out = Scan::default();
    walk(root, &mut vec![], &mut out, decode, 0);
    out.items.sort_by(|a, b| (&a.dirs, &a.title).cmp(&(&b.dirs, &b.title)));
    out
}

fn walk(dir: &Path, dirs: &mut Vec<String>, out: &mut Scan, decode: &dyn Fn(&[u8]) -> String, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        out.failed += 1;
        return;
    };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if depth < 32 {
                dirs.push(clean_name(&name));
                walk(&p, dirs, out, decode, depth + 1);
                dirs.pop();
            }
        } else if is_image(&p) {
            out.images.entry(name.to_lowercase()).or_insert(p);
        } else if is_md(&p) {
            match std::fs::read(&p) {
                Ok(b) => {
                    let text = decode(&b);
                    let (meta, body) = split_front_matter(&text);
                    let stem = p.file_stem().map(|s| clean_name(&s.to_string_lossy())).unwrap_or_default();
                    out.items.push(Item {
                        dirs: dirs.clone(),
                        title: meta.title.unwrap_or(stem),
                        body: body.trim_start_matches(['\r', '\n']).to_string(),
                        tags: meta.tags,
                        pinned: meta.pinned,
                        src_dir: dir.to_path_buf(),
                    });
                }
                Err(_) => out.failed += 1,
            }
        }
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                o.push(v);
                i += 3;
                continue;
            }
        }
        o.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

/// 줄 전체가 그림 하나(`![대체글](경로)` 또는 `![[파일.png]]`)이면 (대체글, 경로, 위키형인가).
fn image_line(line: &str) -> Option<(String, String, bool)> {
    let t = line.trim();
    if let Some(r) = t.strip_prefix("![[").and_then(|x| x.strip_suffix("]]")) {
        let name = r.split('|').next()?.trim();
        let stem = Path::new(name).file_stem()?.to_string_lossy().into_owned();
        return Some((stem, name.to_string(), true));
    }
    let r = t.strip_prefix("![")?;
    let k = r.find("](")?;
    let alt = &r[..k];
    let tail = r[k + 2..].strip_suffix(')')?;
    if alt.contains(['[', ']']) || tail.contains(')') && !tail.starts_with('<') {
        return None;
    }
    let tgt = tail.trim().trim_start_matches('<');
    let tgt = tgt.split(['>', '"']).next()?.trim();
    let tgt = if tail.trim().starts_with('<') { tgt.to_string() } else { tgt.split_whitespace().next()?.to_string() };
    Some((alt.to_string(), tgt, false))
}

/// 본문에서 그림만 있는 줄을 찾아 `resolve(대체글, 파일경로) -> Some((해시, 가로, 세로))`로 아카식 그림 줄(`![대체글](img:해시 가로x세로)`)로 바꾼다.
/// 파일 경로는 문서 폴더 기준 → 없으면 위키형은 `images`에서 이름으로 찾는다. 바꾸지 못한 줄은 그대로 둔다.
pub fn rewrite_image_lines(
    body: &str,
    src_dir: &Path,
    images: &HashMap<String, PathBuf>,
    resolve: &mut dyn FnMut(&str, &Path) -> Option<(String, u32, u32)>,
) -> String {
    let mut o = String::with_capacity(body.len());
    for raw in body.split_inclusive('\n') {
        let eol = &raw[raw.trim_end_matches(['\r', '\n']).len()..];
        let line = &raw[..raw.len() - eol.len()];
        let mut new = None;
        if let Some((alt, target, wiki)) = image_line(line) {
            let low = target.to_lowercase();
            if !(low.starts_with("http://") || low.starts_with("https://") || low.starts_with("data:")) {
                let rel = pct_decode(&target);
                let mut path = src_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if !path.is_file() && wiki {
                    let fname = Path::new(&rel).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if let Some(p) = images.get(&fname) {
                        path = p.clone();
                    }
                }
                if path.is_file() && is_image(&path) {
                    if let Some((hash, w, h)) = resolve(&alt, &path) {
                        new = Some(format!("![{}](img:{hash} {w}x{h})", alt.replace(['[', ']'], "")));
                    }
                }
            }
        }
        match new {
            Some(n) => {
                o.push_str(&n);
                o.push_str(eol);
            }
            None => o.push_str(raw),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_read_and_stripped() {
        let t = "---\ntitle: \"제목: 하나\"\ntags:\n  - a\n  - \"b c\"\ncategory: \"X / Y\"\ncreated: 2026-01-01\npinned: true\n---\n본문\n";
        let (m, b) = split_front_matter(t);
        assert_eq!(m.title.as_deref(), Some("제목: 하나"));
        assert_eq!(m.tags, vec!["a", "b c"]);
        assert!(m.pinned);
        assert_eq!(b, "본문\n");
        let (m, b) = split_front_matter("---\r\ntags: [x, #y]\r\n---\r\nhi");
        assert_eq!(m.tags, vec!["x", "y"]);
        assert_eq!(b, "hi");
        // 블록이 없거나 닫히지 않으면 원문 그대로
        assert_eq!(split_front_matter("그냥 글\n---\n").1, "그냥 글\n---\n");
        assert_eq!(split_front_matter("---\ntitle: x\n본문").1, "---\ntitle: x\n본문");
    }

    #[test]
    fn notion_ids_are_stripped() {
        assert_eq!(clean_name("회의록 0123456789abcdef0123456789abcdef"), "회의록");
        assert_eq!(clean_name("그냥 이름"), "그냥 이름");
        assert_eq!(clean_name("0123456789abcdef0123456789abcdef"), "0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn image_lines_are_rewritten() {
        let dir = std::env::temp_dir().join(format!("aks-imp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("my pic.png"), b"x").unwrap();
        std::fs::write(dir.join("sub").join("w.png"), b"x").unwrap();
        let mut imgs = HashMap::new();
        imgs.insert("w.png".to_string(), dir.join("sub").join("w.png"));
        let body = "a\n![사진](my%20pic.png)\n![[w.png]]\n![웹](https://x/y.png)\n![없음](no.png)\n글 ![인라인](my%20pic.png) 중간\n";
        let mut n = 0;
        let out = rewrite_image_lines(body, &dir, &imgs, &mut |_, _| {
            n += 1;
            Some((format!("{:016x}", n), 10, 20))
        });
        assert_eq!(
            out,
            "a\n![사진](img:0000000000000001 10x20)\n![w](img:0000000000000002 10x20)\n![웹](https://x/y.png)\n![없음](no.png)\n글 ![인라인](my%20pic.png) 중간\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_maps_folders_and_skips_dot_dirs() {
        let dir = std::env::temp_dir().join(format!("aks-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("A").join("B")).unwrap();
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        std::fs::write(dir.join("루트.md"), "---\ntitle: 제목\n---\n내용").unwrap();
        std::fs::write(dir.join("A").join("B").join("깊음 0123456789abcdef0123456789abcdef.md"), "x").unwrap();
        std::fs::write(dir.join(".obsidian").join("c.md"), "x").unwrap();
        let s = scan(&dir, &|b| String::from_utf8_lossy(b).into_owned());
        let names: Vec<(Vec<String>, String)> = s.items.iter().map(|i| (i.dirs.clone(), i.title.clone())).collect();
        assert_eq!(names, vec![(vec![], "제목".to_string()), (vec!["A".to_string(), "B".to_string()], "깊음".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
