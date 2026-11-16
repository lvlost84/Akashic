//! 동기화 저장소: DB 스냅숏을 512KB 조각으로 나눠 폴더에 두고(manifest + 조각), 다시 불러온다.
//! UI와 무관한 순수 로직.
use crate::db::{Db, SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 조각 크기. 실측(20MB DB, 문서 하나 수정): 2MB 조각은 약 6~12MB, 512KB 조각은 약 2.5MB만 다시 올라간다.
pub const CHUNK: usize = 512 * 1024;
pub const MANIFEST: &str = "sync-vault.manifest.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PartInfo {
    pub name: String,
    pub len: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Manifest {
    pub version: u32,
    pub device: String,
    pub saved_at: i64,
    pub chunk_size: u64,
    pub total_len: u64,
    pub sha256: String,
    pub parts: Vec<PartInfo>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WriteStats {
    pub total: usize,
    pub written: usize,
    pub skipped: usize,
    pub removed: usize,
    pub saved_at: i64,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

pub fn part_name(i: usize) -> String {
    format!("sync-vault.{i:04}.part")
}

fn is_part_name(n: &str) -> bool {
    n.strip_prefix("sync-vault.")
        .and_then(|r| r.strip_suffix(".part"))
        .map_or(false, |m| !m.is_empty() && m.bytes().all(|c| c.is_ascii_digit()))
}

fn is_our_tmp(n: &str) -> bool {
    n.strip_suffix(".tmp").map_or(false, |b| is_part_name(b) || b == MANIFEST)
}

/// 이 컴퓨터를 나타내는 기기 ID.
pub fn device_id() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".into())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = f.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

fn write_atomic(dir: &Path, name: &str, data: &[u8]) -> Result<(), String> {
    let tmp = dir.join(format!("{name}.tmp"));
    let r = (|| -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, dir.join(name))
    })();
    if let Err(x) = r {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{name} 쓰기 실패: {x}"));
    }
    Ok(())
}

pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>, String> {
    let p = dir.join(MANIFEST);
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p).map_err(|x| format!("manifest를 읽을 수 없음: {x}"))?;
    serde_json::from_str(&s).map(Some).map_err(|x| format!("manifest가 깨짐: {x}"))
}

/// 스냅숏 파일을 조각으로 나눠 폴더에 쓴다. 안 바뀐 조각은 건너뛰고, manifest는 마지막에 쓴다.
pub fn push_file(snapshot: &Path, dir: &Path, device: &str, now: i64, chunk: usize) -> Result<WriteStats, String> {
    let chunk = chunk.max(1);
    fs::create_dir_all(dir).map_err(|x| format!("동기화 폴더를 만들 수 없음: {x}"))?;
    let old = read_manifest(dir).ok().flatten();
    let mut f = File::open(snapshot).map_err(|x| format!("스냅숏을 열 수 없음: {x}"))?;
    let mut buf = vec![0u8; chunk];
    let mut all = Sha256::new();
    let mut parts: Vec<PartInfo> = Vec::new();
    let mut st = WriteStats { saved_at: now, ..Default::default() };
    let mut total = 0u64;
    loop {
        let n = read_full(&mut f, &mut buf).map_err(|x| format!("스냅숏 읽기 실패: {x}"))?;
        if n == 0 {
            break;
        }
        let data = &buf[..n];
        all.update(data);
        total += n as u64;
        let i = parts.len();
        let name = part_name(i);
        let sha = sha_hex(data);
        let same = old.as_ref().and_then(|m| m.parts.get(i)).map_or(false, |p| {
            p.name == name
                && p.sha256 == sha
                && p.len == n as u64
                && fs::metadata(dir.join(&name)).map_or(false, |md| md.len() == n as u64)
        });
        if same {
            st.skipped += 1;
        } else {
            write_atomic(dir, &name, data)?;
            st.written += 1;
        }
        parts.push(PartInfo { name, len: n as u64, sha256: sha });
        if n < chunk {
            break;
        }
    }
    let m = Manifest {
        version: 1,
        device: device.to_string(),
        saved_at: now,
        chunk_size: chunk as u64,
        total_len: total,
        sha256: hex(&all.finalize()),
        parts,
    };
    let json = serde_json::to_string_pretty(&m).map_err(|x| x.to_string())?;
    write_atomic(dir, MANIFEST, json.as_bytes())?;
    st.total = m.parts.len();
    // 더 이상 필요 없는 옛 조각(과 우리가 남긴 임시 파일)만 정리. 다른 파일은 건드리지 않는다.
    let keep: HashSet<&str> = m.parts.iter().map(|p| p.name.as_str()).collect();
    if let Ok(rd) = fs::read_dir(dir) {
        for ent in rd.flatten() {
            let n = ent.file_name().to_string_lossy().into_owned();
            if (is_part_name(&n) && !keep.contains(n.as_str())) || is_our_tmp(&n) {
                if fs::remove_file(ent.path()).is_ok() {
                    st.removed += 1;
                }
            }
        }
    }
    Ok(st)
}

/// 모든 조각을 검증하며 이어 붙여 `dest` 에 쓴다. 문제가 있으면 dest 를 남기지 않고 오류.
pub fn assemble(dir: &Path, m: &Manifest, dest: &Path) -> Result<(), String> {
    let r = assemble_inner(dir, m, dest);
    if r.is_err() {
        let _ = fs::remove_file(dest);
    }
    r
}

fn assemble_inner(dir: &Path, m: &Manifest, dest: &Path) -> Result<(), String> {
    let mut out = File::create(dest).map_err(|x| format!("임시 파일을 만들 수 없음: {x}"))?;
    let mut all = Sha256::new();
    let mut total = 0u64;
    for (i, p) in m.parts.iter().enumerate() {
        let n = i + 1;
        let path = dir.join(&p.name);
        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(_) => return Err(format!("{n}번째 조각이 없음 ({})", p.name)),
        };
        if data.len() as u64 != p.len {
            return Err(format!("{n}번째 조각이 깨짐 (크기 불일치: {})", p.name));
        }
        if sha_hex(&data) != p.sha256 {
            return Err(format!("{n}번째 조각이 깨짐 (내용 불일치: {})", p.name));
        }
        all.update(&data);
        total += data.len() as u64;
        out.write_all(&data).map_err(|x| format!("임시 파일 쓰기 실패: {x}"))?;
    }
    if total != m.total_len || hex(&all.finalize()) != m.sha256 {
        return Err("조각을 합친 결과가 manifest와 다릅니다 (전체 해시 불일치)".into());
    }
    out.sync_all().map_err(|x| x.to_string())
}

/// 임시 DB 검사: 열림·스키마 버전·무결성.
pub fn check_db_file(path: &Path) -> Result<(), String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|x| format!("불러온 DB를 열 수 없음: {x}"))?;
    let ver: i64 = conn
        .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get::<_, String>(0))
        .map_err(|_| "아카식 DB가 아닙니다 (스키마 정보 없음)".to_string())?
        .parse()
        .unwrap_or(0);
    if ver > SCHEMA_VERSION {
        return Err(format!("더 새로운 버전의 DB입니다(스키마 {ver}). 이 프로그램을 업데이트하세요."));
    }
    if ver < 1 {
        return Err("지원하지 않는 스키마입니다".into());
    }
    let tmp_db = Db { conn, path: path.to_path_buf() };
    if !tmp_db.integrity_ok() {
        return Err("불러올 DB의 무결성 검사에 실패했습니다".into());
    }
    tmp_db.conn.query_row("SELECT count(*) FROM docs", [], |r| r.get::<_, i64>(0)).map_err(|_| "문서 테이블이 없습니다".to_string())?;
    Ok(())
}

/// 폴더의 스냅숏을 검증해 현재 DB 에 반영. 반영 전 현재 DB 를 backup\before-restore-*.db 로 보관하고 그 경로를 돌려준다.
/// 두 번째 값은 불러온 저장본의 시각(saved_at): 호출한 쪽이 이 값을 마지막 동기화 시각으로 기록해야
/// 방금 불러온 내용을 "다른 기기가 올린 새 내용"으로 다시 오해하지 않는다.
pub fn restore_from_dir(db: &mut Db, dir: &Path) -> Result<(PathBuf, i64), String> {
    let m = read_manifest(dir)?.ok_or_else(|| "그 폴더에 동기화 데이터(manifest)가 없습니다".to_string())?;
    let base = db.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    fs::create_dir_all(&base).map_err(|x| x.to_string())?;
    let tmp = base.join("sync-restore.tmp.db");
    assemble(dir, &m, &tmp)?;
    let res = (|| {
        check_db_file(&tmp)?;
        let bdir = base.join("backup");
        let bak = bdir.join(format!("before-restore-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        db.snapshot_to(&bak).map_err(|x| format!("불러오기 전 백업 실패: {x}"))?;
        db.conn
            .restore(rusqlite::MAIN_DB, &tmp, None::<fn(rusqlite::backup::Progress)>)
            .map_err(|x| format!("불러오기 실패: {x}"))?;
        Ok((bak, m.saved_at))
    })();
    let _ = fs::remove_file(&tmp);
    res
}

/// 다른 기기가 우리의 마지막 동기화 이후에 올린 manifest 인가.
pub fn foreign_newer(m: &Manifest, my_device: &str, last_sync: i64) -> bool {
    m.device != my_device && m.saved_at > last_sync
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험 전용 조각 크기(실제 값과 무관하게 같은 시험이 되도록 고정)
    const TC: usize = 2 * 1024 * 1024;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("akashic-sync-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn pseudo(n: usize, seed: u64) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 33) as u8
            })
            .collect()
    }

    #[test]
    fn roundtrip_multi_chunk() {
        let t = tmpdir("rt");
        let src = t.join("snap.bin");
        let data = pseudo(5 * 1024 * 1024 + 123, 1);
        fs::write(&src, &data).unwrap();
        let dir = t.join("sync");
        let st = push_file(&src, &dir, "PC-A", 100, TC).unwrap();
        assert_eq!(st.total, 3);
        assert_eq!(st.written, 3);
        let m = read_manifest(&dir).unwrap().unwrap();
        assert_eq!(m.parts.len(), 3);
        assert_eq!(m.device, "PC-A");
        let out = t.join("out.bin");
        assemble(&dir, &m, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), data);
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn unchanged_not_rewritten_and_partial() {
        let t = tmpdir("skip");
        let src = t.join("snap.bin");
        let mut data = pseudo(5 * 1024 * 1024, 2);
        fs::write(&src, &data).unwrap();
        let dir = t.join("sync");
        push_file(&src, &dir, "A", 1, TC).unwrap();
        let st = push_file(&src, &dir, "A", 2, TC).unwrap();
        assert_eq!((st.written, st.skipped), (0, 3));
        data[TC + 5] ^= 0xFF; // 두 번째 조각만 변경
        fs::write(&src, &data).unwrap();
        let st = push_file(&src, &dir, "A", 3, TC).unwrap();
        assert_eq!((st.written, st.skipped), (1, 2));
        let m = read_manifest(&dir).unwrap().unwrap();
        let out = t.join("o");
        assemble(&dir, &m, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), data);
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn detects_missing_corrupt_size_and_cleans_old() {
        let t = tmpdir("bad");
        let src = t.join("snap.bin");
        fs::write(&src, pseudo(5 * 1024 * 1024, 3)).unwrap();
        let dir = t.join("sync");
        push_file(&src, &dir, "A", 1, TC).unwrap();
        let m = read_manifest(&dir).unwrap().unwrap();
        let out = t.join("o");
        // 손상
        let p1 = dir.join(part_name(1));
        let mut b = fs::read(&p1).unwrap();
        b[10] ^= 1;
        fs::write(&p1, &b).unwrap();
        let e = assemble(&dir, &m, &out).unwrap_err();
        assert!(e.contains("2번째 조각이 깨짐"), "{e}");
        assert!(!out.exists());
        // 크기 불일치
        fs::write(&p1, b"x").unwrap();
        let e = assemble(&dir, &m, &out).unwrap_err();
        assert!(e.contains("크기"), "{e}");
        // 누락
        fs::remove_file(dir.join(part_name(2))).unwrap();
        fs::remove_file(&p1).unwrap();
        let e = assemble(&dir, &m, &out).unwrap_err();
        assert!(e.contains("2번째 조각이 없음"), "{e}");
        // 옛 조각 정리 + 다른 파일 보존
        fs::write(dir.join("other.txt"), "keep").unwrap();
        fs::write(&src, pseudo(1000, 4)).unwrap();
        let st = push_file(&src, &dir, "A", 5, TC).unwrap();
        assert_eq!(st.total, 1);
        assert!(dir.join(part_name(0)).exists());
        assert!(!dir.join(part_name(1)).exists() && !dir.join(part_name(2)).exists());
        assert!(dir.join("other.txt").exists());
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn conflict_rule() {
        let m = Manifest { version: 1, device: "B".into(), saved_at: 200, chunk_size: 1, total_len: 0, sha256: String::new(), parts: vec![] };
        assert!(foreign_newer(&m, "A", 100));
        assert!(!foreign_newer(&m, "A", 200));
        assert!(!foreign_newer(&m, "B", 100));
        assert!(foreign_newer(&m, "A", 0));
    }

    #[test]
    fn end_to_end_db() {
        let t = tmpdir("e2e");
        let db1 = Db::open(&t.join("a").join("vault.db")).unwrap();
        for i in 0..3 {
            let id = db1.new_doc(None).unwrap();
            db1.save_doc(id, &format!("문서{i}"), &format!("본문 {i}")).unwrap();
        }
        let snap = t.join("snap.db");
        db1.snapshot_to(&snap).unwrap();
        let dir = t.join("sync");
        push_file(&snap, &dir, "A", 1, 4096).unwrap();
        assert!(read_manifest(&dir).unwrap().unwrap().parts.len() > 1);

        let mut db2 = Db::open(&t.join("b").join("vault.db")).unwrap();
        let id = db2.new_doc(None).unwrap();
        db2.save_doc(id, "다른 것", "x").unwrap();
        let (bak, at) = restore_from_dir(&mut db2, &dir).unwrap();
        assert!(bak.exists());
        // 불러온 직후에는 같은 저장본을 "다른 기기의 새 내용"으로 보지 않는다
        assert_eq!(at, 1);
        assert!(!foreign_newer(&read_manifest(&dir).unwrap().unwrap(), "B", at));
        assert_eq!(db2.all_docs().unwrap().len(), 3);
        assert!(db2.find_by_title("문서1").unwrap().is_some());
        assert!(db2.find_by_title("다른 것").unwrap().is_none());
        assert!(db2.integrity_ok());
        // 이전 DB 는 백업에 남아 있다
        let old = Db::open(&bak).unwrap();
        assert!(old.find_by_title("다른 것").unwrap().is_some());
        let _ = fs::remove_dir_all(&t);
    }

    /// 실측: 실제 DB에서 문서 한 줄을 고쳤을 때 2MB 조각이 몇 개 바뀌는가(VACUUM INTO vs 페이지 그대로 복사).
    /// `cargo test --release measure_chunk_churn -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn measure_chunk_churn() {
        let t = tmpdir("churn");
        let db = Db::open(&t.join("a").join("vault.db")).unwrap();
        let mut ids = Vec::new();
        for i in 0..3000 {
            let id = db.new_doc(None).unwrap();
            let body: String = (0..40).map(|k| format!("문서 {i} 줄 {k}: 가나다라마바사 abcdefg 0123456789\n")).collect();
            db.save_doc(id, &format!("문서 {i}"), &body).unwrap();
            ids.push(id);
        }
        db.checkpoint().unwrap();
        let src = t.join("a").join("vault.db");
        let copy_pages = |dst: &Path| {
            let _ = fs::remove_file(dst);
            let c = Connection::open_with_flags(&src, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let mut d = Connection::open(dst).unwrap();
            let b = rusqlite::backup::Backup::new(&c, &mut d).unwrap();
            b.run_to_completion(1000, std::time::Duration::from_millis(0), None).unwrap();
        };
        for (label, kind, chunk) in [("VACUUM INTO 2MB", 0usize, TC), ("VACUUM INTO 512KB", 0, 512 * 1024), ("VACUUM INTO 128KB", 0, 128 * 1024), ("페이지 복사 128KB", 1, 128 * 1024)] {
            let dir = t.join(format!("sync{label}").replace(" ", "_"));
            let snap = t.join(format!("snap{kind}.db"));
            let take = |snap: &Path| if kind == 0 { Db::snapshot_file(&src, snap).unwrap() } else { copy_pages(snap) };
            take(&snap);
            let first = push_file(&snap, &dir, "A", 1, chunk).unwrap();
            // 1) 다른 문서 줄 하나를 같은 길이로 고침
            let mid = ids[1500];
            db.save_doc(mid, "문서 1500", &format!("{}", format!("{label}").repeat(300))).unwrap();
            db.checkpoint().unwrap();
            take(&snap);
            let a = push_file(&snap, &dir, "A", 2, chunk).unwrap();
            // 2) 문서 하나를 새로 만듦
            let id = db.new_doc(None).unwrap();
            db.save_doc(id, "새 문서", "새로 쓴 내용").unwrap();
            db.checkpoint().unwrap();
            take(&snap);
            let b = push_file(&snap, &dir, "A", 3, chunk).unwrap();
            println!("[{label}] 총 {}조각 · 문서 고침: {}개 갱신 · 새 문서: {}개 갱신 (첫 올리기 {}개)", first.total, a.written, b.written, first.written);
        }
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn snapshot_file_sees_unmerged_log_and_change_time() {
        let t = tmpdir("snapfile");
        let p = t.join("a").join("vault.db");
        let db = Db::open(&p).unwrap();
        let id = db.new_doc(None).unwrap();
        db.save_doc(id, "병합 전 문서", "본문").unwrap(); // 체크포인트 없음(WAL에만 있음)
        assert!(db.latest_change() > 0);
        let snap = t.join("snap.db");
        Db::snapshot_file(&p, &snap).unwrap(); // 다른 연결(읽기 전용)로 만든다
        let copy = Db::open(&snap).unwrap();
        assert!(copy.find_by_title("병합 전 문서").unwrap().is_some());
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn rejects_newer_schema_and_missing_manifest() {
        let t = tmpdir("ver");
        let db1 = Db::open(&t.join("a").join("vault.db")).unwrap();
        db1.conn.execute("UPDATE meta SET value='99' WHERE key='schema'", []).unwrap();
        let snap = t.join("snap.db");
        db1.snapshot_to(&snap).unwrap();
        let dir = t.join("sync");
        push_file(&snap, &dir, "A", 1, TC).unwrap();
        let mut db2 = Db::open(&t.join("b").join("vault.db")).unwrap();
        let e = restore_from_dir(&mut db2, &dir).unwrap_err();
        assert!(e.contains("새로운 버전"), "{e}");
        assert!(restore_from_dir(&mut db2, &t.join("nothing")).is_err());
        assert!(!t.join("b").join("sync-restore.tmp.db").exists());
        let _ = fs::remove_dir_all(&t);
    }
}
