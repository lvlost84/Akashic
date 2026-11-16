//! SQLite 저장소: 문서·분류 트리·태그·휴지통·FTS5(trigram) 검색·백업.
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type Res<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

pub const MAX_DEPTH: usize = 6;
pub const SCHEMA_VERSION: i64 = 2;
/// 직접 정렬 위치 값 사이의 기본 간격.
pub const POS_GAP: i64 = 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum View {
    All,
    Uncategorized,
    Favorites,
    Trash,
    Category(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Updated,
    Created,
    Title,
    /// 직접 정렬(끌어서 바꾼 순서, docs.pos)
    Manual,
}

#[derive(Clone, Debug)]
pub struct Filter {
    pub view: View,
    pub tags: Vec<String>,
    pub query: String,
    pub sort: Sort,
}

#[derive(Clone, Debug)]
pub struct DocRow {
    pub id: i64,
    pub title: String,
    pub category: Option<i64>,
    pub pinned: bool,
    pub updated: i64,
    pub created: i64,
}

#[derive(Clone, Debug)]
pub struct Doc {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub category: Option<i64>,
    pub pinned: bool,
    pub created: i64,
    pub updated: i64,
    pub trashed: bool,
}

/// 내보내기용 문서 한 건(태그 포함).
#[derive(Clone, Debug)]
pub struct ExportDoc {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub category: Option<i64>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created: i64,
    pub updated: i64,
}

#[derive(Clone, Debug)]
pub struct Category {
    pub id: i64,
    pub parent: Option<i64>,
    pub name: String,
    pub count: i64,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 태그 정규화: 앞의 #·공백(전각 포함) 정리, 공백은 '-', 최대 40자(글자 단위).
pub fn norm_tag(s: &str) -> String {
    let t = s.trim().trim_start_matches('#').trim();
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in t.chars() {
        if ch.is_whitespace() || ch == '\u{3000}' || ch == ',' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(ch);
            prev_dash = false;
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// 변경 카운터: 문서·분류·태그가 바뀔 때마다 meta의 rev가 1씩 오른다(동기화할 변경이 있는지 판단용).
/// 고정·분류 이동·태그·직접 정렬·영구 삭제처럼 `updated`를 건드리지 않는 변경도 잡기 위해 트리거로 올린다.
const REV_SQL: &str = "INSERT OR IGNORE INTO meta(key,value) VALUES('rev','0');
CREATE TRIGGER IF NOT EXISTS rev_docs_i AFTER INSERT ON docs BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_docs_u AFTER UPDATE ON docs BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_docs_d AFTER DELETE ON docs BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_cat_i AFTER INSERT ON categories BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_cat_u AFTER UPDATE ON categories BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_cat_d AFTER DELETE ON categories BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_tag_i AFTER INSERT ON tags BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_tag_d AFTER DELETE ON tags BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_dt_i AFTER INSERT ON doc_tags BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;
CREATE TRIGGER IF NOT EXISTS rev_dt_d AFTER DELETE ON doc_tags BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='rev'; END;";

/// 스키마 v1(처음 만들 때와 v1 파일 시험에 쓴다).
pub const SCHEMA_V1: &str = "BEGIN;
CREATE TABLE categories(id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES categories(id) ON DELETE SET NULL, name TEXT NOT NULL);
CREATE TABLE docs(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL DEFAULT '', body TEXT NOT NULL DEFAULT '',
  category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL,
  pinned INTEGER NOT NULL DEFAULT 0, trashed_at INTEGER);
CREATE INDEX docs_cat ON docs(category_id);
CREATE INDEX docs_upd ON docs(updated);
CREATE TABLE tags(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE doc_tags(doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE, PRIMARY KEY(doc_id, tag_id));
CREATE INDEX doc_tags_tag ON doc_tags(tag_id);
CREATE VIRTUAL TABLE fts USING fts5(title, body, content='docs', content_rowid='id', tokenize='trigram');
CREATE TRIGGER docs_ai AFTER INSERT ON docs BEGIN
  INSERT INTO fts(rowid,title,body) VALUES (new.id,new.title,new.body); END;
CREATE TRIGGER docs_ad AFTER DELETE ON docs BEGIN
  INSERT INTO fts(fts,rowid,title,body) VALUES('delete',old.id,old.title,old.body); END;
CREATE TRIGGER docs_au AFTER UPDATE OF title, body ON docs BEGIN
  INSERT INTO fts(fts,rowid,title,body) VALUES('delete',old.id,old.title,old.body);
  INSERT INTO fts(rowid,title,body) VALUES (new.id,new.title,new.body); END;
INSERT OR REPLACE INTO meta(key,value) VALUES('schema','1');
COMMIT";

pub struct Db {
    pub conn: Connection,
    pub path: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> Res<Db> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(e)?;
        }
        let conn = Connection::open(path).map_err(e)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=3000; PRAGMA wal_autocheckpoint=0;",
        )
        .map_err(e)?;
        // wal_autocheckpoint=0: 저장은 작은 쓰기 로그(WAL)에 이어 붙이기만 하고, 본 파일로의 병합(체크포인트)은
        // 정한 시각(병합 주기·종료·시작·로그가 너무 커졌을 때)에만 한다 → 본 파일을 자주 건드리지 않아 SSD 쓰기·파편화가 줄어든다.
        let db = Db { conn, path: path.to_path_buf() };
        db.migrate()?;
        let _ = db.checkpoint(); // 시작할 때 지난 로그 병합
        Ok(db)
    }

    /// DB 메모리 캐시 상한(MB). 음수 cache_size는 KiB 단위라서 -MB*1024.
    pub fn set_cache_mb(&self, mb: u32) {
        let kib = (mb.clamp(4, 1024) as i64) * 1024;
        let _ = self.conn.execute_batch(&format!("PRAGMA cache_size=-{kib};"));
    }

    /// 쓰기 로그(WAL)를 본 파일에 병합하고 로그를 비운다.
    pub fn checkpoint(&self) -> Res<()> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(e)
    }

    /// 쓰기 로그 파일 크기.
    pub fn wal_size(&self) -> u64 {
        std::fs::metadata(format!("{}-wal", self.path.display())).map(|m| m.len()).unwrap_or(0)
    }

    /// 스키마를 최신으로 맞춘다. DB를 통째로 바꿔 넣은 뒤(불러오기)에도 불러야 한다.
    pub fn migrate(&self) -> Res<()> {
        self.conn
            .execute_batch("CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .map_err(e)?;
        let ver: i64 = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get::<_, String>(0))
            .optional()
            .map_err(e)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if ver > SCHEMA_VERSION {
            return Err(format!("더 새로운 버전의 DB입니다(스키마 {ver}). 이 프로그램을 업데이트하세요."));
        }
        if ver < 1 {
            self.conn
                .execute_batch(SCHEMA_V1)
                .map_err(e)?;
        }
        if ver < 2 {
            // v2: 직접 정렬용 docs.pos. 처음 순서는 최근 수정순 순위.
            self.conn
                .execute_batch(&format!(
                    "BEGIN;
ALTER TABLE docs ADD COLUMN pos INTEGER;
UPDATE docs SET pos=r.rn FROM (SELECT id, (ROW_NUMBER() OVER (ORDER BY updated DESC, id DESC)-1)*{POS_GAP} AS rn FROM docs) AS r WHERE r.id=docs.id;
CREATE INDEX docs_pos ON docs(pos);
INSERT OR REPLACE INTO meta(key,value) VALUES('schema','2');
COMMIT;"
                ))
                .map_err(e)?;
        }
        self.conn.execute_batch(REV_SQL).map_err(e)?;
        Ok(())
    }

    /// 변경 카운터(위 REV_SQL). 값이 달라졌으면 마지막으로 확인한 뒤 무언가 바뀐 것이다.
    pub fn rev(&self) -> i64 {
        self.conn.query_row("SELECT CAST(value AS INTEGER) FROM meta WHERE key='rev'", [], |r| r.get(0)).unwrap_or(0)
    }

    // ---------- 직접 정렬 ----------
    /// 모든 문서의 pos를 현재 순서 그대로 간격 `POS_GAP`으로 다시 매긴다.
    pub fn renumber(&self) -> Res<()> {
        self.conn
            .execute(
                &format!(
                    "UPDATE docs SET pos=r.rn FROM (SELECT id, (ROW_NUMBER() OVER (ORDER BY pos, id)-1)*{POS_GAP} AS rn FROM docs) AS r WHERE r.id=docs.id"
                ),
                [],
            )
            .map_err(e)?;
        Ok(())
    }

    /// 문서 `id`를 `before` 문서 바로 앞으로 옮긴다(`None`이면 같은 고정 구역의 맨 끝).
    /// 순서(pos)만 바꾸고 수정 시각은 건드리지 않는다. 고정 상태가 다른 문서 앞으로는 옮길 수 없다.
    pub fn reorder(&self, id: i64, before: Option<i64>) -> Res<()> {
        if before == Some(id) {
            return Ok(());
        }
        let pin_of = |d: i64| -> Res<i64> {
            self.conn.query_row("SELECT pinned FROM docs WHERE id=?", [d], |r| r.get(0)).map_err(e)
        };
        if let Some(b) = before {
            if pin_of(id)? != pin_of(b)? {
                return Err("고정된 문서와 고정되지 않은 문서 사이로는 옮길 수 없습니다".into());
            }
        }
        for _ in 0..2 {
            let new_pos = match before {
                None => {
                    let m: Option<i64> =
                        self.conn.query_row("SELECT MAX(pos) FROM docs WHERE id<>?", [id], |r| r.get(0)).map_err(e)?;
                    m.unwrap_or(0) + POS_GAP
                }
                Some(b) => {
                    let bpos: i64 = self
                        .conn
                        .query_row("SELECT COALESCE(pos,0) FROM docs WHERE id=?", [b], |r| r.get(0))
                        .map_err(e)?;
                    let prev: Option<i64> = self
                        .conn
                        .query_row("SELECT MAX(pos) FROM docs WHERE id<>?1 AND pos<?2", params![id, bpos], |r| r.get(0))
                        .map_err(e)?;
                    match prev {
                        None => bpos - POS_GAP,
                        Some(p) if bpos - p >= 2 => p + (bpos - p) / 2,
                        Some(_) => {
                            self.renumber()?; // 간격이 없으면 다시 매기고 재시도
                            continue;
                        }
                    }
                }
            };
            self.conn.execute("UPDATE docs SET pos=? WHERE id=?", params![new_pos, id]).map_err(e)?;
            return Ok(());
        }
        Err("순서를 바꾸지 못했습니다".into())
    }

    /// 목록에서 `dragged`를 `target` 위(`after=false`)/아래(`after=true`)에 놓았을 때의 `reorder` 인자.
    /// 고정 상태가 다르면 거절 사유를 돌려준다.
    pub fn plan_reorder(rows: &[DocRow], dragged: i64, target: i64, after: bool) -> Result<(i64, Option<i64>), String> {
        let d = rows.iter().find(|r| r.id == dragged).ok_or("옮길 문서를 찾을 수 없습니다")?;
        let ti = rows.iter().position(|r| r.id == target).ok_or("놓을 위치를 찾을 수 없습니다")?;
        let t = &rows[ti];
        if d.pinned != t.pinned {
            return Err("고정된 문서와 고정되지 않은 문서 사이로는 옮길 수 없습니다".into());
        }
        if !after {
            return Ok((dragged, Some(target)));
        }
        let next = rows.get(ti + 1).filter(|n| n.pinned == t.pinned).map(|n| n.id);
        Ok((dragged, next))
    }

    /// 내보내기용: 휴지통을 뺀 모든 문서(분류 id·태그 포함)와 분류 목록을 한 번에 읽는다.
    pub fn export_data(&self) -> Res<(Vec<ExportDoc>, Vec<Category>)> {
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        {
            let mut st = self
                .conn
                .prepare(
                    "SELECT dt.doc_id, t.name FROM doc_tags dt JOIN tags t ON t.id=dt.tag_id ORDER BY t.name COLLATE NOCASE",
                )
                .map_err(e)?;
            let it = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(e)?;
            for x in it {
                let (d, n) = x.map_err(e)?;
                tags.entry(d).or_default().push(n);
            }
        }
        let mut st = self
            .conn
            .prepare("SELECT id,title,body,category_id,pinned,created,updated FROM docs WHERE trashed_at IS NULL ORDER BY id")
            .map_err(e)?;
        let it = st
            .query_map([], |r| {
                Ok(ExportDoc {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    body: r.get(2)?,
                    category: r.get(3)?,
                    pinned: r.get::<_, i64>(4)? != 0,
                    created: r.get(5)?,
                    updated: r.get(6)?,
                    tags: Vec::new(),
                })
            })
            .map_err(e)?;
        let mut docs = it.collect::<Result<Vec<_>, _>>().map_err(e)?;
        for d in &mut docs {
            d.tags = tags.remove(&d.id).unwrap_or_default();
        }
        Ok((docs, self.categories()?))
    }

    // ---------- 문서 ----------
    pub fn new_doc(&self, category: Option<i64>) -> Res<i64> {
        let t = now();
        self.conn
            .execute(
                "INSERT INTO docs(title,body,category_id,created,updated,pos)
                 VALUES('','',?,?,?,(SELECT COALESCE(MIN(pos),0)-?4 FROM docs))",
                params![category, t, t, POS_GAP],
            )
            .map_err(e)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_doc(&self, id: i64) -> Res<Option<Doc>> {
        self.conn
            .query_row(
                "SELECT id,title,body,category_id,pinned,created,updated,trashed_at IS NOT NULL FROM docs WHERE id=?",
                [id],
                |r| {
                    Ok(Doc {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        body: r.get(2)?,
                        category: r.get(3)?,
                        pinned: r.get::<_, i64>(4)? != 0,
                        created: r.get(5)?,
                        updated: r.get(6)?,
                        trashed: r.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(e)
    }

    /// 내용이 실제로 바뀐 경우에만 쓰고 true를 돌려준다.
    pub fn save_doc(&self, id: i64, title: &str, body: &str) -> Res<bool> {
        let n = self
            .conn
            .execute(
                "UPDATE docs SET title=?1, body=?2, updated=?3 WHERE id=?4 AND (title<>?1 OR body<>?2)",
                params![title, body, now(), id],
            )
            .map_err(e)?;
        Ok(n > 0)
    }

    pub fn set_pinned(&self, id: i64, v: bool) -> Res<()> {
        self.conn.execute("UPDATE docs SET pinned=? WHERE id=?", params![v as i64, id]).map_err(e)?;
        Ok(())
    }

    pub fn set_doc_category(&self, id: i64, cat: Option<i64>) -> Res<()> {
        self.conn.execute("UPDATE docs SET category_id=? WHERE id=?", params![cat, id]).map_err(e)?;
        Ok(())
    }

    pub fn trash(&self, id: i64) -> Res<()> {
        self.conn.execute("UPDATE docs SET trashed_at=? WHERE id=? AND trashed_at IS NULL", params![now(), id]).map_err(e)?;
        Ok(())
    }

    pub fn restore(&self, id: i64) -> Res<()> {
        self.conn.execute("UPDATE docs SET trashed_at=NULL WHERE id=?", [id]).map_err(e)?;
        Ok(())
    }

    pub fn purge(&self, id: i64) -> Res<()> {
        self.conn.execute("DELETE FROM docs WHERE id=? AND trashed_at IS NOT NULL", [id]).map_err(e)?;
        Ok(())
    }

    /// 휴지통에서 `days`일이 지난 문서와 쓰다 만 빈 문서를 영구 삭제.
    pub fn purge_old(&self, days: i64) -> Res<usize> {
        let cutoff = now() - days * 86400;
        let a = self
            .conn
            .execute("DELETE FROM docs WHERE trashed_at IS NOT NULL AND trashed_at<?", [cutoff])
            .map_err(e)?;
        Ok(a)
    }

    pub fn delete_empty(&self, id: i64) -> Res<()> {
        self.conn
            .execute("DELETE FROM docs WHERE id=? AND title='' AND trim(body)=''", [id])
            .map_err(e)?;
        Ok(())
    }

    pub fn find_by_title(&self, title: &str) -> Res<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM docs WHERE trashed_at IS NULL AND title=?1 COLLATE NOCASE ORDER BY updated DESC LIMIT 1",
                [title],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)
    }

    // ---------- 목록·검색 ----------
    pub fn list_docs(&self, f: &Filter) -> Res<Vec<DocRow>> {
        let mut sql = String::from(
            "SELECT d.id, d.title, substr(d.body,1,160), d.category_id, d.pinned, d.updated, d.created FROM docs d WHERE ",
        );
        let mut args: Vec<Value> = Vec::new();
        sql.push_str(if f.view == View::Trash { "d.trashed_at IS NOT NULL" } else { "d.trashed_at IS NULL" });
        // 태그를 고르면 분류와 상관없이 그 태그가 달린 문서를 모두 보인다.
        let tag_mode = !f.tags.is_empty();
        match &f.view {
            View::Uncategorized if !tag_mode => sql.push_str(" AND d.category_id IS NULL"),
            View::Favorites => sql.push_str(" AND d.pinned=1"),
            View::Category(id) if !tag_mode => {
                sql.push_str(" AND d.category_id=?");
                args.push(Value::Integer(*id));
            }
            _ => {}
        }
        for t in &f.tags {
            sql.push_str(
                " AND EXISTS(SELECT 1 FROM doc_tags dt JOIN tags t ON t.id=dt.tag_id WHERE dt.doc_id=d.id AND t.name=?)",
            );
            args.push(Value::Text(t.clone()));
        }
        for term in f.query.split_whitespace() {
            let like = format!("%{}%", like_escape(term));
            // 행마다 다시 계산하지 않도록 IN 부질의로 한 번만 평가한다.
            let tag_cond = "d.id IN (SELECT dt.doc_id FROM doc_tags dt JOIN tags t ON t.id=dt.tag_id WHERE t.name LIKE ? ESCAPE '\\')";
            if term.chars().count() >= 3 {
                sql.push_str(&format!(" AND (d.id IN (SELECT rowid FROM fts WHERE fts MATCH ?) OR {tag_cond})"));
                args.push(Value::Text(format!("\"{}\"", term.replace('"', "\"\""))));
                args.push(Value::Text(like));
            } else {
                sql.push_str(&format!(" AND (d.title LIKE ? ESCAPE '\\' OR d.body LIKE ? ESCAPE '\\' OR {tag_cond})"));
                args.push(Value::Text(like.clone()));
                args.push(Value::Text(like.clone()));
                args.push(Value::Text(like));
            }
        }
        sql.push_str(" ORDER BY d.pinned DESC, ");
        sql.push_str(match f.sort {
            Sort::Updated => "d.updated DESC",
            Sort::Created => "d.created DESC",
            Sort::Title => "d.title COLLATE NOCASE ASC, d.updated DESC",
            Sort::Manual => "d.pos ASC, d.id DESC",
        });
        let mut st = self.conn.prepare_cached(&sql).map_err(e)?;
        let rows = st
            .query_map(params_from_iter(args.iter()), |r| {
                let title: String = r.get(1)?;
                let prev: String = r.get(2)?;
                let shown = if title.trim().is_empty() {
                    prev.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("").to_string()
                } else {
                    title
                };
                Ok(DocRow {
                    id: r.get(0)?,
                    title: shown,
                    category: r.get(3)?,
                    pinned: r.get::<_, i64>(4)? != 0,
                    updated: r.get(5)?,
                    created: r.get(6)?,
                })
            })
            .map_err(e)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    /// 이 제목을 `[[제목]]`/`[[제목|글]]`로 가리키는 다른 문서들.
    pub fn backlinks(&self, id: i64, title: &str) -> Res<Vec<(i64, String)>> {
        let t = title.trim();
        if t.is_empty() {
            return Ok(vec![]);
        }
        let esc = like_escape(t);
        let mut st = self
            .conn
            .prepare(
                "SELECT id, title FROM docs WHERE trashed_at IS NULL AND id<>?1
                 AND (body LIKE ?2 ESCAPE '\\' OR body LIKE ?3 ESCAPE '\\') ORDER BY updated DESC LIMIT 200",
            )
            .map_err(e)?;
        let v = st
            .query_map(params![id, format!("%[[{esc}]]%"), format!("%[[{esc}|%")], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?;
        v.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    /// 휴지통이 아닌 문서의 분류(문서 id → 분류 id, 분류가 없으면 빠짐).
    pub fn doc_categories(&self) -> Res<HashMap<i64, i64>> {
        let mut st = self.conn.prepare("SELECT id, category_id FROM docs WHERE trashed_at IS NULL AND category_id IS NOT NULL").map_err(e)?;
        let v = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(e)?;
        v.collect::<Result<HashMap<_, _>, _>>().map_err(e)
    }

    /// 모든 (id, 제목, 본문)을 훑는다 — 링크 그래프·내보내기용(휴지통 제외).
    pub fn all_docs(&self) -> Res<Vec<(i64, String, String)>> {
        let mut st = self.conn.prepare("SELECT id,title,body FROM docs WHERE trashed_at IS NULL").map_err(e)?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(e)?;
        v.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    pub fn counts(&self) -> Res<(i64, i64, i64, i64)> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FILTER (WHERE trashed_at IS NULL),
                        COUNT(*) FILTER (WHERE trashed_at IS NULL AND category_id IS NULL),
                        COUNT(*) FILTER (WHERE trashed_at IS NULL AND pinned=1),
                        COUNT(*) FILTER (WHERE trashed_at IS NOT NULL) FROM docs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(e)
    }

    // ---------- 분류 트리 ----------
    pub fn categories(&self) -> Res<Vec<Category>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT c.id, c.parent_id, c.name, (SELECT COUNT(*) FROM docs d WHERE d.category_id=c.id AND d.trashed_at IS NULL)
                 FROM categories c ORDER BY c.name COLLATE NOCASE",
            )
            .map_err(e)?;
        let v = st
            .query_map([], |r| Ok(Category { id: r.get(0)?, parent: r.get(1)?, name: r.get(2)?, count: r.get(3)? }))
            .map_err(e)?;
        v.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    pub(crate) fn parent_map(&self) -> Res<HashMap<i64, Option<i64>>> {
        Ok(self.categories()?.into_iter().map(|c| (c.id, c.parent)).collect())
    }

    fn depth_of(map: &HashMap<i64, Option<i64>>, mut id: Option<i64>) -> usize {
        let mut d = 0;
        while let Some(i) = id {
            d += 1;
            id = map.get(&i).copied().flatten();
            if d > 64 {
                break;
            }
        }
        d
    }

    fn subtree_height(map: &HashMap<i64, Option<i64>>, id: i64) -> usize {
        let kids: Vec<i64> = map.iter().filter(|(_, p)| **p == Some(id)).map(|(k, _)| *k).collect();
        1 + kids.into_iter().map(|k| Self::subtree_height(map, k)).max().unwrap_or(0)
    }

    fn name_taken(&self, parent: Option<i64>, name: &str, except: Option<i64>) -> Res<bool> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE parent_id IS ?1 AND name=?2 COLLATE NOCASE AND id IS NOT ?3",
                params![parent, name, except],
                |r| r.get(0),
            )
            .map_err(e)?;
        Ok(n > 0)
    }

    pub fn add_category(&self, parent: Option<i64>, name: &str) -> Res<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err("분류 이름이 비어 있습니다".into());
        }
        let map = self.parent_map()?;
        if Self::depth_of(&map, parent) >= MAX_DEPTH {
            return Err(format!("분류는 {MAX_DEPTH}단계까지만 만들 수 있습니다"));
        }
        if self.name_taken(parent, name, None)? {
            return Err("같은 이름의 분류가 이미 있습니다".into());
        }
        self.conn.execute("INSERT INTO categories(parent_id,name) VALUES(?,?)", params![parent, name]).map_err(e)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_category(&self, id: i64, name: &str) -> Res<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err("분류 이름이 비어 있습니다".into());
        }
        let parent: Option<i64> = self.conn.query_row("SELECT parent_id FROM categories WHERE id=?", [id], |r| r.get(0)).map_err(e)?;
        if self.name_taken(parent, name, Some(id))? {
            return Err("같은 이름의 분류가 이미 있습니다".into());
        }
        self.conn.execute("UPDATE categories SET name=? WHERE id=?", params![name, id]).map_err(e)?;
        Ok(())
    }

    pub fn move_category(&self, id: i64, new_parent: Option<i64>) -> Res<()> {
        let map = self.parent_map()?;
        // 자기 자신·하위로는 이동 불가
        let mut cur = new_parent;
        while let Some(c) = cur {
            if c == id {
                return Err("자기 자신이나 하위 분류 아래로는 옮길 수 없습니다".into());
            }
            cur = map.get(&c).copied().flatten();
        }
        if map.get(&id).copied().flatten() == new_parent {
            return Ok(());
        }
        if Self::depth_of(&map, new_parent) + Self::subtree_height(&map, id) > MAX_DEPTH {
            return Err(format!("분류는 {MAX_DEPTH}단계를 넘을 수 없습니다"));
        }
        let name: String = self.conn.query_row("SELECT name FROM categories WHERE id=?", [id], |r| r.get(0)).map_err(e)?;
        if self.name_taken(new_parent, &name, Some(id))? {
            return Err("그 위치에 같은 이름의 분류가 이미 있습니다".into());
        }
        self.conn.execute("UPDATE categories SET parent_id=? WHERE id=?", params![new_parent, id]).map_err(e)?;
        Ok(())
    }

    /// 삭제 시 하위 분류와 문서는 한 단계 위로 올라간다.
    pub fn delete_category(&self, id: i64) -> Res<()> {
        let parent: Option<i64> = self.conn.query_row("SELECT parent_id FROM categories WHERE id=?", [id], |r| r.get(0)).map_err(e)?;
        // 하위 분류가 한 단계 위로 올라갈 때 그 자리(같은 부모 아래)의 다른 분류와 이름이 겹치면 같은 이름이 둘 생긴다.
        let clash: Option<String> = self
            .conn
            .query_row(
                "SELECT c.name FROM categories c WHERE c.parent_id=?1 AND EXISTS(SELECT 1 FROM categories s WHERE s.parent_id IS ?2 AND s.id<>?1 AND s.name=c.name COLLATE NOCASE) LIMIT 1",
                params![id, parent],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?;
        if let Some(n) = clash {
            return Err(format!("하위 분류 \"{n}\"이(가) 위 단계의 같은 이름 분류와 겹쳐 삭제할 수 없습니다. 이름을 바꾼 뒤 다시 시도하세요"));
        }
        self.conn.execute_batch("BEGIN").map_err(e)?;
        let r = (|| -> Res<()> {
            self.conn.execute("UPDATE categories SET parent_id=? WHERE parent_id=?", params![parent, id]).map_err(e)?;
            self.conn.execute("UPDATE docs SET category_id=? WHERE category_id=?", params![parent, id]).map_err(e)?;
            self.conn.execute("DELETE FROM categories WHERE id=?", [id]).map_err(e)?;
            Ok(())
        })();
        match r {
            Ok(()) => self.conn.execute_batch("COMMIT").map_err(e),
            Err(x) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(x)
            }
        }
    }

    // ---------- 태그 ----------
    pub fn all_tags(&self) -> Res<Vec<String>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT t.name FROM tags t WHERE EXISTS(SELECT 1 FROM doc_tags dt JOIN docs d ON d.id=dt.doc_id
                 WHERE dt.tag_id=t.id AND d.trashed_at IS NULL) ORDER BY t.name COLLATE NOCASE",
            )
            .map_err(e)?;
        let v = st.query_map([], |r| r.get(0)).map_err(e)?;
        v.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    pub fn doc_tags(&self, id: i64) -> Res<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT t.name FROM tags t JOIN doc_tags dt ON dt.tag_id=t.id WHERE dt.doc_id=? ORDER BY t.name COLLATE NOCASE")
            .map_err(e)?;
        let v = st.query_map([id], |r| r.get(0)).map_err(e)?;
        v.collect::<Result<Vec<_>, _>>().map_err(e)
    }

    pub fn add_tag(&self, id: i64, raw: &str) -> Res<()> {
        let n = norm_tag(raw);
        if n.is_empty() {
            return Ok(());
        }
        self.conn.execute("INSERT OR IGNORE INTO tags(name) VALUES(?)", [&n]).map_err(e)?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO doc_tags(doc_id,tag_id) SELECT ?1, id FROM tags WHERE name=?2",
                params![id, n],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn remove_tag(&self, id: i64, name: &str) -> Res<()> {
        self.conn
            .execute("DELETE FROM doc_tags WHERE doc_id=?1 AND tag_id=(SELECT id FROM tags WHERE name=?2)", params![id, name])
            .map_err(e)?;
        self.conn.execute("DELETE FROM tags WHERE NOT EXISTS(SELECT 1 FROM doc_tags WHERE tag_id=tags.id)", []).map_err(e)?;
        Ok(())
    }

    // ---------- 유지보수 ----------
    /// 일관된 스냅숏을 새 파일로 저장(VACUUM INTO, 대상은 없어야 함).
    pub fn snapshot_to(&self, dest: &Path) -> Res<()> {
        if dest.exists() {
            std::fs::remove_file(dest).map_err(e)?;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(e)?;
        }
        let d = dest.to_string_lossy().replace('\'', "''");
        self.conn.execute_batch(&format!("VACUUM INTO '{d}'")).map_err(e)
    }

    /// 저장된 DB 파일(WAL 포함)을 읽기 전용 연결로 열어 내보낼 자료를 읽는다. 화면 스레드 밖에서 쓸 수 있다.
    pub fn export_data_from_file(src: &Path) -> Res<(Vec<ExportDoc>, Vec<Category>)> {
        let conn = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(e)?;
        conn.execute_batch("PRAGMA busy_timeout=3000;").map_err(e)?;
        Db { conn, path: src.to_path_buf() }.export_data()
    }

    /// 이미 저장된 DB 파일(WAL 포함)을 읽기 전용 연결로 열어 스냅숏을 만든다. 화면 스레드 밖에서 쓸 수 있다.
    pub fn snapshot_file(src: &Path, dest: &Path) -> Res<()> {
        if dest.exists() {
            std::fs::remove_file(dest).map_err(e)?;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(e)?;
        }
        let conn = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(e)?;
        conn.execute_batch("PRAGMA busy_timeout=3000;").map_err(e)?;
        let d = dest.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{d}'")).map_err(e)
    }

    /// 하루 한 번 자동 백업. 새로 만들었으면 경로를 돌려준다.
    pub fn daily_backup(&self, keep: usize) -> Res<Option<PathBuf>> {
        let dir = self.path.parent().unwrap_or(Path::new(".")).join("backup");
        std::fs::create_dir_all(&dir).map_err(e)?;
        let today = chrono::Local::now().format("%Y%m%d").to_string();
        let have_today = std::fs::read_dir(&dir)
            .map_err(e)?
            .flatten()
            .any(|f| f.file_name().to_string_lossy().starts_with(&format!("vault-{today}")));
        let mut made = None;
        if !have_today {
            let p = dir.join(format!("vault-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S")));
            self.snapshot_to(&p)?;
            made = Some(p);
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(e)?
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with("vault-")))
            .collect();
        files.sort();
        while files.len() > keep.max(1) {
            let _ = std::fs::remove_file(files.remove(0));
        }
        Ok(made)
    }

    pub fn compact(&self) -> Res<(u64, u64)> {
        let before = self.file_size();
        self.conn.execute_batch("INSERT INTO fts(fts) VALUES('optimize'); PRAGMA wal_checkpoint(TRUNCATE); VACUUM;").map_err(e)?;
        Ok((before, self.file_size()))
    }

    pub fn rebuild_index(&self) -> Res<()> {
        self.conn.execute_batch("INSERT INTO fts(fts) VALUES('rebuild');").map_err(e)
    }

    pub fn integrity_ok(&self) -> bool {
        self.conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).map(|s| s == "ok").unwrap_or(false)
    }

    pub fn file_size(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
}

/// 파일 내용을 UTF-8(BOM 허용)로, 아니면 CP949로 읽는다.
pub fn decode_text(bytes: &[u8]) -> (String, bool) {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(b) {
        Ok(s) => (s.to_string(), false),
        Err(_) => (encoding_rs::EUC_KR.decode(b).0.into_owned(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("akashic-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("vault.db")
    }

    fn filter(q: &str) -> Filter {
        Filter { view: View::All, tags: vec![], query: q.into(), sort: Sort::Updated }
    }

    #[test]
    fn crud_search_korean_and_short_terms() {
        let db = Db::open(&tmp("crud")).unwrap();
        let a = db.new_doc(None).unwrap();
        db.save_doc(a, "품질 보고서", "입도 분석 결과: 평균 12um").unwrap();
        let b = db.new_doc(None).unwrap();
        db.save_doc(b, "", "# 회의록\n오늘 회의 내용").unwrap();
        assert_eq!(db.list_docs(&filter("입도 분석")).unwrap().len(), 1); // 3글자+ (FTS)
        assert_eq!(db.list_docs(&filter("분석")).unwrap().len(), 1); // 2글자 (LIKE)
        assert_eq!(db.list_docs(&filter("보고")).unwrap().len(), 1);
        assert_eq!(db.list_docs(&filter("없는낱말")).unwrap().len(), 0);
        assert_eq!(db.list_docs(&filter("")).unwrap().len(), 2);
        // 제목이 비면 본문 첫 줄
        let rows = db.list_docs(&filter("회의")).unwrap();
        assert_eq!(rows[0].title, "회의록");
        // 수정 후 색인 갱신
        db.save_doc(a, "품질 보고서", "완전히 다른 내용").unwrap();
        assert_eq!(db.list_docs(&filter("입도 분석")).unwrap().len(), 0);
        assert_eq!(db.list_docs(&filter("다른 내용")).unwrap().len(), 1);
        // 특수문자도 안전
        assert_eq!(db.list_docs(&filter("100%")).unwrap().len(), 0);
        assert_eq!(db.list_docs(&filter("a\"b")).unwrap().len(), 0);
        assert!(db.integrity_ok());
    }

    /// 5000문서(약 20MB) 검색·목록 성능. `cargo test --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn perf_5000_docs() {
        let db = Db::open(&tmp("perf")).unwrap();
        db.conn.execute_batch("BEGIN").unwrap();
        for i in 0..5000 {
            let id = db.new_doc(None).unwrap();
            let body = format!("문서 {i} 입도 분석 측정 결과 {}\n", "품질 관리 기록 lorem ipsum dolor sit amet ".repeat(80));
            let body = if i % 500 == 7 { format!("{body} 희귀낱말{i}") } else { body };
            db.save_doc(id, &format!("제목 {i}"), &body).unwrap();
        }
        db.conn.execute_batch("COMMIT").unwrap();
        let t = std::time::Instant::now();
        let rare = db.list_docs(&filter("희귀낱말507")).unwrap();
        let t_rare = t.elapsed();
        let t = std::time::Instant::now();
        let all = db.list_docs(&filter("")).unwrap();
        let t_all = t.elapsed();
        let t = std::time::Instant::now();
        let common = db.list_docs(&filter("입도 분석")).unwrap();
        let t_common = t.elapsed();
        let t = std::time::Instant::now();
        let short = db.list_docs(&filter("분석")).unwrap();
        let t_short = t.elapsed();
        println!("파일 {} / 드문 낱말 {:?}({}건) / 전체 목록 {:?}({}건) / 흔한 낱말 {:?}({}건) / 2글자 {:?}({}건)",
            fmt(db.file_size()), t_rare, rare.len(), t_all, all.len(), t_common, common.len(), t_short, short.len());
        assert_eq!(rare.len(), 1);
        assert_eq!(all.len(), 5000);
    }

    fn fmt(b: u64) -> String {
        format!("{:.1}MB", b as f64 / 1048576.0)
    }

    #[test]
    fn save_is_noop_when_unchanged() {
        let db = Db::open(&tmp("noop")).unwrap();
        let a = db.new_doc(None).unwrap();
        assert!(db.save_doc(a, "t", "b").unwrap());
        assert!(!db.save_doc(a, "t", "b").unwrap());
    }

    #[test]
    fn category_tree_rules() {
        let db = Db::open(&tmp("cat")).unwrap();
        let q = db.add_category(None, "품질").unwrap();
        let m = db.add_category(Some(q), "측정기").unwrap();
        assert!(db.add_category(Some(q), "측정기").is_err()); // 같은 상위에 같은 이름
        assert!(db.add_category(None, "측정기").is_ok()); // 상위가 다르면 허용
        assert!(db.move_category(q, Some(m)).is_err()); // 자기 하위로 이동 불가
        // 6단계 제한
        let mut p = None;
        for i in 0..MAX_DEPTH {
            p = Some(db.add_category(p, &format!("L{i}")).unwrap());
        }
        assert!(db.add_category(p, "너무깊음").is_err());
        // 삭제하면 문서와 하위가 한 단계 위로
        let d = db.new_doc(Some(m)).unwrap();
        db.delete_category(m).unwrap();
        assert_eq!(db.get_doc(d).unwrap().unwrap().category, Some(q));
        db.delete_category(q).unwrap();
        assert_eq!(db.get_doc(d).unwrap().unwrap().category, None);
    }

    #[test]
    fn tags_cross_categories_and_normalize() {
        let db = Db::open(&tmp("tag")).unwrap();
        let c = db.add_category(None, "A").unwrap();
        let d1 = db.new_doc(Some(c)).unwrap();
        let d2 = db.new_doc(None).unwrap();
        db.add_tag(d1, "#중요 표시").unwrap();
        db.add_tag(d2, "중요-표시").unwrap();
        assert_eq!(db.doc_tags(d1).unwrap(), vec!["중요-표시"]);
        let f = Filter { view: View::Category(c), tags: vec!["중요-표시".into()], query: "".into(), sort: Sort::Title };
        assert_eq!(db.list_docs(&f).unwrap().len(), 2); // 분류와 무관하게 전부
        db.remove_tag(d1, "중요-표시").unwrap();
        assert_eq!(db.all_tags().unwrap().len(), 1);
        assert_eq!(norm_tag("가\u{3000}나"), "가-나");
        assert_eq!(norm_tag(&"가".repeat(100)).chars().count(), 40);
    }

    #[test]
    fn trash_restore_purge_and_backup() {
        let path = tmp("trash");
        let db = Db::open(&path).unwrap();
        let a = db.new_doc(None).unwrap();
        db.save_doc(a, "x", "y").unwrap();
        db.trash(a).unwrap();
        assert_eq!(db.list_docs(&filter("")).unwrap().len(), 0);
        let tr = Filter { view: View::Trash, ..filter("") };
        assert_eq!(db.list_docs(&tr).unwrap().len(), 1);
        db.restore(a).unwrap();
        assert_eq!(db.list_docs(&filter("")).unwrap().len(), 1);
        db.trash(a).unwrap();
        db.conn.execute("UPDATE docs SET trashed_at=? WHERE id=?", params![now() - 40 * 86400, a]).unwrap();
        assert_eq!(db.purge_old(30).unwrap(), 1);
        // 백업: 하루 한 번, 개수 제한
        assert!(db.daily_backup(3).unwrap().is_some());
        assert!(db.daily_backup(3).unwrap().is_none());
        let bak = std::fs::read_dir(path.parent().unwrap().join("backup")).unwrap().count();
        assert_eq!(bak, 1);
        let (_, after) = db.compact().unwrap();
        assert!(after > 0);
    }

    #[test]
    fn decode_cp949_and_bom() {
        assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF, b'a']).0, "a");
        let (s, cp) = decode_text(&[0xB0, 0xA1]); // '가' in CP949
        assert_eq!(s, "가");
        assert!(cp);
    }

    fn manual() -> Filter {
        Filter { view: View::All, tags: vec![], query: String::new(), sort: Sort::Manual }
    }
    fn order(db: &Db) -> Vec<i64> {
        db.list_docs(&manual()).unwrap().iter().map(|r| r.id).collect()
    }
    fn mk(db: &Db, t: &str) -> i64 {
        let id = db.new_doc(None).unwrap();
        db.save_doc(id, t, "x").unwrap();
        id
    }

    #[test]
    fn migrate_v1_to_v2() {
        let path = tmp("v1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
            c.execute_batch(SCHEMA_V1).unwrap();
            for (i, u) in [(1, 100), (2, 300), (3, 200)] {
                c.execute(
                    "INSERT INTO docs(id,title,body,created,updated) VALUES(?,?,'b',1,?)",
                    params![i, format!("d{i}"), u],
                )
                .unwrap();
            }
        }
        let db = Db::open(&path).unwrap();
        let v: String = db.conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "2");
        assert_eq!(order(&db), vec![2, 3, 1]); // 최근 수정순 순위
        let n = db.new_doc(None).unwrap();
        assert_eq!(order(&db)[0], n);
        drop(db);
        assert!(Db::open(&path).is_ok()); // 다시 열어도 문제 없음
        // 더 높은 스키마는 거부
        let c = Connection::open(&path).unwrap();
        c.execute("UPDATE meta SET value='99' WHERE key='schema'", []).unwrap();
        drop(c);
        assert!(Db::open(&path).is_err());
    }

    #[test]
    fn manual_new_doc_on_top_and_reorder() {
        let db = Db::open(&tmp("reorder")).unwrap();
        let a = mk(&db, "a");
        let b = mk(&db, "b");
        let c = mk(&db, "c");
        assert_eq!(order(&db), vec![c, b, a]); // 새 문서가 맨 위
        db.reorder(a, Some(c)).unwrap(); // 앞으로
        assert_eq!(order(&db), vec![a, c, b]);
        db.reorder(a, Some(b)).unwrap(); // 가운데로
        assert_eq!(order(&db), vec![c, a, b]);
        db.reorder(c, None).unwrap(); // 맨 끝
        assert_eq!(order(&db), vec![a, b, c]);
        db.reorder(c, Some(c)).unwrap(); // 자기 앞: 변화 없음
        assert_eq!(order(&db), vec![a, b, c]);
        let d = mk(&db, "d");
        assert_eq!(order(&db)[0], d);
    }

    #[test]
    fn reorder_keeps_updated_and_respects_pins() {
        let db = Db::open(&tmp("reorder2")).unwrap();
        let a = mk(&db, "a");
        let b = mk(&db, "b");
        let c = mk(&db, "c");
        db.conn.execute("UPDATE docs SET updated=12345", []).unwrap();
        db.reorder(a, Some(c)).unwrap();
        let ups: Vec<i64> = db.list_docs(&manual()).unwrap().iter().map(|r| r.updated).collect();
        assert!(ups.iter().all(|u| *u == 12345));
        // 고정: 항상 맨 위, 경계 넘기 불가
        db.set_pinned(b, true).unwrap();
        assert_eq!(order(&db), vec![b, a, c]);
        assert!(db.reorder(a, Some(b)).is_err());
        assert!(db.reorder(b, Some(a)).is_err());
        let d = mk(&db, "d");
        db.set_pinned(d, true).unwrap();
        db.reorder(b, None).unwrap(); // 고정 구역 맨 끝 → 그래도 고정 구역 안
        assert_eq!(order(&db), vec![d, b, a, c]);
        // plan_reorder: 위/아래 계산
        let rows = db.list_docs(&manual()).unwrap();
        assert_eq!(Db::plan_reorder(&rows, c, a, false).unwrap(), (c, Some(a)));
        assert_eq!(Db::plan_reorder(&rows, d, b, true).unwrap(), (d, None));
        assert_eq!(Db::plan_reorder(&rows, c, a, true).unwrap(), (c, Some(c)));
        assert!(Db::plan_reorder(&rows, c, d, false).is_err());
    }

    #[test]
    fn renumber_when_gap_exhausted() {
        let db = Db::open(&tmp("renum")).unwrap();
        let a = mk(&db, "a");
        let b = mk(&db, "b");
        let c = mk(&db, "c");
        db.conn.execute("UPDATE docs SET pos=id", []).unwrap(); // 촘촘한 값
        assert_eq!(order(&db), vec![a, b, c]);
        db.reorder(c, Some(b)).unwrap(); // b.pos와 a.pos 사이 간격 없음 → 재번호
        assert_eq!(order(&db), vec![a, c, b]);
        let ps: Vec<i64> = db.conn.prepare("SELECT pos FROM docs ORDER BY pos").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        assert!(ps.windows(2).all(|w| w[1] - w[0] >= 2));
        db.renumber().unwrap();
        assert_eq!(order(&db), vec![a, c, b]);
    }
}

#[cfg(test)]
mod wal_tests {
    use super::*;

    #[test]
    fn wal_grows_until_checkpoint_then_truncates() {
        let dir = std::env::temp_dir().join(format!("ak_wal_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Db::open(&dir.join("v.db")).unwrap();
        let id = db.new_doc(None).unwrap();
        for i in 0..50 {
            db.save_doc(id, "t", &format!("본문 {i} {}", "가".repeat(2000))).unwrap();
        }
        assert!(db.wal_size() > 0, "저장은 로그에 쌓인다(자동 병합 없음)");
        db.checkpoint().unwrap();
        assert_eq!(db.wal_size(), 0, "병합 후 로그가 비워진다");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
