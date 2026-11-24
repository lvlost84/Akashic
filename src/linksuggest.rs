//! `[[` 뒤에 글자를 치는 동안 보여 줄 문서 제목 후보: 커서 앞의 입력 조각 찾기와 후보 거르기(화면과 무관한 순수 함수).

/// 후보 목록에 보일 최대 개수.
pub const MAX_ITEMS: usize = 8;

/// 커서(글자 위치) 바로 앞이 `[[입력중`이면 (입력 시작 위치, 입력한 글)을 돌려준다.
/// 시작 위치는 `[[` 바로 뒤의 글자 위치. 같은 줄에서 `[[` 뒤에 `]`·`[`·`|`가 끼었거나, 입력이 공백뿐·비었거나,
/// `file:`로 시작하거나, 코드 블록·인라인 코드 안이면 None. 입력이 한 글자(공백 제외)라도 있어야 후보를 낸다.
pub fn query_at(text: &str, caret: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let line_start = chars[..caret].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
    let line = &chars[line_start..caret];
    // `[[`를 오른쪽에서부터 찾는다(가장 가까운 것). 그 사이에 닫는/구분 글자가 있으면 이미 끝난 링크다.
    let mut open = None;
    let mut i = line.len();
    while i >= 2 {
        if line[i - 2] == '[' && line[i - 1] == '[' {
            open = Some(i);
            break;
        }
        i -= 1;
    }
    let open = open?;
    let q: String = line[open..].iter().collect();
    if q.chars().any(|c| matches!(c, ']' | '[' | '|')) {
        return None;
    }
    if q.trim().is_empty() || q.trim_start().starts_with("file:") {
        return None;
    }
    // 인라인 코드 안(같은 줄에서 ` 개수가 홀수)이면 링크가 아니다
    if line[..open].iter().filter(|&&c| c == '`').count() % 2 == 1 {
        return None;
    }
    // 코드 블록 안(앞에서 ``` 또는 ~~~ 펜스가 홀수 번 나옴)이면 링크가 아니다
    let before: String = chars[..line_start].iter().collect();
    let fences = before.lines().filter(|l| { let t = l.trim_start(); t.starts_with("```") || t.starts_with("~~~") }).count();
    if fences % 2 == 1 {
        return None;
    }
    Some((line_start + open, q))
}

/// 후보 제목: 입력한 글이 들어 있는 제목(대소문자 무시)만, 제목이 입력으로 시작하는 것을 먼저, 그 안에서는 받은 순서(최근 수정순) 그대로.
/// `[`·`]`·`|`·줄바꿈이 든 제목(링크를 깨뜨림), 현재 문서(`cur`), 빈 제목은 뺀다. 같은 제목은 한 번만.
pub fn candidates(titles: &[(i64, String)], query: &str, cur: Option<i64>, limit: usize) -> Vec<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut starts: Vec<String> = Vec::new();
    let mut inside: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (id, t) in titles {
        let t = t.trim();
        if t.is_empty() || Some(*id) == cur || t.contains(['[', ']', '|', '\n', '\r']) {
            continue;
        }
        let lower = t.to_lowercase();
        if !lower.contains(&q) {
            continue;
        }
        let is_start = lower.starts_with(&q);
        if !seen.insert(lower) {
            continue;
        }
        if is_start {
            starts.push(t.to_string());
            if starts.len() >= limit {
                break; // 입력으로 시작하는 것이 한도만큼 모였으면 나머지는 어차피 잘린다
            }
        } else if inside.len() < limit {
            inside.push(t.to_string());
        }
    }
    starts.extend(inside);
    starts.truncate(limit);
    starts
}

/// 고른 제목으로 입력 조각(`start`~`caret`)을 바꾼 새 본문과 새 커서 위치. 뒤에 `]]`가 이미 있으면 다시 쓰지 않고 그 뒤로 보낸다.
pub fn accept(text: &str, start: usize, caret: usize, title: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let start = start.min(caret);
    let mut out: String = chars[..start].iter().collect();
    out.push_str(title);
    let mut pos = start + title.chars().count();
    if chars[caret..].starts_with(&[']', ']']) {
        out.extend(chars[caret..].iter());
        pos += 2;
    } else {
        out.push_str("]]");
        out.extend(chars[caret..].iter());
        pos += 2;
    }
    (out, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(v: &[&str]) -> Vec<(i64, String)> {
        v.iter().enumerate().map(|(i, s)| (i as i64 + 1, s.to_string())).collect()
    }

    #[test]
    fn query_needs_at_least_one_char() {
        assert_eq!(query_at("[[", 2), None);
        assert_eq!(query_at("[[ ", 3), None);
        assert_eq!(query_at("[[가", 3), Some((2, "가".into())));
        assert_eq!(query_at("앞 글 [[품질 관", 10), Some((6, "품질 관".into())));
    }

    #[test]
    fn query_stops_at_closing_alias_or_other_line() {
        assert_eq!(query_at("[[제목]] 다음", 9), None);
        assert_eq!(query_at("[[제목|보일", 8), None);
        assert_eq!(query_at("[[첫째\n둘째", 8), None);
        assert_eq!(query_at("[[file:abc", 10), None);
        assert_eq!(query_at("[a[[b", 5), Some((4, "b".into())));
    }

    #[test]
    fn query_ignores_code() {
        assert_eq!(query_at("`[[가", 4), None);
        assert_eq!(query_at("```\n[[가", 8), None);
        assert_eq!(query_at("```\nx\n```\n[[가", 14), Some((12, "가".into())));
    }

    #[test]
    fn candidates_prefix_first_case_insensitive_and_filtered() {
        let v = t(&["측정 품질", "품질 관리", "Quality", "품질|별칭", "품질[x]", "", "품질 관리"]);
        assert_eq!(candidates(&v, "품질", None, 8), vec!["품질 관리".to_string(), "측정 품질".to_string()]);
        assert_eq!(candidates(&v, "QUAL", None, 8), vec!["Quality".to_string()]);
        assert_eq!(candidates(&v[..6], "품질", Some(2), 8), vec!["측정 품질".to_string()]);
        assert_eq!(candidates(&v, " ", None, 8), Vec::<String>::new());
        assert_eq!(candidates(&v, "품", None, 1).len(), 1);
    }

    #[test]
    fn accept_closes_or_reuses_brackets() {
        assert_eq!(accept("a [[품", 4, 5, "품질 관리"), ("a [[품질 관리]]".to_string(), 11));
        assert_eq!(accept("[[품]] 끝", 2, 3, "품질"), ("[[품질]] 끝".to_string(), 6));
        assert_eq!(accept("[[품 끝", 2, 3, "품질"), ("[[품질]] 끝".to_string(), 6));
    }
}
