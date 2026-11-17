//! 수식(`$$...$$` 한 줄) 파서와 배치. 글꼴 없이도 돌도록 폭은 `measure`로 받는다(길이 단위는 글자 크기의 배수 = em).
//! 위치는 글줄 기준선에서 잰다: x는 오른쪽, y는 아래가 +.

/// 파싱된 수식 조각.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// 변수·숫자·기호(원문의 공백은 무시).
    Sym(String),
    /// `\text{...}` — 공백을 그대로 둔 글자.
    Text(String),
    Row(Vec<Node>),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Box<Node>),
    Script { base: Box<Node>, sub: Option<Box<Node>>, sup: Option<Box<Node>> },
}

const MAX_DEPTH: usize = 24;

/// `\명령` → 화면에 그릴 글자(없으면 None). 함수 이름(sin 등)은 `Text`로 따로 처리한다.
fn symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "cdot" => "·",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "infty" => "∞",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "partial" => "∂",
        "degree" => "°",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" | "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "phi" | "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "%" => "%",
        "$" => "$",
        "&" => "&",
        "#" => "#",
        "_" => "_",
        "{" => "{",
        "}" => "}",
        _ => return None,
    })
}

const FUNCS: [&str; 12] = ["sin", "cos", "tan", "log", "ln", "exp", "min", "max", "lim", "det", "mod", "gcd"];

/// 한 줄이 `$$수식$$`이면 파싱한 수식을 돌려준다. 문법이 틀리면 None(원문 그대로 둔다).
pub fn parse_block(line: &str) -> Option<Node> {
    let t = line.trim();
    let inner = t.strip_prefix("$$")?.strip_suffix("$$")?;
    parse(inner)
}

pub fn parse(src: &str) -> Option<Node> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = P { c: &chars, i: 0 };
    let row = p.row(0)?;
    if p.i != chars.len() || row.is_empty() {
        return None;
    }
    Some(Node::Row(row))
}

struct P<'a> {
    c: &'a [char],
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().map_or(false, char::is_whitespace) {
            self.i += 1;
        }
    }
    /// `}`나 끝을 만날 때까지 읽는다(`}`는 소비하지 않는다).
    fn row(&mut self, depth: usize) -> Option<Vec<Node>> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None | Some('}') => return Some(out),
                Some('^') | Some('_') => return None, // 앞에 붙을 대상이 없다
                _ => {}
            }
            let atom = self.atom(depth)?;
            let atom = self.scripts(atom, depth)?;
            // 이어지는 변수·숫자 글자는 한 덩어리로 합친다(폭 계산·그리기가 줄어든다).
            match (out.last_mut(), &atom) {
                (Some(Node::Sym(a)), Node::Sym(b)) if is_run(a) && is_run(b) => a.push_str(b),
                _ => out.push(atom),
            }
        }
    }
    fn scripts(&mut self, base: Node, depth: usize) -> Option<Node> {
        let (mut sub, mut sup) = (None, None);
        loop {
            self.skip_ws();
            match self.peek() {
                Some('^') if sup.is_none() => {
                    self.i += 1;
                    sup = Some(Box::new(self.arg(depth)?));
                }
                Some('_') if sub.is_none() => {
                    self.i += 1;
                    sub = Some(Box::new(self.arg(depth)?));
                }
                Some('^') | Some('_') => return None, // 같은 첨자를 두 번 쓸 수 없다
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            return Some(base);
        }
        Some(Node::Script { base: Box::new(base), sub, sup })
    }
    /// 인자 하나: `{...}` 묶음이거나 낱글자/명령 하나.
    fn arg(&mut self, depth: usize) -> Option<Node> {
        self.skip_ws();
        match self.peek()? {
            '{' => {
                self.i += 1;
                let row = self.row(depth + 1)?;
                if self.peek() != Some('}') {
                    return None;
                }
                self.i += 1;
                Some(Node::Row(row))
            }
            '}' | '^' | '_' => None,
            _ => self.atom(depth),
        }
    }
    fn atom(&mut self, depth: usize) -> Option<Node> {
        let ch = self.peek()?;
        match ch {
            '{' => self.arg(depth),
            '\\' => {
                self.i += 1;
                let mut name = String::new();
                match self.peek() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
                            name.push(c);
                            self.i += 1;
                        }
                    }
                    Some(c) => {
                        name.push(c);
                        self.i += 1;
                    }
                    None => return None,
                }
                self.command(&name, depth)
            }
            '-' => {
                self.i += 1;
                Some(Node::Sym("−".into()))
            }
            '*' => {
                self.i += 1;
                Some(Node::Sym("∗".into()))
            }
            _ => {
                self.i += 1;
                Some(Node::Sym(ch.to_string()))
            }
        }
    }
    fn command(&mut self, name: &str, depth: usize) -> Option<Node> {
        match name {
            "frac" => {
                let a = self.arg(depth + 1)?;
                let b = self.arg(depth + 1)?;
                Some(Node::Frac(Box::new(a), Box::new(b)))
            }
            "sqrt" => {
                self.skip_ws();
                if self.peek() == Some('[') {
                    return None; // n제곱근은 아직 지원하지 않는다
                }
                Some(Node::Sqrt(Box::new(self.arg(depth + 1)?)))
            }
            "text" | "mathrm" | "textrm" => {
                self.skip_ws();
                if self.peek() != Some('{') {
                    return None;
                }
                self.i += 1;
                let mut level = 1;
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    self.i += 1;
                    match c {
                        '{' => level += 1,
                        '}' => {
                            level -= 1;
                            if level == 0 {
                                return Some(Node::Text(s));
                            }
                        }
                        '\\' | '^' | '_' => return None, // 글자 안의 명령은 지원하지 않는다
                        _ => {}
                    }
                    s.push(c);
                }
                None
            }
            // 크기 조절 괄호·간격은 모양을 바꾸지 않거나 빈칸으로 둔다.
            "left" | "right" => {
                let c = self.peek()?;
                if c == '.' {
                    self.i += 1;
                    return Some(Node::Row(vec![]));
                }
                self.atom(depth)
            }
            "," | ";" | ":" | "quad" | "qquad" | "!" | " " => {
                let w = match name {
                    "qquad" => "    ",
                    "quad" => "  ",
                    "!" => "",
                    _ => " ",
                };
                Some(Node::Text(w.into()))
            }
            f if FUNCS.contains(&f) => Some(Node::Text(f.into())),
            _ => symbol(name).map(|s| Node::Sym(s.into())),
        }
    }
}

/// 숫자·글자 덩어리(연산자·기호는 합치지 않는다).
fn is_run(s: &str) -> bool {
    s.chars().all(|c| c.is_alphanumeric() || c == '.' || c == ',')
}

fn is_op(s: &str) -> bool {
    matches!(s, "=" | "+" | "−" | "×" | "÷" | "±" | "∓" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "≡" | "→" | "←")
}

// ── 배치 ──────────────────────────────────────────────────────────────

/// 글자 한 줄의 기준선 위·아래 높이(em).
pub const TXT_ASC: f32 = 0.85;
pub const TXT_DESC: f32 = 0.25;
/// 분수선·제곱근선의 굵기(em)와 기준선에서 분수선까지의 높이.
pub const RULE: f32 = 0.055;
const AXIS: f32 = 0.30;
const GAP: f32 = 0.14;
const SCRIPT: f32 = 0.7;
const OP_PAD: f32 = 0.22;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// `scale`배 크기로 그릴 글자. (x, y)는 글자 기준선의 왼쪽 끝.
    Glyph { x: f32, y: f32, text: String, scale: f32 },
    /// 굵기 `RULE`인 선분.
    Line { a: (f32, f32), b: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lay {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub items: Vec<Item>,
}

fn shift(items: &mut [Item], dx: f32, dy: f32) {
    for it in items {
        match it {
            Item::Glyph { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { a, b } => {
                a.0 += dx;
                a.1 += dy;
                b.0 += dx;
                b.1 += dy;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Cx {
    scale: f32,
    /// 분수 안에 든 깊이(두 번째 분수부터 글자를 줄인다).
    fd: u32,
}

/// 수식을 배치한다. `measure(글자, 배율)`은 그 글자의 가로 폭을 em(글자 크기의 배수)으로 돌려준다.
pub fn layout(n: &Node, measure: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    lay(n, Cx { scale: 1.0, fd: 0 }, measure)
}

fn text_lay(s: &str, sc: f32, pad: f32, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let w = if s.is_empty() { 0.0 } else { m(s, sc) };
    let items = if s.trim().is_empty() { vec![] } else { vec![Item::Glyph { x: pad, y: 0.0, text: s.to_string(), scale: sc }] };
    Lay { w: w + pad * 2.0, asc: TXT_ASC * sc, desc: TXT_DESC * sc, items }
}

fn lay(n: &Node, cx: Cx, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let s = cx.scale;
    match n {
        Node::Sym(t) => {
            let t: &str = t;
            let pad = if is_op(t) { OP_PAD * s } else { 0.0 };
            text_lay(t, s, pad, m)
        }
        Node::Text(t) => text_lay(t, s, 0.0, m),
        Node::Row(v) => {
            let mut x = 0.0;
            let (mut asc, mut desc) = (TXT_ASC * s, TXT_DESC * s);
            let mut items = Vec::new();
            for c in v {
                let mut l = lay(c, cx, m);
                shift(&mut l.items, x, 0.0);
                items.append(&mut l.items);
                x += l.w;
                asc = asc.max(l.asc);
                desc = desc.max(l.desc);
            }
            Lay { w: x, asc, desc, items }
        }
        Node::Frac(a, b) => {
            let kid = Cx { scale: if cx.fd == 0 { s } else { s * 0.8 }, fd: cx.fd + 1 };
            let (mut na, mut nb) = (lay(a, kid, m), lay(b, kid, m));
            let w = na.w.max(nb.w) + 0.2 * s;
            let (axis, half, gap) = (AXIS * s, RULE * s / 2.0, GAP * s);
            shift(&mut na.items, (w - na.w) / 2.0, -(axis + half + gap + na.desc));
            shift(&mut nb.items, (w - nb.w) / 2.0, -axis + half + gap + nb.asc);
            let asc = axis + half + gap + na.desc + na.asc;
            let desc = (-axis + half + gap + nb.asc + nb.desc).max(TXT_DESC * s);
            let mut items = na.items;
            items.append(&mut nb.items);
            items.push(Item::Line { a: (0.05 * s, -axis), b: (w - 0.05 * s, -axis) });
            Lay { w, asc, desc, items }
        }
        Node::Sqrt(a) => {
            let mut inner = lay(a, cx, m);
            let lead = 0.62 * s;
            let top = -(inner.asc + 0.12 * s);
            let bottom = inner.desc;
            let w = lead + inner.w + 0.08 * s;
            shift(&mut inner.items, lead, 0.0);
            let mut items = inner.items;
            let mid = top + (bottom - top) * 0.62;
            let pts = [(0.06 * s, mid), (0.2 * s, bottom), (0.42 * s, top), (w, top)];
            for k in 0..3 {
                items.push(Item::Line { a: pts[k], b: pts[k + 1] });
            }
            Lay { w, asc: -top + RULE * s / 2.0, desc: inner.desc, items }
        }
        Node::Script { base, sub, sup } => {
            let b = lay(base, cx, m);
            let ss = Cx { scale: s * SCRIPT, fd: cx.fd };
            let (mut su, mut sb) = (sup.as_ref().map(|n| lay(n, ss, m)), sub.as_ref().map(|n| lay(n, ss, m)));
            let up = (0.42 * s).max(b.asc - 0.38 * s);
            let mut down = (0.2 * s).max(b.desc - 0.05 * s);
            if let (Some(u), Some(d)) = (&su, &sb) {
                // 위·아래 첨자가 겹치지 않게 아래 첨자를 더 내린다.
                let need = -up + u.desc + 0.06 * s + d.asc;
                down = down.max(need);
            }
            let x0 = b.w + 0.04 * s;
            let mut w = b.w;
            let (mut asc, mut desc) = (b.asc, b.desc);
            let mut items = b.items;
            if let Some(u) = su.as_mut() {
                shift(&mut u.items, x0, -up);
                items.append(&mut u.items);
                w = w.max(x0 + u.w);
                asc = asc.max(up + u.asc);
                desc = desc.max(-up + u.desc);
            }
            if let Some(d) = sb.as_mut() {
                shift(&mut d.items, x0, down);
                items.append(&mut d.items);
                w = w.max(x0 + d.w);
                desc = desc.max(down + d.desc);
                asc = asc.max(-down + d.asc);
            }
            Lay { w, asc, desc, items }
        }
    }
}

/// 글꼴 없이 구한 수식의 높이(em). 세로 크기는 글자 폭과 무관하다.
pub fn height_em(n: &Node) -> f32 {
    let l = layout(n, &mut |s, sc| s.chars().count() as f32 * 0.5 * sc);
    l.asc + l.desc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meas() -> impl FnMut(&str, f32) -> f32 {
        |s, sc| s.chars().count() as f32 * 0.5 * sc
    }

    #[test]
    fn block_needs_double_dollar_on_both_ends() {
        assert!(parse_block("$$x$$").is_some());
        assert!(parse_block("  $$ x^2 $$  ").is_some());
        assert!(parse_block("$$x$").is_none());
        assert!(parse_block("$x$").is_none());
        assert!(parse_block("$$$$").is_none());
        assert!(parse_block("값은 $$x$$").is_none());
    }

    #[test]
    fn user_formula_parses_to_text_sym_and_fraction() {
        let src = r"$$\text{추가해야 하는 양 }x=\frac{\text{목표 비율} \times \text{기존 무게}}{1 - \text{목표 비율}}$$";
        let Some(Node::Row(v)) = parse_block(src) else { panic!("parse failed") };
        assert_eq!(v[0], Node::Text("추가해야 하는 양 ".into()));
        assert_eq!(v[1], Node::Sym("x".into()));
        assert_eq!(v[2], Node::Sym("=".into()));
        let Node::Frac(num, den) = &v[3] else { panic!("fraction expected, got {:?}", v[3]) };
        let Node::Row(n) = &**num else { panic!() };
        assert_eq!(n, &vec![Node::Text("목표 비율".into()), Node::Sym("×".into()), Node::Text("기존 무게".into())]);
        let Node::Row(d) = &**den else { panic!() };
        assert_eq!(d, &vec![Node::Sym("1".into()), Node::Sym("−".into()), Node::Text("목표 비율".into())]);
    }

    #[test]
    fn variables_and_digits_merge_into_runs() {
        assert_eq!(parse("ab12").unwrap(), Node::Row(vec![Node::Sym("ab12".into())]));
        // 연산자는 합치지 않는다.
        assert_eq!(parse("a+b").unwrap(), Node::Row(vec![Node::Sym("a".into()), Node::Sym("+".into()), Node::Sym("b".into())]));
    }

    #[test]
    fn scripts_attach_to_the_previous_atom() {
        let Node::Row(v) = parse("x^2_i").unwrap() else { panic!() };
        let Node::Script { base, sub, sup } = &v[0] else { panic!() };
        assert_eq!(**base, Node::Sym("x".into()));
        assert_eq!(sup.as_deref(), Some(&Node::Sym("2".into())));
        assert_eq!(sub.as_deref(), Some(&Node::Sym("i".into())));
    }

    #[test]
    fn invalid_input_is_rejected_not_half_drawn() {
        for bad in [r"\frac{1}", r"\frac{1}{2", r"\unknown{x}", "x^", "^2", "x^2^3", r"\sqrt[3]{x}", "{x", "x}", r"\text{a", r"\text{\alpha}", ""] {
            assert!(parse(bad).is_none(), "{bad:?} should fail");
        }
    }

    #[test]
    fn greek_symbols_and_functions() {
        let Node::Row(v) = parse(r"\alpha\pm\sin x").unwrap() else { panic!() };
        assert_eq!(v, vec![Node::Sym("α".into()), Node::Sym("±".into()), Node::Text("sin".into()), Node::Sym("x".into())]);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let s = "{".repeat(500) + "x" + &"}".repeat(500);
        assert!(parse(&s).is_none());
    }

    #[test]
    fn fraction_is_taller_than_plain_text_and_nests_smaller() {
        let plain = height_em(&parse("x").unwrap());
        let frac = height_em(&parse(r"\frac{a}{b}").unwrap());
        assert!(frac > plain * 1.5, "{frac} vs {plain}");
        let nested = height_em(&parse(r"\frac{\frac{a}{b}}{c}").unwrap());
        assert!(nested > frac);
        // 중첩 분수는 안쪽 글자를 줄이므로 두 배까지는 커지지 않는다.
        assert!(nested < frac * 2.0);
    }

    #[test]
    fn fraction_bar_is_centered_and_children_fit_inside() {
        let l = layout(&parse(r"\frac{abcd}{x}").unwrap(), &mut meas());
        let Some(Item::Line { a, b }) = l.items.iter().find(|i| matches!(i, Item::Line { .. })) else { panic!() };
        assert!(a.0 >= 0.0 && b.0 <= l.w + 1e-4 && a.1 < 0.0); // 선은 기준선 위
        for it in &l.items {
            if let Item::Glyph { x, text, scale, .. } = it {
                assert!(*x >= 0.0 && x + text.chars().count() as f32 * 0.5 * scale <= l.w + 1e-4, "{text} sticks out");
            }
        }
        // 분자는 선 위, 분모는 선 아래.
        let ys: Vec<(String, f32)> = l.items.iter().filter_map(|i| if let Item::Glyph { y, text, .. } = i { Some((text.clone(), *y)) } else { None }).collect();
        let (yn, yd) = (ys.iter().find(|(t, _)| t == "abcd").unwrap().1, ys.iter().find(|(t, _)| t == "x").unwrap().1);
        assert!(yn < a.1 && yd > a.1, "{yn} {} {yd}", a.1);
    }

    #[test]
    fn superscript_is_smaller_and_raised() {
        let l = layout(&parse("x^2").unwrap(), &mut meas());
        let g: Vec<_> = l.items.iter().filter_map(|i| if let Item::Glyph { y, scale, text, x } = i { Some((text.as_str(), *x, *y, *scale)) } else { None }).collect();
        let (_, bx, by, bs) = g[0];
        let (_, sx, sy, ss) = g[1];
        assert!(sy < by && ss < bs && sx > bx);
        assert!(l.asc > TXT_ASC); // 첨자 때문에 위로 더 높아진다
    }

    #[test]
    fn sub_and_sup_do_not_overlap() {
        let l = layout(&parse("x^{a}_{b}").unwrap(), &mut meas());
        let ys: Vec<f32> = l.items.iter().filter_map(|i| if let Item::Glyph { y, text, .. } = i { (text == "a" || text == "b").then_some(*y) } else { None }).collect();
        assert!(ys[1] - ys[0] > 0.7 * 0.5, "{ys:?}"); // 두 첨자 사이가 글자 높이 이상 떨어져 있다
    }

    #[test]
    fn sqrt_draws_radical_above_content() {
        let l = layout(&parse(r"\sqrt{x}").unwrap(), &mut meas());
        let lines = l.items.iter().filter(|i| matches!(i, Item::Line { .. })).count();
        assert_eq!(lines, 3);
        assert!(l.asc > TXT_ASC);
        assert!(l.w > 0.62);
    }

    #[test]
    fn binary_operators_get_padding() {
        let plain = layout(&parse("a").unwrap(), &mut meas()).w;
        let op = layout(&parse("=").unwrap(), &mut meas()).w;
        assert!(op > 0.5 + 0.4 && plain == 0.5);
    }

    #[test]
    fn height_ignores_glyph_widths() {
        let n = parse(r"\frac{\text{긴 글자가 아주 아주 많은 분자}}{x}").unwrap();
        let a = height_em(&n);
        let b = layout(&n, &mut |_, _| 3.0);
        assert!((a - (b.asc + b.desc)).abs() < 1e-5);
    }
}
