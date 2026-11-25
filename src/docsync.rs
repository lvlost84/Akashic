//! 문서 단위 동기화(서버 로직 없음). 서버는 파일 올리기·읽기·목록·삭제와 "없을 때만 쓰기"만 한다.
//!
//! 구조: 각 기기는 자기 로그 파일(`log-<기기>-<순번>.rec`)만 쓰고(덮어쓰지 않음) 다른 기기의 로그를 읽어 적용한다.
//! 모든 파일은 `vault`의 키로 종단간 암호화(파일 이름에 묶임). 병합은 전부 이 모듈(각 PC)이 한다.
//!
//! 버전 비교는 버전 벡터(vv: 기기→카운터). 한쪽이 다른 쪽을 포함하면 최신 쪽을 채택하고, 서로 모르는 변경이면
//! (동시 편집) 필드별 3-way 병합, 본문은 줄 단위 3-way(`diffy`). 겹치면 큰 쪽(rank)을 본문으로 하고 진 쪽은 충돌 사본으로 남긴다.
//! 삭제 대 수정은 수정이 이긴다. 같은 레코드를 여러 번 적용해도 결과가 같다(멱등).
use crate::db::{Db, POS_GAP};
use crate::vault::{open_named, seal_named, PutCond, Remote, RemoteError, VaultKey};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Write};

pub type Res<T> = Result<T, String>;
pub type Data = BTreeMap<String, Value>;
pub type Vv = BTreeMap<String, u64>;

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

#[cfg(test)]
thread_local! {
    pub static TRACE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(test)]
fn trace(s: String) {
    TRACE.with(|t| t.borrow_mut().push(s));
}
#[cfg(not(test))]
fn trace(_: String) {}

pub const CKPT_EVERY: u64 = 50;
pub const RETAIN_DAYS: i64 = 90;
pub const CONFLICT_TAG: &str = "충돌";
/// 이 앱이 만드는 로그·체크포인트 형식 번호.
const FORMAT: u8 = 1;

// ───────────────────────── 레코드 ─────────────────────────

pub const K_DOC: &str = "d";
pub const K_CAT: &str = "c";
pub const K_DEL_DOC: &str = "xd";
pub const K_DEL_CAT: &str = "xc";

/// 동기화 단위 하나(문서·분류·삭제 표식). `data`는 필드 맵: 문서 {title, body, cat, tags, pinned, trashed}, 분류 {name, parent}.
/// `parent`는 이 레코드가 나온 직전 상태(동시 편집을 만난 쪽이 공통 조상으로 쓴다).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rec {
    pub k: String,
    pub uid: String,
    pub dev: String,
    #[serde(default)]
    pub dn: String,
    pub vv: Vv,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub data: Data,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Data>,
    /// (base 전용) 삭제를 만났지만 유지하기로 했으니 내용이 같아도 반드시 다시 올린다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Batch {
    v: u8,
    dev: String,
    seq: u64,
    ts: i64,
    recs: Vec<Rec>,
    /// 체크포인트 전용: 각 기기 로그를 이 순번까지 반영했다.
    #[serde(default)]
    upto: BTreeMap<String, u64>,
}

#[derive(Serialize, Deserialize, Default)]
struct DevFile {
    dev: String,
    name: String,
    seen: i64,
    seq: u64,
    cursors: BTreeMap<String, u64>,
}

// ───────────────────────── 버전 벡터 ─────────────────────────

fn vv_le(a: &Vv, b: &Vv) -> bool {
    a.iter().all(|(k, v)| b.get(k).copied().unwrap_or(0) >= *v)
}
fn vv_join(a: &Vv, b: &Vv) -> Vv {
    let mut o = a.clone();
    for (k, v) in b {
        let x = o.entry(k.clone()).or_insert(0);
        *x = (*x).max(*v);
    }
    o
}
fn vv_sum(a: &Vv) -> u64 {
    a.values().sum()
}

// ───────────────────────── 병합(순수 함수) ─────────────────────────

fn sv<'a>(d: &'a Data, k: &str) -> &'a str {
    d.get(k).and_then(|v| v.as_str()).unwrap_or("")
}
fn tags_of(v: Option<&Value>) -> BTreeSet<String> {
    v.and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// 필드별 3-way 병합. 한쪽만 바꾼 필드는 그 값, 양쪽이 다르게 바꿨으면 태그는 집합 병합, 본문은 줄 단위 3-way,
/// 나머지는 `a_wins` 쪽. 돌려주는 bool은 본문이 겹쳐 자동 병합에 실패했는지(그러면 진 쪽 본문이 충돌 사본 대상).
pub fn merge_data(anc: &Data, a: &Data, b: &Data, a_wins: bool) -> (Data, bool) {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut out = Data::new();
    let mut body_conflict = false;
    for k in keys {
        let (av, bv, cv) = (a.get(k).cloned().unwrap_or(Value::Null), b.get(k).cloned().unwrap_or(Value::Null), anc.get(k).cloned().unwrap_or(Value::Null));
        let v = if av == bv {
            av
        } else if av == cv {
            bv
        } else if bv == cv {
            av
        } else if k == "tags" {
            let (ta, tb, tc) = (tags_of(Some(&av)), tags_of(Some(&bv)), tags_of(Some(&cv)));
            let all: BTreeSet<&String> = ta.iter().chain(tb.iter()).collect();
            Value::Array(
                all.into_iter()
                    .filter(|t| (ta.contains(*t) && tb.contains(*t)) || ((ta.contains(*t) || tb.contains(*t)) && !tc.contains(*t)))
                    .map(|t| Value::String(t.clone()))
                    .collect(),
            )
        } else if k == "body" {
            match diffy::merge(cv.as_str().unwrap_or(""), av.as_str().unwrap_or(""), bv.as_str().unwrap_or("")) {
                Ok(s) => Value::String(s),
                Err(_) => {
                    body_conflict = true;
                    if a_wins { av } else { bv }
                }
            }
        } else if a_wins {
            av
        } else {
            bv
        };
        if !v.is_null() {
            out.insert(k.clone(), v);
        }
    }
    (out, body_conflict)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 충돌 사본: 진 쪽 본문을 별도 문서로. uid·제목·시각은 내용에서만 정해 두 기기가 같은 사본을 만든다.
fn conflict_copy(uid: &str, loser: &Data, winner: &Data, dev: &str, dn: &str, updated: i64) -> Rec {
    let mut h = Sha256::new();
    h.update(uid.as_bytes());
    h.update(dev.as_bytes());
    h.update(sv(loser, "body").as_bytes());
    let cuid = hex(&h.finalize()[..16]);
    let when = chrono::DateTime::from_timestamp(updated, 0).map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
    let who = if dn.is_empty() { dev } else { dn };
    let mut tags = tags_of(winner.get("tags"));
    tags.insert(CONFLICT_TAG.to_string());
    let mut data = Data::new();
    data.insert("title".into(), Value::String(format!("{} (충돌 · {who} · {when})", sv(loser, "title"))));
    data.insert("body".into(), loser.get("body").cloned().unwrap_or_else(|| Value::String(String::new())));
    if let Some(c) = winner.get("cat").filter(|c| !c.is_null()) {
        data.insert("cat".into(), c.clone());
    }
    data.insert("tags".into(), Value::Array(tags.into_iter().map(Value::String).collect()));
    data.insert("pinned".into(), Value::Bool(false));
    let mut vv = Vv::new();
    vv.insert(dev.to_string(), 1);
    Rec { k: K_DOC.into(), uid: cuid, dev: dev.into(), dn: dn.into(), vv, created: updated, updated, data, parent: None, force: false }
}

/// 한 레코드를 로컬 상태에 적용한 결과.
struct Outcome {
    /// 로컬에 새로 쓸 필드(None이면 그대로).
    data: Option<Data>,
    created: i64,
    updated: i64,
    /// 새 base(마지막으로 동기화된 상태).
    base: Rec,
    dirty: bool,
    copy: Option<Rec>,
}

struct Local {
    id: i64,
    dirty: bool,
    base: Option<Rec>,
    cur: Data,
    created: i64,
    updated: i64,
}

fn strip(r: &Rec) -> Rec {
    Rec { parent: None, ..r.clone() }
}

/// 원격 레코드 `r`(문서/분류)를 로컬 `local`에 적용한다. `None`이면 무시(이미 반영됨).
fn resolve(local: Option<&Local>, r: &Rec, me: &str, my_dn: &str) -> Option<Outcome> {
    let base_vv = local.and_then(|l| l.base.as_ref()).map(|b| b.vv.clone()).unwrap_or_default();
    if vv_le(&r.vv, &base_vv) {
        return None;
    }
    let Some(l) = local else {
        return Some(Outcome { data: Some(r.data.clone()), created: r.created, updated: r.updated, base: strip(r), dirty: false, copy: None });
    };
    let dominates = vv_le(&base_vv, &r.vv);
    if !l.dirty && dominates {
        return Some(Outcome { data: Some(r.data.clone()), created: r.created, updated: r.updated, base: strip(r), dirty: false, copy: None });
    }
    let joined = vv_join(&base_vv, &r.vv);
    let new_base = Rec { vv: joined.clone(), ..strip(r) };
    let created = if l.created > 0 { l.created.min(r.created.max(1)) } else { r.created };
    let updated = l.updated.max(r.updated);
    if l.cur == r.data {
        return Some(Outcome { data: Some(r.data.clone()), created, updated, base: new_base, dirty: false, copy: None });
    }
    let anc = if dominates { l.base.as_ref().map(|b| b.data.clone()).or_else(|| r.parent.clone()).unwrap_or_default() } else { r.parent.clone().unwrap_or_default() };
    let (ours_dev, ours_dn) = if l.dirty { (me.to_string(), my_dn.to_string()) } else { l.base.as_ref().map(|b| (b.dev.clone(), b.dn.clone())).unwrap_or_default() };
    let ours_rank = (vv_sum(&base_vv) + l.dirty as u64, ours_dev.clone());
    let theirs_rank = (vv_sum(&r.vv), r.dev.clone());
    let ours_wins = ours_rank > theirs_rank;
    let (m, conflict) = merge_data(&anc, &l.cur, &r.data, ours_wins);
    let copy = conflict.then(|| {
        if ours_wins {
            conflict_copy(&r.uid, &r.data, &m, &r.dev, &r.dn, r.updated)
        } else {
            conflict_copy(&r.uid, &l.cur, &m, &ours_dev, &ours_dn, l.updated)
        }
    });
    if m == r.data {
        Some(Outcome { data: Some(m), created, updated, base: new_base, dirty: false, copy })
    } else if !l.dirty && m == l.cur {
        // 내 쪽이 상대 변경을 이미 포함: 그대로 두고 base만 합친다.
        let b = l.base.as_ref().map(|b| Rec { vv: joined, ..b.clone() }).unwrap_or(new_base);
        Some(Outcome { data: None, created, updated, base: b, dirty: false, copy })
    } else {
        Some(Outcome { data: Some(m), created, updated, base: new_base, dirty: true, copy })
    }
}

// ───────────────────────── 파일 이름·묶음 코덱 ─────────────────────────

fn log_name(dev: &str, seq: u64) -> String {
    format!("log-{dev}-{seq:08}.rec")
}
/// 체크포인트 이름에 "각 기기 로그를 어디까지 담았는지"를 넣는다(받지 않고도 서로 비교·고를 수 있게).
fn ckpt_name(dev: &str, seq: u64, upto: &BTreeMap<String, u64>) -> String {
    let u: String = upto.iter().map(|(d, n)| format!("-{d}.{n}")).collect();
    format!("ckpt-{dev}-{seq:08}{u}.snap")
}
fn dev_name(dev: &str) -> String {
    format!("dev-{dev}.json")
}
fn img_name(hash: &str) -> String {
    format!("img-{hash}.enc")
}
fn file_name(hash: &str) -> String {
    format!("file-{hash}.enc")
}

enum FileKind {
    Log(String, u64),
    Ckpt(BTreeMap<String, u64>),
    Dev(String),
}

fn parse_name(n: &str) -> Option<FileKind> {
    let split = |s: &str| -> Option<(String, u64)> {
        let (d, q) = s.rsplit_once('-')?;
        Some((d.to_string(), q.parse().ok()?))
    };
    if let Some(r) = n.strip_prefix("log-").and_then(|r| r.strip_suffix(".rec")) {
        return split(r).map(|(d, q)| FileKind::Log(d, q));
    }
    if let Some(r) = n.strip_prefix("ckpt-").and_then(|r| r.strip_suffix(".snap")) {
        let mut parts = r.split('-').skip(2);
        let mut upto = BTreeMap::new();
        for p in parts.by_ref() {
            let (d, q) = p.split_once('.')?;
            upto.insert(d.to_string(), q.parse().ok()?);
        }
        return Some(FileKind::Ckpt(upto));
    }
    n.strip_prefix("dev-").and_then(|r| r.strip_suffix(".json")).map(|d| FileKind::Dev(d.to_string()))
}

fn encode<T: Serialize>(key: &VaultKey, name: &str, v: &T) -> Res<Vec<u8>> {
    let json = serde_json::to_vec(v).map_err(e)?;
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&json).map_err(e)?;
    let packed = z.finish().map_err(e)?;
    seal_named(key, name, &packed).map_err(e)
}

fn decode<T: for<'de> Deserialize<'de>>(key: &VaultKey, name: &str, data: &[u8]) -> Res<T> {
    let packed = open_named(key, name, data).ok_or_else(|| format!("{name}: 복호화 실패(키가 다르거나 파일이 변조됨)"))?;
    let mut json = Vec::new();
    flate2::read::DeflateDecoder::new(&packed[..]).read_to_end(&mut json).map_err(|x| format!("{name}: 압축 해제 실패: {x}"))?;
    serde_json::from_slice(&json).map_err(|x| format!("{name}: 형식 오류: {x}"))
}

fn rerr(x: RemoteError) -> String {
    match x {
        RemoteError::Conflict => "서버 충돌(412)".to_string(),
        RemoteError::Other(s) => s,
    }
}

// ───────────────────────── 로컬 DB 도우미 ─────────────────────────

fn mget(db: &Db, k: &str) -> Option<String> {
    db.conn.query_row("SELECT value FROM meta WHERE key=?", [k], |r| r.get(0)).optional().ok().flatten()
}
fn mset(db: &Db, k: &str, v: &str) -> Res<()> {
    db.conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)", params![k, v]).map_err(e)?;
    Ok(())
}
fn mget_u64(db: &Db, k: &str) -> u64 {
    mget(db, k).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// 쓰기 트랜잭션. `applying`을 켜 두면(원격 변경 적용) 변경 표시(dirty)가 서지 않는다.
fn tx<T>(db: &Db, applying: bool, f: impl FnOnce() -> Res<T>) -> Res<T> {
    db.conn.execute_batch("BEGIN IMMEDIATE").map_err(e)?;
    if applying {
        db.conn.execute_batch("UPDATE meta SET value='1' WHERE key='applying'").map_err(e)?;
    }
    let r = f();
    let fin = match &r {
        Ok(_) => db.conn.execute_batch("UPDATE meta SET value='0' WHERE key='applying'; COMMIT"),
        Err(_) => db.conn.execute_batch("ROLLBACK"),
    };
    match (r, fin) {
        (Ok(v), Ok(())) => Ok(v),
        (Err(x), _) => Err(x),
        (Ok(_), Err(x)) => Err(e(x)),
    }
}

pub fn device_id(db: &Db) -> Option<String> {
    mget(db, "ds_dev")
}

pub fn is_connected(db: &Db, key: &VaultKey) -> bool {
    mget(db, "ds_store").as_deref() == Some(key.vault_id().as_str())
}

fn new_dev_id() -> String {
    let mut b = [0u8; 6];
    getrandom::fill(&mut b).expect("난수");
    hex(&b)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Connect {
    /// 서버가 비어 있는 새 저장소: 이 PC의 전 문서가 처음 올라간다.
    New,
    /// 서버 문서를 이 PC 문서에 추가(중복 가능).
    Merge,
    /// 이 PC의 문서를 모두 비우고 서버 내용으로 채운다(호출한 쪽이 DB 백업을 먼저 한다).
    Replace,
}

/// 서버에 문서 단위 동기화 파일이 이미 있는지(첫 연결에서 사용자 선택이 필요한지).
pub fn remote_has_data(remote: &dyn Remote) -> Res<bool> {
    Ok(remote.list().map_err(rerr)?.iter().any(|n| parse_name(n).is_some_and(|k| !matches!(k, FileKind::Dev(_)))))
}

/// 이 DB를 저장소에 연결한다(첫 연결). 새 기기 ID를 받고 읽은 위치·동기화 기준을 모두 초기화한다.
pub fn connect(db: &Db, key: &VaultKey, mode: Connect) -> Res<()> {
    tx(db, true, || {
        if mode == Connect::Replace {
            db.conn
                .execute_batch("DELETE FROM docs; DELETE FROM categories; DELETE FROM tags; DELETE FROM doc_tags;")
                .map_err(e)?;
        }
        db.conn
            .execute_batch(
                "DELETE FROM tombstones; UPDATE docs SET base_json=NULL, dirty=1; UPDATE categories SET base_json=NULL, dirty=1;
                 DELETE FROM meta WHERE key LIKE 'ds_cur:%' OR key IN ('ds_seq','ds_ckpt_seq','ds_img_pending');",
            )
            .map_err(e)?;
        mset(db, "ds_dev", &new_dev_id())?;
        mset(db, "ds_store", &key.vault_id())
    })
}

// ───────────────────────── 로컬 상태 읽기 ─────────────────────────

fn tags_json(db: &Db, doc_id: i64) -> Res<Value> {
    let mut st = db
        .conn
        .prepare_cached("SELECT t.name FROM doc_tags dt JOIN tags t ON t.id=dt.tag_id WHERE dt.doc_id=? ORDER BY t.name")
        .map_err(e)?;
    let v: Vec<Value> = st.query_map([doc_id], |r| r.get::<_, String>(0)).map_err(e)?.flatten().map(Value::String).collect();
    Ok(Value::Array(v))
}

fn doc_data(r: &rusqlite::Row, db: &Db, id: i64) -> Res<Data> {
    let mut d = Data::new();
    d.insert("title".into(), Value::String(r.get::<_, String>(1).map_err(e)?));
    d.insert("body".into(), Value::String(r.get::<_, String>(2).map_err(e)?));
    if let Some(c) = r.get::<_, Option<String>>(3).map_err(e)? {
        d.insert("cat".into(), Value::String(c));
    }
    d.insert("tags".into(), tags_json(db, id)?);
    d.insert("pinned".into(), Value::Bool(r.get::<_, i64>(4).map_err(e)? != 0));
    if let Some(t) = r.get::<_, Option<i64>>(5).map_err(e)? {
        d.insert("trashed".into(), Value::from(t));
    }
    Ok(d)
}

const DOC_SEL: &str = "SELECT d.id,d.title,d.body,c.uid,d.pinned,d.trashed_at,d.created,d.updated,d.dirty,d.base_json,d.uid
  FROM docs d LEFT JOIN categories c ON c.id=d.category_id";
const CAT_SEL: &str = "SELECT c.id,c.name,p.uid,0,0,NULL,0,0,c.dirty,c.base_json,c.uid FROM categories c LEFT JOIN categories p ON p.id=c.parent_id";

fn local_from_row(db: &Db, r: &rusqlite::Row, is_doc: bool) -> Res<(String, Local)> {
    let id: i64 = r.get(0).map_err(e)?;
    let cur = if is_doc {
        doc_data(r, db, id)?
    } else {
        let mut d = Data::new();
        d.insert("name".into(), Value::String(r.get::<_, String>(1).map_err(e)?));
        if let Some(p) = r.get::<_, Option<String>>(2).map_err(e)? {
            d.insert("parent".into(), Value::String(p));
        }
        d
    };
    let base: Option<Rec> = r.get::<_, Option<String>>(9).map_err(e)?.and_then(|s| serde_json::from_str(&s).ok());
    let uid: String = r.get(10).map_err(e)?;
    Ok((uid, Local { id, dirty: r.get::<_, i64>(8).map_err(e)? != 0 && base.as_ref().is_none_or(|b| b.data != cur || b.force), base, cur, created: r.get(6).map_err(e)?, updated: r.get(7).map_err(e)? }))
}

fn load(db: &Db, is_doc: bool, uid: &str) -> Res<Option<Local>> {
    let sql = format!("{} WHERE {}.uid=?", if is_doc { DOC_SEL } else { CAT_SEL }, if is_doc { "d" } else { "c" });
    let mut st = db.conn.prepare_cached(&sql).map_err(e)?;
    let mut rows = st.query([uid]).map_err(e)?;
    match rows.next().map_err(e)? {
        Some(r) => Ok(Some(local_from_row(db, r, is_doc)?.1)),
        None => Ok(None),
    }
}

fn load_dirty(db: &Db, is_doc: bool) -> Res<Vec<(String, Local)>> {
    let sql = format!("{} WHERE {}.dirty=1 ORDER BY {}.id", if is_doc { DOC_SEL } else { CAT_SEL }, if is_doc { "d" } else { "c" }, if is_doc { "d" } else { "c" });
    let mut st = db.conn.prepare(&sql).map_err(e)?;
    let mut rows = st.query([]).map_err(e)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().map_err(e)? {
        out.push(local_from_row(db, r, is_doc)?);
    }
    Ok(out)
}

fn load_all_bases(db: &Db, table: &str) -> Res<Vec<Rec>> {
    let mut st = db.conn.prepare(&format!("SELECT base_json FROM {table} WHERE base_json IS NOT NULL")).map_err(e)?;
    let v = st.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.flatten().filter_map(|s| serde_json::from_str(&s).ok()).collect();
    Ok(v)
}

// ───────────────────────── 로컬 상태 쓰기 ─────────────────────────

fn base_str(b: &Rec) -> String {
    serde_json::to_string(&strip(b)).unwrap_or_default()
}

fn write_doc(db: &Db, id: Option<i64>, uid: &str, d: &Data, created: i64, updated: i64, dirty: bool, base: Option<&Rec>) -> Res<i64> {
    let pinned = d.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false) as i64;
    let trashed = d.get("trashed").and_then(|v| v.as_i64());
    let cat = d.get("cat").and_then(|v| v.as_str());
    let b = base.map(base_str);
    let did = match id {
        Some(i) => {
            db.conn
                .execute(
                    "UPDATE docs SET title=?1, body=?2, category_id=(SELECT id FROM categories WHERE uid=?3), pinned=?4, trashed_at=?5, created=?6, updated=?7, dirty=?8, base_json=?9 WHERE id=?10",
                    params![sv(d, "title"), sv(d, "body"), cat, pinned, trashed, created, updated, dirty as i64, b, i],
                )
                .map_err(e)?;
            i
        }
        None => {
            db.conn
                .execute(
                    "INSERT INTO docs(uid,title,body,category_id,created,updated,pinned,trashed_at,pos,dirty,base_json)
                     VALUES(?1,?2,?3,(SELECT id FROM categories WHERE uid=?4),?5,?6,?7,?8,(SELECT COALESCE(MIN(pos),0)-?9 FROM docs),?10,?11)",
                    params![uid, sv(d, "title"), sv(d, "body"), cat, created, updated, pinned, trashed, POS_GAP, dirty as i64, b],
                )
                .map_err(e)?;
            db.conn.last_insert_rowid()
        }
    };
    let want = tags_of(d.get("tags"));
    if id.is_some() {
        let have = tags_of(Some(&tags_json(db, did)?));
        if have == want {
            return Ok(did);
        }
        db.conn.execute("DELETE FROM doc_tags WHERE doc_id=?", [did]).map_err(e)?;
    }
    for t in &want {
        db.conn.execute("INSERT OR IGNORE INTO tags(name) VALUES(?)", [t]).map_err(e)?;
        db.conn.execute("INSERT OR IGNORE INTO doc_tags(doc_id,tag_id) SELECT ?1,id FROM tags WHERE name=?2", params![did, t]).map_err(e)?;
    }
    Ok(did)
}

/// 분류 행을 쓴다(부모는 모든 분류가 들어온 뒤 `fix_parents`로 연결).
fn write_cat(db: &Db, id: Option<i64>, uid: &str, d: &Data, dirty: bool, base: Option<&Rec>) -> Res<i64> {
    let b = base.map(base_str);
    match id {
        Some(i) => {
            db.conn.execute("UPDATE categories SET name=?1, dirty=?2, base_json=?3 WHERE id=?4", params![sv(d, "name"), dirty as i64, b, i]).map_err(e)?;
            Ok(i)
        }
        None => {
            db.conn
                .execute("INSERT INTO categories(uid,name,dirty,base_json) VALUES(?1,?2,?3,?4)", params![uid, sv(d, "name"), dirty as i64, b])
                .map_err(e)?;
            Ok(db.conn.last_insert_rowid())
        }
    }
}

// ───────────────────────── 적용 ─────────────────────────

#[derive(Default, Debug, Clone)]
pub struct Report {
    pub pulled_batches: usize,
    /// 원격 변경으로 로컬이 바뀐 레코드 수.
    pub applied: usize,
    /// 동시 편집을 병합한 수.
    pub merged: usize,
    /// 만들어진 충돌 사본 제목.
    pub conflicts: Vec<String>,
    pub pushed: usize,
    pub images_up: usize,
    pub images_down: usize,
    /// 원격 변경으로 바뀌거나 지워진 로컬 문서 id(열려 있는 문서 갱신용).
    pub changed_docs: Vec<i64>,
    pub deleted_docs: Vec<i64>,
    pub ckpt_written: bool,
    pub pruned: usize,
    /// 여러 번 읽지 못해 건너뛴 다른 기기의 로그(이름과 이유).
    pub skipped_logs: Vec<String>,
}

struct Ctx<'a> {
    me: &'a str,
    dn: &'a str,
    rep: &'a mut Report,
    cat_parent: HashMap<String, Option<String>>,
    copies: Vec<Rec>,
}

fn tomb_get(db: &Db, uid: &str) -> Option<Rec> {
    db.conn
        .query_row("SELECT base_json FROM tombstones WHERE uid=?", [uid], |r| r.get::<_, Option<String>>(0))
        .optional()
        .ok()
        .flatten()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
}
fn tomb_set(db: &Db, r: &Rec, dirty: bool) -> Res<()> {
    // 삭제 표식은 본 삭제들의 버전을 합쳐 둔다(같은 문서의 옛 삭제가 다시 와도 더 새로운 삭제를 덮어쓰지 않게).
    let merged = match tomb_get(db, &r.uid) {
        Some(old) if old.k == r.k => Rec { vv: vv_join(&old.vv, &r.vv), ..r.clone() },
        _ => r.clone(),
    };
    let r = &merged;
    db.conn
        .execute(
            "INSERT OR REPLACE INTO tombstones(uid,kind,base_json,dirty) VALUES(?,?,?,?)",
            params![r.uid, if r.k == K_DEL_DOC { "d" } else { "c" }, base_str(r), dirty as i64],
        )
        .map_err(e)?;
    Ok(())
}

fn apply_upsert(db: &Db, cx: &mut Ctx, r: &Rec) -> Res<()> {
    let is_doc = r.k == K_DOC;
    if let Some(t) = tomb_get(db, &r.uid) {
        if vv_le(&r.vv, &t.vv) {
            return Ok(()); // 삭제보다 오래된 레코드
        }
        db.conn.execute("DELETE FROM tombstones WHERE uid=?", [&r.uid]).map_err(e)?; // 수정이 삭제를 이긴다(되살림)
    }
    let local = load(db, is_doc, &r.uid)?;
    trace(format!("[{}] UPS {} vv={:?} from {} local={:?}", &cx.me[..4], &r.uid[..6], r.vv, &r.dev[..4], local.as_ref().map(|l| (l.dirty, l.base.as_ref().map(|b| b.vv.clone())))));
    let Some(o) = resolve(local.as_ref(), r, cx.me, cx.dn) else { return Ok(()) };
    cx.rep.applied += 1;
    if local.as_ref().is_some_and(|l| l.dirty || l.base.as_ref().is_some_and(|b| !vv_le(&r.vv, &b.vv) && !vv_le(&b.vv, &r.vv))) {
        cx.rep.merged += 1;
    }
    let id = local.as_ref().map(|l| l.id);
    let did = match &o.data {
        Some(d) if is_doc => write_doc(db, id, &r.uid, d, o.created, o.updated, o.dirty, Some(&o.base))?,
        Some(d) => {
            cx.cat_parent.insert(r.uid.clone(), d.get("parent").and_then(|v| v.as_str()).map(String::from));
            write_cat(db, id, &r.uid, d, o.dirty, Some(&o.base))?
        }
        None => {
            let cur = &local.as_ref().unwrap().cur;
            if is_doc {
                write_doc(db, id, &r.uid, cur, o.created, o.updated, o.dirty, Some(&o.base))?
            } else {
                write_cat(db, id, &r.uid, cur, o.dirty, Some(&o.base))?
            }
        }
    };
    if is_doc {
        cx.rep.changed_docs.push(did);
    }
    if let Some(c) = o.copy {
        cx.copies.push(c);
    }
    Ok(())
}

fn apply_delete(db: &Db, cx: &mut Ctx, r: &Rec) -> Res<()> {
    let is_doc = r.k == K_DEL_DOC;
    trace(format!("[{}] DEL {} vv={:?} from {}", &cx.me[..4], &r.uid[..6], r.vv, &r.dev[..4]));
    if let Some(t) = tomb_get(db, &r.uid) {
        if vv_le(&r.vv, &t.vv) {
            return Ok(());
        }
    }
    match load(db, is_doc, &r.uid)? {
        None => tomb_set(db, r, false),
        Some(l) => {
            let base_vv = l.base.as_ref().map(|b| b.vv.clone()).unwrap_or_default();
            if vv_le(&r.vv, &base_vv) {
                return Ok(()); // 이 삭제는 이미 내 기준에 반영(유지하기로 한 뒤)된 것
            }
            // 깨끗하고 삭제가 내 기준을 포함할 때만 지운다. 아니면 수정이 이긴다(유지).
            if !l.dirty && l.base.is_some() && vv_le(&base_vv, &r.vv) {
                db.conn.execute(if is_doc { "DELETE FROM docs WHERE id=?" } else { "DELETE FROM categories WHERE id=?" }, [l.id]).map_err(e)?;
                cx.rep.applied += 1;
                if is_doc {
                    cx.rep.deleted_docs.push(l.id);
                }
                tomb_set(db, r, false)
            } else {
                // 수정이 이긴다: 삭제를 본 것으로 기준에 합치고, 내용이 같아도 다시 올려 다른 기기가 되살리게 한다.
                if let Some(b) = &l.base {
                    let nb = Rec { vv: vv_join(&b.vv, &r.vv), force: true, ..b.clone() };
                    let sql = if is_doc { "UPDATE docs SET dirty=1, base_json=?1 WHERE id=?2" } else { "UPDATE categories SET dirty=1, base_json=?1 WHERE id=?2" };
                    db.conn.execute(sql, params![serde_json::to_string(&nb).map_err(e)?, l.id]).map_err(e)?;
                }
                Ok(())
            }
        }
    }
}

/// 모든 분류의 부모를 uid로 연결하고(순서 무관), 순환이 생겼으면 각 순환에서 uid가 가장 작은 분류를 최상위로 올린다.
fn fix_parents(db: &Db, cat_parent: &HashMap<String, Option<String>>) -> Res<()> {
    for (uid, p) in cat_parent {
        db.conn
            .execute("UPDATE categories SET parent_id=(SELECT id FROM categories WHERE uid=?1) WHERE uid=?2", params![p, uid])
            .map_err(e)?;
    }
    // 깨끗한 분류의 부모는 동기화된 상태(base)를 따른다(분류가 지워졌다가 되살아나도 연결이 돌아오게).
    db.conn
        .execute_batch(
            "UPDATE categories SET parent_id=(SELECT p.id FROM categories p WHERE p.uid=json_extract(categories.base_json,'$.data.parent'))
             WHERE dirty=0 AND base_json IS NOT NULL
               AND parent_id IS NOT (SELECT p.id FROM categories p WHERE p.uid=json_extract(categories.base_json,'$.data.parent'))",
        )
        .map_err(e)?;
    let rows: Vec<(String, Option<String>)> = {
        let mut st = db.conn.prepare("SELECT c.uid, p.uid FROM categories c LEFT JOIN categories p ON p.id=c.parent_id").map_err(e)?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.flatten().collect();
        v
    };
    let parent: HashMap<&str, Option<&str>> = rows.iter().map(|(c, p)| (c.as_str(), p.as_deref())).collect();
    let mut done: HashSet<&str> = HashSet::new();
    for (start, _) in &rows {
        let mut path: Vec<&str> = Vec::new();
        let mut cur = Some(start.as_str());
        while let Some(c) = cur {
            if done.contains(c) {
                break;
            }
            if let Some(i) = path.iter().position(|x| *x == c) {
                let cyc = &path[i..];
                let cut = cyc.iter().min().unwrap();
                db.conn.execute("UPDATE categories SET parent_id=NULL, dirty=1 WHERE uid=?", [cut]).map_err(e)?;
                break;
            }
            path.push(c);
            cur = parent.get(c).copied().flatten();
        }
        done.extend(path);
    }
    Ok(())
}

fn apply_records(db: &Db, recs: &[Rec], me: &str, dn: &str, rep: &mut Report) -> Res<()> {
    if recs.is_empty() {
        return Ok(());
    }
    let mut order: Vec<&Rec> = recs.iter().collect();
    // 분류 먼저(부모·삭제), 문서는 나중. 같은 종류 안에서는 오래된 것부터(불필요한 병합을 줄임).
    order.sort_by(|a, b| {
        let g = |r: &Rec| (r.k == K_DOC || r.k == K_DEL_DOC) as u8;
        (g(a), vv_sum(&a.vv), &a.dev, &a.k, &a.uid).cmp(&(g(b), vv_sum(&b.vv), &b.dev, &b.k, &b.uid))
    });
    tx(db, true, || {
        let mut cx = Ctx { me, dn, rep, cat_parent: HashMap::new(), copies: Vec::new() };
        let mut cats_done = false;
        for r in order {
            if !cats_done && (r.k == K_DOC || r.k == K_DEL_DOC) {
                fix_parents(db, &cx.cat_parent)?;
                cx.cat_parent.clear();
                cats_done = true;
            }
            match r.k.as_str() {
                K_DOC | K_CAT => apply_upsert(db, &mut cx, r)?,
                K_DEL_DOC | K_DEL_CAT => apply_delete(db, &mut cx, r)?,
                _ => {}
            }
        }
        fix_parents(db, &cx.cat_parent)?;
        // 충돌 사본: 새 문서로(올라가야 하므로 dirty, base 없음). 이미 있거나 삭제된 uid는 건너뛴다.
        for c in std::mem::take(&mut cx.copies) {
            if tomb_get(db, &c.uid).is_none() && load(db, true, &c.uid)?.is_none() {
                let id = write_doc(db, None, &c.uid, &c.data, c.created, c.updated, true, None)?;
                cx.rep.conflicts.push(sv(&c.data, "title").to_string());
                cx.rep.changed_docs.push(id);
            }
        }
        db.conn
            .execute_batch(
                "UPDATE docs SET category_id=(SELECT c.id FROM categories c WHERE c.uid=json_extract(docs.base_json,'$.data.cat'))
                 WHERE dirty=0 AND base_json IS NOT NULL
                   AND category_id IS NOT (SELECT c.id FROM categories c WHERE c.uid=json_extract(docs.base_json,'$.data.cat'))",
            )
            .map_err(e)?;
        db.conn.execute("DELETE FROM tags WHERE NOT EXISTS(SELECT 1 FROM doc_tags WHERE tag_id=tags.id)", []).map_err(e)?;
        Ok(())
    })
}

// ───────────────────────── 올리기 ─────────────────────────

fn next_vv(base: Option<&Rec>, me: &str) -> Vv {
    let mut v = base.map(|b| b.vv.clone()).unwrap_or_default();
    *v.entry(me.to_string()).or_insert(0) += 1;
    v
}

/// 올릴 레코드를 모은다(변경 없는 dirty는 여기서 정리하고 올리지 않는다).
fn collect_dirty(db: &Db, me: &str, dn: &str) -> Res<Vec<Rec>> {
    let mut out = Vec::new();
    let mut clean: Vec<(bool, String)> = Vec::new();
    for is_doc in [false, true] {
        for (uid, l) in load_dirty(db, is_doc)? {
            if l.base.as_ref().is_some_and(|b| b.data == l.cur && !b.force) {
                clean.push((is_doc, uid));
                continue;
            }
            out.push(Rec {
                k: if is_doc { K_DOC } else { K_CAT }.into(),
                uid,
                dev: me.into(),
                dn: dn.into(),
                vv: next_vv(l.base.as_ref(), me),
                created: l.created,
                updated: l.updated,
                parent: l.base.as_ref().map(|b| b.data.clone()),
                data: l.cur,
                force: false,
            });
        }
    }
    for (is_doc, uid) in clean {
        db.conn.execute(if is_doc { "UPDATE docs SET dirty=0 WHERE uid=?" } else { "UPDATE categories SET dirty=0 WHERE uid=?" }, [uid]).map_err(e)?;
    }
    let mut st = db.conn.prepare("SELECT uid,kind,base_json FROM tombstones WHERE dirty=1").map_err(e)?;
    let rows: Vec<(String, String, Option<String>)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(e)?.flatten().collect();
    for (uid, kind, b) in rows {
        let base: Option<Rec> = b.and_then(|s| serde_json::from_str(&s).ok());
        out.push(Rec {
            k: if kind == "d" { K_DEL_DOC } else { K_DEL_CAT }.into(),
            uid,
            dev: me.into(),
            dn: dn.into(),
            vv: next_vv(base.as_ref(), me),
            created: 0,
            updated: 0,
            data: Data::new(),
            parent: None,
            force: false,
        });
    }
    Ok(out)
}

/// 올린 레코드를 확정한다. 올리는 사이 사용자가 더 고쳤으면(현재 값이 올린 값과 다르면) dirty를 유지한다.
fn confirm_pushed(db: &Db, recs: &[Rec]) -> Res<()> {
    tx(db, true, || {
        for r in recs {
            match r.k.as_str() {
                K_DOC | K_CAT => {
                    let is_doc = r.k == K_DOC;
                    if let Some(l) = load(db, is_doc, &r.uid)? {
                        let same = l.cur == r.data;
                        let sql = if is_doc { "UPDATE docs SET dirty=?1, base_json=?2 WHERE uid=?3" } else { "UPDATE categories SET dirty=?1, base_json=?2 WHERE uid=?3" };
                        db.conn.execute(sql, params![(!same) as i64, base_str(r), r.uid]).map_err(e)?;
                    }
                }
                _ => tomb_set(db, r, false)?,
            }
        }
        Ok(())
    })
}

// ───────────────────────── 이미지 ─────────────────────────

fn image_hashes(recs: &[Rec]) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for r in recs.iter().filter(|r| r.k == K_DOC) {
        s.extend(crate::md::image_refs(sv(&r.data, "body")));
    }
    s
}

fn push_images(db: &Db, remote: &dyn Remote, key: &VaultKey, hashes: &BTreeSet<String>, have: &HashSet<String>, rep: &mut Report, o: &Opts) -> Res<()> {
    for h in hashes {
        o.check()?;
        let name = img_name(h);
        if have.contains(&name) {
            continue;
        }
        let Some((webp, w, hh)) = db.get_image(h)? else { continue };
        let mut plain = Vec::with_capacity(webp.len() + 8);
        plain.extend_from_slice(&w.to_le_bytes());
        plain.extend_from_slice(&hh.to_le_bytes());
        plain.extend_from_slice(&webp);
        let data = seal_named(key, &name, &plain).map_err(e)?;
        match remote.put(&name, &data, PutCond::IfNoneMatch) {
            Ok(_) | Err(RemoteError::Conflict) => rep.images_up += 1,
            Err(x) => return Err(rerr(x)),
        }
    }
    Ok(())
}

fn file_hashes(recs: &[Rec]) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for r in recs.iter().filter(|r| r.k == K_DOC) {
        s.extend(crate::md::file_refs(sv(&r.data, "body")));
    }
    s
}

/// 올리는 문서가 가리키는 첨부 파일을 서버에 올린다(이미 있으면 건너뜀).
fn push_files(db: &Db, remote: &dyn Remote, key: &VaultKey, hashes: &BTreeSet<String>, have: &HashSet<String>, o: &Opts) -> Res<()> {
    for h in hashes {
        o.check()?;
        let name = file_name(h);
        if have.contains(&name) {
            continue;
        }
        let Some(data) = db.get_file(h)? else { continue };
        let sealed = seal_named(key, &name, &data).map_err(e)?;
        match remote.put(&name, &sealed, PutCond::IfNoneMatch) {
            Ok(_) | Err(RemoteError::Conflict) => {}
            Err(x) => return Err(rerr(x)),
        }
    }
    Ok(())
}

/// 본문이 가리키는데 로컬에 없는 첨부 파일을 받는다. 못 받은 것이 있으면 true.
fn pull_files(db: &Db, remote: &dyn Remote, key: &VaultKey, o: &Opts) -> Res<bool> {
    let mut want: BTreeSet<String> = BTreeSet::new();
    {
        let mut st = db.conn.prepare("SELECT body FROM docs WHERE body LIKE '%[[file:%'").map_err(e)?;
        for b in st.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.flatten() {
            want.extend(crate::md::file_refs(&b));
        }
    }
    let mut missing = false;
    for h in want {
        o.check()?;
        if db.has_file(&h)? {
            continue;
        }
        let name = file_name(&h);
        match remote.get(&name).map_err(rerr)? {
            Some((data, _)) => match open_named(key, &name, &data) {
                Some(p) if p.len() <= crate::db::ATTACH_MAX => db.put_file(&h, &p)?,
                _ => missing = true,
            },
            None => missing = true,
        }
    }
    Ok(missing)
}

/// 본문이 가리키는데 로컬에 없는 이미지를 받는다. 못 받은 것이 있으면 다음 동기화 때 다시 시도하도록 표시한다.
fn pull_images(db: &Db, remote: &dyn Remote, key: &VaultKey, rep: &mut Report, o: &Opts) -> Res<()> {
    let mut want: BTreeSet<String> = BTreeSet::new();
    {
        let mut st = db.conn.prepare("SELECT body FROM docs WHERE body LIKE '%img:%'").map_err(e)?;
        for b in st.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.flatten() {
            want.extend(crate::md::image_refs(&b));
        }
    }
    let mut missing = false;
    for h in want {
        o.check()?;
        if db.get_image(&h)?.is_some() {
            continue;
        }
        let name = img_name(&h);
        match remote.get(&name).map_err(rerr)? {
            Some((data, _)) => match open_named(key, &name, &data) {
                Some(p) if p.len() > 8 => {
                    let w = u32::from_le_bytes(p[0..4].try_into().unwrap());
                    let hh = u32::from_le_bytes(p[4..8].try_into().unwrap());
                    db.put_image(&h, &p[8..], w, hh)?;
                    rep.images_down += 1;
                }
                _ => missing = true,
            },
            None => missing = true,
        }
    }
    let files_missing = pull_files(db, remote, key, o)?;
    mset(db, "ds_img_pending", if missing || files_missing { "1" } else { "0" })
}

// ───────────────────────── 동기화 한 번 ─────────────────────────

pub struct Opts<'a> {
    /// 이 기기의 표시 이름(충돌 사본 제목에 쓴다).
    pub name: &'a str,
    pub now: i64,
    pub ckpt_every: u64,
    pub retain_days: i64,
    /// 켜지면 `sync`가 안전한 지점에서 멈춘다(올리기 도중 쪼개지 않는다). 없으면 취소할 수 없다.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// 취소로 멈췄을 때의 오류 문구.
pub const CANCELLED: &str = "동기화를 취소했습니다";

impl<'a> Opts<'a> {
    pub fn new(name: &'a str, now: i64) -> Self {
        Opts { name, now, ckpt_every: CKPT_EVERY, retain_days: RETAIN_DAYS, cancel: None }
    }

    pub fn with_cancel(mut self, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// 취소됐으면 오류를 돌려 `?`로 빠져나가게 한다. 서버에 쓰는 한 단계(올리기 → 순번 기록)의 사이에는 부르지 않는다.
    fn check(&self) -> Res<()> {
        if self.cancelled() { Err(CANCELLED.into()) } else { Ok(()) }
    }
}

struct Index {
    logs: BTreeMap<String, Vec<u64>>,
    /// (이름, 각 기기 로그를 담은 순번)
    ckpts: Vec<(String, BTreeMap<String, u64>)>,
}

fn index(names: &[String]) -> Index {
    let mut ix = Index { logs: BTreeMap::new(), ckpts: Vec::new() };
    for n in names {
        match parse_name(n) {
            Some(FileKind::Log(d, s)) => ix.logs.entry(d).or_default().push(s),
            Some(FileKind::Ckpt(u)) => ix.ckpts.push((n.clone(), u)),
            _ => {}
        }
    }
    for v in ix.logs.values_mut() {
        v.sort_unstable();
    }
    ix.ckpts.sort_by_key(|(n, u)| (u.values().sum::<u64>(), n.clone()));
    ix
}

fn cursor(db: &Db, dev: &str) -> u64 {
    mget_u64(db, &format!("ds_cur:{dev}"))
}

/// 묶음 읽기 실패의 종류. 네트워크처럼 다시 하면 될 수 있는 것과, 파일 자체를 읽을 수 없는 것(복호화·형식·이름 불일치)을 가른다.
enum BatchErr {
    Transient(String),
    Permanent(String),
}

impl BatchErr {
    fn msg(self) -> String {
        match self {
            BatchErr::Transient(m) | BatchErr::Permanent(m) => m,
        }
    }
}

/// 같은 로그가 이 횟수만큼 연속으로 "읽을 수 없음"이면 영구 오류로 보고 건너뛴다(클라우드 폴더에서 덜 받은 파일 등 일시적인 경우를 거르려고 여러 번 본다).
const BAD_LOG_LIMIT: u64 = 3;

fn read_batch(remote: &dyn Remote, key: &VaultKey, name: &str) -> Result<Batch, BatchErr> {
    let (data, _) = remote
        .get(name)
        .map_err(|x| BatchErr::Transient(rerr(x)))?
        .ok_or_else(|| BatchErr::Transient(format!("{name}: 서버에서 사라졌습니다")))?;
    decode::<Batch>(key, name, &data).map_err(BatchErr::Permanent)
}

/// 문서 단위 동기화를 한 번 한다: 받기 → 적용(병합) → 올리기 → (가끔) 체크포인트·정리.
/// 올리기 전 단계가 실패하면 올리지 않는다. 어느 단계든 끊겨도 다음에 이어서 하면 같은 결과가 된다.
pub fn sync(db: &Db, remote: &dyn Remote, key: &VaultKey, o: &Opts) -> Res<Report> {
    if !is_connected(db, key) {
        return Err("이 저장소에 아직 연결되지 않았습니다(첫 연결이 필요합니다)".into());
    }
    let me = device_id(db).ok_or("기기 ID가 없습니다")?;
    let mut rep = Report::default();
    o.check()?;
    let names = remote.list().map_err(rerr)?;
    let ix = index(&names);
    let mut pull_err: Option<String> = None;

    // 1) 받을 묶음 모으기. 틈(정리된 로그)이 있거나 처음이면 체크포인트부터.
    let mut recs: Vec<Rec> = Vec::new();
    let mut new_cur: BTreeMap<String, u64> = BTreeMap::new();
    let fresh = db.conn.query_row("SELECT COUNT(*) FROM meta WHERE key LIKE 'ds_cur:%'", [], |r| r.get::<_, i64>(0)).map_err(e)? == 0;
    let gap = ix.logs.iter().filter(|(d, _)| **d != me).any(|(d, seqs)| {
        let c = cursor(db, d);
        seqs.iter().any(|s| *s > c) && !seqs.contains(&(c + 1))
    });
    if (fresh || gap) && !ix.ckpts.is_empty() {
        o.check()?;
        // 가장 새 체크포인트부터. 읽을 수 없는 것(손상)이면 더 오래된 것으로 넘어간다. 일시적 오류면 거기서 멈춘다.
        for (name, _) in ix.ckpts.iter().rev() {
            match read_batch(remote, key, name) {
                Ok(b) => {
                    for (d, s) in &b.upto {
                        if *d != me {
                            new_cur.insert(d.clone(), (*s).max(cursor(db, d)));
                        }
                    }
                    recs.extend(b.recs);
                    rep.pulled_batches += 1;
                    pull_err = None;
                    break;
                }
                Err(BatchErr::Permanent(m)) => pull_err = Some(m),
                Err(BatchErr::Transient(m)) => {
                    pull_err = Some(m);
                    break;
                }
            }
        }
    }
    if pull_err.is_none() {
        'devs: for (d, seqs) in ix.logs.iter().filter(|(d, _)| **d != me) {
            let mut c = new_cur.get(d).copied().unwrap_or_else(|| cursor(db, d));
            let c0 = c;
            for s in seqs.iter().filter(|s| **s > c0) {
                if *s != c + 1 {
                    pull_err = Some(format!("{d}의 로그 {}번이 없습니다(정리되었거나 누락). 체크포인트로 다시 시작해야 합니다", c + 1));
                    continue 'devs;
                }
                o.check()?; // 아직 아무것도 적용하지 않았다: 여기서 멈추면 읽은 위치도 그대로라 다음에 처음부터 다시 받는다
                let name = log_name(d, *s);
                let got = read_batch(remote, key, &name).and_then(|b| {
                    if b.dev == *d && b.seq == *s { Ok(b) } else { Err(BatchErr::Permanent(format!("{name}: 이름과 내용이 맞지 않습니다"))) }
                });
                match got {
                    Ok(b) => {
                        recs.extend(b.recs);
                        c = *s;
                        new_cur.insert(d.clone(), c);
                        rep.pulled_batches += 1;
                    }
                    Err(BatchErr::Permanent(m)) => {
                        // 읽을 수 없는 로그가 올리기를 영원히 막지 않도록, 여러 번 연속 실패하면 건너뛰고 경고로 남긴다.
                        // (그 로그의 변경은 이 기기에 없을 수 있다. 다른 기기가 체크포인트를 올리면 채워진다.)
                        let bad = format!("ds_bad:{name}");
                        let n = mget_u64(db, &bad) + 1;
                        mset(db, &bad, &n.to_string())?;
                        if n >= BAD_LOG_LIMIT {
                            rep.skipped_logs.push(format!("{name}: {m}"));
                            c = *s;
                            new_cur.insert(d.clone(), c);
                        } else {
                            pull_err = Some(format!("{m} ({n}/{BAD_LOG_LIMIT}번째 실패. 계속되면 건너뜁니다)"));
                            continue 'devs;
                        }
                    }
                    Err(x) => {
                        pull_err = Some(x.msg());
                        continue 'devs;
                    }
                }
            }
        }
    }

    // 2) 적용 + 읽은 위치 저장(한 트랜잭션처럼: 적용 뒤 위치 저장, 실패해도 멱등이라 다시 받아도 안전)
    o.check()?;
    apply_records(db, &recs, &me, o.name, &mut rep)?;
    for (d, s) in &new_cur {
        mset(db, &format!("ds_cur:{d}"), &s.to_string())?;
    }
    if fresh && new_cur.is_empty() {
        mset(db, "ds_cur:-", "0")?; // 읽을 것이 없었어도 "처음"이 끝났음을 표시
    }
    if let Some(x) = pull_err {
        return Err(x);
    }
    if rep.applied > 0 || mget(db, "ds_img_pending").as_deref() == Some("1") {
        pull_images(db, remote, key, &mut rep, o)?;
    }

    // 3) 올리기
    o.check()?;
    let out = collect_dirty(db, &me, o.name)?;
    let mut seq = mget_u64(db, "ds_seq");
    if !out.is_empty() {
        let have: HashSet<String> = names.iter().cloned().collect();
        push_images(db, remote, key, &image_hashes(&out), &have, &mut rep, o)?;
        push_files(db, remote, key, &file_hashes(&out), &have, o)?;
        o.check()?; // 로그를 올리고 순번을 기록하는 사이에는 멈추지 않는다
        let mut tries = 0;
        loop {
            let n = seq + 1;
            let name = log_name(&me, n);
            let batch = Batch { v: FORMAT, dev: me.clone(), seq: n, ts: o.now, recs: out.clone(), upto: BTreeMap::new() };
            match remote.put(&name, &encode(key, &name, &batch)?, PutCond::IfNoneMatch) {
                Ok(_) => {
                    seq = n;
                    break;
                }
                // 이미 있음: 앞서 올렸는데 응답을 못 받은 경우. 같은 레코드는 멱등이라 다음 순번으로 다시 올려도 안전하다.
                Err(RemoteError::Conflict) if tries < 5 => {
                    tries += 1;
                    seq = n;
                }
                Err(x) => return Err(rerr(x)),
            }
        }
        mset(db, "ds_seq", &seq.to_string())?;
        confirm_pushed(db, &out)?;
        rep.pushed = out.len();
    }

    // 4) 기기 파일, 체크포인트, 정리(실패해도 동기화 자체는 성공). 여기까지 왔으면 올리기는 끝났으므로
    // 취소돼도 오류가 아니라 성공으로 마친다(남은 정리는 다음 동기화가 한다).
    if o.cancelled() {
        rep.changed_docs.sort_unstable();
        rep.changed_docs.dedup();
        return Ok(rep);
    }
    let mut cursors: BTreeMap<String, u64> = BTreeMap::new();
    {
        let mut st = db.conn.prepare("SELECT key,value FROM meta WHERE key LIKE 'ds_cur:%'").map_err(e)?;
        for (k, v) in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(e)?.flatten() {
            if let (Some(d), Ok(n)) = (k.strip_prefix("ds_cur:"), v.parse::<u64>()) {
                if d != "-" {
                    cursors.insert(d.to_string(), n);
                }
            }
        }
    }
    cursors.insert(me.clone(), seq);
    let df = DevFile { dev: me.clone(), name: o.name.to_string(), seen: o.now, seq, cursors: cursors.clone() };
    let dname = dev_name(&me);
    let _ = remote.put(&dname, &encode(key, &dname, &df)?, PutCond::Any);
    if seq > 0 && seq - mget_u64(db, "ds_ckpt_seq").min(seq) >= o.ckpt_every {
        if let Ok(pruned) = write_checkpoint_and_prune(db, remote, key, &me, seq, &cursors, o) {
            rep.ckpt_written = true;
            rep.pruned = pruned;
        }
    }
    rep.changed_docs.sort_unstable();
    rep.changed_docs.dedup();
    Ok(rep)
}

fn upto_le(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> bool {
    a.iter().all(|(k, v)| b.get(k).copied().unwrap_or(0) >= *v)
}

/// 지금까지 확정된 전체 상태를 체크포인트로 올리고(새 기기 시작·정리용), 모두가 읽은 옛 로그와 덮어쓰인 옛 체크포인트를 지운다.
/// 이미 더 새로운(모든 기기에서 같거나 앞선) 체크포인트가 있으면 만들지 않는다.
pub fn write_checkpoint_and_prune(db: &Db, remote: &dyn Remote, key: &VaultKey, me: &str, seq: u64, cursors: &BTreeMap<String, u64>, o: &Opts) -> Res<usize> {
    let ix0 = index(&remote.list().map_err(rerr)?);
    mset(db, "ds_ckpt_seq", &seq.to_string())?;
    if ix0.ckpts.iter().any(|(_, u)| upto_le(cursors, u)) {
        return Ok(0);
    }
    let mut recs = load_all_bases(db, "categories")?;
    recs.extend(load_all_bases(db, "docs")?);
    let tombs: Vec<Rec> = {
        let mut st = db.conn.prepare("SELECT base_json FROM tombstones WHERE base_json IS NOT NULL AND dirty=0").map_err(e)?;
        let v = st.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.flatten().filter_map(|s| serde_json::from_str(&s).ok()).collect();
        v
    };
    recs.extend(tombs);
    let name = ckpt_name(me, seq, cursors);
    let batch = Batch { v: FORMAT, dev: me.to_string(), seq, ts: o.now, recs, upto: cursors.clone() };
    match remote.put(&name, &encode(key, &name, &batch)?, PutCond::IfNoneMatch) {
        Ok(_) | Err(RemoteError::Conflict) => {}
        Err(x) => return Err(rerr(x)),
    }
    // 내 것에 완전히 덮이는 옛 체크포인트 삭제. 남은 것(서로 앞선 부분이 다른 것)이 있으면 그 범위까지만 로그를 지운다.
    let mut limit: BTreeMap<String, u64> = cursors.clone();
    for (n, u) in &ix0.ckpts {
        if upto_le(u, cursors) {
            remote.delete(n).map_err(rerr)?;
        } else {
            for (d, v) in limit.iter_mut() {
                *v = (*v).min(u.get(d).copied().unwrap_or(0));
            }
        }
    }
    let names = remote.list().map_err(rerr)?;
    let ix = index(&names);
    let mut active: Vec<DevFile> = Vec::new();
    let mut known: HashSet<String> = HashSet::new();
    for n in &names {
        if let Some(FileKind::Dev(d)) = parse_name(n) {
            if let Some((data, _)) = remote.get(n).map_err(rerr)? {
                if let Ok(f) = decode::<DevFile>(key, n, &data) {
                    known.insert(d);
                    if o.now - f.seen <= o.retain_days * 86400 {
                        active.push(f);
                    }
                }
            }
        }
    }
    let mut pruned = 0;
    for (d, seqs) in &ix.logs {
        if !known.contains(d) {
            continue; // 기기 파일이 없으면 누가 읽었는지 모른다
        }
        let mut lim = limit.get(d).copied().unwrap_or(0);
        for f in active.iter().filter(|f| f.dev != *d) {
            lim = lim.min(f.cursors.get(d).copied().unwrap_or(0));
        }
        for s in seqs.iter().filter(|s| **s <= lim) {
            remote.delete(&log_name(d, *s)).map_err(rerr)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests;
