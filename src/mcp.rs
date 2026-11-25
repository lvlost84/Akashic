//! MCP(Model Context Protocol) 서버: Claude Code 같은 AI 클라이언트가 아카식 문서를 읽고(옵션으로) 쓸 수 있게 한다.
//!
//! 두 가지로 쓴다. 같은 코드(Server)를 공유한다.
//! - `Akashic.exe --mcp`: 창 없이 표준 입출력(stdio)으로 JSON-RPC를 주고받는다(클라이언트가 프로그램을 띄운다).
//! - 앱 실행 중 서버 가동: 127.0.0.1의 HTTP(Streamable HTTP, 응답은 JSON 한 번). 무작위 토큰(Bearer)이 있어야 한다.
//!
//! 정책: 암호화한 글(🔒【…】🔒)은 어떤 도구로도 내용을 내보내지 않고 "암호화됨"으로 가린다.
//! 쓰기 도구(만들기·고치기·덧붙이기)는 옵션(Config.mcp_write)을 켠 때만 보이고 동작한다.
//! 이 모듈은 Anthropic API를 호출하지 않는다(로컬 프로그램일 뿐이라 API 비용이 없다).
use crate::db::{self, Db, Filter, Sort, View};
use crate::textcrypt::{CLOSE, OPEN};
use chrono::TimeZone;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 39218;
/// 암호화한 글 자리에 대신 보내는 글.
pub const MASK: &str = "🔒【암호화됨】🔒";
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// 본문 한 번에 보내는 기본·최대 글자 수(토큰 사용을 줄이려는 상한).
const READ_DEFAULT: usize = 20_000;
const READ_MAX: usize = 200_000;
/// 쓰기로 받는 본문 상한(바이트).
const WRITE_MAX: usize = 1_000_000;

/// 암호화한 글을 가린 본문과 가린 곳 수.
pub fn mask(body: &str) -> (String, usize) {
    if !body.contains(OPEN) {
        return (body.to_string(), 0);
    }
    let mut out = String::with_capacity(body.len());
    let mut n = 0;
    let mut rest = body;
    while let Some(a) = rest.find(OPEN) {
        out.push_str(&rest[..a]);
        out.push_str(MASK);
        n += 1;
        let after = &rest[a + OPEN.len()..];
        match after.find(CLOSE) {
            Some(b) => rest = &after[b + CLOSE.len()..],
            None => {
                rest = ""; // 닫는 표식이 없으면 끝까지 가린다(내용을 새지 않게)
                break;
            }
        }
    }
    out.push_str(rest);
    (out, n)
}

fn fmt_ts(ts: i64) -> String {
    chrono::Local.timestamp_opt(ts, 0).single().map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}
fn int_arg(a: &Value, k: &str) -> Option<i64> {
    a.get(k).and_then(Value::as_i64)
}

fn snippet(body: &str, terms: &[String], width: usize) -> String {
    let lc = |c: char| c.to_lowercase().next().unwrap_or(c);
    let chars: Vec<char> = body.chars().collect();
    let low: Vec<char> = chars.iter().map(|c| lc(*c)).collect();
    let mut at = None;
    for t in terms {
        let tc: Vec<char> = t.chars().map(lc).collect();
        if tc.is_empty() || tc.len() > low.len() {
            continue;
        }
        if let Some(p) = low.windows(tc.len()).position(|w| w == tc.as_slice()) {
            at = Some(p);
            break;
        }
    }
    let start = at.map_or(0, |p| p.saturating_sub(width / 4));
    let end = (start + width).min(chars.len());
    let s: String = chars[start..end].iter().map(|c| if *c == '\n' || *c == '\r' { ' ' } else { *c }).collect();
    format!("{}{}{}", if start > 0 { "…" } else { "" }, s.trim(), if end < chars.len() { "…" } else { "" })
}

pub struct Server {
    db: Db,
    /// 쓰기 도구를 허용하는지(호출 때마다 물어 옵션을 바로 반영한다).
    write: Box<dyn Fn() -> bool + Send>,
}

impl Server {
    pub fn new(db: Db, write: Box<dyn Fn() -> bool + Send>) -> Server {
        Server { db, write }
    }

    /// JSON-RPC 메시지 한 건을 처리한다. 응답이 없는 메시지(알림)는 None.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(rpc_err(Value::Null, -32700, "JSON을 해석할 수 없습니다")),
        };
        let Some(obj) = req.as_object() else {
            return Some(rpc_err(Value::Null, -32600, "잘못된 요청입니다"));
        };
        // id가 없으면 알림(또는 클라이언트의 응답)이라 답하지 않는다.
        let id = obj.get("id").cloned()?;
        let method = obj.get("method").and_then(Value::as_str).unwrap_or("");
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let res = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tool_defs() })),
            "tools/call" => Ok(self.call(&params)),
            _ => Err(format!("지원하지 않는 메서드: {method}")),
        };
        Some(match res {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }).to_string(),
            Err(m) => rpc_err(id, -32601, &m),
        })
    }

    fn initialize(&self, p: &Value) -> Value {
        let want = str_arg(p, "protocolVersion").unwrap_or("");
        let ver = PROTOCOLS.iter().find(|v| **v == want).unwrap_or(&PROTOCOLS[0]);
        json!({
            "protocolVersion": ver,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "akashic", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "아카식 마크다운 노트 저장소입니다. 문서 id는 list_documents·search_documents로 찾고 get_document로 읽습니다. 암호화한 글은 '암호화됨'으로 가려져 보입니다."
        })
    }

    fn tool_defs(&self) -> Value {
        let ro = json!({ "readOnlyHint": true });
        let mut t = vec![
            json!({ "name": "list_documents", "description": "문서 목록(최근 수정순). 분류·태그로 거를 수 있다. 휴지통 문서는 제외.",
                "inputSchema": { "type": "object", "properties": {
                    "category_id": { "type": "integer", "description": "이 분류의 문서만(list_categories의 id)" },
                    "tag": { "type": "string", "description": "이 태그가 달린 문서만" },
                    "limit": { "type": "integer", "description": "최대 개수(기본 30, 최대 100)" } } },
                "annotations": ro }),
            json!({ "name": "search_documents", "description": "제목·본문·태그에서 검색한다. 결과마다 일치한 부분 근처의 짧은 발췌를 준다.",
                "inputSchema": { "type": "object", "properties": {
                    "query": { "type": "string", "description": "검색어(공백으로 나누면 모두 포함하는 문서)" },
                    "limit": { "type": "integer", "description": "최대 개수(기본 10, 최대 30)" } }, "required": ["query"] },
                "annotations": ro }),
            json!({ "name": "get_document", "description": "문서 한 건의 본문을 읽는다. 길면 앞부분만 주고 전체 길이를 알려 준다.",
                "inputSchema": { "type": "object", "properties": {
                    "id": { "type": "integer" },
                    "max_chars": { "type": "integer", "description": "돌려줄 본문 글자 수 상한(기본 20000, 최대 200000)" } }, "required": ["id"] },
                "annotations": ro }),
            json!({ "name": "list_categories", "description": "분류 트리(들여쓰기)와 분류별 문서 수.",
                "inputSchema": { "type": "object", "properties": {} }, "annotations": ro }),
            json!({ "name": "list_tags", "description": "모든 태그.",
                "inputSchema": { "type": "object", "properties": {} }, "annotations": ro }),
            json!({ "name": "list_versions", "description": "문서의 변경 이력 목록(문서당 최근 10개). 내용은 주지 않는다.",
                "inputSchema": { "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] },
                "annotations": ro }),
        ];
        if (self.write)() {
            t.push(json!({ "name": "create_document", "description": "새 문서를 만든다.",
                "inputSchema": { "type": "object", "properties": {
                    "title": { "type": "string" }, "body": { "type": "string", "description": "마크다운 본문" },
                    "category_id": { "type": "integer" }, "tags": { "type": "array", "items": { "type": "string" } } }, "required": ["title"] },
                "annotations": { "readOnlyHint": false, "destructiveHint": false } }));
            t.push(json!({ "name": "update_document", "description": "문서의 제목·본문을 바꾼다(주어진 항목만). 이전 내용은 변경 이력에 남는다. 암호화한 글이 있는 문서의 본문은 바꿀 수 없다(append_to_document만 가능).",
                "inputSchema": { "type": "object", "properties": {
                    "id": { "type": "integer" }, "title": { "type": "string" }, "body": { "type": "string", "description": "새 본문 전체" } }, "required": ["id"] },
                "annotations": { "readOnlyHint": false, "destructiveHint": true } }));
            t.push(json!({ "name": "append_to_document", "description": "문서 본문 끝에 글을 덧붙인다(기존 내용은 그대로).",
                "inputSchema": { "type": "object", "properties": { "id": { "type": "integer" }, "text": { "type": "string" } }, "required": ["id", "text"] },
                "annotations": { "readOnlyHint": false, "destructiveHint": false } }));
        }
        Value::Array(t)
    }

    fn call(&mut self, p: &Value) -> Value {
        let name = str_arg(p, "name").unwrap_or("");
        let args = p.get("arguments").cloned().unwrap_or_else(|| json!({}));
        match self.dispatch(name, &args) {
            Ok(t) => json!({ "content": [{ "type": "text", "text": t }] }),
            Err(m) => json!({ "content": [{ "type": "text", "text": m }], "isError": true }),
        }
    }

    fn dispatch(&mut self, name: &str, a: &Value) -> Result<String, String> {
        match name {
            "list_documents" => self.list_documents(a),
            "search_documents" => self.search_documents(a),
            "get_document" => self.get_document(a),
            "list_categories" => self.list_categories(),
            "list_tags" => Ok(self.db.all_tags()?.join("\n")),
            "list_versions" => self.list_versions(a),
            "create_document" | "update_document" | "append_to_document" => {
                if !(self.write)() {
                    return Err("쓰기가 꺼져 있습니다. 아카식 옵션 > 백업·동기화 > MCP에서 '쓰기 도구 허용'을 켜야 합니다.".into());
                }
                match name {
                    "create_document" => self.create_document(a),
                    "update_document" => self.update_document(a),
                    _ => self.append_to_document(a),
                }
            }
            _ => Err(format!("알 수 없는 도구: {name}")),
        }
    }

    fn cat_names(&self) -> HashMap<i64, String> {
        self.db.categories().unwrap_or_default().into_iter().map(|c| (c.id, c.name)).collect()
    }

    fn list_documents(&self, a: &Value) -> Result<String, String> {
        let limit = int_arg(a, "limit").unwrap_or(30).clamp(1, 100) as usize;
        let view = int_arg(a, "category_id").map_or(View::All, View::Category);
        let tags = str_arg(a, "tag").map(db::norm_tag).filter(|t| !t.is_empty()).into_iter().collect();
        let rows = self.db.list_docs(&Filter { view, tags, query: String::new(), sort: Sort::Updated })?;
        let cats = self.cat_names();
        let mut out = format!("문서 {}건 중 {}건:\n", rows.len(), rows.len().min(limit));
        for r in rows.iter().take(limit) {
            out.push_str(&format!(
                "[{}] {} · 수정 {}{}\n",
                r.id,
                if r.title.trim().is_empty() { "(제목 없음)" } else { &r.title },
                fmt_ts(r.updated),
                r.category.and_then(|c| cats.get(&c)).map(|n| format!(" · 분류 {n}")).unwrap_or_default()
            ));
        }
        Ok(out)
    }

    fn search_documents(&self, a: &Value) -> Result<String, String> {
        let q = str_arg(a, "query").unwrap_or("").trim().to_string();
        if q.is_empty() {
            return Err("query가 비었습니다".into());
        }
        let limit = int_arg(a, "limit").unwrap_or(10).clamp(1, 30) as usize;
        let rows = self.db.list_docs(&Filter { view: View::All, tags: vec![], query: q.clone(), sort: Sort::Updated })?;
        let terms: Vec<String> = q.split_whitespace().map(str::to_string).collect();
        let mut out = format!("검색 결과 {}건 중 {}건:\n", rows.len(), rows.len().min(limit));
        for r in rows.iter().take(limit) {
            let body = self.db.get_doc(r.id)?.map(|d| mask(&d.body).0).unwrap_or_default();
            out.push_str(&format!(
                "[{}] {} · 수정 {}\n    {}\n",
                r.id,
                if r.title.trim().is_empty() { "(제목 없음)" } else { &r.title },
                fmt_ts(r.updated),
                snippet(&body, &terms, 100)
            ));
        }
        Ok(out)
    }

    fn get_document(&self, a: &Value) -> Result<String, String> {
        let id = int_arg(a, "id").ok_or("id가 필요합니다")?;
        let max = (int_arg(a, "max_chars").unwrap_or(READ_DEFAULT as i64).max(1) as usize).min(READ_MAX);
        let d = self.db.get_doc(id)?.ok_or_else(|| format!("문서 {id}이(가) 없습니다"))?;
        let (body, hidden) = mask(&d.body);
        let total = body.chars().count();
        let shown: String = body.chars().take(max).collect();
        let cats = self.cat_names();
        let tags = self.db.doc_tags(id).unwrap_or_default();
        let mut out = format!(
            "# {} (id {})\n분류: {} · 태그: {} · 만든 {} · 수정 {}{}\n",
            if d.title.trim().is_empty() { "(제목 없음)" } else { d.title.trim() },
            d.id,
            d.category.and_then(|c| cats.get(&c).cloned()).unwrap_or_else(|| "없음".into()),
            if tags.is_empty() { "없음".into() } else { tags.join(", ") },
            fmt_ts(d.created),
            fmt_ts(d.updated),
            if d.trashed { " · 휴지통에 있음" } else { "" }
        );
        if hidden > 0 {
            out.push_str(&format!("암호화한 글 {hidden}곳은 '{MASK}'로 가렸습니다.\n"));
        }
        if total > shown.chars().count() {
            out.push_str(&format!("본문이 길어 앞 {max}자만 보냅니다(전체 {total}자). 더 읽으려면 max_chars를 늘리세요.\n"));
        }
        out.push_str("\n---\n");
        out.push_str(&shown);
        Ok(out)
    }

    fn list_categories(&self) -> Result<String, String> {
        let cats = self.db.categories()?;
        // 부모 아래에 자식이 오도록 깊이 우선으로 늘어놓는다.
        fn walk(cats: &[db::Category], parent: Option<i64>, depth: usize, out: &mut String, seen: &mut Vec<i64>) {
            for c in cats.iter().filter(|c| c.parent == parent) {
                if seen.contains(&c.id) {
                    continue;
                }
                seen.push(c.id);
                out.push_str(&format!("{}[{}] {} ({}건)\n", "  ".repeat(depth), c.id, c.name, c.count));
                walk(cats, Some(c.id), depth + 1, out, seen);
            }
        }
        let mut out = String::new();
        walk(&cats, None, 0, &mut out, &mut Vec::new());
        Ok(if out.is_empty() { "분류가 없습니다".into() } else { out })
    }

    fn list_versions(&self, a: &Value) -> Result<String, String> {
        let id = int_arg(a, "id").ok_or("id가 필요합니다")?;
        let v = self.db.versions(id)?;
        if v.is_empty() {
            return Ok("변경 이력이 없습니다".into());
        }
        Ok(v.iter().map(|x| format!("버전 {} · {} · {} · {}자", x.id, fmt_ts(x.saved_at), x.title, x.body.chars().count())).collect::<Vec<_>>().join("\n"))
    }

    // ---------- 쓰기 ----------
    fn check_text(s: &str, what: &str) -> Result<(), String> {
        if s.len() > WRITE_MAX {
            return Err(format!("{what}이(가) 너무 깁니다(최대 {WRITE_MAX}바이트)"));
        }
        if s.contains(OPEN) || s.contains(CLOSE) {
            return Err(format!("{what}에 암호화 표식(🔒【 】🔒)은 쓸 수 없습니다. 암호화는 아카식 화면에서만 합니다"));
        }
        Ok(())
    }

    fn editable(&self, id: i64) -> Result<db::Doc, String> {
        let d = self.db.get_doc(id)?.ok_or_else(|| format!("문서 {id}이(가) 없습니다"))?;
        if d.trashed {
            return Err("휴지통에 있는 문서는 바꿀 수 없습니다".into());
        }
        Ok(d)
    }

    /// 문서를 읽고 `f`가 정한 (제목, 본문)으로 바꾼다. 읽은 뒤 다른 쪽이 먼저 고쳤으면 새로 읽어 다시 한다(최대 5번).
    /// `f`가 `None`이거나 결과가 같으면 바꾸지 않고 `false`. 앱 화면의 저장과 겹쳐도 그쪽 내용을 덮어쓰지 않는다.
    fn modify(&mut self, id: i64, mut f: impl FnMut(&db::Doc) -> Result<Option<(String, String)>, String>) -> Result<bool, String> {
        for _ in 0..5 {
            let d = self.editable(id)?;
            let Some((title, body)) = f(&d)? else { return Ok(false) };
            if title == d.title && body == d.body {
                return Ok(false);
            }
            if self.db.save_doc_if(id, &d.title, &d.body, &title, &body)? {
                return Ok(true);
            }
        }
        Err("문서가 계속 다른 곳에서 바뀌고 있어 저장하지 못했습니다. 잠시 뒤 다시 시도하세요".into())
    }

    fn create_document(&mut self, a: &Value) -> Result<String, String> {
        let title = str_arg(a, "title").unwrap_or("");
        let body = str_arg(a, "body").unwrap_or("");
        Self::check_text(title, "제목")?;
        Self::check_text(body, "본문")?;
        let cat = int_arg(a, "category_id");
        if let Some(c) = cat {
            if !self.db.categories()?.iter().any(|x| x.id == c) {
                return Err(format!("분류 {c}이(가) 없습니다"));
            }
        }
        let id = self.db.new_doc(cat)?;
        if let Err(x) = self.db.save_doc(id, title, body) {
            let _ = self.db.delete_empty(id);
            return Err(x);
        }
        if let Some(tags) = a.get("tags").and_then(Value::as_array) {
            for t in tags.iter().filter_map(Value::as_str).take(20) {
                let _ = self.db.add_tag(id, t);
            }
        }
        Ok(format!("문서를 만들었습니다: id {id}"))
    }

    fn update_document(&mut self, a: &Value) -> Result<String, String> {
        let id = int_arg(a, "id").ok_or("id가 필요합니다")?;
        let d = self.editable(id)?;
        let title = str_arg(a, "title");
        let body = str_arg(a, "body");
        if title.is_none() && body.is_none() {
            return Err("title 또는 body 중 하나는 있어야 합니다".into());
        }
        if let Some(t) = title {
            Self::check_text(t, "제목")?;
        }
        if let Some(b) = body {
            Self::check_text(b, "본문")?;
            if d.body.contains(OPEN) {
                return Err("암호화한 글이 있는 문서의 본문은 바꿀 수 없습니다(암호문이 지워질 수 있어서). append_to_document로 덧붙이세요".into());
            }
        }
        let changed = self.modify(id, |d| {
            if body.is_some() && d.body.contains(OPEN) {
                return Err("암호화한 글이 있는 문서의 본문은 바꿀 수 없습니다(암호문이 지워질 수 있어서). append_to_document로 덧붙이세요".to_string());
            }
            Ok(Some((title.unwrap_or(&d.title).to_string(), body.unwrap_or(&d.body).to_string())))
        })?;
        Ok(if changed { format!("문서 {id}을(를) 바꿨습니다(이전 내용은 변경 이력에 남았습니다)") } else { "바뀐 내용이 없습니다".into() })
    }

    fn append_to_document(&mut self, a: &Value) -> Result<String, String> {
        let id = int_arg(a, "id").ok_or("id가 필요합니다")?;
        let text = str_arg(a, "text").ok_or("text가 필요합니다")?;
        Self::check_text(text, "덧붙일 글")?;
        self.modify(id, |d| {
            if d.body.len() + text.len() > WRITE_MAX * 4 {
                return Err("문서가 너무 커집니다".to_string());
            }
            let sep = if d.body.is_empty() || d.body.ends_with('\n') { "" } else { "\n" };
            Ok(Some((d.title.clone(), format!("{}{}{}", d.body, sep, text))))
        })?;
        Ok(format!("문서 {id} 끝에 덧붙였습니다"))
    }
}

fn rpc_err(id: Value, code: i32, msg: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }).to_string()
}

// ───────────────────────── stdio ─────────────────────────

/// `--mcp`: 표준 입출력으로 서버를 돌린다. 종료 코드를 돌려준다.
/// (로그는 표준 출력에 섞이면 프로토콜이 깨지므로 diag 로그 파일로만 남긴다.)
pub fn run_stdio() -> i32 {
    let cfg = crate::app::Config::load();
    let db = match Db::open(Path::new(&cfg.db_path)) {
        Ok(d) => d,
        Err(x) => {
            crate::diag::log(&format!("MCP(stdio): 저장소를 열 수 없음: {x}"));
            return 2;
        }
    };
    // 쓰기 허용 옵션은 호출 때마다 설정 파일에서 다시 읽는다(앱에서 바꾸면 바로 반영).
    let mut srv = Server::new(db, Box::new(|| crate::app::Config::load().mcp_write));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(r) = srv.handle(&line) {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    0
}

// ───────────────────────── HTTP ─────────────────────────

/// 실행 중인 HTTP 서버. 버리면 멈춘다.
pub struct Http {
    stop: Arc<AtomicBool>,
    pub port: u16,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Http {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join(); // 곧바로 같은 포트를 다시 열 수 있게 기다린다
        }
    }
}

/// 새 토큰(무작위 32바이트의 16진수).
pub fn new_token() -> Result<String, String> {
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).map_err(|e| format!("난수를 얻을 수 없음: {e}"))?;
    Ok(k.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn start_http(port: u16, token: String, db_path: PathBuf, write: Arc<AtomicBool>) -> Result<Http, String> {
    if token.len() < 16 {
        return Err("토큰이 너무 짧습니다".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("포트 {port}을(를) 열 수 없습니다: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    // 저장소를 못 열면 서버가 켜진 것처럼 보이지 않도록 스레드를 띄우기 전에 열어 오류를 돌려준다.
    let db = Db::open(&db_path).map_err(|x| format!("MCP 저장소를 열 수 없습니다: {x}"))?;
    let thread = std::thread::spawn(move || {
        let mut srv = Server::new(db, Box::new(move || write.load(Ordering::Relaxed)));
        while !st.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, _)) => {
                    let _ = serve(s, &mut srv, &token, port);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
    Ok(Http { stop, port, thread: Some(thread) })
}

fn reply(s: &mut TcpStream, code: u16, reason: &str, extra: &str, body: &str) -> std::io::Result<()> {
    let ct = if body.is_empty() { "" } else { "Content-Type: application/json\r\n" };
    write!(s, "HTTP/1.1 {code} {reason}\r\n{ct}{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
    s.flush()
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn serve(mut s: TcpStream, srv: &mut Server, token: &str, port: u16) -> std::io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let hend = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > 32 * 1024 {
            return reply(&mut s, 431, "Request Header Fields Too Large", "", "");
        }
        let n = s.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..hend]).into_owned();
    let mut lines = head.split("\r\n");
    let method = lines.next().unwrap_or("").split(' ').next().unwrap_or("").to_string();
    let mut h: HashMap<String, String> = HashMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            h.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    // DNS 리바인딩·다른 웹페이지의 접근을 막는다: Host와 Origin은 이 PC 주소여야 한다.
    let host_ok = h.get("host").is_none_or(|v| *v == format!("127.0.0.1:{port}") || *v == format!("localhost:{port}"));
    let origin_ok = h.get("origin").is_none_or(|o| {
        ["http://127.0.0.1", "http://localhost"].iter().any(|p| o == p || o.starts_with(&format!("{p}:")))
    });
    if !host_ok || !origin_ok {
        return reply(&mut s, 403, "Forbidden", "", "");
    }
    let given = h.get("authorization").map(String::as_str).unwrap_or("");
    if !ct_eq(given.as_bytes(), format!("Bearer {token}").as_bytes()) {
        return reply(&mut s, 401, "Unauthorized", "WWW-Authenticate: Bearer\r\n", "");
    }
    if method != "POST" {
        return reply(&mut s, 405, "Method Not Allowed", "Allow: POST\r\n", "");
    }
    let Some(len) = h.get("content-length").and_then(|v| v.parse::<usize>().ok()) else {
        return reply(&mut s, 411, "Length Required", "", "");
    };
    if len > 2 * 1024 * 1024 {
        return reply(&mut s, 413, "Payload Too Large", "", "");
    }
    let mut body: Vec<u8> = buf[hend..].to_vec();
    while body.len() < len {
        let n = s.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    let Ok(text) = String::from_utf8(body) else {
        return reply(&mut s, 400, "Bad Request", "", "");
    };
    match srv.handle(&text) {
        Some(r) => reply(&mut s, 200, "OK", "", &r),
        None => reply(&mut s, 202, "Accepted", "", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(write: bool) -> (Server, PathBuf) {
        let p = std::env::temp_dir().join(format!("akashic-mcp-test-{}.db", fastrand::u64(..)));
        let db = Db::open(&p).unwrap();
        (Server::new(db, Box::new(move || write)), p)
    }
    fn rpc(s: &mut Server, method: &str, params: Value) -> Value {
        let r = s.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string()).unwrap();
        serde_json::from_str(&r).unwrap()
    }
    fn tool(s: &mut Server, name: &str, args: Value) -> (bool, String) {
        let r = rpc(s, "tools/call", json!({ "name": name, "arguments": args }));
        let c = &r["result"];
        (c["isError"].as_bool().unwrap_or(false), c["content"][0]["text"].as_str().unwrap_or("").to_string())
    }
    fn names(s: &mut Server) -> Vec<String> {
        rpc(s, "tools/list", json!({}))["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn handshake_and_notification() {
        let (mut s, _p) = server(false);
        let r = rpc(&mut s, "initialize", json!({ "protocolVersion": "2025-06-18" }));
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "akashic");
        assert!(s.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(rpc(&mut s, "nope", json!({}))["error"].is_object());
    }

    #[test]
    fn write_tools_follow_the_option() {
        let (mut s, _p) = server(false);
        assert!(!names(&mut s).iter().any(|n| n == "create_document"));
        let (err, msg) = tool(&mut s, "create_document", json!({ "title": "x" }));
        assert!(err && msg.contains("쓰기가 꺼져"));
        let (mut w, _p2) = server(true);
        assert!(names(&mut w).iter().any(|n| n == "create_document"));
    }

    #[test]
    fn create_read_search_append_update() {
        let (mut s, _p) = server(true);
        let (err, msg) = tool(&mut s, "create_document", json!({ "title": "제목A", "body": "첫 줄\n특별한단어 있음", "tags": ["메모"] }));
        assert!(!err, "{msg}");
        let id: i64 = msg.rsplit("id ").next().unwrap().trim().parse().unwrap();
        let (_, doc) = tool(&mut s, "get_document", json!({ "id": id }));
        assert!(doc.contains("제목A") && doc.contains("특별한단어") && doc.contains("메모"));
        let (_, found) = tool(&mut s, "search_documents", json!({ "query": "특별한단어" }));
        assert!(found.contains(&format!("[{id}]")), "{found}");
        assert!(tool(&mut s, "append_to_document", json!({ "id": id, "text": "덧붙임" })).0 == false);
        assert!(tool(&mut s, "update_document", json!({ "id": id, "title": "제목B" })).0 == false);
        let (_, doc) = tool(&mut s, "get_document", json!({ "id": id }));
        assert!(doc.contains("제목B") && doc.contains("덧붙임") && doc.contains("특별한단어"));
        let (_, v) = tool(&mut s, "list_versions", json!({ "id": id }));
        assert!(v.contains("버전"), "{v}");
    }

    #[test]
    fn encrypted_text_is_masked_and_protected() {
        let (mut s, _p) = server(true);
        let id = s.db.new_doc(None).unwrap();
        s.db.save_doc(id, "비밀", &format!("공개 {OPEN}enc:abcd:0011{CLOSE} 끝")).unwrap();
        let (_, doc) = tool(&mut s, "get_document", json!({ "id": id }));
        assert!(doc.contains(MASK) && !doc.contains("enc:abcd") && doc.contains("공개") && doc.contains("끝"));
        let (_, found) = tool(&mut s, "search_documents", json!({ "query": "공개" }));
        assert!(!found.contains("enc:"));
        // 본문 교체는 거부, 제목 변경과 덧붙이기는 허용, 표식 쓰기는 거부
        assert!(tool(&mut s, "update_document", json!({ "id": id, "body": "덮어쓰기" })).0);
        assert!(!tool(&mut s, "update_document", json!({ "id": id, "title": "새 제목" })).0);
        assert!(!tool(&mut s, "append_to_document", json!({ "id": id, "text": "추가" })).0);
        assert!(tool(&mut s, "append_to_document", json!({ "id": id, "text": format!("{OPEN}x{CLOSE}") })).0);
        let raw = s.db.get_doc(id).unwrap().unwrap().body;
        assert!(raw.contains("enc:abcd:0011") && raw.ends_with("추가"));
    }

    #[test]
    fn mask_unclosed_hides_rest() {
        assert_eq!(mask(&format!("a{OPEN}secret")).0, format!("a{MASK}"));
        assert_eq!(mask("평문").1, 0);
    }

    fn http(port: u16, head_extra: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{head_extra}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut r = String::new();
        let _ = s.read_to_string(&mut r);
        r
    }

    #[test]
    fn http_auth_and_roundtrip() {
        let p = std::env::temp_dir().join(format!("akashic-mcp-http-{}.db", fastrand::u64(..)));
        let port = 20000 + fastrand::u16(..20000);
        let tok = new_token().unwrap();
        let h = start_http(port, tok.clone(), p, Arc::new(AtomicBool::new(false))).unwrap();
        let req = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
        assert!(http(port, "", req).starts_with("HTTP/1.1 401"));
        assert!(http(port, "Authorization: Bearer wrong\r\n", req).starts_with("HTTP/1.1 401"));
        assert!(http(port, &format!("Authorization: Bearer {tok}\r\nOrigin: https://evil.example\r\n"), req).starts_with("HTTP/1.1 403"));
        let ok = http(port, &format!("Authorization: Bearer {tok}\r\n"), req);
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("list_documents") && !ok.contains("create_document"), "{ok}");
        let n = http(port, &format!("Authorization: Bearer {tok}\r\n"), r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert!(n.starts_with("HTTP/1.1 202"));
        drop(h);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn max_chars_truncates() {
        let (mut s, _p) = server(true);
        let id = s.db.new_doc(None).unwrap();
        s.db.save_doc(id, "긴 글", &"가".repeat(500)).unwrap();
        let (_, doc) = tool(&mut s, "get_document", json!({ "id": id, "max_chars": 100 }));
        assert!(doc.contains("전체 500자") && doc.matches('가').count() == 100);
    }
}
