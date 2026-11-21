//! 마크다운 → HTML 변환(문서 하나를 HTML·PDF로 내보낼 때 쓴다). 외부 크레이트 없이 아카식이 쓰는 문법만 다룬다.
//! 모든 글은 HTML 이스케이프하고, 링크 주소는 http(s)·mailto·상대 경로·# 만 허용한다.

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn safe_url(u: &str) -> Option<String> {
    let u = u.trim();
    let l = u.to_ascii_lowercase();
    let ok = l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:") || l.starts_with('#') || (!l.contains(':') && !l.starts_with("//"));
    if ok { Some(esc(u)) } else { None }
}

/// 한 줄(또는 문단) 안의 서식.
pub fn inline(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let mut o = String::new();
    let mut i = 0;
    let find = |from: usize, pat: &[char]| -> Option<usize> { (from..ch.len().saturating_sub(pat.len() - 1)).find(|&k| ch[k..k + pat.len()] == *pat) };
    while i < ch.len() {
        let c = ch[i];
        // 코드
        if c == '`' {
            if let Some(e) = find(i + 1, &['`']) {
                o.push_str(&format!("<code>{}</code>", esc(&ch[i + 1..e].iter().collect::<String>())));
                i = e + 1;
                continue;
            }
        }
        // 굵게
        if c == '*' && ch.get(i + 1) == Some(&'*') {
            if let Some(e) = find(i + 2, &['*', '*']) {
                if e > i + 2 {
                    o.push_str(&format!("<strong>{}</strong>", inline(&ch[i + 2..e].iter().collect::<String>())));
                    i = e + 2;
                    continue;
                }
            }
        }
        // 취소선
        if c == '~' && ch.get(i + 1) == Some(&'~') {
            if let Some(e) = find(i + 2, &['~', '~']) {
                if e > i + 2 {
                    o.push_str(&format!("<del>{}</del>", inline(&ch[i + 2..e].iter().collect::<String>())));
                    i = e + 2;
                    continue;
                }
            }
        }
        // 기울임
        if c == '*' && ch.get(i + 1).is_some_and(|n| !n.is_whitespace() && *n != '*') {
            let mut k = i + 1;
            let mut end = None;
            while k < ch.len() {
                if ch[k] == '*' && ch[k - 1] != ' ' && ch.get(k + 1) != Some(&'*') && ch[k - 1] != '*' {
                    end = Some(k);
                    break;
                }
                k += 1;
            }
            if let Some(e) = end {
                o.push_str(&format!("<em>{}</em>", inline(&ch[i + 1..e].iter().collect::<String>())));
                i = e + 1;
                continue;
            }
        }
        // 위키 링크 [[제목]] / [[제목|표시]]
        if c == '[' && ch.get(i + 1) == Some(&'[') {
            if let Some(e) = find(i + 2, &[']', ']']) {
                let inner: String = ch[i + 2..e].iter().collect();
                let shown = inner.split_once('|').map(|x| x.1).unwrap_or(&inner);
                o.push_str(&format!("<span class=\"wiki\">{}</span>", esc(shown)));
                i = e + 2;
                continue;
            }
        }
        // 각주 참조 [^1]
        if c == '[' && ch.get(i + 1) == Some(&'^') {
            if let Some(e) = find(i + 2, &[']']) {
                o.push_str(&format!("<sup>{}</sup>", esc(&ch[i + 2..e].iter().collect::<String>())));
                i = e + 1;
                continue;
            }
        }
        // 링크 [글](주소)
        if c == '[' {
            if let Some(e) = find(i + 1, &[']']) {
                if ch.get(e + 1) == Some(&'(') {
                    let close = {
                        let mut depth = 0;
                        (e + 2..ch.len()).find(|&k| match ch[k] {
                            '(' => {
                                depth += 1;
                                false
                            }
                            ')' if depth > 0 => {
                                depth -= 1;
                                false
                            }
                            ')' => true,
                            _ => false,
                        })
                    };
                    if let Some(p) = close {
                        let text: String = ch[i + 1..e].iter().collect();
                        let url: String = ch[e + 2..p].iter().collect();
                        match safe_url(url.split_whitespace().next().unwrap_or("")) {
                            Some(u) => o.push_str(&format!("<a href=\"{u}\">{}</a>", inline(&text))),
                            None => o.push_str(&inline(&text)),
                        }
                        i = p + 1;
                        continue;
                    }
                }
            }
        }
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
        i += 1;
    }
    o
}

fn is_hr(t: &str) -> bool {
    let t = t.trim();
    t.len() >= 3 && ["-", "*", "_"].iter().any(|m| t.chars().all(|c| c.to_string() == *m || c == ' ') && t.matches(m).count() >= 3)
}

fn heading(t: &str) -> Option<(usize, &str)> {
    let n = t.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&n) && t[n..].starts_with(' ') { Some((n, t[n..].trim())) } else { None }
}

fn is_fence(t: &str) -> bool {
    let t = t.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 목록 줄이면 (들여쓰기 폭, 숫자 목록인가, 기호 뒤 내용)
fn list_item(line: &str) -> Option<(usize, bool, &str)> {
    let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let t = line.trim_start_matches([' ', '\t']);
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            return Some((indent, false, r));
        }
    }
    let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if d > 0 && d <= 9 {
        let r = &t[d..];
        if let Some(r) = r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")) {
            return Some((indent, true, r));
        }
    }
    None
}

fn split_row(l: &str) -> Vec<String> {
    let t = l.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_table_sep(l: &str) -> bool {
    let t = l.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// 본문 전체를 HTML 조각으로. `img(해시)`는 그 그림의 src(보통 data URI)를 돌려준다.
pub fn to_html(body: &str, img: &dyn Fn(&str) -> Option<String>) -> String {
    let lines: Vec<&str> = body.lines().collect();
    block(&lines, img)
}

fn block(lines: &[&str], img: &dyn Fn(&str) -> Option<String>) -> String {
    let mut o = String::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        // 코드 블록
        if is_fence(l) {
            let fence: String = t.chars().take(3).collect();
            let lang = esc(t[3..].trim());
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(&fence) {
                code.push(lines[i]);
                i += 1;
            }
            i += 1; // 닫는 울타리
            let cls = if lang.is_empty() { String::new() } else { format!(" class=\"lang-{lang}\"") };
            o.push_str(&format!("<pre><code{cls}>{}</code></pre>\n", esc(&code.join("\n"))));
            continue;
        }
        // 수식 블록 $$ … $$
        if t.starts_with("$$") {
            let mut m = Vec::new();
            let first = t.trim_start_matches('$');
            if first.ends_with("$$") && !first.is_empty() {
                m.push(first.trim_end_matches('$'));
                i += 1;
            } else {
                if !first.is_empty() {
                    m.push(first);
                }
                i += 1;
                while i < lines.len() && !lines[i].trim_end().ends_with("$$") {
                    m.push(lines[i]);
                    i += 1;
                }
                if i < lines.len() {
                    m.push(lines[i].trim_end().trim_end_matches('$'));
                    i += 1;
                }
            }
            o.push_str(&format!("<pre class=\"math\">{}</pre>\n", esc(&m.join("\n"))));
            continue;
        }
        if let Some((n, text)) = heading(t) {
            o.push_str(&format!("<h{n}>{}</h{n}>\n", inline(text)));
            i += 1;
            continue;
        }
        if is_hr(t) {
            o.push_str("<hr>\n");
            i += 1;
            continue;
        }
        // 그림 줄
        if let Some(refs) = crate::md::parse_image_row(t) {
            o.push_str("<p class=\"imgs\">");
            for r in refs {
                match img(&r.hash) {
                    Some(src) => o.push_str(&format!("<img src=\"{src}\" alt=\"{}\"> ", esc(&r.alt))),
                    None => o.push_str(&format!("<span class=\"noimg\">[그림: {}]</span> ", esc(&r.alt))),
                }
            }
            o.push_str("</p>\n");
            i += 1;
            continue;
        }
        // 표
        if t.contains('|') && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
            let head = split_row(t);
            let aligns: Vec<&str> = split_row(lines[i + 1])
                .iter()
                .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
                    (true, true) => "center",
                    (false, true) => "right",
                    (true, false) => "left",
                    _ => "",
                })
                .collect();
            let cell = |tag: &str, k: usize, txt: &str| {
                let a = aligns.get(k).copied().unwrap_or("");
                let st = if a.is_empty() { String::new() } else { format!(" style=\"text-align:{a}\"") };
                format!("<{tag}{st}>{}</{tag}>", inline(txt))
            };
            o.push_str("<table>\n<thead><tr>");
            for (k, h) in head.iter().enumerate() {
                o.push_str(&cell("th", k, h));
            }
            o.push_str("</tr></thead>\n<tbody>\n");
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                o.push_str("<tr>");
                for (k, c) in split_row(lines[i]).iter().enumerate() {
                    o.push_str(&cell("td", k, c));
                }
                o.push_str("</tr>\n");
                i += 1;
            }
            o.push_str("</tbody></table>\n");
            continue;
        }
        // 인용
        if t.starts_with('>') {
            let mut inner: Vec<String> = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let s = lines[i].trim_start().trim_start_matches('>');
                inner.push(s.strip_prefix(' ').unwrap_or(s).to_string());
                i += 1;
            }
            let refs: Vec<&str> = inner.iter().map(|s| s.as_str()).collect();
            o.push_str(&format!("<blockquote>\n{}</blockquote>\n", block(&refs, img)));
            continue;
        }
        // 목록
        if list_item(l).is_some() {
            let mut j = i;
            while j < lines.len() && !lines[j].trim().is_empty() && (list_item(lines[j]).is_some() || lines[j].starts_with(' ') || lines[j].starts_with('\t')) {
                j += 1;
            }
            o.push_str(&list(&lines[i..j]));
            i = j;
            continue;
        }
        // 문단: 다음 빈 줄이나 다른 블록이 시작할 때까지(줄바꿈은 그대로 보임)
        let mut p = Vec::new();
        while i < lines.len() {
            let x = lines[i];
            let xt = x.trim();
            if xt.is_empty() || is_fence(x) || heading(xt).is_some() || is_hr(xt) || xt.starts_with('>') || list_item(x).is_some() || xt.starts_with("$$") {
                break;
            }
            p.push(inline(xt));
            i += 1;
        }
        if p.is_empty() {
            p.push(inline(t));
            i += 1;
        }
        o.push_str(&format!("<p>{}</p>\n", p.join("<br>\n")));
    }
    o
}

fn list(lines: &[&str]) -> String {
    // 들여쓰기 기준으로 중첩 목록을 만든다.
    fn build(items: &[(usize, bool, String)], k: &mut usize, indent: usize, o: &mut String) {
        let ordered = items[*k].1;
        let tag = if ordered { "ol" } else { "ul" };
        o.push_str(&format!("<{tag}>\n"));
        while *k < items.len() && items[*k].0 >= indent {
            let (ind, _, text) = &items[*k];
            if *ind > indent {
                // 더 깊은 항목은 바로 앞 항목 안으로
                build(items, k, *ind, o);
                continue;
            }
            let (cls, txt) = if let Some(r) = text.strip_prefix("[ ] ") {
                (" class=\"task\"", format!("<input type=\"checkbox\" disabled> {}", inline(r)))
            } else if let Some(r) = text.strip_prefix("[x] ").or_else(|| text.strip_prefix("[X] ")) {
                (" class=\"task\"", format!("<input type=\"checkbox\" checked disabled> {}", inline(r)))
            } else {
                ("", inline(text))
            };
            o.push_str(&format!("<li{cls}>{txt}"));
            *k += 1;
            if *k < items.len() && items[*k].0 > indent {
                o.push('\n');
                build(items, k, items[*k].0, o);
            }
            o.push_str("</li>\n");
        }
        o.push_str(&format!("</{tag}>\n"));
    }
    let items: Vec<(usize, bool, String)> = lines.iter().filter_map(|l| list_item(l).map(|(i, o, t)| (i, o, t.trim().to_string()))).collect();
    if items.is_empty() {
        return String::new();
    }
    let base = items.iter().map(|x| x.0).min().unwrap_or(0);
    let mut o = String::new();
    let mut k = 0;
    while k < items.len() {
        build(&items, &mut k, base, &mut o);
    }
    o
}

/// base64(표준, 패딩 있음)
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    o
}

/// 완성된 HTML 문서(스타일 포함, 인쇄에 알맞음).
pub fn page(title: &str, inner: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"ko\"><head><meta charset=\"utf-8\"><title>{t}</title>\n<style>\n{CSS}</style></head>\n<body><main>\n<h1 class=\"doc-title\">{t}</h1>\n{inner}</main></body></html>\n",
        t = esc(title)
    )
}

const CSS: &str = "body{font-family:'Malgun Gothic','Segoe UI',sans-serif;line-height:1.7;color:#222;background:#fff;margin:0}
main{max-width:820px;margin:0 auto;padding:32px 28px}
h1,h2,h3,h4,h5,h6{line-height:1.3;margin:1.4em 0 .5em}
h1.doc-title{margin-top:0;border-bottom:2px solid #ddd;padding-bottom:.3em}
p{margin:.6em 0}
code{background:#f3f3f3;padding:.1em .35em;border-radius:3px;font-family:Consolas,monospace;font-size:.92em}
pre{background:#f6f6f6;padding:12px 14px;border-radius:6px;overflow:auto;white-space:pre-wrap}
pre code{background:none;padding:0}
pre.math{font-family:'Cambria Math',Consolas,monospace;text-align:center;background:#fafafa}
blockquote{margin:.8em 0;padding:.1em 1em;border-left:4px solid #bbb;color:#555}
table{border-collapse:collapse;margin:.8em 0}
th,td{border:1px solid #ccc;padding:5px 10px}
th{background:#f0f0f0}
li.task{list-style:none;margin-left:-1.2em}
img{max-width:100%;height:auto}
.wiki{color:#2a6;border-bottom:1px dotted #2a6}
a{color:#1a5fb4}
hr{border:0;border-top:1px solid #ccc;margin:1.4em 0}
@media print{main{max-width:none;padding:0}pre{page-break-inside:avoid}h1,h2,h3{page-break-after:avoid}}
";

#[cfg(test)]
mod tests {
    use super::*;
    fn h(b: &str) -> String {
        to_html(b, &|_| None)
    }

    #[test]
    fn inline_formats_and_escapes() {
        assert_eq!(inline("a **b** *c* ~~d~~ `<x>` & e"), "a <strong>b</strong> <em>c</em> <del>d</del> <code>&lt;x&gt;</code> &amp; e");
        assert_eq!(inline("[링크](https://a.b/c?x=1&y=2)"), "<a href=\"https://a.b/c?x=1&amp;y=2\">링크</a>");
        assert_eq!(inline("[악성](javascript:alert(1))"), "악성");
        assert_eq!(inline("[[문서|별칭]] 와 [[그냥]]"), "<span class=\"wiki\">별칭</span> 와 <span class=\"wiki\">그냥</span>");
        assert_eq!(inline("2 * 3 * 4"), "2 * 3 * 4");
        assert_eq!(inline("a_b_c"), "a_b_c");
    }

    #[test]
    fn blocks_headings_lists_tables_code() {
        let s = h("# 제목\n\n본문 1\n본문 2\n\n- a\n  - b\n- [x] 끝\n\n1. 하나\n2. 둘\n\n| A | B |\n|:-:|--:|\n| 1 | 2 |\n\n> 인용\n\n```rust\nlet a = <1>;\n```\n\n---");
        assert!(s.contains("<h1>제목</h1>"));
        assert!(s.contains("<p>본문 1<br>\n본문 2</p>"));
        assert!(s.contains("<ul>\n<li>a\n<ul>\n<li>b</li>\n</ul>\n</li>"), "{s}");
        assert!(s.contains("<li class=\"task\"><input type=\"checkbox\" checked disabled> 끝</li>"));
        assert!(s.contains("<ol>\n<li>하나</li>\n<li>둘</li>\n</ol>"));
        assert!(s.contains("<th style=\"text-align:center\">A</th>") && s.contains("<td style=\"text-align:right\">2</td>"));
        assert!(s.contains("<blockquote>\n<p>인용</p>\n</blockquote>"));
        assert!(s.contains("<pre><code class=\"lang-rust\">let a = &lt;1&gt;;</code></pre>"));
        assert!(s.contains("<hr>"));
    }

    #[test]
    fn image_rows_use_callback_and_base64_works() {
        let b = "![사진](img:0123456789abcdef 8x6)";
        let s = to_html(b, &|h| Some(format!("data:x/{h}")));
        assert!(s.contains("<img src=\"data:x/0123456789abcdef\" alt=\"사진\">"));
        assert!(h(b).contains("[그림: 사진]"));
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn page_escapes_title() {
        assert!(page("<b>&", "").contains("<title>&lt;b&gt;&amp;</title>"));
    }
}
