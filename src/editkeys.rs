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

#[cfg(test)]
mod tests {
    use super::*;

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
}
