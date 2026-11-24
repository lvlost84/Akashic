//! 태그 입력칸의 자동완성: 마지막으로 치는 태그 조각 찾기, 후보 거르기, 고른 태그로 바꾸기(화면과 무관한 순수 함수).

/// 후보 목록에 보일 최대 개수.
pub const MAX_ITEMS: usize = 8;

fn is_sep(c: char) -> bool {
    c == ',' || c.is_whitespace()
}

/// 입력칸 글에서 마지막 태그 조각의 시작 바이트 위치와 그 글. 끝이 구분자(쉼표·공백)면 빈 조각이다.
pub fn last_token(text: &str) -> (usize, &str) {
    let start = text.rfind(is_sep).map_or(0, |p| p + text[p..].chars().next().map_or(1, |c| c.len_utf8()));
    (start, &text[start..])
}

/// 후보 태그: 마지막 조각이 들어 있는 태그(대소문자 무시)만, 조각으로 시작하는 것 먼저(받은 순서 유지).
/// 이 문서에 이미 있는 태그와 입력칸에 이미 쓴 태그는 뺀다. 조각이 비면 후보 없음.
pub fn candidates(all: &[String], doc_tags: &[String], text: &str, limit: usize) -> Vec<String> {
    let (start, tok) = last_token(text);
    let q = tok.trim_start_matches('#').to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let typed: Vec<String> = text[..start].split(is_sep).filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('#').to_lowercase()).collect();
    let (mut starts, mut inside) = (Vec::new(), Vec::new());
    for t in all {
        let lower = t.to_lowercase();
        if !lower.contains(&q) || lower == q && doc_tags.iter().any(|d| d.to_lowercase() == lower) {
            continue;
        }
        if doc_tags.iter().any(|d| d.to_lowercase() == lower) || typed.contains(&lower) {
            continue;
        }
        if lower.starts_with(&q) { starts.push(t.clone()) } else { inside.push(t.clone()) }
    }
    starts.extend(inside);
    starts.truncate(limit);
    starts
}

/// 마지막 조각을 고른 태그로 바꾸고 뒤에 공백을 붙인다(다음 태그를 바로 치도록).
pub fn complete(text: &str, tag: &str) -> String {
    let (start, _) = last_token(text);
    format!("{}{} ", &text[..start], tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn last_token_splits_on_comma_and_space() {
        assert_eq!(last_token("품질"), (0, "품질"));
        assert_eq!(last_token("a, 품질"), (3, "품질"));
        assert_eq!(last_token("a,"), (2, ""));
        assert_eq!(last_token("가 나 "), (8, "")); // 바이트 위치
    }

    #[test]
    fn candidates_prefix_first_and_skip_existing() {
        let all = v(&["측정", "품질", "품질관리", "Quality", "검품"]);
        assert_eq!(candidates(&all, &[], "품", 8), v(&["품질", "품질관리", "검품"]));
        assert_eq!(candidates(&all, &v(&["품질"]), "품", 8), v(&["품질관리", "검품"]));
        assert_eq!(candidates(&all, &[], "QUAL", 8), v(&["Quality"]));
        assert_eq!(candidates(&all, &[], "품질관리, 품", 8), v(&["품질", "검품"]));
        assert_eq!(candidates(&all, &[], "품질, 품질", 8), v(&["품질관리"]));
        assert!(candidates(&all, &[], "", 8).is_empty());
        assert!(candidates(&all, &[], "품 ", 8).is_empty());
        assert_eq!(candidates(&all, &[], "품", 1).len(), 1);
    }

    #[test]
    fn complete_replaces_last_token() {
        assert_eq!(complete("a, 품", "품질"), "a, 품질 ");
        assert_eq!(complete("품", "품질"), "품질 ");
    }
}
