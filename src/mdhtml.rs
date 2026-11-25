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
    if let Some(w) = crate::md::safe_url(u).filter(|_| u.to_ascii_lowercase().starts_with("www.")) {
        return Some(esc(&w));
    }
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
                let target = inner.split('|').next().unwrap_or("");
                match crate::md::safe_url(target) {
                    Some(u) => o.push_str(&format!("<a href=\"{}\">{}</a>", esc(&u), esc(shown))),
                    None => o.push_str(&format!("<span class=\"wiki\">{}</span>", esc(shown))),
                }
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
        // 맨 주소 (앞 글자가 영숫자가 아닐 때만)
        if matches!(c, 'h' | 'H' | 'w' | 'W' | 'm' | 'M') && (i == 0 || !ch[i - 1].is_alphanumeric()) {
            let rest: String = ch[i..].iter().collect();
            if let Some(len) = crate::md::bare_url_len(&rest) {
                let text = &rest[..len];
                if let Some(u) = crate::md::safe_url(text) {
                    o.push_str(&format!("<a href=\"{}\">{}</a>", esc(&u), esc(text)));
                    i += text.chars().count();
                    continue;
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

/// 목록 줄이면 (들여쓰기 폭, 숫자 목록인가, 기호 뒤 내용). 기호 판정은 화면(`md::list_marker_len`)과 같다.
fn list_item(line: &str) -> Option<(usize, bool, &str)> {
    let m = crate::md::list_marker_len(line)?;
    let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let ordered = line.trim_start().starts_with(|c: char| c.is_ascii_digit());
    Some((indent, ordered, &line[m..]))
}

fn split_row(l: &str) -> Vec<String> {
    let t = l.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// 본문 전체를 HTML 조각으로. `img(해시)`는 그 그림의 src(보통 data URI)를 돌려준다.
pub fn to_html(body: &str, img: &dyn Fn(&str) -> Option<String>, math: &dyn Fn(&crate::math::Node) -> Option<String>) -> String {
    let lines: Vec<&str> = body.lines().collect();
    block(&lines, img, math)
}

/// 블록 경계(코드·표·수식·그림·가로줄)는 화면과 같은 `md::scan_blocks`가 정한다. 여기서는 그 구간을 HTML로 바꾸고,
/// 제목·인용·목록·문단만 직접 읽는다.
fn block(lines: &[&str], img: &dyn Fn(&str) -> Option<String>, math: &dyn Fn(&crate::math::Node) -> Option<String>) -> String {
    use crate::md::{Block, BlockKind};
    let blocks: Vec<Block> = crate::md::scan_blocks(&lines.join("\n")).into_iter().filter(|b| !matches!(b.kind, BlockKind::Check { .. })).collect();
    let starts_block = |n: usize| blocks.iter().any(|b| b.start == n);
    let mut o = String::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(b) = blocks.iter().find(|b| b.start == i) {
            match &b.kind {
                BlockKind::Code { lang, closed } => {
                    let body_end = if *closed { b.end } else { b.end + 1 };
                    let code = &lines[i + 1..body_end.min(lines.len())];
                    let lang = esc(lang);
                    let cls = if lang.is_empty() { String::new() } else { format!(" class=\"lang-{lang}\"") };
                    o.push_str(&format!("<pre><code{cls}>{}</code></pre>\n", esc(&code.join("\n"))));
                }
                // 파싱한 수식은 MathML(또는 화면과 같은 SVG)로 내보낸다.
                BlockKind::Math { node } => o.push_str(&math_div(node, math)),
                BlockKind::Hr => o.push_str("<hr>\n"),
                BlockKind::Image { items } => {
                    o.push_str("<p class=\"imgs\">");
                    for r in items {
                        match img(&r.hash) {
                            Some(src) => o.push_str(&format!("<img src=\"{src}\" alt=\"{}\"> ", esc(&r.alt))),
                            None => o.push_str(&format!("<span class=\"noimg\">[그림: {}]</span> ", esc(&r.alt))),
                        }
                    }
                    o.push_str("</p>\n");
                }
                BlockKind::Table { rows, .. } => {
                    // 열 맞춤은 구분선 줄의 `:` 위치로 정한다(`:---`와 `---`를 구별하려고 원문을 본다).
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
                    for (k, h) in rows[0].iter().enumerate() {
                        o.push_str(&cell("th", k, h));
                    }
                    o.push_str("</tr></thead>\n<tbody>\n");
                    for r in &rows[1..] {
                        o.push_str("<tr>");
                        for (k, c) in r.iter().enumerate() {
                            o.push_str(&cell("td", k, c));
                        }
                        o.push_str("</tr>\n");
                    }
                    o.push_str("</tbody></table>\n");
                }
                BlockKind::Check { .. } => {}
            }
            i = b.end + 1;
            continue;
        }
        // 틀린 수식(`$$`로 감쌌지만 읽을 수 없음)은 원문 그대로 보인다.
        if let Some(j) = crate::md::math_fence_end(lines, i) {
            o.push_str(&format!("<pre class=\"math\">{}</pre>\n", esc(&lines[i + 1..j].join("\n"))));
            i = j + 1;
            continue;
        }
        if let Some((n, text)) = crate::md::heading_parts(l) {
            o.push_str(&format!("<h{n}>{}</h{n}>\n", inline(text.trim())));
            i += 1;
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
            o.push_str(&format!("<blockquote>\n{}</blockquote>\n", block(&refs, img, math)));
            continue;
        }
        // 목록: 들여쓴 줄도 항목에 딸린 줄로 모은다. 코드·표 같은 블록이 시작하면 거기서 끊는다.
        if list_item(l).is_some() {
            let mut j = i + 1;
            while j < lines.len()
                && !lines[j].trim().is_empty()
                && !starts_block(j)
                && (list_item(lines[j]).is_some() || lines[j].starts_with(' ') || lines[j].starts_with('\t'))
            {
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
            if xt.is_empty() || starts_block(i) || crate::md::heading_parts(x).is_some() || xt.starts_with('>') || list_item(x).is_some() || xt.starts_with("$$") {
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

/// 수식 한 블록. `math`가 SVG(화면과 같은 배치)를 주면 그것을, 아니면 MathML을 쓴다.
fn math_div(n: &crate::math::Node, math: &dyn Fn(&crate::math::Node) -> Option<String>) -> String {
    match math(n) {
        Some(svg) => format!("<div class=\"math\">{svg}</div>\n"),
        None => format!("<div class=\"math\"><math display=\"block\">{}</math></div>\n", crate::math::to_mathml(n)),
    }
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
    let mut items: Vec<(usize, bool, String)> = Vec::new();
    for l in lines {
        match list_item(l) {
            Some((i, o, t)) => items.push((i, o, t.trim().to_string())),
            // 항목에 딸린 들여쓴 줄(기호 없는 줄)은 앞 항목에 이어 붙인다(예전에는 내보낼 때 사라졌다)
            None => {
                if let Some(last) = items.last_mut() {
                    last.2.push(' ');
                    last.2.push_str(l.trim());
                }
            }
        }
    }
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

/// 화면(앱)과 같은 모양으로 만들기 위한 값. 색은 `#rrggbb`.
#[derive(Clone, Debug)]
pub struct Style {
    pub bg: String,
    pub text: String,
    pub dim: String,
    pub link: String,
    pub quote: String,
    pub code_bg: String,
    pub code_fg: String,
    pub border: String,
    /// CSS font-family 값(따옴표 포함)
    pub font: String,
    /// 본문 글자 크기(px)
    pub size: f32,
    /// 줄 간격 배율(설정의 줄 간격)
    pub line: f32,
    /// 문단 사이(빈 줄) 간격 배율
    pub para: f32,
    /// 목록 항목 간격 배율
    pub list: f32,
}

impl Default for Style {
    /// 밝은 테마 기본값
    fn default() -> Style {
        Style {
            bg: "#ffffff".into(),
            text: "#1f2328".into(),
            dim: "#9aa1ab".into(),
            link: "#1a6fd6".into(),
            quote: "#5b6573".into(),
            code_bg: "#eceff3".into(),
            code_fg: "#b3261e".into(),
            border: "#c9ced6".into(),
            font: "'Malgun Gothic','Segoe UI',sans-serif".into(),
            size: 15.0,
            line: 1.0,
            para: 1.0,
            list: 1.3,
        }
    }
}

/// 완성된 HTML 문서(기본 스타일).
pub fn page(title: &str, inner: &str) -> String {
    page_styled(title, inner, &Style::default())
}

/// 완성된 HTML 문서: 앱 화면의 색·글꼴·크기·간격을 따른다. 인쇄(PDF)에서도 배경색을 그대로 낸다.
pub fn page_styled(title: &str, inner: &str, s: &Style) -> String {
    let lh = (1.45 * s.line).max(1.0);
    let css = format!(
        "html{{background:{bg};-webkit-print-color-adjust:exact;print-color-adjust:exact}}
body{{font-family:{font};font-size:{size}px;line-height:{lh:.3};color:{text};background:{bg};margin:0}}
main{{max-width:860px;margin:0 auto;padding:28px 30px}}
h1,h2,h3,h4,h5,h6{{font-weight:700;line-height:1.35;margin:{hm:.2}em 0 .3em}}
h1{{font-size:1.7em}}h2{{font-size:1.4em}}h3{{font-size:1.2em}}h4{{font-size:1.1em}}h5,h6{{font-size:1em}}
h1.doc-title{{font-size:1.7em;margin-top:0;border-bottom:1px solid {border};padding-bottom:.3em}}
p{{margin:{pm:.2}em 0}}
code{{background:{code_bg};color:{code_fg};padding:.08em .35em;border-radius:4px;font-family:Consolas,'Malgun Gothic',monospace;font-size:.94em}}
pre{{background:{code_bg};border:1px solid {border};padding:10px 14px;border-radius:5px;overflow:auto;white-space:pre-wrap;margin:.7em 0}}
pre code{{background:none;color:{text};padding:0;border-radius:0}}
pre.math{{font-family:Consolas,monospace}}
div.math{{text-align:center;margin:.7em 0;overflow-x:auto}}
div.math svg{{max-width:100%;height:auto;display:inline-block}}
math{{font-family:'Cambria Math','STIX Two Math',serif;font-size:1.25em}}
blockquote{{margin:.7em 0;padding:.05em 1em;border-left:4px solid {border};color:{quote}}}
ul,ol{{margin:.4em 0;padding-left:1.6em}}
li{{margin:{lm:.2}em 0}}
table{{border-collapse:collapse;margin:.7em 0}}
th,td{{border:1px solid {border};padding:4px 10px}}
th{{background:{code_bg}}}
li.task{{list-style:none;margin-left:-1.4em}}
img{{max-width:100%;height:auto}}
.wiki{{color:{link};border-bottom:1px dotted {link}}}
a{{color:{link}}}
hr{{border:0;border-top:1px solid {border};margin:1.2em 0}}
.noimg{{color:{dim}}}
@page{{margin:12mm}}
@media print{{main{{max-width:none;padding:0}}pre,table,div.math{{page-break-inside:avoid}}h1,h2,h3{{page-break-after:avoid}}}}
",
        bg = s.bg,
        font = s.font,
        size = s.size.clamp(8.0, 40.0),
        text = s.text,
        border = s.border,
        link = s.link,
        quote = s.quote,
        code_bg = s.code_bg,
        code_fg = s.code_fg,
        dim = s.dim,
        hm = 0.6 * s.para.max(0.5),
        pm = 0.5 * s.para.max(0.3),
        lm = 0.15 * s.list.max(0.5),
    );
    format!(
        "<!doctype html>\n<html lang=\"ko\"><head><meta charset=\"utf-8\"><title>{t}</title>\n<style>\n{css}</style></head>\n<body><main>\n<h1 class=\"doc-title\">{t}</h1>\n{inner}</main></body></html>\n",
        t = esc(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(b: &str) -> String {
        to_html(b, &|_| None, &|_| None)
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
    fn bare_and_wiki_urls_become_links() {
        assert_eq!(inline("see https://a.b/c?x=1&y=2."), "see <a href=\"https://a.b/c?x=1&amp;y=2\">https://a.b/c?x=1&amp;y=2</a>.");
        assert_eq!(inline("www.a.b"), "<a href=\"https://www.a.b\">www.a.b</a>");
        assert_eq!(inline("[[https://a.b|사이트]]"), "<a href=\"https://a.b\">사이트</a>");
        assert_eq!(inline("[글](www.a.b)"), "<a href=\"https://www.a.b\">글</a>");
        assert_eq!(inline("`https://a.b`"), "<code>https://a.b</code>");
        assert_eq!(inline("xhttps://a.b"), "xhttps://a.b");
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
        let s = to_html(b, &|h| Some(format!("data:x/{h}")), &|_| None);
        assert!(s.contains("<img src=\"data:x/0123456789abcdef\" alt=\"사진\">"));
        assert!(h(b).contains("[그림: 사진]"));
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn math_blocks_become_mathml() {
        let s = h("앞\n$$x=\\frac{a}{b}$$\n뒤\n\n$$\n\\sqrt{y}\n$$\n\n$$\\frac{1}$$");
        assert!(s.contains("<math display=\"block\">") && s.contains("<mfrac>") && s.contains("<msqrt>"), "{s}");
        assert!(!s.contains("\\frac{a}")); // 원문 코드가 그대로 나오지 않는다
        assert!(s.contains("$$\\frac{1}$$")); // 문법이 틀린 한 줄은 앱처럼 글자 그대로
    }

    /// 개발용: 샘플 HTML을 `AKASHIC_SAMPLE_OUT` 경로에 쓴다(다크 스타일 + 수식 SVG). Edge로 PDF를 만들어 눈으로 확인할 때 쓴다.
    #[test]
    #[ignore]
    fn write_sample_html() {
        let Ok(out) = std::env::var("AKASHIC_SAMPLE_OUT") else { return };
        let body = "# 큰 제목\n\n본문 **굵게** *기울임* `코드` [[위키]]\n\n## 수식\n\n$$x=\\frac{-b\\pm\\sqrt{b^2-4ac}}{2a}$$\n\n$$\n\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}\n$$\n\n- 목록 1\n  - 안쪽\n- [x] 완료\n\n| 이름 | 값 |\n|---|--:|\n| a | 1 |\n\n> 인용문\n\n```\nlet a = 1;\n```\n";
        let dark = Style {
            bg: "#1b1d21".into(),
            text: "#e3e6ea".into(),
            dim: "#6b7380".into(),
            link: "#6fb1ff".into(),
            quote: "#9aa3b0".into(),
            code_bg: "#2d3137".into(),
            code_fg: "#f08d80".into(),
            border: "#3a3f47".into(),
            ..Style::default()
        };
        let mut measure = |s: &str, sc: f32| s.chars().map(|c| if c.is_ascii() { 0.52 } else { 1.0 }).sum::<f32>() * sc;
        let size = 15.0;
        let inner = to_html(body, &|_| None, &|n| Some(crate::math::to_svg(n, size, &mut |s, sc| measure(s, sc))));
        std::fs::write(out, page_styled("샘플 문서", &inner, &dark)).unwrap();
    }

    #[test]
    fn page_escapes_title() {
        assert!(page("<b>&", "").contains("<title>&lt;b&gt;&amp;</title>"));
    }

    #[test]
    fn list_continuation_lines_are_kept() {
        let s = h("- 설치 방법\n  주의: 관리자 권한 필요\n- 다음");
        assert!(s.contains("<li>설치 방법 주의: 관리자 권한 필요</li>"), "{s}");
    }

    #[test]
    fn blocks_follow_the_screen_rules() {
        // 문단 바로 아래의 표도 표다(화면과 같다)
        let s = h("앞 문단\n| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(s.contains("<p>앞 문단</p>") && s.contains("<th>A</th>") && s.contains("<td>2</td>"), "{s}");
        // 목록 항목 아래에 들여쓴 코드 블록이 있어도 사라지지 않는다
        let s = h("- 항목\n  ```\n  코드\n  ```");
        assert!(s.contains("<li>항목</li>") && s.contains("<pre><code>  코드</code></pre>"), "{s}");
        // ~~~ 도 코드 울타리이고, 안의 ``` 줄은 코드다
        let s = h("~~~python\n```\nx\n```\n~~~\n뒤");
        assert!(s.contains("<pre><code class=\"lang-python\">```\nx\n```</code></pre>") && s.contains("<p>뒤</p>"), "{s}");
        // 틀린 수식은 원문으로, 닫히지 않은 코드 블록은 끝까지
        let s = h("$$\n\\frac{\n$$");
        assert!(s.contains("<pre class=\"math\">"), "{s}");
        let s = h("```\n끝까지\n계속");
        assert!(s.contains("<pre><code>끝까지\n계속</code></pre>"), "{s}");
    }
}
