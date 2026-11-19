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

/// 조각 이름은 올린 세대(gen)를 넣어 만든다. 새 조각이 옛 저장본의 조각을 덮어쓰지 않으므로,
/// 올리는 도중에 끊겨도 manifest가 가리키는 마지막 정상 저장본은 그대로 남는다.
pub fn part_name(generation: i64, i: usize) -> String {
    format!("sync-vault.{generation}.{i:04}.part")
}

/// 옛 형식(`sync-vault.0000.part`)과 세대 형식(`sync-vault.<세대>.0000.part`)을 모두 알아본다.
fn is_part_name(n: &str) -> bool {
    n.strip_prefix("sync-vault.").and_then(|r| r.strip_suffix(".part")).map_or(false, |m| {
        let segs: Vec<&str> = m.split('.').collect();
        (segs.len() == 1 || segs.len() == 2) && segs.iter().all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
    })
}

fn is_our_tmp(n: &str) -> bool {
    n.strip_suffix(".tmp").map_or(false, |b| is_part_name(b) || b == MANIFEST)
}

/// 이 컴퓨터를 나타내는 기기 ID.
pub fn device_id() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "unknown".into())
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

/// 동시에 올리는 것을 막는 잠금 파일. 이 시간(초)이 지나면 비정상 종료로 보고 넘겨받는다.
pub const LOCK: &str = "sync-vault.lock";
const LOCK_STALE_SECS: i64 = 20 * 60;

/// 잠금을 잡고 있는 동안만 올릴 수 있다. 값이 버려지면 잠금 파일을 지운다.
struct UploadLock {
    path: PathBuf,
}

impl Drop for UploadLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_lock(dir: &Path, device: &str, now: i64) -> Result<UploadLock, String> {
    let path = dir.join(LOCK);
    for _ in 0..2 {
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{device}\n{now}");
                let _ = f.sync_all();
                return Ok(UploadLock { path });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let s = fs::read_to_string(&path).unwrap_or_default();
                let mut it = s.lines();
                let owner = it.next().unwrap_or("").to_string();
                let at: i64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
                // 다른 기기가 최근에 잡은 잠금만 존중한다. 오래됐거나 읽을 수 없거나 우리 것이면 넘겨받는다.
                if !owner.is_empty() && owner != device && (now - at).abs() <= LOCK_STALE_SECS {
                    return Err(format!("다른 기기({owner})가 올리는 중입니다. 잠시 뒤 다시 시도하세요"));
                }
                let _ = fs::remove_file(&path);
            }
            Err(e) => return Err(format!("동기화 잠금을 만들 수 없음: {e}")),
        }
    }
    Err("동기화 잠금을 얻지 못했습니다".into())
}

/// 스냅숏 파일을 조각으로 나눠 폴더에 쓴다. 안 바뀐 조각은 건너뛰고, manifest는 마지막에 쓴다.
pub fn push_file(snapshot: &Path, dir: &Path, device: &str, now: i64, chunk: usize) -> Result<WriteStats, String> {
    let chunk = chunk.max(1);
    fs::create_dir_all(dir).map_err(|x| format!("동기화 폴더를 만들 수 없음: {x}"))?;
    let _lock = acquire_lock(dir, device, now)?;
    let old = read_manifest(dir).ok().flatten();
    // 이번 세대 번호: 옛 manifest의 조각 이름과 겹치지 않는 값(같은 초에 다시 올려도 덮어쓰지 않도록)
    let mut generation = now;
    if let Some(o) = &old {
        while o.parts.iter().any(|p| p.name.starts_with(&format!("sync-vault.{generation}."))) {
            generation += 1;
        }
    }
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
        let sha = sha_hex(data);
        // 안 바뀐 조각은 옛 이름 그대로 가리키고, 바뀐 조각만 새 세대 이름으로 쓴다.
        let same = old.as_ref().and_then(|m| m.parts.get(i)).filter(|p| {
            p.sha256 == sha && p.len == n as u64 && fs::metadata(dir.join(&p.name)).map_or(false, |md| md.len() == n as u64)
        });
        let name = match same {
            Some(p) => p.name.clone(),
            None => part_name(generation, i),
        };
        if same.is_some() {
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
/// 불러오기 전 백업(before-restore-*.db)은 이 개수만 남긴다(복원할 때마다 DB 크기만큼 쌓이는 것을 막는다).
const RESTORE_BACKUPS_KEEP: usize = 3;

fn prune_restore_backups(dir: &Path, keep: usize) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("before-restore-") && n.ends_with(".db"))
        .collect();
    names.sort(); // 이름에 시각이 들어 있어 사전순 = 시간순
    let drop = names.len().saturating_sub(keep);
    for n in names.into_iter().take(drop) {
        let _ = fs::remove_file(dir.join(n));
    }
}

/// 두 번째 값은 불러온 저장본의 시각(saved_at): 호출한 쪽이 이 값을 마지막 동기화 시각으로 기록해야
/// 방금 불러온 내용을 "다른 기기가 올린 새 내용"으로 다시 오해하지 않는다.
pub fn restore_from_dir(db: &mut Db, dir: &Path) -> Result<(PathBuf, i64), String> {
    let m = read_manifest(dir)?.ok_or_else(|| "그 폴더에 동기화 데이터(manifest)가 없습니다".to_string())?;
    let base = db.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    fs::create_dir_all(&base).map_err(|x| x.to_string())?;
    let tmp = base.join("sync-restore.tmp.db");
    assemble(dir, &m, &tmp)?;
    let res = restore_from_file(db, &tmp);
    let _ = fs::remove_file(&tmp);
    res.map(|bak| (bak, m.saved_at))
}

/// 이미 받아 둔 DB 파일(`tmp`)을 검증해 현재 DB 에 반영한다(WebDAV 불러오기도 이것을 쓴다). 반영 전 DB 를 backup 에 보관하고 그 경로를 돌려준다.
/// `tmp` 파일은 지우지 않는다(호출한 쪽이 정리).
pub fn restore_from_file(db: &mut Db, tmp: &Path) -> Result<PathBuf, String> {
    let base = db.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let tmp = tmp.to_path_buf();
    {
        check_db_file(&tmp)?;
        let bdir = base.join("backup");
        // 밀리초까지 넣어 같은 초에 두 번 불러와도 먼저 만든 백업을 지우지 않는다.
        let bak = bdir.join(format!("before-restore-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S%3f")));
        db.snapshot_to(&bak).map_err(|x| format!("불러오기 전 백업 실패: {x}"))?;
        if let Err(x) = db.conn.restore(rusqlite::MAIN_DB, &tmp, None::<fn(rusqlite::backup::Progress)>) {
            return Err(format!("불러오기 실패: {x} (불러오기 전 상태는 {} 에 보관되어 있습니다)", bak.display()));
        }
        // 구버전(v1) 저장본이면 지금 실행 중인 연결의 구조를 최신으로 올린다(다시 시작하지 않아도 새 문서·정렬이 동작하도록).
        db.migrate().map_err(|x| format!("불러온 DB를 최신 구조로 바꾸지 못했습니다: {x} (불러오기 전 상태: {})", bak.display()))?;
        prune_restore_backups(&bdir, RESTORE_BACKUPS_KEEP);
        Ok(bak)
    }
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
    fn new_generation_never_overwrites_live_parts() {
        let t = tmpdir("gen");
        let src = t.join("snap.bin");
        let a = pseudo(3 * 1024 * 1024, 7);
        fs::write(&src, &a).unwrap();
        let dir = t.join("sync");
        push_file(&src, &dir, "A", 10, TC).unwrap();
        let m1 = read_manifest(&dir).unwrap().unwrap();
        // 같은 초에 내용을 바꿔 다시 올려도 옛 조각 이름과 겹치지 않는다
        fs::write(&src, pseudo(3 * 1024 * 1024, 8)).unwrap();
        push_file(&src, &dir, "A", 10, TC).unwrap();
        let m2 = read_manifest(&dir).unwrap().unwrap();
        assert!(m1.parts.iter().all(|p| m2.parts.iter().all(|q| q.name != p.name)));
        // 올리다 끊겨 manifest 없이 새 조각만 남은 상태: 마지막 정상 저장본은 그대로 불러온다
        fs::write(dir.join(part_name(99, 0)), b"partial").unwrap();
        let out = t.join("o");
        assemble(&dir, &m2, &out).unwrap();
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn upload_lock_blocks_other_device_but_not_stale_or_own() {
        let t = tmpdir("lock");
        let src = t.join("snap.bin");
        fs::write(&src, pseudo(1000, 9)).unwrap();
        let dir = t.join("sync");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(LOCK), "PC-B\n1000").unwrap();
        let e = push_file(&src, &dir, "PC-A", 1100, TC).unwrap_err();
        assert!(e.contains("PC-B"), "{e}");
        assert!(dir.join(LOCK).exists(), "남의 잠금은 지우지 않는다");
        // 오래된 잠금은 넘겨받고, 끝나면 잠금이 남지 않는다
        push_file(&src, &dir, "PC-A", 1000 + LOCK_STALE_SECS + 5, TC).unwrap();
        assert!(!dir.join(LOCK).exists());
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
        let p1 = dir.join(&m.parts[1].name);
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
        fs::remove_file(dir.join(&m.parts[2].name)).unwrap();
        fs::remove_file(&p1).unwrap();
        let e = assemble(&dir, &m, &out).unwrap_err();
        assert!(e.contains("2번째 조각이 없음"), "{e}");
        // 옛 조각 정리 + 다른 파일 보존
        fs::write(dir.join("other.txt"), "keep").unwrap();
        fs::write(&src, pseudo(1000, 4)).unwrap();
        let st = push_file(&src, &dir, "A", 5, TC).unwrap();
        assert_eq!(st.total, 1);
        let m2 = read_manifest(&dir).unwrap().unwrap();
        assert!(dir.join(&m2.parts[0].name).exists());
        assert!(!dir.join(&m.parts[1].name).exists() && !dir.join(&m.parts[2].name).exists());
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

    /// 이미지를 하나씩 추가할 때 동기화 조각이 얼마나 바뀌는지 잰다(512KB 조각).
    /// `cargo test measure_image_churn -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn measure_image_churn() {
        let t = tmpdir("imgchurn");
        let src = t.join("a").join("vault.db");
        let db = Db::open(&src).unwrap();
        let mut docs = Vec::new();
        for i in 0..200 {
            let id = db.new_doc(None).unwrap();
            let body: String = (0..40).map(|k| format!("문서 {i} 줄 {k}: 가나다라마바사 abcdefg 0123456789\n")).collect();
            db.save_doc(id, &format!("문서 {i}"), &body).unwrap();
            docs.push(id);
        }
        db.checkpoint().unwrap();
        let snap = t.join("snap.db");
        let dir = t.join("sync");
        Db::snapshot_file(&src, &snap).unwrap();
        let first = push_file(&snap, &dir, "A", 1, CHUNK).unwrap();
        println!("그림 없음: 총 {}조각 (첫 올리기 {}개)", first.total, first.written);
        const PRE: u32 = 40; // 먼저 그림 40장(약 3.4MB)을 넣어 둔 상태에서 한 장씩 더하는 비용을 잰다
        for k in 0..PRE + 4 {
            // 사진 비슷한 800×600 그림(해마다 다른 잡음)
            let rgba: Vec<u8> = (0..800 * 600)
                .flat_map(|p| {
                    let (x, y) = (p % 800, p / 800);
                    let n = ((x * 2654435761u32 as usize) ^ (y * 40503 + k as usize * 977)) % 29;
                    [((x * 255 / 800) as u8).saturating_add(n as u8), ((y * 255 / 600) as u8).saturating_add(n as u8), 128, 255]
                })
                .collect();
            let r = crate::img::process_rgba(&rgba, 800, 600, "t", 1600, 80).unwrap();
            db.put_image(&r.hash, &r.webp, r.w, r.h).unwrap();
            let id = docs[100];
            let body = format!("그림 {k}\n\n{}\n", crate::md::format_image_line("t", &r.hash, r.w, r.h));
            db.save_doc(id, "문서 100", &body).unwrap();
            if k + 1 < PRE {
                continue;
            }
            db.checkpoint().unwrap();
            Db::snapshot_file(&src, &snap).unwrap();
            let p = push_file(&snap, &dir, "A", 2 + k as i64, CHUNK).unwrap();
            println!("그림 {}번째({}KB) 추가: 총 {}조각 중 {}개 갱신", k + 1, r.webp.len() / 1024, p.total, p.written);
        }
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn snapshot_file_sees_unmerged_log_and_rev_counter() {
        let t = tmpdir("snapfile");
        let p = t.join("a").join("vault.db");
        let db = Db::open(&p).unwrap();
        let id = db.new_doc(None).unwrap();
        db.save_doc(id, "병합 전 문서", "본문").unwrap(); // 체크포인트 없음(WAL에만 있음)
        // 변경 카운터: updated를 건드리지 않는 변경(고정·태그·분류·영구 삭제)도 값을 올린다
        let r0 = db.rev();
        assert!(r0 > 0);
        db.set_pinned(id, true).unwrap();
        let r1 = db.rev();
        assert!(r1 > r0, "고정");
        db.add_tag(id, "시험").unwrap();
        let r2 = db.rev();
        assert!(r2 > r1, "태그");
        let c = db.add_category(None, "분류").unwrap();
        let r3 = db.rev();
        assert!(r3 > r2, "분류 추가");
        db.rename_category(c, "분류2").unwrap();
        assert!(db.rev() > r3, "분류 이름");
        // 휴지통이 아닌 문서는 purge로 지워지지 않고, 이미 휴지통인 문서의 시각은 다시 찍히지 않는다
        db.purge(id).unwrap();
        assert!(db.get_doc(id).unwrap().is_some());
        db.trash(id).unwrap();
        let t1 = db.get_doc(id).unwrap().unwrap();
        assert!(t1.trashed);
        db.restore(id).unwrap(); // 아래 스냅숏 확인을 위해 되돌린다
        let snap = t.join("snap.db");
        Db::snapshot_file(&p, &snap).unwrap(); // 다른 연결(읽기 전용)로 만든다
        let copy = Db::open(&snap).unwrap();
        assert!(copy.find_by_title("병합 전 문서").unwrap().is_some());
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn delete_category_refuses_name_clash() {
        let t = tmpdir("catclash");
        let db = Db::open(&t.join("a").join("vault.db")).unwrap();
        let p = db.add_category(None, "P").unwrap();
        let _a = db.add_category(Some(p), "A").unwrap();
        let x = db.add_category(Some(p), "X").unwrap();
        let a2 = db.add_category(Some(x), "a").unwrap(); // 대소문자만 다른 같은 이름
        let before = db.parent_map().unwrap().len();
        assert!(db.delete_category(x).is_err());
        assert_eq!(db.parent_map().unwrap().len(), before, "삭제되지 않아야 한다");
        // 이름을 바꾸면 삭제할 수 있고 하위 분류는 P 아래로 올라간다
        db.rename_category(a2, "B").unwrap();
        db.delete_category(x).unwrap();
        assert_eq!(db.parent_map().unwrap().get(&a2).copied().flatten(), Some(p));
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn restore_v1_snapshot_is_migrated_in_place() {
        let t = tmpdir("v1restore");
        // v1(pos 컬럼 없음) 저장본을 만든다
        let v1 = t.join("v1.db");
        {
            let c = Connection::open(&v1).unwrap();
            c.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
            c.execute_batch(crate::db::SCHEMA_V1).unwrap();
            c.execute("INSERT INTO docs(title,body,created,updated) VALUES('옛 문서','x',1,1)", []).unwrap();
        }
        let dir = t.join("sync");
        push_file(&v1, &dir, "A", 5, 4096).unwrap();
        let mut db = Db::open(&t.join("b").join("vault.db")).unwrap();
        let (_, at) = restore_from_dir(&mut db, &dir).unwrap();
        assert_eq!(at, 5);
        // 다시 시작하지 않아도 새 문서 만들기(pos 사용)가 동작해야 한다
        let id = db.new_doc(None).unwrap();
        assert!(id > 0);
        assert!(db.find_by_title("옛 문서").unwrap().is_some());
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
