//! 인덱스 문서(허브 문서) 자동 정리: 새 문서의 [[링크]]를 알맞은 섹션에 덧붙이고, 빠진 문서·끊긴 링크를 찾아 정리한다.
//! 본문 문자열 변환은 DB를 모르는 순수 함수로 두고(시험하기 쉽게), 맨 아래 'impl Db'가 저장소와 이어 준다.
use crate::db::{Db, Doc, Res};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

/// 새로 넣을 문서: 제목과, 섹션을 찾을 단서(분류 이름 리프→루트, 이어서 태그 이름. 앞쪽이 우선).
#[derive(Clone, Debug)]
pub struct Add {
    pub title: String,
    pub hints: Vec<String>,
}

const UNCAT: &str = "미분류";

fn eol(body: &str) -> &'static str {
    if body.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn join(lines: &[String], eol: &str, trailing_nl: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join(eol);
    if trailing_nl {
        s.push_str(eol);
    }
    s
}

/// 줄마다 '코드 블록 안(펜스 줄 포함)'인지.
fn code_flags(lines: &[String]) -> Vec<bool> {
    let mut in_fence = false;
    lines
        .iter()
        .map(|l| {
            if crate::md::is_fence_line(l) {
                in_fence = !in_fence;
                true
            } else {
                in_fence
            }
        })
        .collect()
}

/// 섹션 비교용 이름: 앞의 '숫자.'와 괄호 설명을 빼고 소문자로.
fn norm(s: &str) -> String {
    let s = s.trim();
    let s = match s.find(|c: char| !c.is_ascii_digit()) {
        Some(i) if i > 0 && s[i..].starts_with('.') => s[i + 1..].trim_start(),
        _ => s,
    };
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '（' => depth += 1,
            ')' | '）' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_lowercase()
}

/// (줄 번호, 제목 글자) — '## ' 제목들(코드 블록 안 제외).
fn headings(lines: &[String], code: &[bool]) -> Vec<(usize, String)> {
    lines.iter().enumerate().filter(|(i, l)| !code[*i] && l.starts_with("## ")).map(|(i, l)| (i, l[3..].trim().to_string())).collect()
}

/// hints 순서대로 정확히 같은 제목 → 포함하는 제목을 찾는다. 찾은 제목의 줄 번호.
fn pick_idx(lines: &[String], code: &[bool], hints: &[String]) -> Option<usize> {
    let hs: Vec<(usize, String)> = headings(lines, code).into_iter().map(|(i, t)| (i, norm(&t))).collect();
    for h in hints {
        let h = norm(h);
        if h.is_empty() {
            continue;
        }
        if let Some((i, _)) = hs.iter().find(|(_, t)| *t == h) {
            return Some(*i);
        }
        // 한 글자짜리 이름은 우연히 들어 있는 경우가 많아(예: '일' ⊂ '일일보고') 포함 비교에서 뺀다.
        if let Some((i, _)) = hs.iter().find(|(_, t)| t.chars().count().min(h.chars().count()) >= 2 && (t.contains(&h) || h.contains(t.as_str()))) {
            return Some(*i);
        }
    }
    None
}

/// 들어갈 섹션의 제목 글자(없으면 None = 미분류).
pub fn pick_section(body: &str, hints: &[String]) -> Option<String> {
    let lines: Vec<String> = body.lines().map(String::from).collect();
    let code = code_flags(&lines);
    pick_idx(&lines, &code, hints).map(|i| lines[i][3..].trim().to_string())
}

/// 본문의 링크 제목들(소문자). 코드 블록·인라인 코드 안은 제외.
pub fn linked_titles(body: &str) -> HashSet<String> {
    crate::md::wiki_links(body).into_iter().map(|t| t.trim().to_lowercase()).collect()
}

/// 새 문서 링크를 섹션 끝에 '- [[제목]]'으로 덧붙인다. 이미 링크가 있거나 링크로 못 쓰는 제목은 건너뛴다.
pub fn add_links(body: &str, adds: &[Add]) -> String {
    let eol = eol(body);
    let trailing_nl = body.is_empty() || body.ends_with('\n');
    let mut lines: Vec<String> = body.lines().map(String::from).collect();
    let mut linked = linked_titles(body);
    for a in adds {
        if crate::md::wiki_tag(&a.title).is_err() {
            continue;
        }
        let t = a.title.trim();
        if !linked.insert(t.to_lowercase()) {
            continue;
        }
        let item = format!("- [[{t}]]");
        let code = code_flags(&lines);
        let target = pick_idx(&lines, &code, &a.hints).or_else(|| pick_idx(&lines, &code, &[UNCAT.to_string()]));
        match target {
            Some(start) => {
                // 섹션은 다음 '# '·'## ' 제목 전까지. 그 안의 마지막 비어 있지 않은 줄 다음에 넣는다.
                let end = (start + 1..lines.len()).find(|&i| !code[i] && (lines[i].starts_with("# ") || lines[i].starts_with("## "))).unwrap_or(lines.len());
                let last = (start..end).rev().find(|&i| !lines[i].trim().is_empty()).unwrap_or(start);
                lines.insert(last + 1, item);
            }
            None => {
                if lines.last().map_or(false, |l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("## {UNCAT}"));
                lines.push(item);
            }
        }
    }
    join(&lines, eol, trailing_nl)
}

/// 한 줄에서 '[[...]]' 링크의 (시작, 끝(미포함), 제목 소문자)를 글자 위치로 모은다(인라인 코드 안·외부 주소 제외).
fn link_spans(chars: &[char]) -> Vec<(usize, usize, String)> {
    let mut in_code = false;
    let mask: Vec<bool> = chars
        .iter()
        .map(|&c| {
            if c == '`' {
                in_code = !in_code;
                true
            } else {
                in_code
            }
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '[' && chars[i + 1] == '[' && !mask[i] {
            if let Some(off) = (i + 2..chars.len().saturating_sub(1)).find(|&j| chars[j] == ']' && chars[j + 1] == ']') {
                let seg: String = chars[i..off + 2].iter().collect();
                if let Some(t) = crate::md::wiki_links(&seg).into_iter().next() {
                    out.push((i, off + 2, t.trim().to_lowercase()));
                }
                i = off + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 링크 한 줄에서 지울 링크를 뺀다. 이웃한 ' · ' 구분자 하나도 함께 지운다. 아무것도 안 남으면 None(줄 삭제).
fn strip_line(line: &str, drop: &HashSet<String>) -> Option<String> {
    let mut chars: Vec<char> = line.chars().collect();
    let spans: Vec<(usize, usize)> = link_spans(&chars).into_iter().filter(|(_, _, t)| drop.contains(t)).map(|(s, e, _)| (s, e)).collect();
    if spans.is_empty() {
        return Some(line.to_string());
    }
    let sep: Vec<char> = " · ".chars().collect();
    for (s, e) in spans.into_iter().rev() {
        if chars[e..].starts_with(&sep) {
            chars.drain(s..e + sep.len());
        } else if s >= sep.len() && chars[..s].ends_with(&sep) {
            chars.drain(s - sep.len()..e);
        } else {
            chars.drain(s..e);
            if s > 0 && s < chars.len() && chars[s - 1] == ' ' && chars[s] == ' ' {
                chars.remove(s);
            }
        }
    }
    if chars.iter().all(|c| matches!(c, '-' | '*' | '+' | '·' | ' ' | '\t')) {
        return None;
    }
    Some(chars.into_iter().collect())
}

/// 본문에서 해당 제목의 링크를 지운다(코드 블록·인라인 코드 안은 그대로).
pub fn remove_links(body: &str, titles: &[String]) -> String {
    let drop: HashSet<String> = titles.iter().map(|t| t.trim().to_lowercase()).collect();
    if drop.is_empty() {
        return body.to_string();
    }
    let eol = eol(body);
    let trailing_nl = body.ends_with('\n');
    let lines: Vec<String> = body.lines().map(String::from).collect();
    let code = code_flags(&lines);
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if code[i] {
            out.push(l.clone());
        } else if let Some(x) = strip_line(l, &drop) {
            out.push(x);
        }
    }
    join(&out, eol, trailing_nl)
}

// ---------------- 저장소와 잇기 ----------------

const KEY: &str = "index_doc_id";

/// 인덱스 정리 검사 결과.
#[derive(Clone, Debug, Default)]
pub struct IndexScan {
    pub missing: Vec<Missing>,
    pub broken: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Missing {
    pub id: i64,
    pub title: String,
    /// 들어갈 섹션 이름(맞는 곳이 없으면 '미분류')
    pub section: String,
}

fn err<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

impl Db {
    /// 인덱스로 지정한 문서의 번호(이 PC 전용 설정, meta 표).
    pub fn index_doc(&self) -> Option<i64> {
        self.conn.query_row("SELECT value FROM meta WHERE key=?", [KEY], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|v| v.parse().ok())
    }

    pub fn set_index_doc(&self, id: Option<i64>) -> Res<()> {
        match id {
            Some(i) => self.conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)", params![KEY, i.to_string()]),
            None => self.conn.execute("DELETE FROM meta WHERE key=?", [KEY]),
        }
        .map_err(err)?;
        Ok(())
    }

    /// 쓸 수 있는 인덱스 문서(지정됨·존재·휴지통 아님).
    fn index_live(&self) -> Option<(i64, Doc)> {
        let id = self.index_doc()?;
        let d = self.get_doc(id).ok().flatten()?;
        if d.trashed {
            None
        } else {
            Some((id, d))
        }
    }

    /// 섹션을 찾을 단서: 분류 이름(리프→루트) 다음 태그 이름.
    pub fn hints_for(&self, id: i64) -> Res<Vec<String>> {
        let mut out = Vec::new();
        if let Some(d) = self.get_doc(id)? {
            let cats = self.categories()?;
            let mut cur = d.category;
            let mut guard = 0;
            while let Some(c) = cur {
                let Some(cat) = cats.iter().find(|x| x.id == c) else { break };
                out.push(cat.name.clone());
                cur = cat.parent;
                guard += 1;
                if guard > 64 {
                    break;
                }
            }
        }
        out.extend(self.doc_tags(id)?);
        Ok(out)
    }

    /// 제목이 처음 생긴 문서를 인덱스에 넣는다. 어떤 이유로든 못 하면 조용히 넘어간다(문서 저장은 이미 끝났다).
    pub(crate) fn auto_index(&self, id: i64) {
        let _ = self.try_auto_index(id);
    }

    fn try_auto_index(&self, id: i64) -> Res<()> {
        let Some((ix, idx)) = self.index_live() else { return Ok(()) };
        if ix == id {
            return Ok(());
        }
        let Some(d) = self.get_doc(id)? else { return Ok(()) };
        if d.trashed || crate::textcrypt::has_markers(&d.body) {
            return Ok(());
        }
        let nb = add_links(&idx.body, &[Add { title: d.title.clone(), hints: self.hints_for(id)? }]);
        if nb != idx.body {
            self.save_doc_plain(ix, &idx.title, &nb)?;
        }
        Ok(())
    }

    /// 인덱스에 빠진 문서와 끊긴 링크를 찾는다(인덱스가 없으면 None).
    pub fn index_scan(&self) -> Res<Option<IndexScan>> {
        let Some((ix, idx)) = self.index_live() else { return Ok(None) };
        let linked = linked_titles(&idx.body);
        let mut docs = self.all_docs()?;
        docs.sort_by_key(|d| d.0);
        let mut seen = HashSet::new();
        let mut scan = IndexScan::default();
        for (id, title, body) in docs {
            let t = title.trim();
            if id == ix || t.is_empty() || crate::md::wiki_tag(t).is_err() || crate::textcrypt::has_markers(&body) {
                continue;
            }
            let key = t.to_lowercase();
            if linked.contains(&key) || !seen.insert(key) {
                continue;
            }
            let section = pick_section(&idx.body, &self.hints_for(id)?).unwrap_or_else(|| UNCAT.to_string());
            scan.missing.push(Missing { id, title: t.to_string(), section });
        }
        let mut seen = HashSet::new();
        for t in crate::md::wiki_links(&idx.body) {
            let t = t.trim().to_string();
            if seen.insert(t.to_lowercase()) && self.find_by_title(&t)?.is_none() {
                scan.broken.push(t);
            }
        }
        Ok(Some(scan))
    }

    /// 선택한 끊긴 링크를 지우고 선택한 문서를 넣는다. 인덱스 저장은 한 번(변경 이력 한 건). 바뀐 게 있으면 true.
    pub fn index_apply(&self, add_ids: &[i64], remove: &[String]) -> Res<bool> {
        let Some((ix, idx)) = self.index_live() else { return Ok(false) };
        let mut adds = Vec::new();
        for &id in add_ids {
            if id == ix {
                continue;
            }
            if let Some(d) = self.get_doc(id)? {
                if !d.trashed && !crate::textcrypt::has_markers(&d.body) {
                    adds.push(Add { title: d.title.clone(), hints: self.hints_for(id)? });
                }
            }
        }
        let nb = add_links(&remove_links(&idx.body, remove), &adds);
        if nb == idx.body {
            return Ok(false);
        }
        self.save_doc_plain(ix, &idx.title, &nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const IDX: &str = "# 문서 지도\n\n## 1. 업무일지\n- 월간: [[11월 업무일지]]\n- 일간: [[a]] · [[b]] · [[c]]\n\n## 2. 성적서 (발행 기록 · 양식)\n- [[양식]]\n\n## 3. 초음파(UT) 성적서\n- [[UT1]]\n";

    fn h(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }
    fn add(t: &str, hints: &[&str]) -> Add {
        Add { title: t.into(), hints: h(hints) }
    }

    #[test]
    fn pick_section_exact_before_contains() {
        assert_eq!(pick_section(IDX, &h(&["성적서"])).as_deref(), Some("2. 성적서 (발행 기록 · 양식)"));
        assert_eq!(pick_section(IDX, &h(&["초음파"])).as_deref(), Some("3. 초음파(UT) 성적서"));
    }

    #[test]
    fn pick_section_ignores_one_char_containment() {
        assert_eq!(pick_section("## 일\n- [[x]]\n", &h(&["일일보고"])), None);
        assert_eq!(pick_section("## 일\n- [[x]]\n", &h(&["일"])).as_deref(), Some("일"), "정확 일치는 한 글자도 허용");
    }

    #[test]
    fn pick_section_none_when_no_match() {
        assert_eq!(pick_section(IDX, &h(&["없는분류"])), None);
        assert_eq!(pick_section(IDX, &[]), None);
        assert_eq!(pick_section(IDX, &h(&[""])), None);
    }

    #[test]
    fn add_to_matching_section_end() {
        let r = add_links(IDX, &[add("새 문서", &["성적서"])]);
        assert!(r.contains("- [[양식]]\n- [[새 문서]]\n\n## 3."), "{r}");
        let r = add_links(IDX, &[add("새 업무", &["업무일지"])]);
        assert!(r.contains("[[c]]\n- [[새 업무]]\n\n## 2."), "{r}");
    }

    #[test]
    fn add_creates_uncategorized_section() {
        let r = add_links(IDX, &[add("x", &["없음"])]);
        assert!(r.ends_with("- [[UT1]]\n\n## 미분류\n- [[x]]\n"), "{r}");
        // 이미 있으면 거기에 추가
        let r2 = add_links(&r, &[add("y", &["없음"])]);
        assert!(r2.ends_with("## 미분류\n- [[x]]\n- [[y]]\n"), "{r2}");
        // 섹션이 없는 본문, 빈 본문
        assert_eq!(add_links("그냥 글", &[add("x", &[])]), "그냥 글\n\n## 미분류\n- [[x]]");
        assert_eq!(add_links("", &[add("x", &[])]), "## 미분류\n- [[x]]\n");
    }

    #[test]
    fn add_skips_already_linked() {
        assert_eq!(add_links(IDX, &[add("양식", &["성적서"])]), IDX);
        assert_eq!(add_links(IDX, &[add("11월 업무일지", &[])]), IDX);
        assert_eq!(add_links("## 가\n- [[새 문서|별칭]]\n", &[add("새 문서", &["가"])]), "## 가\n- [[새 문서|별칭]]\n");
        // 코드 블록 안 링크는 링크가 아니다
        let r = add_links("## 가\n```\n[[새 문서]]\n```\n", &[add("새 문서", &["가"])]);
        assert!(r.ends_with("```\n- [[새 문서]]\n"), "{r}");
    }

    #[test]
    fn add_skips_unlinkable_title() {
        assert_eq!(add_links(IDX, &[add("a|b", &[]), add("  ", &[])]), IDX);
    }

    #[test]
    fn add_keeps_crlf() {
        let r = add_links("## 가\r\n- [[x]]\r\n", &[add("y", &["가"])]);
        assert_eq!(r, "## 가\r\n- [[x]]\r\n- [[y]]\r\n");
    }

    #[test]
    fn add_several_in_order() {
        let r = add_links("## 가\n- [[x]]\n", &[add("y", &["가"]), add("z", &["가"]), add("Y", &["가"])]);
        assert_eq!(r, "## 가\n- [[x]]\n- [[y]]\n- [[z]]\n");
    }

    #[test]
    fn add_into_empty_section() {
        assert_eq!(add_links("## 가\n\n## 나\n- [[q]]\n", &[add("y", &["가"])]), "## 가\n- [[y]]\n\n## 나\n- [[q]]\n");
    }

    #[test]
    fn remove_whole_bullet_line() {
        assert_eq!(remove_links("## 가\n- [[a]]\n- [[b|별칭]]\n- [[c]]\n", &h(&["a", "b"])), "## 가\n- [[c]]\n");
    }

    #[test]
    fn remove_from_dot_joined_line() {
        let l = |t: &str| remove_links("- 일간: [[a]] · [[b]] · [[c]]\n", &h(&[t]));
        assert_eq!(l("b"), "- 일간: [[a]] · [[c]]\n");
        assert_eq!(l("c"), "- 일간: [[a]] · [[b]]\n");
        assert_eq!(l("a"), "- 일간: [[b]] · [[c]]\n");
        assert_eq!(remove_links("- [[a]] · [[b]]\n", &h(&["a", "b"])), "");
    }

    #[test]
    fn remove_leaves_prose_and_code() {
        assert_eq!(remove_links("참고 [[a]] 문서\n", &h(&["a"])), "참고 문서\n");
        let s = "```\n- [[a]]\n```\n인라인 `[[a]]` 그대로\n";
        assert_eq!(remove_links(s, &h(&["a"])), s);
        assert_eq!(remove_links("- 라벨: [[a]]\n", &h(&["a"])), "- 라벨: \n");
    }

    #[test]
    fn remove_cleans_empty_bullet_and_keeps_crlf() {
        assert_eq!(remove_links("## 가\r\n- [[a]]\r\n- [[b]]\r\n", &h(&["A"])), "## 가\r\n- [[b]]\r\n");
        assert_eq!(remove_links("[[a]]", &h(&["a"])), "");
    }

    #[test]
    fn linked_titles_ignores_code() {
        let s = linked_titles("[[A]] [[b|c]] `[[d]]`\n```\n[[e]]\n```\n");
        assert!(s.contains("a") && s.contains("b") && !s.contains("d") && !s.contains("e"));
    }

    // ---- DB ----
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("akashic-idx-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("vault.db")
    }
    fn doc(db: &Db, title: &str, body: &str) -> i64 {
        let id = db.new_doc(None).unwrap();
        db.save_doc(id, title, body).unwrap();
        id
    }
    fn idx_db(name: &str) -> (Db, i64) {
        let db = Db::open(&tmp(name)).unwrap();
        let ix = doc(&db, "00. 문서 지도", "# 지도\n\n## 성적서\n- [[양식]]\n\n## 일기\n");
        db.set_index_doc(Some(ix)).unwrap();
        (db, ix)
    }
    fn body(db: &Db, id: i64) -> String {
        db.get_doc(id).unwrap().unwrap().body
    }

    #[test]
    fn index_doc_roundtrip() {
        let db = Db::open(&tmp("rt")).unwrap();
        assert_eq!(db.index_doc(), None);
        db.set_index_doc(Some(7)).unwrap();
        assert_eq!(db.index_doc(), Some(7));
        db.set_index_doc(None).unwrap();
        assert_eq!(db.index_doc(), None);
    }

    #[test]
    fn auto_adds_on_first_title_by_category_and_tag() {
        let (db, ix) = idx_db("auto");
        let cat = db.add_category(None, "성적서").unwrap();
        let a = db.new_doc(Some(cat)).unwrap();
        assert_eq!(body(&db, ix).matches("[[").count(), 1, "빈 제목 문서는 아직 넣지 않는다");
        db.save_doc(a, "11 24 성적서", "x").unwrap();
        assert!(body(&db, ix).contains("- [[양식]]\n- [[11 24 성적서]]\n"), "{}", body(&db, ix));
        let b = db.new_doc(None).unwrap();
        db.add_tag(b, "일기").unwrap();
        db.save_doc(b, "오늘", "x").unwrap();
        assert!(body(&db, ix).trim_end().ends_with("## 일기\n- [[오늘]]"), "{}", body(&db, ix));
        let c = doc(&db, "분류없음", "x");
        assert!(body(&db, ix).ends_with("## 미분류\n- [[분류없음]]\n"), "{}", body(&db, ix));
        let _ = c;
    }

    #[test]
    fn auto_runs_once_and_off_without_index() {
        let (db, ix) = idx_db("once");
        let a = doc(&db, "처음", "x");
        let before = body(&db, ix);
        db.save_doc(a, "바꾼 제목", "x").unwrap();
        assert_eq!(body(&db, ix), before);
        db.set_index_doc(None).unwrap();
        doc(&db, "꺼진 뒤", "x");
        assert_eq!(body(&db, ix), before);
    }

    #[test]
    fn auto_skips_and_never_fails_save() {
        let (db, ix) = idx_db("skip");
        let before = body(&db, ix);
        // 암호화 글·링크 못 쓰는 제목
        assert!(db.save_doc(db.new_doc(None).unwrap(), "비밀", "🔒【enc:1:abc】🔒").unwrap());
        assert!(db.save_doc(db.new_doc(None).unwrap(), "a|b", "x").unwrap());
        assert_eq!(body(&db, ix), before);
        // 인덱스를 휴지통으로 → 저장은 성공, 인덱스 불변
        db.trash(ix).unwrap();
        assert!(db.save_doc(db.new_doc(None).unwrap(), "휴지통뒤", "x").unwrap());
        assert_eq!(body(&db, ix), before);
        // 지워진 인덱스
        db.purge(ix).unwrap();
        assert!(db.save_doc(db.new_doc(None).unwrap(), "삭제뒤", "x").unwrap());
        // 인덱스 자신의 제목 저장은 자기 자신을 넣지 않는다
        let (db2, ix2) = idx_db("self");
        let before2 = body(&db2, ix2);
        db2.save_doc(ix2, "00. 새 이름", &before2).unwrap();
        assert_eq!(body(&db2, ix2), before2);
    }

    #[test]
    fn auto_keeps_history() {
        let (db, ix) = idx_db("hist");
        let before = body(&db, ix);
        doc(&db, "새글", "x");
        assert!(db.versions(ix).unwrap().iter().any(|v| v.body == before));
    }

    #[test]
    fn save_doc_plain_has_no_hook() {
        let (db, ix) = idx_db("plain");
        let before = body(&db, ix);
        let id = db.new_doc(None).unwrap();
        db.save_doc_plain(id, "동기화로 온 글", "x").unwrap();
        assert_eq!(body(&db, ix), before);
    }

    #[test]
    fn scan_finds_missing_and_broken() {
        let (db, ix) = idx_db("scan");
        db.set_index_doc(None).unwrap();
        let a = doc(&db, "양식", "x"); // 이미 링크됨
        let b = doc(&db, "빠진 글", "x");
        let t = doc(&db, "지운 글", "x");
        db.trash(t).unwrap();
        doc(&db, "비밀글", "🔒【enc:1:z】🔒");
        doc(&db, "a|b", "x");
        let ix_body = format!("{}- [[없는 문서]] [[없는 문서]] [[ZZZ]]\n```\n[[코드 안]]\n```\n", body(&db, ix));
        db.save_doc(ix, "00. 문서 지도", &ix_body).unwrap();
        db.set_index_doc(Some(ix)).unwrap();
        let s = db.index_scan().unwrap().unwrap();
        let ids: Vec<i64> = s.missing.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![b], "{:?}", s.missing);
        assert_eq!(s.missing[0].section, "미분류");
        assert_eq!(s.broken, vec!["없는 문서".to_string(), "ZZZ".to_string()]);
        let _ = a;
        db.set_index_doc(None).unwrap();
        assert!(db.index_scan().unwrap().is_none());
    }

    #[test]
    fn apply_adds_and_removes_in_one_version() {
        let (db, ix) = idx_db("apply");
        db.set_index_doc(None).unwrap();
        let cat = db.add_category(None, "일기").unwrap();
        let a = db.new_doc(Some(cat)).unwrap();
        db.save_doc(a, "오늘 일기", "x").unwrap();
        db.set_index_doc(Some(ix)).unwrap();
        let n0 = db.versions(ix).unwrap().len();
        assert!(db.index_apply(&[a], &["양식".to_string()]).unwrap());
        let b = body(&db, ix);
        assert!(b.contains("## 일기\n- [[오늘 일기]]") && !b.contains("[[양식]]"), "{b}");
        assert_eq!(db.versions(ix).unwrap().len(), n0 + 1);
        assert!(!db.index_apply(&[], &[]).unwrap());
        assert!(!db.index_apply(&[a], &[]).unwrap(), "이미 들어 있으면 변화 없음");
    }
}
