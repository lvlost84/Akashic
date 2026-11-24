//! 편집 키(들여쓰기·줄 이동/복제/삭제·바꾸기·서식 토글·페이지 이동)의 순수 글자 처리.
//! 모든 위치는 egui가 쓰는 **char 인덱스**다. 줄 끝(`\n`·`\r\n`)은 줄 내용과 따로 들고 다녀서
//! 마지막 줄에 개행이 없거나 줄 끝이 섞인 문서에서도 개행 모양이 그대로 보존된다.

/// 편집 결과: 새 본문과 선택 범위(작은 쪽, 큰 쪽).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Edit {
    pub text: String,
    pub lo: usize,
    pub hi: usize,
}

/// 줄 내용(개행 제외)과 줄 끝을 나눠 든다. `lines.len() == eols.len()`, 마지막 줄 끝은 항상 "".
struct Lines {
    content: Vec<String>,
    eol: Vec<&'static str>,
}

impl Lines {
    fn parse(text: &str) -> Lines {
        let mut content = vec![];
        let mut eol = vec![];
        for piece in text.split('\n') {
            content.push(piece.strip_suffix('\r').unwrap_or(piece).to_string());
            eol.push(if piece.ends_with('\r') { "\r\n" } else { "\n" });
        }
        *eol.last_mut().unwrap() = ""; // 마지막 조각 뒤에는 개행이 없다
        // 마지막 조각이 `\r`로 끝나면(문서가 \r로 끝남) 그 \r은 내용이 아니라 줄 끝으로 둔다.
        if text.ends_with('\r') {
            *eol.last_mut().unwrap() = "\r";
        }
        Lines { content, eol }
    }

    fn join(&self) -> String {
        let mut s = String::new();
        for (c, e) in self.content.iter().zip(&self.eol) {
            s.push_str(c);
            s.push_str(e);
        }
        s
    }

    fn len(&self) -> usize {
        self.content.len()
    }

    /// 줄 `k`가 시작하는 char 위치
    fn start(&self, k: usize) -> usize {
        (0..k).map(|i| self.content[i].chars().count() + self.eol[i].chars().count()).sum()
    }

    /// char 위치 → (줄, 칸). 줄 끝 개행 위에 있으면 그 줄의 끝 칸.
    fn locate(&self, pos: usize) -> (usize, usize) {
        let mut rem = pos;
        for k in 0..self.len() {
            let w = self.content[k].chars().count();
            let full = w + self.eol[k].chars().count();
            if rem < full || k + 1 == self.len() {
                return (k, rem.min(w));
            }
            rem -= full;
        }
        (self.len() - 1, 0)
    }

    fn at(&self, line: usize, col: usize) -> usize {
        self.start(line) + col.min(self.content[line].chars().count())
    }

    /// 선택 `lo..hi`가 걸친 줄 범위(양끝 포함). 선택이 있고 `hi`가 줄 맨 앞이면 그 줄은 뺀다.
    fn block(&self, lo: usize, hi: usize) -> (usize, usize) {
        let (l0, _) = self.locate(lo);
        let (mut l1, c1) = self.locate(hi);
        if hi > lo && c1 == 0 && l1 > l0 {
            l1 -= 1;
        }
        (l0, l1)
    }
}

fn fence_flags(lines: &Lines) -> Vec<bool> {
    let mut inf = false;
    lines
        .content
        .iter()
        .map(|l| {
            if l.trim_start().starts_with("```") {
                inf = !inf;
                true // 펜스 줄 자체는 코드 블록 취급
            } else {
                inf
            }
        })
        .collect()
}

/// 목록 줄이면 그 기호 줄의 들여쓰기 단위(막대 2칸, 숫자 `N. `은 기호 폭 — `1. `=3, `10. `=4).
fn list_unit(line: &str) -> Option<usize> {
    let r = line.trim_start_matches([' ', '\t']);
    if r.starts_with("- ") || r.starts_with("* ") || r.starts_with("+ ") || r.starts_with("> ") {
        return Some(2);
    }
    let d = r.bytes().take_while(|b| b.is_ascii_digit()).count();
    if d > 0 && d <= 9 && r[d..].starts_with(". ") {
        return Some(d + 2);
    }
    None
}

/// 이 줄을 들여쓸 칸 수: 목록 항목은 기호 폭(마크다운 중첩 조건), 코드 블록 안은 4칸, 그 밖은 2칸.
fn unit_of(line: &str, in_fence: bool) -> usize {
    if in_fence {
        return 4;
    }
    list_unit(line).unwrap_or(2)
}

fn unindent(s: &mut String, unit: usize) -> bool {
    if s.starts_with('\t') {
        s.remove(0);
        return true;
    }
    let n = s.chars().take_while(|c| *c == ' ').count().min(unit);
    if n == 0 {
        return false;
    }
    s.drain(..n);
    true
}

/// Tab(`out=false`)/Shift+Tab(`out=true`). 선택이 걸친 모든 줄을 들여쓰고 선택은 유지한다.
/// 선택이 없을 때: 바깥은 목록 줄이면 그 줄, 아니면 커서 자리에 4칸(코드 블록·표 안 포함). 안쪽은 늘 현재 줄.
pub fn indent(text: &str, lo: usize, hi: usize, out: bool) -> Option<Edit> {
    let mut ls = Lines::parse(text);
    let fence = fence_flags(&ls);
    let (l0, l1) = ls.block(lo, hi);
    if !out && lo == hi {
        let l = &ls.content[l0];
        if list_unit(l).is_none() || fence[l0] {
            // 커서 자리에 4칸(같은 줄에 글이 있어도 자연스럽게 칸을 맞춘다)
            let (_, col) = ls.locate(lo);
            let b = l.char_indices().nth(col).map_or(l.len(), |(b, _)| b);
            ls.content[l0].insert_str(b, "    ");
            return Some(Edit { text: ls.join(), lo: lo + 4, hi: lo + 4 });
        }
    }
    let (lc, cc) = ls.locate(lo);
    let (hc, ch) = ls.locate(hi);
    let multi = l1 > l0 || hi > lo;
    let mut deltas = vec![0isize; ls.len()];
    for k in l0..=l1 {
        let unit = unit_of(&ls.content[k], fence[k]);
        if out {
            let before = ls.content[k].chars().count();
            if unindent(&mut ls.content[k], unit) {
                deltas[k] = ls.content[k].chars().count() as isize - before as isize;
            }
        } else if !(multi && ls.content[k].trim().is_empty()) {
            ls.content[k].insert_str(0, &" ".repeat(unit));
            deltas[k] = unit as isize;
        }
    }
    if deltas[l0..=l1].iter().all(|d| *d == 0) {
        return None;
    }
    let shift = |col: usize, k: usize| -> usize { (col as isize + deltas[k]).max(0) as usize };
    // 줄 첫머리 앞 칸(들여쓰기 안쪽)은 0으로 눌러 선택이 줄 앞으로 새지 않게 한다.
    let (nlo, nhi) = (ls.at(lc, shift(cc, lc)), ls.at(hc, shift(ch, hc)));
    let nlo = if out && nlo < ls.start(lc) { ls.start(lc) } else { nlo };
    Some(Edit { text: ls.join(), lo: nlo, hi: nhi.max(nlo) })
}

/// Alt+↑/↓: 선택이 걸친 줄들을 위/아래로 한 줄 옮긴다(줄 끝 모양은 자리 그대로).
pub fn move_lines(text: &str, lo: usize, hi: usize, up: bool) -> Option<Edit> {
    let mut ls = Lines::parse(text);
    let (l0, l1) = ls.block(lo, hi);
    if (up && l0 == 0) || (!up && l1 + 1 >= ls.len()) {
        return None;
    }
    let (lc, cc) = ls.locate(lo);
    let (hc, ch) = ls.locate(hi);
    if up {
        let prev = ls.content.remove(l0 - 1);
        ls.content.insert(l1, prev);
    } else {
        let next = ls.content.remove(l1 + 1);
        ls.content.insert(l0, next);
    }
    let d: isize = if up { -1 } else { 1 };
    let mv = |k: usize| (k as isize + d) as usize;
    Some(Edit { text: ls.join(), lo: ls.at(mv(lc), cc), hi: ls.at(mv(hc), ch) })
}

/// Ctrl+D: 걸친 줄들을 바로 아래에 복제하고 선택을 복제본으로 옮긴다.
pub fn duplicate_lines(text: &str, lo: usize, hi: usize) -> Edit {
    let mut ls = Lines::parse(text);
    let (l0, l1) = ls.block(lo, hi);
    let (lc, cc) = ls.locate(lo);
    let (hc, ch) = ls.locate(hi);
    let n = l1 - l0 + 1;
    // 문서의 줄 끝 모양: 이미 있는 줄 끝 중 첫 것, 없으면 \n
    let nl = ls.eol.iter().copied().find(|e| !e.is_empty()).unwrap_or("\n");
    let copy: Vec<String> = ls.content[l0..=l1].to_vec();
    let mut copy_eol: Vec<&'static str> = ls.eol[l0..=l1].to_vec();
    if ls.eol[l1].is_empty() {
        // 마지막 줄(개행 없음): 원본 끝에 개행을 달고 복제본이 개행 없는 끝을 이어받는다.
        ls.eol[l1] = nl;
    } else if ls.eol[l1] == "\r" {
        ls.eol[l1] = nl;
        copy_eol[n - 1] = "\r";
    }
    for (i, c) in copy.into_iter().enumerate() {
        ls.content.insert(l1 + 1 + i, c);
        ls.eol.insert(l1 + 1 + i, copy_eol[i]);
    }
    Edit { text: ls.join(), lo: ls.at(lc + n, cc), hi: ls.at(hc + n, ch) }
}

/// Ctrl+Shift+K: 걸친 줄들을 지운다. 커서는 그 자리를 이은 줄 맨 앞에 둔다.
pub fn delete_lines(text: &str, lo: usize, hi: usize) -> Edit {
    let mut ls = Lines::parse(text);
    let (l0, l1) = ls.block(lo, hi);
    if l0 == 0 && l1 + 1 == ls.len() {
        return Edit { text: String::new(), lo: 0, hi: 0 };
    }
    let last = l1 + 1 == ls.len();
    ls.content.drain(l0..=l1);
    let tail = ls.eol[l1];
    ls.eol.drain(l0..=l1);
    if last {
        // 맨 끝 줄을 지웠으면 앞 줄의 개행도 떨어내 빈 줄이 남지 않게 한다.
        *ls.eol.last_mut().unwrap() = tail;
    }
    let k = l0.min(ls.len() - 1);
    let p = ls.start(k);
    Edit { text: ls.join(), lo: p, hi: p }
}

/// 찾은 위치(`matches`, char 시작·끝)를 모두 `repl`로 바꾼다. 돌려주는 것: (새 본문, 바꾼 개수, 옮겨진 커서).
pub fn replace_matches(text: &str, matches: &[(usize, usize)], repl: &str, cursor: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let rl = repl.chars().count() as isize;
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut cur = cursor as isize;
    let mut done = 0;
    for &(s, e) in matches {
        if s < last || e > chars.len() || s > e {
            continue;
        }
        out.extend(&chars[last..s]);
        out.push_str(repl);
        last = e;
        done += 1;
        if e <= cursor {
            cur += rl - (e - s) as isize;
        } else if s < cursor {
            cur = s as isize + rl; // 커서가 일치 안쪽 → 바뀐 글 뒤로
        }
    }
    out.extend(&chars[last..]);
    (out, done, cur.max(0) as usize)
}

/// Ctrl+B/I 등 감싸기 토글. 이미 감싸져 있으면 기호를 걷어내고, 아니면 감싼다.
/// 감싸진 것으로 보는 경우: ① 선택한 글 자체가 `l…r`로 시작·끝남 ② 선택 바로 앞뒤가 `l`·`r`.
/// `*`(기울임)는 `**`(굵게)와 구별한다: 별표가 홀수 개 이어질 때만 기울임이 있다고 본다.
pub fn toggle_wrap(text: &str, lo: usize, hi: usize, l: &str, r: &str) -> Edit {
    let ch: Vec<char> = text.chars().collect();
    let (lo, hi) = (lo.min(ch.len()), hi.min(ch.len()));
    let lc: Vec<char> = l.chars().collect();
    let rc: Vec<char> = r.chars().collect();
    let (ln, rn) = (lc.len(), rc.len());
    let star = l == "*" && r == "*";
    let run_before = |p: usize| ch[..p].iter().rev().take_while(|c| **c == '*').count();
    let run_after = |p: usize| ch[p..].iter().take_while(|c| **c == '*').count();
    let rebuild = |a: usize, b: usize, c: usize, d: usize, nlo: usize, nhi: usize| -> Edit {
        // a..b와 c..d 구간을 지운다
        let mut s: String = ch[..a].iter().collect();
        s.extend(&ch[b..c]);
        s.extend(&ch[d..]);
        Edit { text: s, lo: nlo, hi: nhi }
    };
    if hi > lo {
        // ① 선택 안쪽이 감싸져 있음
        let inner = &ch[lo..hi];
        let inside_ok = inner.len() >= ln + rn
            && inner[..ln] == lc[..]
            && inner[inner.len() - rn..] == rc[..]
            && (!star || (inner.iter().take_while(|c| **c == '*').count() % 2 == 1 && inner.iter().rev().take_while(|c| **c == '*').count() % 2 == 1));
        if inside_ok {
            return rebuild(lo, lo + ln, hi - rn, hi, lo, hi - ln - rn);
        }
    }
    // ② 선택 바깥이 감싸져 있음
    if lo >= ln && hi + rn <= ch.len() && ch[lo - ln..lo] == lc[..] && ch[hi..hi + rn] == rc[..] {
        let ok = !star || (run_before(lo) % 2 == 1 && run_after(hi) % 2 == 1);
        // 빈 선택(커서)은 `****`처럼 빈 쌍만 걷어낸다
        if ok {
            return rebuild(lo - ln, lo, hi, hi + rn, lo - ln, hi - ln);
        }
    }
    let mut s: String = ch[..lo].iter().collect();
    s.push_str(l);
    s.extend(&ch[lo..hi]);
    s.push_str(r);
    s.extend(&ch[hi..]);
    Edit { text: s, lo: lo + ln, hi: hi + ln }
}

/// 공백 정리 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceMode {
    /// 글자 사이의 공백(연속 포함)을 탭 하나로
    ToTab,
    /// 글자 사이의 공백을 모두 지움
    Remove,
}

/// 공백으로 보는 글자: 보통 공백·전각 공백·NBSP(탭은 `ToTab`에서만 함께 묶는다)
fn is_gap(c: char) -> bool {
    matches!(c, ' ' | '\u{3000}' | '\u{00A0}')
}

/// 한 줄의 공백을 바꾼다. 줄 앞 들여쓰기는 그대로 두고, 줄 끝 공백은 지운다.
fn respace_line(line: &str, mode: SpaceMode) -> String {
    let body_start = line.find(|c: char| !(is_gap(c) || c == '\t')).unwrap_or(line.len());
    let (lead, rest) = line.split_at(body_start);
    let mut out = String::from(lead);
    let mut run = false; // 공백 덩어리 안
    for c in rest.chars() {
        if is_gap(c) || (mode == SpaceMode::ToTab && c == '\t') {
            run = true;
            continue;
        }
        if run && mode == SpaceMode::ToTab {
            out.push('\t');
        }
        run = false;
        out.push(c);
    }
    out // 줄 끝에 남은 공백 덩어리는 버린다(탭을 끝에 남기지 않는다)
}

/// 코드 울타리(``` 또는 ~~~) 줄인가
fn is_fence_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 표 줄인가(들여쓰기 뒤가 `|`로 시작)
fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// 줄 앞의 마크다운 기호(들여쓰기·인용 `>`·목록 기호·체크박스·제목 `#`)와 나머지 본문으로 나눈다.
fn split_md_prefix(line: &str) -> (&str, &str) {
    let b = line.as_bytes();
    let mut i = 0;
    let skip_sp = |i: &mut usize| {
        while *i < b.len() && (b[*i] == b' ' || b[*i] == b'\t') {
            *i += 1;
        }
    };
    skip_sp(&mut i);
    while i < b.len() && b[i] == b'>' {
        i += 1;
        skip_sp(&mut i);
    }
    let rest = &line[i..];
    let rb = rest.as_bytes();
    // 목록 기호: - * + 또는 숫자 + . )
    let mut m = 0;
    if !rb.is_empty() && matches!(rb[0], b'-' | b'*' | b'+') {
        m = 1;
    } else {
        let d = rb.iter().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 && d <= 9 && d < rb.len() && matches!(rb[d], b'.' | b')') {
            m = d + 1;
        }
    }
    if m == 0 {
        let h = rb.iter().take_while(|&&c| c == b'#').count();
        if (1..=6).contains(&h) {
            m = h;
        }
    }
    if m > 0 && m < rb.len() && rb[m] == b' ' {
        let mut j = i + m;
        let mut k = j;
        while k < b.len() && b[k] == b' ' {
            k += 1;
        }
        j = k;
        // 체크박스 [ ] [x]
        let t = &line.as_bytes()[j..];
        if t.len() >= 3 && t[0] == b'[' && matches!(t[1], b' ' | b'x' | b'X') && t[2] == b']' {
            j += 3;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
        }
        return line.split_at(j);
    }
    // 기호 없는 인용만 있는 경우
    line.split_at(i)
}

/// 선택이 걸친 줄들(선택이 없으면 문서 전체)의 글자 사이 공백을 탭으로 바꾸거나 지운다.
/// 바뀐 줄 범위를 선택으로 돌려준다. 바뀔 것이 없으면 None.
/// `protect`가 켜져 있으면 마크다운 구조(목록·인용·제목 기호, 체크박스, 표 줄, 코드 블록)는 건드리지 않는다.
pub fn respace(text: &str, lo: usize, hi: usize, mode: SpaceMode, protect: bool) -> Option<Edit> {
    let mut ls = Lines::parse(text);
    let (l0, l1) = if lo == hi { (0, ls.len() - 1) } else { ls.block(lo, hi) };
    let mut changed = false;
    let mut fenced = false; // 문서 처음부터 세어, 선택 앞의 코드 블록 안인지 안다
    for k in 0..=l1 {
        let is_fence = protect && is_fence_line(&ls.content[k]);
        if is_fence {
            fenced = !fenced;
        }
        if k < l0 || (protect && (fenced || is_fence || is_table_line(&ls.content[k]))) {
            continue;
        }
        let n = if protect {
            let (head, body) = split_md_prefix(&ls.content[k]);
            format!("{head}{}", respace_line(body, mode))
        } else {
            respace_line(&ls.content[k], mode)
        };
        if n != ls.content[k] {
            ls.content[k] = n;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    let start = ls.start(l0);
    let end = ls.start(l1) + ls.content[l1].chars().count();
    Some(Edit { text: ls.join(), lo: start, hi: end })
}

/// 열 자르기의 단위.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColUnit {
    /// 줄 안의 글자 위치
    Char,
    /// 탭으로 나뉜 칸(열)
    Tab,
}

/// 열 자르기의 범위(번호는 1부터, 끝 번호 포함).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColRange {
    /// A번째부터 B번째까지(A>B이면 맞바꿈)
    Span(usize, usize),
    /// 처음부터 N번째까지
    First(usize),
    /// 끝에서 N번째까지(끝의 N개)
    Last(usize),
}

/// 한 줄의 단위 목록에서 범위가 고른 구간 `[s, e)`. 범위가 줄 밖이면 빈 구간(s == e).
fn col_span(len: usize, r: ColRange) -> (usize, usize) {
    let (s, e) = match r {
        ColRange::Span(a, b) => {
            let (a, b) = (a.max(1), b.max(1));
            let (a, b) = if a > b { (b, a) } else { (a, b) };
            (a - 1, b)
        }
        ColRange::First(n) => (0, n),
        ColRange::Last(n) => (len.saturating_sub(n), len),
    };
    let e = e.min(len);
    (s.min(e), e)
}

/// 선택이 걸친 줄들(선택이 없으면 문서 전체)에서 각 줄의 `range`로 고른 열만 남기거나(`keep`) 지운다.
/// 단위가 `Tab`이면 탭으로 나뉜 칸 단위이고 남은 칸은 탭으로 다시 잇는다. 줄 끝 모양은 보존한다.
/// 바뀐 줄 범위를 선택으로 돌려준다. 바뀔 것이 없으면 None.
pub fn cut_columns(text: &str, lo: usize, hi: usize, unit: ColUnit, range: ColRange, keep: bool) -> Option<Edit> {
    let mut ls = Lines::parse(text);
    let (l0, l1) = if lo == hi { (0, ls.len() - 1) } else { ls.block(lo, hi) };
    let mut changed = false;
    for k in l0..=l1 {
        let line = &ls.content[k];
        let new = match unit {
            ColUnit::Char => {
                let ch: Vec<char> = line.chars().collect();
                let (s, e) = col_span(ch.len(), range);
                let pick = |from: usize, to: usize| ch[from..to].iter().collect::<String>();
                if keep { pick(s, e) } else { pick(0, s) + &pick(e, ch.len()) }
            }
            ColUnit::Tab => {
                let cols: Vec<&str> = line.split('\t').collect();
                let (s, e) = col_span(cols.len(), range);
                if keep { cols[s..e].join("\t") } else { cols[..s].iter().chain(&cols[e..]).copied().collect::<Vec<_>>().join("\t") }
            }
        };
        if new != ls.content[k] {
            ls.content[k] = new;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    let start = ls.start(l0);
    let end = ls.start(l1) + ls.content[l1].chars().count();
    Some(Edit { text: ls.join(), lo: start, hi: end })
}

/// 캐럿 바로 앞의 단어가 스니펫 이름과 같으면 그 내용으로 바꾼다(이름 앞은 문서 처음이나 공백이어야 함).
/// 이름이 여럿 맞으면 가장 긴 것. 바꾼 뒤 캐럿은 넣은 글 끝이다. 맞는 이름이 없으면 None.
pub fn expand_abbrev(text: &str, caret: usize, snips: &[(String, String)]) -> Option<Edit> {
    let ch: Vec<char> = text.chars().collect();
    let caret = caret.min(ch.len());
    let mut best: Option<(usize, &str)> = None; // (이름 글자 수, 내용)
    for (name, body) in snips {
        let n: Vec<char> = name.chars().collect();
        if n.is_empty() || n.len() > caret || ch[caret - n.len()..caret] != n[..] {
            continue;
        }
        let before_ok = caret == n.len() || ch[caret - n.len() - 1].is_whitespace();
        if before_ok && best.map_or(true, |(l, _)| n.len() > l) {
            best = Some((n.len(), body));
        }
    }
    let (len, body) = best?;
    let start = caret - len;
    let mut s: String = ch[..start].iter().collect();
    s.push_str(body);
    s.extend(&ch[caret..]);
    let pos = start + body.chars().count();
    Some(Edit { text: s, lo: pos, hi: pos })
}

/// 선택 `lo..hi`를 `ins`로 바꾼다. 캐럿은 넣은 글 끝.
pub fn replace_range(text: &str, lo: usize, hi: usize, ins: &str) -> Edit {
    let ch: Vec<char> = text.chars().collect();
    let (lo, hi) = (lo.min(ch.len()), hi.min(ch.len()));
    let mut s: String = ch[..lo].iter().collect();
    s.push_str(ins);
    s.extend(&ch[hi..]);
    let pos = lo + ins.chars().count();
    Edit { text: s, lo: pos, hi: pos }
}

/// 줄 단위 정리 작업.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOp {
    /// 오름차순 정렬(글자 순서)
    SortAsc,
    /// 내림차순 정렬
    SortDesc,
    /// 같은 줄이 또 나오면 뒤의 것을 지움(처음 것만 남김)
    Dedup,
    /// 빈 줄이 아닌 줄 앞에 `1. `, `2. ` … 번호를 붙임
    Number,
}

/// 선택이 걸친 줄들(선택이 없으면 문서 전체)에 `op`를 적용한다. 줄 끝 모양은 보존한다.
/// 바뀔 것이 없으면 None.
pub fn line_op(text: &str, lo: usize, hi: usize, op: LineOp) -> Option<Edit> {
    let mut ls = Lines::parse(text);
    let (l0, l1) = if lo == hi { (0, ls.len() - 1) } else { ls.block(lo, hi) };
    let old: Vec<String> = ls.content[l0..=l1].to_vec();
    let new: Vec<String> = match op {
        LineOp::SortAsc | LineOp::SortDesc => {
            let mut v = old.clone();
            v.sort();
            if op == LineOp::SortDesc {
                v.reverse();
            }
            v
        }
        LineOp::Dedup => {
            let mut seen = std::collections::HashSet::new();
            old.iter().filter(|l| l.trim().is_empty() || seen.insert((*l).clone())).cloned().collect()
        }
        LineOp::Number => {
            let mut n = 0;
            old.iter()
                .map(|l| {
                    if l.trim().is_empty() {
                        l.clone()
                    } else {
                        n += 1;
                        format!("{n}. {l}")
                    }
                })
                .collect()
        }
    };
    if new == old {
        return None;
    }
    let removed = old.len() - new.len();
    let last_removed = removed > 0 && l1 + 1 == ls.len();
    ls.content.splice(l0..=l1, new.iter().cloned());
    if removed > 0 {
        // 지운 줄의 줄 끝도 함께 뺀다(뒤쪽 줄 끝을 지움)
        let cut = l0 + new.len();
        ls.eol.drain(cut..cut + removed);
        if last_removed {
            if let Some(e) = ls.eol.last_mut() {
                *e = "";
            }
        }
    }
    let last = l0 + new.len().max(1) - 1;
    let start = ls.start(l0);
    let end = ls.start(last) + ls.content[last].chars().count();
    Some(Edit { text: ls.join(), lo: start, hi: end })
}

/// PageUp/PageDown: 화면 높이만큼 움직인 뒤의 (캐럿 y, 스크롤 위치).
/// `caret_y` 캐럿 줄 윗선(글 좌표), `scroll` 현재 스크롤 위쪽 오프셋, `view_h` 보이는 높이, `content_h` 전체 높이.
/// 캐럿이 화면에서 있던 자리를 유지하도록 같은 만큼 옮기되, 문서 끝·처음에서 넘치지 않게 자른다.
pub fn page_move(caret_y: f32, scroll: f32, view_h: f32, content_h: f32, down: bool) -> (f32, f32) {
    let step = (view_h - 24.0).max(view_h * 0.5).max(1.0); // 한 줄쯤 겹치게
    let max_scroll = (content_h - view_h).max(0.0);
    let (ny, ns) = if down { (caret_y + step, scroll + step) } else { (caret_y - step, scroll - step) };
    (ny.clamp(0.0, content_h.max(0.0)), ns.clamp(0.0, max_scroll))
}

/// 줄 번호(1부터)를 입력 글에서 읽는다. 숫자가 아니면 None, 범위를 넘으면 맨 끝 줄(0부터 줄 번호를 돌려줌). 0은 1로.
pub fn parse_goto(input: &str, total_lines: usize) -> Option<usize> {
    let t = input.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: usize = t.parse().unwrap_or(usize::MAX);
    Some(n.max(1).min(total_lines.max(1)) - 1)
}

/// 본문의 줄 수(개행 수 + 1).
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count() + 1
}

/// 한글 조합을 확정하는 Enter는 편집창이 그냥 줄바꿈만 넣으므로 목록 이어쓰기가 빠진다.
/// 그 줄바꿈 직후(커서가 빈 줄 맨 앞, 바로 윗줄이 목록·인용·체크박스 항목)이면 접두어를 채워 넣는다.
/// 돌려주는 것은 (새 본문, 새 커서 글자 위치). 해당 없으면 None.
pub fn continue_after_plain_enter(text: &str, caret: usize) -> Option<(String, usize)> {
    let bi = text.char_indices().nth(caret).map_or(text.len(), |(b, _)| b);
    let before = &text[..bi];
    if !before.ends_with('\n') || !(text[bi..].is_empty() || text[bi..].starts_with('\n') || text[bi..].starts_with('\r')) {
        return None;
    }
    let prev_all = &before[..before.len() - 1];
    let prev = prev_all[prev_all.rfind('\n').map_or(0, |p| p + 1)..].trim_end_matches('\r');
    // 윗줄이 코드 블록 안이면 이어쓰지 않는다
    let in_fence = prev_all.lines().filter(|l| l.trim_start().starts_with("```")).count() % 2 == 1;
    if in_fence {
        return None;
    }
    match crate::md::continue_list(prev) {
        Some(crate::md::Enter::Insert(p)) => {
            let mut t = String::with_capacity(text.len() + p.len());
            t.push_str(before);
            t.push_str(&p);
            t.push_str(&text[bi..]);
            Some((t, caret + p.chars().count()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_enter_after_ime_gets_list_prefix() {
        // "1. 가나" + Enter가 그냥 줄바꿈만 넣은 뒤: 커서는 빈 줄 맨 앞(글자 위치 6)
        assert_eq!(continue_after_plain_enter("1. 가나\n", 6), Some(("1. 가나\n2. ".to_string(), 9)));
        assert_eq!(continue_after_plain_enter("- 항목\r\n", 6), Some(("- 항목\r\n- ".to_string(), 8)));
        // 빈 줄이 아니거나 윗줄이 목록이 아니면 건드리지 않는다
        assert_eq!(continue_after_plain_enter("1. 가나\n다", 6), None);
        assert_eq!(continue_after_plain_enter("그냥 글\n", 5), None);
        assert_eq!(continue_after_plain_enter("```\n- 코드\n", 9), None);
        assert_eq!(continue_after_plain_enter("1. 가나", 5), None);
    }

    fn ind(t: &str, lo: usize, hi: usize, out: bool) -> (String, usize, usize) {
        let e = indent(t, lo, hi, out).expect("바뀌어야 함");
        (e.text, e.lo, e.hi)
    }

    // ---- 들여쓰기 ----
    #[test]
    fn tab_with_selection_keeps_text_and_selection() {
        // 회귀: 선택한 글이 지워지고 탭이 들어가던 오동작
        let t = "첫째\n둘째\n셋째";
        let (s, lo, hi) = ind(t, 0, 5, false); // "첫째\n둘째" 선택
        assert_eq!(s, "  첫째\n  둘째\n셋째");
        assert_eq!((lo, hi), (2, 9));
        assert!(s.contains("첫째") && s.contains("둘째"));
    }

    #[test]
    fn multiline_indent_uses_marker_width() {
        let t = "- a\n1. b\n10. c\n본문";
        let (s, ..) = ind(t, 0, t.chars().count(), false);
        assert_eq!(s, "  - a\n   1. b\n    10. c\n  본문");
    }

    #[test]
    fn selection_ending_at_line_start_excludes_that_line() {
        let t = "a\nb\nc";
        let (s, ..) = ind(t, 0, 4, false); // "a\nb\n" → 줄 0,1만
        assert_eq!(s, "  a\n  b\nc");
    }

    #[test]
    fn caret_on_list_item_indents_line_keeps_caret_char() {
        let t = "- 가나\n- 다라";
        let (s, lo, hi) = ind(t, 8, 8, false); // "다" 앞... 인덱스 8 = 2번째 줄 "- " 뒤
        assert_eq!(s, "- 가나\n  - 다라");
        assert_eq!((lo, hi), (10, 10));
    }

    #[test]
    fn caret_on_plain_line_inserts_four_spaces_at_caret() {
        let (s, lo, _) = ind("가나다", 1, 1, false);
        assert_eq!(s, "가    나다");
        assert_eq!(lo, 5);
    }

    #[test]
    fn caret_in_code_fence_inserts_four_spaces_at_caret() {
        let t = "```\nx = 1\n```";
        let (s, ..) = ind(t, 4, 4, false);
        assert_eq!(s, "```\n    x = 1\n```");
    }

    #[test]
    fn multiline_indent_skips_blank_lines() {
        let (s, ..) = ind("a\n\nb", 0, 4, false);
        assert_eq!(s, "  a\n\n  b");
    }

    #[test]
    fn shift_tab_removes_unit_and_clamps_selection() {
        let t = "  - a\n   1. b";
        let (s, lo, hi) = ind(t, 0, t.chars().count(), true);
        assert_eq!(s, "- a\n1. b");
        assert_eq!((lo, hi), (0, s.chars().count()));
    }

    #[test]
    fn shift_tab_removes_only_what_exists_and_none_when_nothing() {
        let (s, ..) = ind(" a", 0, 0, true);
        assert_eq!(s, "a");
        assert!(indent("a", 0, 0, true).is_none());
        let (s, ..) = ind("\ta", 1, 1, true);
        assert_eq!(s, "a");
    }

    #[test]
    fn indent_preserves_crlf() {
        let (s, ..) = ind("a\r\nb\r\n", 0, 4, false);
        assert_eq!(s, "  a\r\n  b\r\n");
    }

    // ---- 줄 이동 ----
    #[test]
    fn move_down_up_roundtrip_with_cursor_column() {
        let t = "가나다\nB\nC";
        let e = move_lines(t, 1, 1, false).unwrap();
        assert_eq!(e.text, "B\n가나다\nC");
        assert_eq!((e.lo, e.hi), (3, 3)); // 둘째 줄 칸 1
        let back = move_lines(&e.text, e.lo, e.hi, true).unwrap();
        assert_eq!(back.text, t);
        assert_eq!(back.lo, 1);
    }

    #[test]
    fn move_limits() {
        assert!(move_lines("a\nb", 0, 0, true).is_none());
        assert!(move_lines("a\nb", 2, 2, false).is_none());
    }

    #[test]
    fn move_last_line_without_newline_keeps_endings() {
        let e = move_lines("a\nb\nc", 4, 4, true).unwrap();
        assert_eq!(e.text, "a\nc\nb"); // 끝에 개행이 생기지 않는다
    }

    #[test]
    fn move_block_of_selected_lines_keeps_selection() {
        let t = "1\n2\n3\n4";
        let e = move_lines(t, 2, 5, false).unwrap(); // 줄 1~2
        assert_eq!(e.text, "1\n4\n2\n3");
        assert_eq!((e.lo, e.hi), (4, 7));
    }

    #[test]
    fn move_crlf_mixed_keeps_eol_positions() {
        let t = "a\r\nb\nc";
        let e = move_lines(t, 0, 0, false).unwrap();
        assert_eq!(e.text, "b\r\na\nc");
        assert!(!e.text.contains("\r\r"));
    }

    #[test]
    fn move_table_row_and_math_block_lines_as_plain_lines() {
        let t = "|a|b|\n|-|-|\n|1|2|\n$$\nx^2\n$$";
        let e = move_lines(t, 12, 12, false).unwrap();
        assert_eq!(e.text.lines().count(), 6);
        assert!(e.text.starts_with("|a|b|\n|-|-|\n$$\n|1|2|"));
    }

    // ---- 복제 ----
    #[test]
    fn duplicate_line_and_caret_on_copy() {
        let e = duplicate_lines("가나\n다", 1, 1);
        assert_eq!(e.text, "가나\n가나\n다");
        assert_eq!((e.lo, e.hi), (4, 4));
    }

    #[test]
    fn duplicate_last_line_without_newline() {
        let e = duplicate_lines("a\nb", 3, 3);
        assert_eq!(e.text, "a\nb\nb");
        assert_eq!(e.lo, 5);
    }

    #[test]
    fn duplicate_selected_lines_crlf() {
        let e = duplicate_lines("a\r\nb\r\nc", 0, 4);
        assert_eq!(e.text, "a\r\nb\r\na\r\nb\r\nc");
        let e = duplicate_lines("a\r\nb", 3, 3); // 마지막 줄: 줄 끝은 CRLF로
        assert_eq!(e.text, "a\r\nb\r\nb");
    }

    // ---- 삭제 ----
    #[test]
    fn delete_middle_line() {
        let e = delete_lines("a\nb\nc", 2, 2);
        assert_eq!(e.text, "a\nc");
        assert_eq!((e.lo, e.hi), (2, 2));
    }

    #[test]
    fn delete_last_line_drops_preceding_newline() {
        let e = delete_lines("a\nb", 3, 3);
        assert_eq!(e.text, "a");
        assert_eq!(e.lo, 0);
        let e = delete_lines("a\r\nb\r\nc", 6, 6);
        assert_eq!(e.text, "a\r\nb");
    }

    #[test]
    fn delete_only_line_and_selection() {
        assert_eq!(delete_lines("a", 0, 0).text, "");
        let e = delete_lines("1\n2\n3\n4", 2, 5); // 줄 1~2
        assert_eq!(e.text, "1\n4");
        assert_eq!(e.lo, 2);
    }

    // ---- 바꾸기 ----
    #[test]
    fn replace_all_counts_and_maps_cursor_korean_emoji() {
        let t = "가나 가나😀가나";
        let m = crate::md::find_all(t, "가나", true);
        let (s, n, c) = replace_matches(t, &m, "X", t.chars().count());
        assert_eq!(s, "X X😀X");
        assert_eq!(n, 3);
        assert_eq!(c, s.chars().count());
    }

    #[test]
    fn replace_with_empty_and_longer() {
        let m = crate::md::find_all("a-a", "a", true);
        assert_eq!(replace_matches("a-a", &m, "", 0).0, "-");
        assert_eq!(replace_matches("a-a", &m, "aa", 0).0, "aa-aa");
    }

    #[test]
    fn replace_case_insensitive_matches_from_find_all() {
        let t = "Abc aBC";
        let m = crate::md::find_all(t, "abc", false);
        let (s, n, _) = replace_matches(t, &m, "z", 0);
        assert_eq!((s.as_str(), n), ("z z", 2));
    }

    // ---- 서식 토글 ----
    #[test]
    fn bold_wrap_then_unwrap_selected_inner() {
        let e = toggle_wrap("가나다", 0, 3, "**", "**");
        assert_eq!(e.text, "**가나다**");
        assert_eq!((e.lo, e.hi), (2, 5));
        let back = toggle_wrap(&e.text, e.lo, e.hi, "**", "**"); // 안쪽 선택 → 걷어냄
        assert_eq!(back.text, "가나다");
        assert_eq!((back.lo, back.hi), (0, 3));
    }

    #[test]
    fn unwrap_when_selection_includes_markers() {
        let e = toggle_wrap("x **y** z", 2, 7, "**", "**");
        assert_eq!(e.text, "x y z");
        assert_eq!((e.lo, e.hi), (2, 3));
    }

    #[test]
    fn italic_does_not_strip_bold_but_wraps() {
        let e = toggle_wrap("**x**", 2, 3, "*", "*"); // x 선택, 겉이 굵게
        assert_eq!(e.text, "***x***");
        let e = toggle_wrap("**x**", 0, 5, "*", "*");
        assert_eq!(e.text, "***x***");
    }

    #[test]
    fn italic_toggles_off_and_bold_italic() {
        let e = toggle_wrap("*x*", 1, 2, "*", "*");
        assert_eq!(e.text, "x");
        let e = toggle_wrap("***x***", 3, 4, "*", "*");
        assert_eq!(e.text, "**x**");
        let e = toggle_wrap("***x***", 3, 4, "**", "**");
        assert_eq!(e.text, "*x*");
    }

    #[test]
    fn empty_selection_wraps_and_empty_pair_unwraps() {
        let e = toggle_wrap("ab", 1, 1, "**", "**");
        assert_eq!(e.text, "a****b");
        assert_eq!((e.lo, e.hi), (3, 3));
        let back = toggle_wrap(&e.text, 3, 3, "**", "**");
        assert_eq!(back.text, "ab");
    }

    // ---- 페이지 이동 ----
    #[test]
    fn page_move_keeps_screen_position_and_clamps() {
        let (y, s) = page_move(300.0, 100.0, 500.0, 5000.0, true);
        assert!((y - 300.0 - (s - 100.0)).abs() < 0.01); // 캐럿 화면 위치 유지
        let (y, s) = page_move(4900.0, 4400.0, 500.0, 5000.0, true);
        assert_eq!((y, s), (5000.0, 4500.0));
        let (y, s) = page_move(100.0, 50.0, 500.0, 5000.0, false);
        assert_eq!((y, s), (0.0, 0.0));
        let (y, s) = page_move(10.0, 0.0, 500.0, 300.0, true); // 문서가 화면보다 짧음
        assert_eq!(s, 0.0);
        assert!(y <= 300.0);
    }

    // ---- 줄 번호 ----
    #[test]
    fn goto_parsing() {
        assert_eq!(parse_goto("3", 10), Some(2));
        assert_eq!(parse_goto(" 12 ", 10), Some(9)); // 범위 밖 → 맨 끝 줄
        assert_eq!(parse_goto("0", 10), Some(0));
        assert_eq!(parse_goto("99999999999999999999999", 10), Some(9));
        assert_eq!(parse_goto("abc", 10), None);
        assert_eq!(parse_goto("", 10), None);
        assert_eq!(parse_goto("-3", 10), None);
        assert_eq!(line_count("a\nb\n"), 3);
    }

    // ---- 열 자르기 ----
    fn cut(t: &str, u: ColUnit, r: ColRange, keep: bool) -> Option<String> {
        cut_columns(t, 0, 0, u, r, keep).map(|e| e.text)
    }

    #[test]
    fn cut_chars_span_first_last() {
        use ColRange::*;
        let c = ColUnit::Char;
        assert_eq!(cut("abcdef", c, Span(2, 4), true).unwrap(), "bcd");
        assert_eq!(cut("abcdef", c, Span(4, 2), true).unwrap(), "bcd"); // 거꾸로 써도 같게
        assert_eq!(cut("abcdef", c, Last(3), true).unwrap(), "def");
        assert_eq!(cut("abcdef", c, First(2), true).unwrap(), "ab");
        assert_eq!(cut("abcdef", c, First(2), false).unwrap(), "cdef");
        assert_eq!(cut("abcdef", c, Last(2), false).unwrap(), "abcd");
        assert_eq!(cut("abcdef", c, Span(2, 4), false).unwrap(), "aef");
        assert_eq!(cut("가나다라", c, Span(2, 3), true).unwrap(), "나다"); // 한글은 한 글자씩
    }

    #[test]
    fn cut_tab_columns() {
        use ColRange::*;
        let t = ColUnit::Tab;
        assert_eq!(cut("a\tb\tc\td", t, Span(2, 3), true).unwrap(), "b\tc");
        assert_eq!(cut("a\tb\tc\td", t, Last(1), true).unwrap(), "d");
        assert_eq!(cut("a\tb\tc\td", t, First(1), false).unwrap(), "b\tc\td");
        assert_eq!(cut("a\tb\tc\td", t, Last(2), false).unwrap(), "a\tb");
    }

    #[test]
    fn cut_short_lines_and_scope_and_crlf() {
        use ColRange::*;
        let c = ColUnit::Char;
        assert_eq!(cut("ab", c, Span(5, 9), true).unwrap(), ""); // 줄 밖이면 남길 것이 없음
        assert!(cut("ab", c, Span(5, 9), false).is_none()); // 지울 것도 없음
        assert!(cut("ab", c, Last(5), true).is_none()); // 전부 남김 = 변화 없음
        let e = cut_columns("abc\r\ndef\r\nghi", 5, 8, c, First(1), true).unwrap(); // 둘째 줄만
        assert_eq!(e.text, "abc\r\nd\r\nghi");
        assert_eq!((e.lo, e.hi), (5, 6));
    }

    // ---- 공백 정리 ----
    #[test]
    fn respace_to_tab_collapses_runs_and_keeps_indent() {
        let e = respace("A   10  20\n  - 가 나\n", 0, 0, SpaceMode::ToTab, false).unwrap();
        assert_eq!(e.text, "A\t10\t20\n  -\t가\t나\n"); // 줄 앞 들여쓰기만 유지, 목록 기호 뒤 공백도 글자 사이로 본다
    }

    #[test]
    fn respace_remove_keeps_newlines_and_indent() {
        let e = respace("A 1 2\r\n  b  c\n", 0, 0, SpaceMode::Remove, false).unwrap();
        assert_eq!(e.text, "A12\r\n  bc\n");
    }

    #[test]
    fn respace_handles_fullwidth_nbsp_and_trailing() {
        let e = respace("a\u{3000}b\u{00A0}c   \nx", 0, 0, SpaceMode::ToTab, false).unwrap();
        assert_eq!(e.text, "a\tb\tc\nx");
        let e = respace("a \t b\t", 0, 0, SpaceMode::ToTab, false).unwrap();
        assert_eq!(e.text, "a\tb");
    }

    #[test]
    fn respace_protect_keeps_markdown_structure() {
        let src = "- 가 나\n  1. a  b\n> c  d\n- [ ] 할 일  하나\n| a | b |\n## 제목  글\n```\nx  y\n```\n본문  끝\n";
        let e = respace(src, 0, 0, SpaceMode::ToTab, true).unwrap();
        assert_eq!(e.text, "- 가\t나\n  1. a\tb\n> c\td\n- [ ] 할\t일\t하나\n| a | b |\n## 제목\t글\n```\nx  y\n```\n본문\t끝\n");
        let e = respace(src, 0, 0, SpaceMode::Remove, true).unwrap();
        assert!(e.text.starts_with("- 가나\n  1. ab\n> cd\n- [ ] 할일하나\n| a | b |\n"));
        assert!(e.text.contains("x  y"));
    }

    #[test]
    fn respace_protect_counts_fence_before_selection() {
        let src = "```\na  b\nc  d\n```\ne  f";
        // 선택이 코드 블록 안(둘째 줄)에서 시작해도 건드리지 않는다
        assert!(respace(src, 6, 8, SpaceMode::ToTab, true).is_none());
        let e = respace(src, 0, 0, SpaceMode::ToTab, true).unwrap();
        assert_eq!(e.text, "```\na  b\nc  d\n```\ne\tf");
    }

    #[test]
    fn line_ops_sort_dedup_number() {
        let e = line_op("c\nb\na\n", 0, 0, LineOp::SortAsc).unwrap();
        assert_eq!(e.text, "\na\nb\nc");
        let e = line_op("b\r\na\r\nc", 0, 0, LineOp::SortDesc).unwrap();
        assert_eq!(e.text, "c\r\nb\r\na");
        let e = line_op("x\ny\nx\n\n\nz\ny", 0, 0, LineOp::Dedup).unwrap();
        assert_eq!(e.text, "x\ny\n\n\nz");
        let e = line_op("가\n\n나", 0, 0, LineOp::Number).unwrap();
        assert_eq!(e.text, "1. 가\n\n2. 나");
        // 선택한 줄만
        let e = line_op("z\nb\na\nq", 2, 5, LineOp::SortAsc).unwrap();
        assert_eq!(e.text, "z\na\nb\nq");
        assert!(line_op("a\nb", 0, 0, LineOp::SortAsc).is_none());
        assert!(line_op("a\nb", 0, 0, LineOp::Dedup).is_none());
    }

    #[test]
    fn abbrev_expands_longest_after_space() {
        let sn = vec![("~a".to_string(), "AA".to_string()), ("x~a".to_string(), "XX".to_string())];
        let e = expand_abbrev("hi ~a bye", 5, &sn).unwrap();
        assert_eq!(e.text, "hi AA bye");
        assert_eq!((e.lo, e.hi), (5, 5));
        assert!(expand_abbrev("hi~a", 4, &sn).is_none()); // 앞이 공백이 아님
        assert_eq!(expand_abbrev("x~a", 3, &sn).unwrap().text, "XX"); // 긴 이름 우선
        assert_eq!(expand_abbrev("~a", 2, &sn).unwrap().text, "AA");
        assert!(expand_abbrev("zz", 2, &sn).is_none());
        let e = replace_range("가나다", 1, 2, "XY");
        assert_eq!((e.text.as_str(), e.lo), ("가XY다", 3));
    }

    #[test]
    fn respace_selection_limits_to_lines_and_none_when_unchanged() {
        let e = respace("a b\nc d\ne f", 4, 7, SpaceMode::ToTab, false).unwrap(); // 둘째 줄만
        assert_eq!(e.text, "a b\nc\td\ne f");
        assert_eq!((e.lo, e.hi), (4, 7));
        assert!(respace("ab\ncd", 0, 0, SpaceMode::ToTab, false).is_none());
        assert!(respace("a\tb", 0, 0, SpaceMode::Remove, false).is_none()); // 탭은 공백으로 지우지 않음
    }
}
