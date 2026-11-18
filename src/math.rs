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
    /// `\\`로 나뉜 여러 줄(가운데 정렬로 쌓는다).
    Stack(Vec<Node>),
    /// `\dfrac`·`\tfrac`: 안의 수식을 큰/작은 스타일로 배치한다.
    Style(Sty, Box<Node>),
    /// `\binom{n}{k}`: 줄 없는 분수를 괄호로 감싼다.
    Binom(Box<Node>, Box<Node>),
    /// `\sqrt[n]{x}`: (차수, 속).
    Root(Box<Node>, Box<Node>),
    /// 기호 위·아래에 놓는 글(`\sum\limits`, `\overset`, `\underset`): 첨자 크기로 가운데 정렬해 쌓는다.
    Over { base: Box<Node>, over: Option<Box<Node>>, under: Option<Box<Node>> },
    /// 장식(`\hat`, `\overline`, `\boxed`, `\cancel` …).
    Deco(Deco, Box<Node>),
    /// `\left( … \right)`: 여는·닫는 괄호(`""`은 `.` 곧 없음)를 내용 높이에 맞춘다.
    Delim { open: String, close: String, body: Box<Node> },
    /// `aligned`/`align`의 줄들: (`&` 앞, `&` 뒤). `&`가 없는 줄은 뒤가 None이고 가운데 정렬한다.
    Align(Vec<(Node, Option<Node>)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sty {
    Display,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deco {
    Hat,
    WideHat,
    Bar,
    Vec,
    Dot,
    DDot,
    Tilde,
    WideTilde,
    Overline,
    Underline,
    Boxed,
    Cancel,
    BCancel,
    XCancel,
}

const MAX_DEPTH: usize = 24;

/// `\명령` → 화면에 그릴 글자(없으면 None). 함수 이름(sin 등)은 `Text`로 따로 처리한다.
fn symbol(cmd: &str) -> Option<&'static str> {
    symbol_short(cmd).or_else(|| SYMBOLS_EXT.iter().find(|(k, _)| *k == cmd).map(|(_, v)| *v))
}

fn symbol_short(cmd: &str) -> Option<&'static str> {
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
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "coprod" => "∐",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "bigvee" => "⋁",
        "bigwedge" => "⋀",
        "bigoplus" => "⨁",
        "bigotimes" => "⨂",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "partial" => "∂",
        "degree" => "°",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "varepsilon" => "ε",
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
        "varphi" => "φ",
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

/// 5단계 기호 표: `\이름` → 화면에 그릴 글자. (`symbol()`의 짧은 표에 없을 때 찾는다.)
const SYMBOLS_EXT: &[(&str, &str)] = &[
    // 그리스 문자(소문자 변형·대문자)
    ("epsilon", "ϵ"), ("phi", "ϕ"), ("iota", "ι"), ("kappa", "κ"), ("varkappa", "ϰ"), ("vartheta", "ϑ"), ("varpi", "ϖ"), ("varrho", "ϱ"), ("varsigma", "ς"), ("upsilon", "υ"),
    ("Xi", "Ξ"), ("Upsilon", "Υ"), ("digamma", "ϝ"),
    // 관계
    ("lt", "<"), ("gt", ">"), ("ll", "≪"), ("gg", "≫"), ("lll", "⋘"), ("ggg", "⋙"), ("sim", "∼"), ("simeq", "≃"), ("cong", "≅"), ("ncong", "≇"), ("nsim", "≁"), ("propto", "∝"),
    ("doteq", "≐"), ("asymp", "≍"), ("approxeq", "≊"), ("triangleq", "≜"), ("prec", "≺"), ("succ", "≻"), ("preceq", "⪯"), ("succeq", "⪰"), ("perp", "⊥"), ("parallel", "∥"),
    ("nparallel", "∦"), ("mid", "∣"), ("nmid", "∤"), ("models", "⊨"), ("vdash", "⊢"), ("dashv", "⊣"), ("vDash", "⊨"), ("bowtie", "⋈"), ("lesssim", "≲"), ("gtrsim", "≳"),
    ("leqslant", "⩽"), ("geqslant", "⩾"), ("nless", "≮"), ("ngtr", "≯"), ("nleq", "≰"), ("ngeq", "≱"), ("lessgtr", "≶"), ("gtrless", "≷"), ("between", "≬"), ("smile", "⌣"), ("frown", "⌢"),
    // 이항 연산자
    ("ast", "∗"), ("star", "⋆"), ("circ", "∘"), ("bullet", "∙"), ("oplus", "⊕"), ("ominus", "⊖"), ("otimes", "⊗"), ("oslash", "⊘"), ("odot", "⊙"), ("wedge", "∧"), ("land", "∧"),
    ("vee", "∨"), ("lor", "∨"), ("cup", "∪"), ("cap", "∩"), ("setminus", "∖"), ("smallsetminus", "∖"), ("amalg", "⨿"), ("dagger", "†"), ("ddagger", "‡"), ("dag", "†"), ("ddag", "‡"),
    ("uplus", "⊎"), ("sqcup", "⊔"), ("sqcap", "⊓"), ("triangleleft", "◁"), ("triangleright", "▷"), ("diamond", "⋄"), ("bigtriangleup", "△"), ("bigtriangledown", "▽"), ("wr", "≀"),
    ("ltimes", "⋉"), ("rtimes", "⋊"), ("boxplus", "⊞"), ("boxminus", "⊟"), ("boxtimes", "⊠"), ("boxdot", "⊡"), ("dotplus", "∔"), ("divideontimes", "⋇"), ("curlywedge", "⋏"), ("curlyvee", "⋎"),
    ("barwedge", "⊼"), ("veebar", "⊻"), ("centerdot", "·"), ("cdotp", "·"), ("intercal", "⊺"),
    // 화살표
    ("gets", "←"), ("leftrightarrow", "↔"), ("Leftarrow", "⇐"), ("Rightarrow", "⇒"), ("Leftrightarrow", "⇔"), ("implies", "⟹"), ("impliedby", "⟸"), ("iff", "⟺"),
    ("longrightarrow", "⟶"), ("longleftarrow", "⟵"), ("longleftrightarrow", "⟷"), ("Longrightarrow", "⟹"), ("Longleftarrow", "⟸"), ("Longleftrightarrow", "⟺"),
    ("mapsto", "↦"), ("longmapsto", "⟼"), ("uparrow", "↑"), ("downarrow", "↓"), ("updownarrow", "↕"), ("Uparrow", "⇑"), ("Downarrow", "⇓"), ("Updownarrow", "⇕"),
    ("nearrow", "↗"), ("searrow", "↘"), ("swarrow", "↙"), ("nwarrow", "↖"), ("hookrightarrow", "↪"), ("hookleftarrow", "↩"), ("rightharpoonup", "⇀"), ("rightharpoondown", "⇁"),
    ("leftharpoonup", "↼"), ("leftharpoondown", "↽"), ("rightleftharpoons", "⇌"), ("leftrightharpoons", "⇋"), ("leadsto", "⇝"), ("rightsquigarrow", "⇝"), ("twoheadrightarrow", "↠"),
    ("twoheadleftarrow", "↞"), ("nrightarrow", "↛"), ("nleftarrow", "↚"), ("nleftrightarrow", "↮"), ("nRightarrow", "⇏"), ("nLeftarrow", "⇍"), ("nLeftrightarrow", "⇎"),
    ("curvearrowright", "↷"), ("curvearrowleft", "↶"), ("circlearrowleft", "↺"), ("circlearrowright", "↻"), ("Rrightarrow", "⇛"), ("Lleftarrow", "⇚"), ("rightleftarrows", "⇄"),
    ("leftrightarrows", "⇆"), ("rightrightarrows", "⇉"), ("leftleftarrows", "⇇"), ("looparrowright", "↬"), ("looparrowleft", "↫"), ("rightarrowtail", "↣"), ("leftarrowtail", "↢"),
    ("Lsh", "↰"), ("Rsh", "↱"), ("upuparrows", "⇈"), ("downdownarrows", "⇊"), ("upharpoonleft", "↿"), ("upharpoonright", "↾"), ("downharpoonleft", "⇃"), ("downharpoonright", "⇂"),
    // 집합·논리
    ("in", "∈"), ("notin", "∉"), ("ni", "∋"), ("owns", "∋"), ("subset", "⊂"), ("supset", "⊃"), ("subseteq", "⊆"), ("supseteq", "⊇"), ("subsetneq", "⊊"), ("supsetneq", "⊋"),
    ("nsubseteq", "⊈"), ("nsupseteq", "⊉"), ("subseteqq", "⫅"), ("supseteqq", "⫆"), ("sqsubset", "⊏"), ("sqsupset", "⊐"), ("sqsubseteq", "⊑"), ("sqsupseteq", "⊒"),
    ("emptyset", "∅"), ("varnothing", "∅"), ("forall", "∀"), ("exists", "∃"), ("nexists", "∄"), ("neg", "¬"), ("lnot", "¬"), ("top", "⊤"), ("bot", "⊥"), ("therefore", "∴"), ("because", "∵"),
    ("complement", "∁"), ("Subset", "⋐"), ("Supset", "⋑"), ("Cup", "⋓"), ("Cap", "⋒"),
    // 기타 기호
    ("aleph", "ℵ"), ("beth", "ℶ"), ("gimel", "ℷ"), ("daleth", "ℸ"), ("Re", "ℜ"), ("Im", "ℑ"), ("wp", "℘"), ("ell", "ℓ"), ("hbar", "ℏ"), ("hslash", "ℏ"), ("imath", "ı"), ("jmath", "ȷ"),
    ("angle", "∠"), ("measuredangle", "∡"), ("sphericalangle", "∢"), ("triangle", "△"), ("blacktriangle", "▲"), ("blacktriangledown", "▼"), ("square", "□"), ("blacksquare", "■"),
    ("Box", "□"), ("lozenge", "◊"), ("blacklozenge", "⧫"), ("bigcirc", "○"), ("prime", "′"), ("backprime", "‵"), ("nabla", "∇"), ("surd", "√"), ("checkmark", "✓"), ("maltese", "✠"),
    ("flat", "♭"), ("natural", "♮"), ("sharp", "♯"), ("clubsuit", "♣"), ("diamondsuit", "♢"), ("heartsuit", "♡"), ("spadesuit", "♠"), ("mho", "℧"), ("eth", "ð"), ("AA", "Å"),
    ("S", "§"), ("P", "¶"), ("copyright", "©"), ("pounds", "£"), ("yen", "¥"), ("euro", "€"), ("circledR", "®"), ("smallint", "∫"), ("Finv", "Ⅎ"), ("Game", "⅁"),
    ("complexes", "ℂ"), ("naturals", "ℕ"), ("integers", "ℤ"), ("rationals", "ℚ"), ("reals", "ℝ"), ("lbrace", "{"), ("rbrace", "}"), ("lbrack", "["), ("rbrack", "]"),
    ("langle", "⟨"), ("rangle", "⟩"), ("lfloor", "⌊"), ("rfloor", "⌋"), ("lceil", "⌈"), ("rceil", "⌉"), ("vert", "|"), ("Vert", "‖"), ("lvert", "|"), ("rvert", "|"), ("lVert", "‖"), ("rVert", "‖"),
    ("|", "‖"), ("backslash", "\\"), ("lq", "‘"), ("rq", "’"), ("ldotp", "."), ("colon", ":"), ("triangledown", "▽"), ("vartriangle", "▵"), ("diagup", "╱"), ("diagdown", "╲"),
    // 점
    ("ldots", "…"), ("dots", "…"), ("dotsc", "…"), ("dotsb", "⋯"), ("dotsm", "⋯"), ("dotsi", "⋯"), ("dotso", "…"), ("mathellipsis", "…"), ("cdots", "⋯"), ("vdots", "⋮"), ("ddots", "⋱"), ("adots", "⋰"),
];

/// 관계·이항 연산자 기호(앞뒤에 빈칸을 두어 그린다).
const OPS_EXT: &[&str] = &["≪", "≫", "⋘", "⋙", "∼", "≃", "≅", "≇", "≁", "∝", "≐", "≍", "≊", "≜", "≺", "≻", "⪯", "⪰", "⊥", "∥", "∦", "∣", "∤", "⊨", "⊢", "⊣", "⋈", "≲", "≳", "⩽", "⩾", "≮", "≯", "≰", "≱", "≶", "≷", "≬", "⌣", "⌢",
    "∗", "⋆", "∘", "∙", "⊕", "⊖", "⊗", "⊘", "⊙", "∧", "∨", "∪", "∩", "∖", "⨿", "†", "‡", "⊎", "⊔", "⊓", "◁", "▷", "⋄", "△", "▽", "≀", "⋉", "⋊", "⊞", "⊟", "⊠", "⊡", "∔", "⋇", "⋏", "⋎", "⊼", "⊻", "⊺",
    "↔", "⇐", "⇒", "⇔", "⟹", "⟸", "⟺", "⟶", "⟵", "⟷", "↦", "⟼", "↑", "↓", "↕", "⇑", "⇓", "⇕", "↗", "↘", "↙", "↖", "↪", "↩", "⇀", "⇁", "↼", "↽", "⇌", "⇋", "⇝", "↠", "↞", "↛", "↚", "↮", "⇏", "⇍", "⇎",
    "↷", "↶", "↺", "↻", "⇛", "⇚", "⇄", "⇆", "⇉", "⇇", "↬", "↫", "↣", "↢", "↰", "↱", "⇈", "⇊", "↿", "↾", "⇃", "⇂",
    "∈", "∉", "∋", "⊂", "⊃", "⊆", "⊇", "⊊", "⊋", "⊈", "⊉", "⫅", "⫆", "⊏", "⊐", "⊑", "⊒", "∴", "∵", "⋐", "⋑", "⋓", "⋒"];

const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos", "arctan", "log", "ln", "lg", "exp", "min", "max", "sup", "inf", "lim", "limsup", "liminf", "det", "mod", "gcd",
    "arg", "dim", "ker", "deg", "hom", "Pr",
];
/// 블록 수식에서 한계를 위·아래에 놓는 함수 이름.
const LIMIT_FUNCS: &[&str] = &["lim", "limsup", "liminf", "max", "min", "sup", "inf", "det", "gcd", "Pr"];
/// 블록 수식에서 크게 그리는 큰 연산자. 앞의 것들은 한계를 위·아래에, 적분은 옆에 놓는다.
const BIG_LIMITS: &[&str] = &["∑", "∏", "∐", "⋃", "⋂", "⋁", "⋀", "⨁", "⨂"];
const BIG_INTS: &[&str] = &["∫", "∬", "∭", "∮"];

/// 한 줄이 `$$수식$$`이면 파싱한 수식을 돌려준다. 문법이 틀리면 None(원문 그대로 둔다).
pub fn parse_block(line: &str) -> Option<Node> {
    let t = line.trim();
    let inner = t.strip_prefix("$$")?.strip_suffix("$$")?;
    parse_multi(inner)
}

pub fn parse(src: &str) -> Option<Node> {
    match parse_row(src)? {
        Node::Row(r) if r.is_empty() => None,
        n => Some(n),
    }
}

/// 빈 줄도 허용하는 한 줄 파싱(`\\`·`&`로 쪼갠 조각용).
fn parse_row(src: &str) -> Option<Node> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = P { c: &chars, i: 0 };
    let row = p.row(0)?;
    if p.i != chars.len() {
        return None;
    }
    Some(Node::Row(row))
}

/// `$$` 사이의 내용(여러 줄 가능). 줄바꿈은 공백이고 `\\`가 새 줄이다.
/// `\begin{aligned|align|align*}…\end{…}`이면 `&` 기준으로 정렬한다.
pub fn parse_multi(src: &str) -> Option<Node> {
    let t = src.trim();
    for env in ["aligned", "align*", "align"] {
        let (open, close) = (format!("\\begin{{{env}}}"), format!("\\end{{{env}}}"));
        if let Some(body) = t.strip_prefix(&open) {
            let body = body.strip_suffix(&close)?;
            let mut rows = Vec::new();
            for piece in split_top(body, '\\')? {
                let mut cells = split_top(&piece, '&')?;
                if cells.len() > 2 {
                    return None;
                }
                let right = if cells.len() == 2 { Some(parse_row(&cells.pop()?)?) } else { None };
                rows.push((parse_row(&cells.pop()?)?, right));
            }
            if matches!(rows.last(), Some((Node::Row(l), None)) if l.is_empty()) {
                rows.pop(); // 끝의 \\ 뒤 빈 줄
            }
            if rows.is_empty() || rows.iter().all(|(l, r)| matches!(l, Node::Row(v) if v.is_empty()) && r.as_ref().map_or(true, |r| matches!(r, Node::Row(v) if v.is_empty()))) {
                return None;
            }
            return Some(Node::Align(rows));
        }
    }
    let mut rows = Vec::new();
    for piece in split_top(t, '\\')? {
        if split_top(&piece, '&')?.len() > 1 {
            return None; // aligned 밖의 &
        }
        rows.push(parse_row(&piece)?);
    }
    if matches!(rows.last(), Some(Node::Row(v)) if v.is_empty()) {
        rows.pop();
    }
    match rows.len() {
        0 => None,
        1 => match rows.pop()? {
            Node::Row(v) if v.is_empty() => None,
            n => Some(n),
        },
        _ => Some(Node::Stack(rows)),
    }
}

/// 중괄호 밖의 구분자로 쪼갠다. `sep == '\\'`이면 `\\`가 구분자, `&`이면 `&`가 구분자다.
/// `\&`·`\{` 같은 이스케이프와 `{}` 안은 건드리지 않는다. 중괄호가 안 맞으면 None.
fn split_top(s: &str, sep: char) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                let nx = it.next();
                if nx == Some('\\') && depth == 0 && sep == '\\' {
                    out.push(std::mem::take(&mut cur));
                    continue;
                }
                cur.push('\\');
                if let Some(n) = nx {
                    cur.push(n);
                }
            }
            '{' => {
                depth += 1;
                cur.push(c);
            }
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                cur.push(c);
            }
            c if c == sep && sep == '&' && depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if depth != 0 {
        return None;
    }
    out.push(cur);
    Some(out)
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
                Some('\\') if self.at_cmd("right") => return Some(out),
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
    /// 지금 자리가 `\이름`(뒤에 영문자가 이어지지 않음)이면 읽고 true.
    fn eat_cmd(&mut self, name: &str) -> bool {
        let n = name.chars().count();
        if self.peek() != Some('\\') || self.c.len() < self.i + 1 + n {
            return false;
        }
        let word: String = self.c[self.i + 1..self.i + 1 + n].iter().collect();
        let after = self.c.get(self.i + 1 + n).copied();
        if word != name || after.map_or(false, |c| c.is_ascii_alphabetic()) {
            return false;
        }
        self.i += 1 + n;
        true
    }
    fn at_cmd(&self, name: &str) -> bool {
        let n = name.chars().count();
        let ok = self.c.len() >= self.i + 1 + n && self.c[self.i + 1..self.i + 1 + n].iter().copied().eq(name.chars());
        ok && !self.c.get(self.i + 1 + n).map_or(false, |c| c.is_ascii_alphabetic())
    }
    fn scripts(&mut self, mut base: Node, depth: usize) -> Option<Node> {
        let (mut sub, mut sup) = (None, None);
        self.skip_ws();
        let limits = self.eat_cmd("limits");
        let nolimits = !limits && self.eat_cmd("nolimits");
        if nolimits {
            base = Node::Row(vec![base]); // 기호가 아니게 감싸서 위·아래 배치를 막는다
        }
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
        if limits {
            return Some(Node::Over { base: Box::new(base), over: sup, under: sub });
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
    /// `\left`·`\right` 뒤의 괄호 글자. `.`은 빈 괄호(`""`).
    fn delim(&mut self) -> Option<String> {
        self.skip_ws();
        let c = self.peek()?;
        self.i += 1;
        let s = match c {
            '(' | ')' | '[' | ']' | '|' | '/' | '<' | '>' => match c {
                '<' => "⟨",
                '>' => "⟩",
                _ => return Some(c.to_string()),
            },
            '.' => "",
            '\\' => {
                let mut name = String::new();
                match self.peek()? {
                    c if c.is_ascii_alphabetic() => {
                        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
                            name.push(c);
                            self.i += 1;
                        }
                    }
                    c => {
                        name.push(c);
                        self.i += 1;
                    }
                }
                match name.as_str() {
                    "{" | "lbrace" => "{",
                    "}" | "rbrace" => "}",
                    "|" | "Vert" | "lVert" | "rVert" => "‖",
                    "vert" | "lvert" | "rvert" => "|",
                    "langle" => "⟨",
                    "rangle" => "⟩",
                    "lfloor" => "⌊",
                    "rfloor" => "⌋",
                    "lceil" => "⌈",
                    "rceil" => "⌉",
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some(s.to_string())
    }
    fn command(&mut self, name: &str, depth: usize) -> Option<Node> {
        match name {
            "frac" | "dfrac" | "tfrac" => {
                let a = self.arg(depth + 1)?;
                let b = self.arg(depth + 1)?;
                let f = Node::Frac(Box::new(a), Box::new(b));
                Some(match name {
                    "dfrac" => Node::Style(Sty::Display, Box::new(f)),
                    "tfrac" => Node::Style(Sty::Text, Box::new(f)),
                    _ => f,
                })
            }
            "binom" | "dbinom" | "tbinom" => {
                let a = self.arg(depth + 1)?;
                let b = self.arg(depth + 1)?;
                let f = Node::Binom(Box::new(a), Box::new(b));
                Some(match name {
                    "dbinom" => Node::Style(Sty::Display, Box::new(f)),
                    "tbinom" => Node::Style(Sty::Text, Box::new(f)),
                    _ => f,
                })
            }
            "sqrt" => {
                self.skip_ws();
                if self.peek() == Some('[') {
                    // n제곱근: `]`까지(중괄호 안의 `]`는 건너뜀)를 차수로 읽는다.
                    self.i += 1;
                    let start = self.i;
                    let mut level = 0;
                    while let Some(c) = self.peek() {
                        match c {
                            '{' => level += 1,
                            '}' => level -= 1,
                            ']' if level == 0 => break,
                            _ => {}
                        }
                        if level < 0 {
                            return None;
                        }
                        self.i += 1;
                    }
                    if self.peek() != Some(']') {
                        return None;
                    }
                    let src: String = self.c[start..self.i].iter().collect();
                    self.i += 1;
                    let index = parse(&src)?;
                    return Some(Node::Root(Box::new(index), Box::new(self.arg(depth + 1)?)));
                }
                Some(Node::Sqrt(Box::new(self.arg(depth + 1)?)))
            }
            "overset" | "stackrel" => {
                let over = self.arg(depth + 1)?;
                let base = self.arg(depth + 1)?;
                Some(Node::Over { base: Box::new(base), over: Some(Box::new(over)), under: None })
            }
            "underset" => {
                let under = self.arg(depth + 1)?;
                let base = self.arg(depth + 1)?;
                Some(Node::Over { base: Box::new(base), over: None, under: Some(Box::new(under)) })
            }
            "hat" | "widehat" | "bar" | "vec" | "overrightarrow" | "dot" | "ddot" | "tilde" | "widetilde" | "overline" | "underline" | "boxed" | "cancel" | "bcancel" | "xcancel" => {
                let d = match name {
                    "hat" => Deco::Hat,
                    "widehat" => Deco::WideHat,
                    "bar" => Deco::Bar,
                    "vec" | "overrightarrow" => Deco::Vec,
                    "dot" => Deco::Dot,
                    "ddot" => Deco::DDot,
                    "tilde" => Deco::Tilde,
                    "widetilde" => Deco::WideTilde,
                    "overline" => Deco::Overline,
                    "underline" => Deco::Underline,
                    "boxed" => Deco::Boxed,
                    "cancel" => Deco::Cancel,
                    "bcancel" => Deco::BCancel,
                    _ => Deco::XCancel,
                };
                Some(Node::Deco(d, Box::new(self.arg(depth + 1)?)))
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
            "left" => {
                let open = self.delim()?;
                let body = self.row(depth + 1)?;
                if !self.eat_cmd("right") {
                    return None;
                }
                let close = self.delim()?;
                Some(Node::Delim { open, close, body: Box::new(Node::Row(body)) })
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
    matches!(s, "=" | "+" | "−" | "×" | "÷" | "±" | "∓" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "≡" | "→" | "←") || OPS_EXT.contains(&s)
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

/// 줄들(왼쪽 들여쓰기, 배치)을 위에서 아래로 쌓는다. 기준선은 첫 줄에 둔다.
fn stack(rows: Vec<(f32, Lay)>, w: f32, s: f32) -> Lay {
    let gap = 0.25 * s;
    let mut items = Vec::new();
    let mut top = 0.0; // 지금 줄의 윗선(첫 줄 기준선이 0이 되도록 나중에 보정)
    let mut first_asc = 0.0;
    for (k, (dx, mut l)) in rows.into_iter().enumerate() {
        if k == 0 {
            first_asc = l.asc;
        } else {
            top += gap;
        }
        shift(&mut l.items, dx, top + l.asc - first_asc);
        items.append(&mut l.items);
        top += l.asc + l.desc;
    }
    Lay { w, asc: first_asc, desc: top - first_asc, items }
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
            if is_display(cx) && (BIG_LIMITS.contains(&t) || BIG_INTS.contains(&t)) {
                // 블록 수식의 큰 연산자: 크게 그리고 글자 가운데를 수식 축에 맞춘다.
                let mut l = text_lay(t, s * BIG_OP, 0.0, m);
                let dy = 0.17 * s;
                shift(&mut l.items, 0.0, dy);
                l.asc -= dy;
                l.desc += dy;
                return l;
            }
            let pad = if is_op(t) { OP_PAD * s } else { 0.0 };
            text_lay(t, s, pad, m)
        }
        Node::Text(t) => {
            let mut l = text_lay(t, s, 0.0, m);
            if FUNCS.contains(&t.as_str()) {
                l.w += 0.17 * s; // sin x 처럼 함수 이름 뒤에 가는 빈칸
            }
            l
        }
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
        Node::Frac(a, b) => frac_lay(a, b, cx, m, true),
        Node::Binom(a, b) => {
            let body = frac_lay(a, b, cx, m, false);
            delim_lay("(", ")", body, s, m)
        }
        Node::Style(sty, inner) => {
            let c = match sty {
                Sty::Display => Cx { fd: 0, ..cx },
                Sty::Text => Cx { fd: cx.fd.max(1), ..cx },
            };
            lay(inner, c, m)
        }
        Node::Sqrt(a) => root_lay(None, a, cx, m),
        Node::Root(idx, a) => root_lay(Some(idx), a, cx, m),
        Node::Over { base, over, under } => over_lay(base, over.as_deref(), under.as_deref(), cx, m),
        Node::Deco(d, a) => deco_lay(*d, a, cx, m),
        Node::Delim { open, close, body } => {
            let b = lay(body, cx, m);
            delim_lay(open, close, b, s, m)
        }
        Node::Stack(rows) => {
            let ls: Vec<Lay> = rows.iter().map(|r| lay(r, cx, m)).collect();
            let w = ls.iter().map(|l| l.w).fold(0.0, f32::max);
            stack(ls.into_iter().map(|l| ((w - l.w) / 2.0, l)).collect(), w, s)
        }
        Node::Align(rows) => {
            let ls: Vec<(Lay, Option<Lay>)> = rows.iter().map(|(l, r)| (lay(l, cx, m), r.as_ref().map(|r| lay(r, cx, m)))).collect();
            let lw = ls.iter().filter(|(_, r)| r.is_some()).map(|(l, _)| l.w).fold(0.0, f32::max);
            let rw = ls.iter().filter_map(|(_, r)| r.as_ref()).map(|r| r.w).fold(0.0, f32::max);
            let w = ls.iter().filter(|(_, r)| r.is_none()).map(|(l, _)| l.w).fold(lw + rw, f32::max);
            let off = (w - lw - rw) / 2.0;
            let mut out = Vec::new();
            for (l, r) in ls {
                match r {
                    None => out.push(((w - l.w) / 2.0, l)),
                    Some(r) => {
                        // 앞부분은 `&` 자리에 오른쪽을 맞추고, 뒷부분은 `&` 자리에서 왼쪽부터 시작한다.
                        let (lx, rx) = (off + lw - l.w, off + lw);
                        let (asc, desc) = (l.asc.max(r.asc), l.desc.max(r.desc));
                        let mut items = l.items;
                        shift(&mut items, lx, 0.0);
                        let mut ri = r.items;
                        shift(&mut ri, rx, 0.0);
                        items.append(&mut ri);
                        out.push((0.0, Lay { w, asc, desc, items }));
                    }
                }
            }
            stack(out, w, s)
        }
        Node::Script { base, sub, sup } => {
            if is_display(cx) && is_limit_base(base) {
                return over_lay(base, sup.as_deref(), sub.as_deref(), cx, m);
            }
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

/// 블록 수식에서 큰 연산자(∑ ∫ …)를 키우는 배율.
const BIG_OP: f32 = 1.35;

/// 블록(디스플레이) 스타일인가: 분수·첨자 안이 아니고 글자를 줄이지 않았다.
fn is_display(cx: Cx) -> bool {
    cx.fd == 0 && cx.scale >= 0.999
}

/// 블록 스타일에서 한계를 위·아래에 놓는 기호나 함수 이름인가.
fn is_limit_base(n: &Node) -> bool {
    match n {
        Node::Sym(t) => BIG_LIMITS.contains(&t.as_str()),
        Node::Text(t) => LIMIT_FUNCS.contains(&t.as_str()),
        _ => false,
    }
}

fn frac_lay(a: &Node, b: &Node, cx: Cx, m: &mut dyn FnMut(&str, f32) -> f32, bar: bool) -> Lay {
    let s = cx.scale;
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
    if bar {
        items.push(Item::Line { a: (0.05 * s, -axis), b: (w - 0.05 * s, -axis) });
    }
    Lay { w, asc, desc, items }
}

/// 제곱근(차수가 있으면 왼쪽 위에 작게 단다).
fn root_lay(index: Option<&Node>, a: &Node, cx: Cx, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let s = cx.scale;
    let mut inner = lay(a, cx, m);
    let mut idx = index.map(|n| lay(n, Cx { scale: s * 0.62, fd: cx.fd }, m));
    let lead = 0.62 * s;
    let top = -(inner.asc + 0.12 * s);
    let bottom = inner.desc;
    let dx = idx.as_ref().map_or(0.0, |i| (i.w - 0.2 * s).max(0.0));
    let w = lead + inner.w + 0.08 * s + dx;
    shift(&mut inner.items, lead + dx, 0.0);
    let mut items = inner.items;
    let mid = top + (bottom - top) * 0.62;
    let pts = [(0.06 * s + dx, mid), (0.2 * s + dx, bottom), (0.42 * s + dx, top), (w, top)];
    for k in 0..3 {
        items.push(Item::Line { a: pts[k], b: pts[k + 1] });
    }
    let mut asc = -top + RULE * s / 2.0;
    if let Some(i) = idx.as_mut() {
        let by = mid - 0.08 * s;
        shift(&mut i.items, (dx + 0.2 * s - i.w).max(0.0), by);
        asc = asc.max(-by + i.asc);
        items.append(&mut i.items);
    }
    Lay { w, asc, desc: inner.desc, items }
}

/// 기호 위·아래에 글을 쌓는다(한계, `\overset`, `\underset`). 위·아래 글은 첨자 크기다.
fn over_lay(base: &Node, over: Option<&Node>, under: Option<&Node>, cx: Cx, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let s = cx.scale;
    let mut b = lay(base, cx, m);
    let ss = Cx { scale: s * SCRIPT, fd: cx.fd };
    let (mut o, mut u) = (over.map(|n| lay(n, ss, m)), under.map(|n| lay(n, ss, m)));
    let w = b.w.max(o.as_ref().map_or(0.0, |l| l.w)).max(u.as_ref().map_or(0.0, |l| l.w));
    let gap = 0.08 * s;
    let (mut asc, mut desc) = (b.asc, b.desc);
    shift(&mut b.items, (w - b.w) / 2.0, 0.0);
    let mut items = b.items;
    if let Some(o) = o.as_mut() {
        shift(&mut o.items, (w - o.w) / 2.0, -(asc + gap + o.desc));
        asc += gap + o.desc + o.asc;
        items.append(&mut o.items);
    }
    if let Some(u) = u.as_mut() {
        shift(&mut u.items, (w - u.w) / 2.0, desc + gap + u.asc);
        desc += gap + u.asc + u.desc;
        items.append(&mut u.items);
    }
    Lay { w, asc, desc, items }
}

/// 위·아래 장식(모자, 막대, 화살표, 점, 물결), 줄 긋기(윗줄·밑줄), 상자, 취소선.
fn deco_lay(d: Deco, a: &Node, cx: Cx, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let s = cx.scale;
    let mut b = lay(a, cx, m);
    let (w, mid) = (b.w, b.w / 2.0);
    let y0 = -(b.asc - 0.14 * s); // 장식이 놓이는 기준 높이(글자 윗머리 바로 위)
    let line = |a: (f32, f32), b: (f32, f32)| Item::Line { a, b };
    let (mut asc, mut desc) = (b.asc, b.desc);
    let mut extra: Vec<Item> = Vec::new();
    match d {
        Deco::Overline => {
            let y = -(b.asc + 0.1 * s);
            extra.push(line((0.0, y), (w, y)));
            asc += 0.1 * s + RULE * s;
        }
        Deco::Underline => {
            let y = b.desc + 0.1 * s;
            extra.push(line((0.0, y), (w, y)));
            desc += 0.1 * s + RULE * s;
        }
        Deco::Hat | Deco::WideHat => {
            let half = if d == Deco::WideHat { (w / 2.0).max(0.2 * s) } else { 0.2 * s };
            extra.push(line((mid - half, y0), (mid, y0 - 0.16 * s)));
            extra.push(line((mid, y0 - 0.16 * s), (mid + half, y0)));
            asc = asc.max(-y0 + 0.16 * s);
        }
        Deco::Tilde | Deco::WideTilde => {
            let half = if d == Deco::WideTilde { (w / 2.0).max(0.22 * s) } else { 0.22 * s };
            let yc = y0 - 0.05 * s;
            let n = 10;
            let pt = |k: usize| {
                let t = k as f32 / n as f32;
                (mid - half + 2.0 * half * t, yc - 0.045 * s * (t * std::f32::consts::TAU).sin())
            };
            for k in 0..n {
                extra.push(line(pt(k), pt(k + 1)));
            }
            asc = asc.max(-yc + 0.05 * s);
        }
        Deco::Bar => {
            let half = (w * 0.5).max(0.2 * s);
            let y = y0 - 0.04 * s;
            extra.push(line((mid - half, y), (mid + half, y)));
            asc = asc.max(-y + RULE * s);
        }
        Deco::Vec => {
            let half = (w * 0.5).max(0.22 * s);
            let y = y0 - 0.05 * s;
            extra.push(line((mid - half, y), (mid + half, y)));
            extra.push(line((mid + half - 0.1 * s, y - 0.06 * s), (mid + half, y)));
            extra.push(line((mid + half - 0.1 * s, y + 0.06 * s), (mid + half, y)));
            asc = asc.max(-y + 0.06 * s);
        }
        Deco::Dot | Deco::DDot => {
            let y = y0 - 0.06 * s;
            let wd = m(".", s);
            let xs: &[f32] = if d == Deco::Dot { &[0.0] } else { &[-0.09, 0.09] };
            for dx in xs {
                extra.push(Item::Glyph { x: mid + dx * s - wd / 2.0, y, text: ".".into(), scale: s });
            }
            asc = asc.max(-y + 0.11 * s);
        }
        Deco::Boxed => {
            let pad = 0.12 * s;
            shift(&mut b.items, pad, 0.0);
            let (l, r, t, bt) = (0.0, w + 2.0 * pad, -(b.asc + pad), b.desc + pad);
            extra.push(line((l, t), (r, t)));
            extra.push(line((r, t), (r, bt)));
            extra.push(line((r, bt), (l, bt)));
            extra.push(line((l, bt), (l, t)));
            let mut items = b.items;
            items.append(&mut extra);
            return Lay { w: w + 2.0 * pad, asc: b.asc + pad + RULE * s, desc: b.desc + pad + RULE * s, items };
        }
        Deco::Cancel | Deco::BCancel | Deco::XCancel => {
            let (t, bt) = (-(b.asc - 0.1 * s), b.desc - 0.05 * s);
            if d != Deco::BCancel {
                extra.push(line((0.0, bt), (w, t)));
            }
            if d != Deco::Cancel {
                extra.push(line((0.0, t), (w, bt)));
            }
        }
    }
    let mut items = std::mem::take(&mut b.items);
    items.append(&mut extra);
    Lay { w, asc, desc, items }
}

/// 괄호 한 개의 모양: 0..1 단위 상자(x는 가로, y는 위→아래) 안의 꺾은선들과 가로 폭(em). 닫는 괄호는 좌우로 뒤집는다.
fn delim_shape(ch: &str) -> Option<(Vec<Vec<(f32, f32)>>, f32)> {
    let mir = |v: Vec<(f32, f32)>| -> Vec<(f32, f32)> { v.into_iter().map(|(x, y)| (1.0 - x, y)).collect() };
    let paren: Vec<(f32, f32)> = (0..=10)
        .map(|k| {
            let t = k as f32 / 10.0;
            (0.9 - 3.2 * t * (1.0 - t), t)
        })
        .collect();
    let brace = vec![(0.95, 0.0), (0.6, 0.04), (0.5, 0.12), (0.5, 0.42), (0.35, 0.48), (0.05, 0.5), (0.35, 0.52), (0.5, 0.58), (0.5, 0.88), (0.6, 0.96), (0.95, 1.0)];
    Some(match ch {
        "(" => (vec![paren], 0.3),
        ")" => (vec![mir(paren)], 0.3),
        "[" => (vec![vec![(0.85, 0.0), (0.2, 0.0), (0.2, 1.0), (0.85, 1.0)]], 0.28),
        "]" => (vec![mir(vec![(0.85, 0.0), (0.2, 0.0), (0.2, 1.0), (0.85, 1.0)])], 0.28),
        "{" => (vec![brace], 0.36),
        "}" => (vec![mir(brace)], 0.36),
        "|" => (vec![vec![(0.5, 0.0), (0.5, 1.0)]], 0.16),
        "‖" => (vec![vec![(0.3, 0.0), (0.3, 1.0)], vec![(0.7, 0.0), (0.7, 1.0)]], 0.28),
        "⟨" => (vec![vec![(0.8, 0.0), (0.2, 0.5), (0.8, 1.0)]], 0.3),
        "⟩" => (vec![mir(vec![(0.8, 0.0), (0.2, 0.5), (0.8, 1.0)])], 0.3),
        "⌊" => (vec![vec![(0.25, 0.0), (0.25, 1.0), (0.85, 1.0)]], 0.28),
        "⌋" => (vec![mir(vec![(0.25, 0.0), (0.25, 1.0), (0.85, 1.0)])], 0.28),
        "⌈" => (vec![vec![(0.85, 0.0), (0.25, 0.0), (0.25, 1.0)]], 0.28),
        "⌉" => (vec![mir(vec![(0.85, 0.0), (0.25, 0.0), (0.25, 1.0)])], 0.28),
        "/" => (vec![vec![(0.1, 1.0), (0.9, 0.0)]], 0.3),
        _ => return None,
    })
}

/// 내용 `body`의 앞뒤에 괄호를 붙인다. 내용이 글줄 높이 안이면 글꼴의 괄호 글자를, 더 크면 높이에 맞춰 선으로 그린 괄호를 쓴다.
fn delim_lay(open: &str, close: &str, body: Lay, s: f32, m: &mut dyn FnMut(&str, f32) -> f32) -> Lay {
    let small = body.asc <= TXT_ASC * s + 1e-3 && body.desc <= TXT_DESC * s + 1e-3;
    let (top, bot) = if small {
        (-TXT_ASC * s, TXT_DESC * s)
    } else {
        let half = (body.asc - AXIS * s).max(body.desc + AXIS * s) + 0.04 * s;
        (-AXIS * s - half, -AXIS * s + half)
    };
    let gap = 0.04 * s;
    let mut items = Vec::new();
    let mut x = 0.0;
    let mut one = |d: &str, x: &mut f32, items: &mut Vec<Item>| {
        if d.is_empty() {
            return;
        }
        if small && matches!(d, "(" | ")" | "[" | "]" | "{" | "}" | "|") {
            items.push(Item::Glyph { x: *x, y: 0.0, text: d.to_string(), scale: s });
            *x += m(d, s);
        } else if let Some((lines, w)) = delim_shape(d) {
            let dw = w * s;
            for l in lines {
                for p in l.windows(2) {
                    let q = |(ux, uy): (f32, f32)| (*x + ux * dw, top + uy * (bot - top));
                    items.push(Item::Line { a: q(p[0]), b: q(p[1]) });
                }
            }
            *x += dw;
        }
    };
    one(open, &mut x, &mut items);
    x += gap;
    let mut bi = body.items;
    shift(&mut bi, x, 0.0);
    items.append(&mut bi);
    x += body.w + gap;
    one(close, &mut x, &mut items);
    Lay { w: x, asc: (-top).max(body.asc), desc: bot.max(body.desc), items }
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
        for bad in [r"\frac{1}", r"\frac{1}{2", r"\unknown{x}", "x^", "^2", "x^2^3", r"\sqrt[3]", r"\sqrt[3{x}", r"\sqrt[]{x}", "{x", "x}", r"\text{a", r"\text{\alpha}", ""] {
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

    // ── 여러 줄 ──

    #[test]
    fn newline_in_block_is_a_space_not_a_row_break() {
        let one = parse_multi("a b").unwrap();
        assert_eq!(parse_multi("a\n   b\n").unwrap(), one);
        assert!(matches!(one, Node::Row(_)));
    }

    #[test]
    fn double_backslash_stacks_rows() {
        let Node::Stack(rows) = parse_multi(r"a \\ b \\ c").unwrap() else { panic!() };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], Node::Row(vec![Node::Sym("b".into())]));
        // 끝의 \\ 는 빈 줄을 만들지 않는다. 한 줄이면 그냥 Row.
        assert!(matches!(parse_multi(r"a \\").unwrap(), Node::Row(_)));
        // \\ 가 {} 안에 있으면 줄바꿈이 아니다(여기선 문법 오류).
        assert!(parse_multi(r"\frac{a \\ b}{c}").is_none());
    }

    #[test]
    fn aligned_splits_at_ampersand() {
        let n = parse_multi("\\begin{aligned}\n a &= b + c \\\\\n &= d \\\\ e\n\\end{aligned}").unwrap();
        let Node::Align(rows) = n else { panic!() };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, Node::Row(vec![Node::Sym("a".into())]));
        assert!(rows[0].1.is_some());
        assert_eq!(rows[1].0, Node::Row(vec![])); // &= d : 앞부분이 비어도 된다
        assert!(rows[2].1.is_none()); // & 없는 줄
        for env in ["align", "align*"] {
            assert!(matches!(parse_multi(&format!("\\begin{{{env}}} a&=b \\end{{{env}}}")), Some(Node::Align(_))), "{env}");
        }
    }

    #[test]
    fn multiline_syntax_errors_are_rejected() {
        for bad in [
            r"\begin{aligned} a&=b",                 // 안 닫힘
            r"\begin{aligned} a&=b \end{align}",     // 짝이 다름
            r"\begin{cases} a \end{cases}",          // 지원 안 함
            r"x = \begin{aligned} a&=b \end{aligned}", // 앞에 다른 글
            r"\begin{aligned} a&=b&=c \end{aligned}", // & 두 개
            r"a & b",                                 // aligned 밖의 &
            r"\begin{aligned} a&=\frac{1} \end{aligned}",
            r"\begin{aligned} \end{aligned}",
            "  \n ",
            r"\\",
        ] {
            assert!(parse_multi(bad).is_none(), "{bad:?} should fail");
        }
        assert!(parse_multi(r"a \& b").is_some()); // 이스케이프한 &는 글자
    }

    #[test]
    fn one_line_block_accepts_multiline_syntax_too() {
        assert!(matches!(parse_block(r"$$a \\ b$$"), Some(Node::Stack(_))));
        assert!(matches!(parse_block(r"$$\begin{aligned}a&=b\end{aligned}$$"), Some(Node::Align(_))));
    }

    fn glyph_pos(l: &Lay, t: &str) -> (f32, f32, f32) {
        l.items.iter().find_map(|i| if let Item::Glyph { x, y, text, .. } = i { (text == t).then_some((*x, *y, 0.0)) } else { None }).unwrap_or_else(|| panic!("no glyph {t}"))
    }

    #[test]
    fn stacked_rows_go_down_and_are_centered() {
        let l = layout(&parse_multi(r"abcd \\ x").unwrap(), &mut meas());
        let (x1, y1, _) = glyph_pos(&l, "abcd");
        let (x2, y2, _) = glyph_pos(&l, "x");
        assert!(y2 > y1 + TXT_ASC + TXT_DESC - 1e-4, "{y1} {y2}");
        // 가운데 정렬: 두 줄의 중심 x가 같다.
        assert!(((x1 + 1.0) - (x2 + 0.25)).abs() < 1e-4, "{x1} {x2}");
        assert!(l.asc + l.desc > 2.0 * (TXT_ASC + TXT_DESC));
        assert!((height_em(&parse_multi(r"abcd \\ x").unwrap()) - (l.asc + l.desc)).abs() < 1e-5);
    }

    #[test]
    fn aligned_rows_line_up_on_ampersand() {
        let l = layout(&parse_multi(r"\begin{aligned} a &= b \\ longer &= c \end{aligned}").unwrap(), &mut meas());
        let eq = |y: f32| l.items.iter().find_map(|i| if let Item::Glyph { x, y: gy, text, .. } = i { (text == "=" && (*gy - y).abs() < 1e-4).then_some(*x) } else { None });
        let ys: Vec<f32> = l.items.iter().filter_map(|i| if let Item::Glyph { y, text, .. } = i { (text == "=").then_some(*y) } else { None }).collect();
        assert_eq!(ys.len(), 2);
        assert!((eq(ys[0]).unwrap() - eq(ys[1]).unwrap()).abs() < 1e-4); // = 의 x가 같다
        assert!(ys[1] > ys[0]);
        // 앞부분은 오른쪽 정렬: a 가 longer 보다 오른쪽에서 시작한다.
        assert!(glyph_pos(&l, "a").0 > glyph_pos(&l, "longer").0);
        // 뒷부분은 왼쪽 정렬: b 와 c 가 같은 x 에서 시작한다.
        assert!((glyph_pos(&l, "b").0 - glyph_pos(&l, "c").0).abs() < 1e-4);
    }

    #[test]
    fn aligned_row_without_ampersand_is_centered() {
        let l = layout(&parse_multi(r"\begin{aligned} abcdefgh &= b \\ x \end{aligned}").unwrap(), &mut meas());
        let (x, _, _) = glyph_pos(&l, "x");
        assert!(x > 0.5 && x + 0.5 < l.w, "{x} {}", l.w);
    }

    // ── 2단계: 구조 명령 ──

    fn lines(l: &Lay) -> Vec<((f32, f32), (f32, f32))> {
        l.items.iter().filter_map(|i| if let Item::Line { a, b } = i { Some((*a, *b)) } else { None }).collect()
    }
    fn glyphs(l: &Lay) -> Vec<(String, f32, f32, f32)> {
        l.items.iter().filter_map(|i| if let Item::Glyph { x, y, text, scale } = i { Some((text.clone(), *x, *y, *scale)) } else { None }).collect()
    }
    fn g(l: &Lay, t: &str) -> (f32, f32, f32) {
        glyphs(l).into_iter().find(|(s, ..)| s == t).map(|(_, x, y, sc)| (x, y, sc)).unwrap_or_else(|| panic!("no glyph {t}"))
    }

    #[test]
    fn new_structure_commands_parse() {
        for ok in [
            r"\dfrac{a}{b}", r"\tfrac{a}{b}", r"\binom{n}{k}", r"\sqrt[3]{x}", r"\sqrt[n+1]{x^2}", r"\sum\limits_{i=1}^{n} i", r"\int\nolimits_0^1 x",
            r"\hat{x}\bar y\vec{v}\dot x\ddot x\tilde x\overline{ab}\underline{ab}\widehat{abc}", r"\boxed{a=b}", r"\cancel{x}", r"\overset{a}{=}", r"\underset{a}{=}",
            r"\stackrel{!}{=}", r"\left( \frac{a}{b} \right)", r"\left. x \right|", r"\left\{ a \right\}", r"\left\langle x \right\rangle", r"\left[ \left( x \right) \right]",
        ] {
            assert!(parse(ok).is_some(), "{ok:?} should parse");
        }
        for bad in [r"\left( x", r"x \right)", r"\left x \right)", r"\left( x \right", r"\overset{a}", r"\hat", r"\boxed", r"\binom{a}", r"\left\foo x \right)", r"\limits"] {
            assert!(parse(bad).is_none(), "{bad:?} should fail");
        }
        let Some(Node::Row(v)) = parse(r"\sqrt[3]{x}") else { panic!() };
        assert!(matches!(&v[0], Node::Root(i, _) if **i == Node::Row(vec![Node::Sym("3".into())])));
    }

    #[test]
    fn dfrac_and_tfrac_force_style() {
        let h = |s: &str| height_em(&parse(s).unwrap());
        // 중첩 분수 안쪽: 보통 분수는 줄여 그리고 dfrac은 줄이지 않으며 tfrac은 처음부터 줄인다.
        assert!(h(r"\frac{\dfrac{a}{b}}{c}") > h(r"\frac{\frac{a}{b}}{c}"));
        assert!(h(r"\tfrac{a}{b}") < h(r"\frac{a}{b}"));
        assert!((h(r"\dfrac{a}{b}") - h(r"\frac{a}{b}")).abs() < 1e-5);
    }

    #[test]
    fn binom_has_parens_and_no_bar() {
        let l = layout(&parse(r"\binom{n}{k}").unwrap(), &mut meas());
        let ls = lines(&l);
        assert!(!ls.is_empty());
        // 가로선(분수선)이 없다: 모든 선의 두 끝이 같은 y가 아니다.
        assert!(ls.iter().all(|(a, b)| (a.1 - b.1).abs() > 1e-4), "{ls:?}");
        // 괄호는 두 글자(n, k)가 쌓인 높이를 덮는다.
        let (yn, yk) = (g(&l, "n").1, g(&l, "k").1);
        let top = ls.iter().map(|(a, b)| a.1.min(b.1)).fold(f32::MAX, f32::min);
        let bot = ls.iter().map(|(a, b)| a.1.max(b.1)).fold(f32::MIN, f32::max);
        assert!(top < yn - TXT_ASC + 1e-3 && bot > yk + TXT_DESC - 1e-3, "{top} {bot} {yn} {yk}");
        assert!(yn < yk);
    }

    #[test]
    fn nth_root_puts_small_index_left_above() {
        let plain = layout(&parse(r"\sqrt{x}").unwrap(), &mut meas());
        let l = layout(&parse(r"\sqrt[3]{x}").unwrap(), &mut meas());
        let (ix, iy, isc) = g(&l, "3");
        let (xx, xy, _) = g(&l, "x");
        assert!(isc < 1.0 && ix < xx && iy < xy, "{ix} {iy} {xx} {xy}");
        assert_eq!(lines(&l).len(), 3);
        assert!(l.w >= plain.w);
        // 차수가 넓어도 왼쪽으로 삐져나가지 않는다.
        let l = layout(&parse(r"\sqrt[12345678]{x}").unwrap(), &mut meas());
        assert!(glyphs(&l).iter().all(|(_, x, ..)| *x >= -1e-4));
        assert!(lines(&l).iter().all(|(a, b)| a.0 >= -1e-4 && b.0 <= l.w + 1e-4));
    }

    #[test]
    fn display_limits_go_above_and_below_but_inline_go_beside() {
        let l = layout(&parse(r"\sum_{i}^{n} x").unwrap(), &mut meas());
        let (sx, sy, ssc) = g(&l, "∑");
        let (nx, ny, nsc) = g(&l, "n");
        let (ix, iy, _) = g(&l, "i");
        assert!(ssc > 1.0 && nsc < 1.0); // 큰 연산자, 작은 한계
        assert!(ny < sy && iy > sy); // 위·아래
        // 가운데 정렬: 세 글자의 중심 x가 같다.
        let c = |x: f32, t: &str, sc: f32| x + (t.chars().count() as f32 * 0.5 * sc) / 2.0;
        assert!((c(sx, "∑", ssc) - c(nx, "n", nsc)).abs() < 1e-3 && (c(sx, "∑", ssc) - c(ix, "i", nsc)).abs() < 1e-3);
        // 분수 안(줄 안 스타일)에서는 첨자 위치.
        let l = layout(&parse(r"\frac{\sum_{i}^{n} x}{2}").unwrap(), &mut meas());
        let (sx, _, ssc) = g(&l, "∑");
        assert!(ssc <= 1.0);
        assert!(g(&l, "n").0 > sx);
        // lim 도 위·아래, \nolimits 면 옆, \limits 는 어디서든 위·아래.
        let l = layout(&parse(r"\lim_{x}").unwrap(), &mut meas());
        assert!(g(&l, "x").1 > g(&l, "lim").1);
        let l = layout(&parse(r"\sum\nolimits_{i}").unwrap(), &mut meas());
        assert!(g(&l, "i").0 > g(&l, "∑").0 + 0.4);
        let l = layout(&parse(r"\frac{\sum\limits_{i}^{n}}{2}").unwrap(), &mut meas());
        assert!(g(&l, "n").1 < g(&l, "∑").1);
        // 적분은 보통 옆에 달고 \limits 를 주면 위·아래.
        let l = layout(&parse(r"\int_0^1").unwrap(), &mut meas());
        assert!(g(&l, "1").0 > g(&l, "∫").0 + 0.4);
        let l = layout(&parse(r"\int\limits_0^1").unwrap(), &mut meas());
        assert!(g(&l, "1").1 < g(&l, "∫").1 && g(&l, "0").1 > g(&l, "∫").1);
    }

    #[test]
    fn accents_decorate_above_or_around() {
        let plain = layout(&parse("x").unwrap(), &mut meas());
        for (src, n) in [(r"\hat{x}", 2), (r"\bar{x}", 1), (r"\vec{x}", 3), (r"\tilde{x}", 10), (r"\overline{x}", 1), (r"\underline{x}", 1), (r"\boxed{x}", 4), (r"\cancel{x}", 1), (r"\xcancel{x}", 2)] {
            let l = layout(&parse(src).unwrap(), &mut meas());
            assert_eq!(lines(&l).len(), n, "{src}");
            assert!(glyphs(&l).iter().any(|(t, ..)| t == "x"), "{src}");
        }
        for src in [r"\hat{x}", r"\overline{x}", r"\dot{x}", r"\ddot{x}"] {
            let l = layout(&parse(src).unwrap(), &mut meas());
            assert!(l.asc > plain.asc && l.desc == plain.desc, "{src}");
        }
        let l = layout(&parse(r"\underline{x}").unwrap(), &mut meas());
        assert!(l.desc > plain.desc && l.asc == plain.asc);
        assert_eq!(glyphs(&layout(&parse(r"\ddot{x}").unwrap(), &mut meas())).iter().filter(|(t, ..)| t == ".").count(), 2);
        // 윗줄은 내용 폭 전체, 상자는 내용을 감싼다.
        let l = layout(&parse(r"\overline{abcd}").unwrap(), &mut meas());
        assert!(lines(&l).iter().all(|(a, b)| a.0 == 0.0 && (b.0 - 2.0).abs() < 1e-4 && a.1 < -TXT_ASC));
        let l = layout(&parse(r"\boxed{abcd}").unwrap(), &mut meas());
        let (x, ..) = g(&l, "abcd");
        assert!(x > 0.0 && l.w > 2.0 + x);
        assert!(lines(&l).iter().all(|(a, b)| a.0 >= 0.0 && b.0 <= l.w + 1e-4));
    }

    #[test]
    fn overset_and_underset_stack_small_text() {
        let l = layout(&parse(r"\overset{!}{=}").unwrap(), &mut meas());
        let (ox, oy, osc) = g(&l, "!");
        let (_, by, _) = g(&l, "=");
        assert!(oy < by && osc < 1.0 && ox >= 0.0);
        let l = layout(&parse(r"\underset{a}{=}").unwrap(), &mut meas());
        assert!(g(&l, "a").1 > g(&l, "=").1);
    }

    #[test]
    fn left_right_grow_to_content_height() {
        let tall = layout(&parse(r"\left( \frac{a}{b} \right)").unwrap(), &mut meas());
        let inner = layout(&parse(r"\frac{a}{b}").unwrap(), &mut meas());
        let ls = lines(&tall);
        let top = ls.iter().map(|(a, b)| a.1.min(b.1)).fold(f32::MAX, f32::min);
        let bot = ls.iter().map(|(a, b)| a.1.max(b.1)).fold(f32::MIN, f32::max);
        assert!(top <= -inner.asc + 1e-3 && bot >= inner.desc - 1e-3, "{top} {bot}");
        assert!(tall.w > inner.w);
        // 안쪽이 글줄 높이 안이면 보통 글꼴의 괄호 글자를 쓴다.
        let small = layout(&parse(r"\left( x \right)").unwrap(), &mut meas());
        assert!(lines(&small).is_empty());
        assert_eq!(glyphs(&small).iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), vec!["(", "x", ")"]);
        // `.` 은 그 쪽 괄호가 없다. 이 경우 폭이 줄어든다.
        let dot = layout(&parse(r"\left. \frac{a}{b} \right|").unwrap(), &mut meas());
        assert_eq!(lines(&dot).len() - 1, 1); // 오른쪽 | 한 줄(분수선 제외)
        assert!(dot.w < tall.w);
        // 안쪽 괄호가 더 높으면 바깥도 그만큼 커진다.
        let nest = layout(&parse(r"\left[ \left( \frac{a}{b} \right)^2 \right]").unwrap(), &mut meas());
        assert!(nest.asc >= tall.asc);
        // 각 종류가 선으로 그려진다.
        for d in [r"\{ \}", r"\langle \rangle", r"\lfloor \rfloor", r"\lceil \rceil", r"\| \|", r"[ ]", r"| |"] {
            let (o, c) = d.split_once(' ').unwrap();
            let l = layout(&parse(&format!(r"\left{o} \frac{{a}}{{b}} \right{c}")).unwrap(), &mut meas());
            assert!(lines(&l).len() >= 3, "{d}");
        }
    }

    #[test]
    fn new_structures_height_ignores_glyph_widths() {
        for src in [
            r"\sum_{i=1}^{n} x_i", r"\lim_{x \to 0} \frac{\sin x}{x}", r"\binom{n}{k}", r"\sqrt[3]{\frac{a}{b}}", r"\hat{x} + \vec{v} + \ddot{y}", r"\left( \frac{a}{b} \right)", r"\boxed{\dfrac{a}{b}}",
            r"\overset{a}{\underset{b}{=}}", r"\int\limits_0^1 \tfrac{1}{2}", r"\begin{aligned} \sum_{i}^{n} &= \frac{a}{b} \\ &= \binom{n}{2} \end{aligned}",
        ] {
            let n = parse_multi(src).unwrap_or_else(|| panic!("{src}"));
            let a = layout(&n, &mut |s, sc| s.chars().count() as f32 * 0.5 * sc);
            let b = layout(&n, &mut |s, sc| s.chars().count() as f32 * 1.3 * sc + 0.4);
            assert!((a.asc - b.asc).abs() < 1e-5 && (a.desc - b.desc).abs() < 1e-5, "{src}");
            assert!((height_em(&n) - (a.asc + a.desc)).abs() < 1e-5, "{src}");
        }
    }

    // ── 5단계: 기호 표 ──

    #[test]
    fn symbol_table_has_hundreds_of_entries_without_duplicates() {
        assert!(SYMBOLS_EXT.len() >= 250, "{}", SYMBOLS_EXT.len());
        let mut seen = std::collections::HashSet::new();
        for (k, v) in SYMBOLS_EXT {
            assert!(seen.insert(*k), "중복 이름 {k}");
            assert!(!v.is_empty() && v.chars().count() == 1, "{k}: {v:?}"); // 기호 하나(글자 한 개)
            assert!(symbol_short(k).is_none(), "{k}는 짧은 표에도 있다");
        }
    }

    #[test]
    fn common_symbols_resolve() {
        for (cmd, ch) in [
            ("neq", "≠"), ("leftrightarrow", "↔"), ("Rightarrow", "⇒"), ("mapsto", "↦"), ("in", "∈"), ("notin", "∉"), ("subset", "⊂"), ("cup", "∪"), ("cap", "∩"), ("emptyset", "∅"),
            ("forall", "∀"), ("exists", "∃"), ("nabla", "∇"), ("ldots", "…"), ("cdots", "⋯"), ("vdots", "⋮"), ("ddots", "⋱"), ("varepsilon", "ε"), ("epsilon", "ϵ"), ("varphi", "φ"), ("phi", "ϕ"),
            ("vartheta", "ϑ"), ("Xi", "Ξ"), ("Upsilon", "Υ"), ("upsilon", "υ"), ("iota", "ι"), ("kappa", "κ"), ("sim", "∼"), ("propto", "∝"), ("oplus", "⊕"), ("otimes", "⊗"), ("wedge", "∧"), ("vee", "∨"),
        ] {
            assert_eq!(symbol(cmd), Some(ch), "{cmd}");
        }
        let Node::Row(v) = parse(r"A \subset B \cup C \Rightarrow \forall x \in \emptyset \ldots").unwrap() else { panic!() };
        assert!(v.contains(&Node::Sym("⊂".into())) && v.contains(&Node::Sym("…".into())));
        assert!(parse(r"\notasymbol").is_none());
    }

    #[test]
    fn relations_and_arrows_get_operator_spacing_but_dots_do_not() {
        let w = |s: &str| layout(&parse(s).unwrap(), &mut meas()).w;
        assert!(w(r"\subset") > 0.5 + 0.4);
        assert!(w(r"\Rightarrow") > 0.5 + 0.4);
        assert!((w(r"\ldots") - 0.5).abs() < 1e-4);
        assert!((w(r"\nabla") - 0.5).abs() < 1e-4);
    }

    /// 실제 글꼴로 기호가 그려지는지(네모 □로 나오지 않는지) 확인하는 개발용 점검.
    /// `cargo test --release -- --ignored glyph_coverage --nocapture`
    #[test]
    #[ignore]
    fn glyph_coverage() {
        use eframe::egui::{Context, FontId, RawInput};
        let ctx = Context::default();
        crate::theme::install_with(&ctx, None, None);
        let _ = ctx.run_ui(RawInput::default(), |_| {});
        let _ = ctx.run_ui(RawInput::default(), |_| {});
        let id = FontId::proportional(14.0);
        let base = "×÷±∓·≤≥≠≈≡∞∑∏∫→←∂°αβγδεζηθλμνξπρστφχψωΓΔΘΛΠΣΦΨΩ−∗%$&#_{}()[]|.,:;!?'";
        let mut missing: Vec<String> = Vec::new();
        ctx.fonts_mut(|f| {
            for c in base.chars() {
                if !f.has_glyph(&id, c) {
                    missing.push(format!("(기본) {c}"));
                }
            }
            for (k, v) in SYMBOLS_EXT {
                let c = v.chars().next().unwrap();
                if !f.has_glyph(&id, c) {
                    missing.push(format!("{k} {v} U+{:04X}", c as u32));
                }
            }
            for c in "⨁⨂⋃⋂⋁⋀∐∬∭∮⟨⟩⌊⌋⌈⌉‖".chars() {
                if !f.has_glyph(&id, c) {
                    missing.push(format!("(큰 연산자·괄호) {c} U+{:04X}", c as u32));
                }
            }
        });
        println!("글꼴에 없는 기호 {}개:", missing.len());
        for m in &missing {
            println!("  {m}");
        }
        // 참고: 시스템 기호 글꼴을 대체 글꼴로 더하면 얼마나 메워지는가(앱은 아직 이 글꼴들을 쓰지 않는다).
        let want: Vec<char> = base.chars().chain(SYMBOLS_EXT.iter().map(|(_, v)| v.chars().next().unwrap())).chain("⨁⨂⋃⋂⋁⋀∐∬∭∮⟨⟩⌊⌋⌈⌉‖".chars()).collect();
        for (file, index) in [("seguisym.ttf", 0u32), ("cambria.ttc", 1), ("l_10646.ttf", 0)] {
            let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{file}")) else {
                println!("[{file}] 없음");
                continue;
            };
            let ctx = Context::default();
            let mut fonts = eframe::egui::FontDefinitions::default();
            let mut data = eframe::egui::FontData::from_owned(bytes);
            data.index = index;
            fonts.font_data.insert("x".into(), std::sync::Arc::new(data));
            fonts.families.get_mut(&eframe::egui::FontFamily::Proportional).unwrap().insert(0, "x".into());
            ctx.set_fonts(fonts);
            let _ = ctx.run_ui(RawInput::default(), |_| {});
            let _ = ctx.run_ui(RawInput::default(), |_| {});
            let mut miss: Vec<char> = Vec::new();
            ctx.fonts_mut(|f| {
                for c in &want {
                    if !f.has_glyph(&id, *c) {
                        miss.push(*c);
                    }
                }
            });
            println!("[{file}] 이 글꼴이 있으면 못 그리는 기호 {}개: {}", miss.len(), miss.iter().collect::<String>());
        }
    }

    #[test]
    fn height_ignores_glyph_widths() {
        let n = parse(r"\frac{\text{긴 글자가 아주 아주 많은 분자}}{x}").unwrap();
        let a = height_em(&n);
        let b = layout(&n, &mut |_, _| 3.0);
        assert!((a - (b.asc + b.desc)).abs() < 1e-5);
    }
}
