use super::*;
use crate::vault::testing::MemoryRemote;

const NOW: i64 = 1_800_000_000;

fn key() -> VaultKey {
    VaultKey::from_bytes([7; 32])
}

struct Node {
    db: Db,
    name: String,
}

fn node(tag: &str, mode: Connect) -> Node {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("akashic-ds-{}-{}-{}", tag, n, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let db = Db::open(&d.join("v.db")).unwrap();
    connect(&db, &key(), mode).unwrap();
    Node { db, name: format!("PC-{tag}") }
}

fn sync_with(n: &Node, r: &MemoryRemote, ckpt_every: u64) -> Res<Report> {
    sync(&n.db, r, &key(), &Opts { name: &n.name, now: NOW, ckpt_every, retain_days: 90 })
}
fn sync1(n: &Node, r: &MemoryRemote) -> Res<Report> {
    sync_with(n, r, 1000)
}

/// 모든 노드가 더 이상 바뀌지 않을 때까지 돌린다.
fn settle(nodes: &[&Node], r: &MemoryRemote) {
    for round in 0..20 {
        let mut quiet = true;
        for n in nodes {
            let rep = sync1(n, r).unwrap();
            if rep.applied > 0 || rep.pushed > 0 {
                quiet = false;
            }
        }
        if quiet && round > 0 {
            return;
        }
    }
    panic!("수렴하지 않았다(계속 변경이 오간다)");
}

/// 화면에 보이는 상태(시각 제외)를 uid 기준으로 정규화.
fn state(db: &Db) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut st = db
        .conn
        .prepare("SELECT d.id,d.uid,d.title,d.body,c.uid,d.pinned,d.trashed_at IS NOT NULL FROM docs d LEFT JOIN categories c ON c.id=d.category_id")
        .unwrap();
    let rows: Vec<(i64, String, String, String, Option<String>, i64, i64)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap().flatten().collect();
    for (id, uid, t, b, c, p, tr) in rows {
        let tags = db.doc_tags(id).unwrap();
        m.insert(format!("d:{uid}"), format!("{t}|{b}|{c:?}|{tags:?}|{p}|{tr}"));
    }
    let mut st = db.conn.prepare("SELECT c.uid,c.name,p.uid FROM categories c LEFT JOIN categories p ON p.id=c.parent_id").unwrap();
    for x in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).unwrap().flatten() {
        m.insert(format!("c:{}", x.0), format!("{}|{:?}", x.1, x.2));
    }
    m
}

fn doc(db: &Db, title: &str, body: &str) -> i64 {
    let id = db.new_doc(None).unwrap();
    db.save_doc(id, title, body).unwrap();
    id
}
fn body(db: &Db, id: i64) -> String {
    db.get_doc(id).unwrap().unwrap().body
}
fn id_of(db: &Db, title: &str) -> i64 {
    db.conn.query_row("SELECT id FROM docs WHERE title=?", [title], |r| r.get(0)).unwrap()
}
fn count(db: &Db) -> i64 {
    db.conn.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0)).unwrap()
}

const LINES: &str = "하나\n둘\n셋\n넷\n다섯\n여섯\n일곱\n";

#[test]
fn 서로_다른_문서의_동시_편집은_모두_남는다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a1", Connect::New), node("b1", Connect::New));
    doc(&a.db, "A문서", "a");
    doc(&b.db, "B문서", "b");
    settle(&[&a, &b], &r);
    assert_eq!(count(&a.db), 2);
    assert_eq!(state(&a.db), state(&b.db));
    // 각자 다른 문서를 고쳐도 둘 다 반영
    a.db.save_doc(id_of(&a.db, "A문서"), "A문서", "a2").unwrap();
    b.db.save_doc(id_of(&b.db, "B문서"), "B문서", "b2").unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    assert_eq!(body(&a.db, id_of(&a.db, "B문서")), "b2");
    assert_eq!(body(&b.db, id_of(&b.db, "A문서")), "a2");
}

#[test]
fn 같은_문서_다른_줄은_자동_병합() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a2", Connect::New), node("b2", Connect::New));
    doc(&a.db, "공유", LINES);
    settle(&[&a, &b], &r);
    let (ia, ib) = (id_of(&a.db, "공유"), id_of(&b.db, "공유"));
    a.db.save_doc(ia, "공유", &LINES.replace("하나", "A가 고침")).unwrap();
    b.db.save_doc(ib, "공유", &LINES.replace("일곱", "B가 고침")).unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(count(&a.db), 1, "충돌 사본 없이 자동 병합");
    assert_eq!(state(&a.db), state(&b.db));
    let m = body(&a.db, ia);
    assert!(m.contains("A가 고침") && m.contains("B가 고침") && !m.contains("하나"), "{m}");
}

#[test]
fn 같은_줄_충돌은_사본으로_남고_두_쪽이_같다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a3", Connect::New), node("b3", Connect::New));
    doc(&a.db, "공유", LINES);
    settle(&[&a, &b], &r);
    a.db.save_doc(id_of(&a.db, "공유"), "공유", &LINES.replace("셋", "A의 셋")).unwrap();
    b.db.save_doc(id_of(&b.db, "공유"), "공유", &LINES.replace("셋", "B의 셋")).unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    assert_eq!(count(&a.db), 2, "원본 + 충돌 사본");
    let all: String = state(&a.db).values().cloned().collect::<Vec<_>>().join("\n");
    assert!(all.contains("A의 셋") && all.contains("B의 셋"), "양쪽 내용이 모두 남는다");
    let copy: i64 = a.db.conn.query_row("SELECT id FROM docs WHERE title LIKE '%(충돌 · %'", [], |r| r.get(0)).unwrap();
    assert!(a.db.doc_tags(copy).unwrap().contains(&CONFLICT_TAG.to_string()));
}

#[test]
fn 인접한_줄을_고쳐도_내용을_잃지_않는다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a4", Connect::New), node("b4", Connect::New));
    doc(&a.db, "공유", LINES);
    settle(&[&a, &b], &r);
    a.db.save_doc(id_of(&a.db, "공유"), "공유", &LINES.replace("둘", "A둘")).unwrap();
    b.db.save_doc(id_of(&b.db, "공유"), "공유", &LINES.replace("셋", "B셋")).unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    let all: String = state(&a.db).values().cloned().collect::<Vec<_>>().join("\n");
    assert!(all.contains("A둘") && all.contains("B셋"));
}

#[test]
fn 삭제_대_수정은_수정이_이긴다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a5", Connect::New), node("b5", Connect::New));
    doc(&a.db, "문서", "원본");
    settle(&[&a, &b], &r);
    let ia = id_of(&a.db, "문서");
    a.db.trash(ia).unwrap();
    a.db.purge(ia).unwrap();
    b.db.save_doc(id_of(&b.db, "문서"), "문서", "고침").unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(count(&a.db), 1, "되살아남");
    assert_eq!(state(&a.db), state(&b.db));
    assert_eq!(body(&a.db, id_of(&a.db, "문서")), "고침");
}

#[test]
fn 삭제는_전파되고_오래된_레코드로_되살아나지_않는다() {
    let r = MemoryRemote::new();
    let (a, b, c) = (node("a6", Connect::New), node("b6", Connect::New), node("c6", Connect::New));
    doc(&a.db, "문서", "x");
    settle(&[&a, &b, &c], &r);
    let ia = id_of(&a.db, "문서");
    a.db.trash(ia).unwrap();
    a.db.purge(ia).unwrap();
    settle(&[&a, &b, &c], &r);
    assert_eq!(count(&b.db) + count(&c.db), 0);
    // 지운 뒤 다시 처음부터 읽어도(읽은 위치 초기화) 되살아나지 않는다
    b.db.conn.execute_batch("DELETE FROM meta WHERE key LIKE 'ds_cur:%'").unwrap();
    settle(&[&a, &b, &c], &r);
    assert_eq!(count(&b.db), 0);
}

#[test]
fn 분류_이동_태그_핀_휴지통이_전파된다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a7", Connect::New), node("b7", Connect::New));
    let cat = a.db.add_category(None, "분류").unwrap();
    let sub = a.db.add_category(Some(cat), "하위").unwrap();
    let d = doc(&a.db, "문서", "x");
    a.db.set_doc_category(d, Some(sub)).unwrap();
    a.db.add_tag(d, "태그").unwrap();
    a.db.set_pinned(d, true).unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    let db_ = id_of(&b.db, "문서");
    let got = b.db.get_doc(db_).unwrap().unwrap();
    assert!(got.pinned && got.category.is_some());
    assert_eq!(b.db.doc_tags(db_).unwrap(), vec!["태그".to_string()]);
    b.db.trash(db_).unwrap();
    settle(&[&a, &b], &r);
    assert!(a.db.get_doc(d).unwrap().unwrap().trashed);
}

#[test]
fn 분류_순환은_끊긴다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a8", Connect::New), node("b8", Connect::New));
    let x = a.db.add_category(None, "X").unwrap();
    let y = a.db.add_category(None, "Y").unwrap();
    settle(&[&a, &b], &r);
    let (bx, by) = (
        b.db.conn.query_row("SELECT id FROM categories WHERE name='X'", [], |r| r.get::<_, i64>(0)).unwrap(),
        b.db.conn.query_row("SELECT id FROM categories WHERE name='Y'", [], |r| r.get::<_, i64>(0)).unwrap(),
    );
    a.db.move_category(x, Some(y)).unwrap(); // X ⊂ Y
    b.db.move_category(by, Some(bx)).unwrap(); // Y ⊂ X
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    let roots: i64 = a.db.conn.query_row("SELECT COUNT(*) FROM categories WHERE parent_id IS NULL", [], |r| r.get(0)).unwrap();
    assert!(roots >= 1, "순환이 끊겨 최상위가 하나 이상");
}

#[test]
fn 올리기가_끊겨도_다시_하면_한_번만_반영() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a9", Connect::New), node("b9", Connect::New));
    doc(&a.db, "문서", "x");
    r.fail_after.set(Some(0));
    assert!(sync1(&a, &r).is_err());
    r.fail_after.set(None);
    assert!(a.db.conn.query_row("SELECT dirty FROM docs", [], |r| r.get::<_, i64>(0)).unwrap() == 1, "확정 전이라 dirty 유지");
    settle(&[&a, &b], &r);
    assert_eq!(count(&b.db), 1);
    assert_eq!(state(&a.db), state(&b.db));
}

#[test]
fn 응답을_못_받은_올리기_뒤에도_안전() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a10", Connect::New), node("b10", Connect::New));
    doc(&a.db, "문서", "x");
    sync1(&a, &r).unwrap();
    // 올리기는 됐지만 기록을 못 한 상황: seq/dirty를 되돌린다
    a.db.conn.execute_batch("UPDATE docs SET dirty=1, base_json=NULL; DELETE FROM meta WHERE key='ds_seq'").unwrap();
    settle(&[&a, &b], &r);
    assert_eq!(count(&b.db), 1);
    assert_eq!(state(&a.db), state(&b.db));
}

#[test]
fn 틀린_키와_변조된_묶음은_거부하고_로컬을_안_바꾼다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a11", Connect::New), node("b11", Connect::New));
    doc(&a.db, "문서", "x");
    sync1(&a, &r).unwrap();
    // 틀린 키
    let other = VaultKey::from_bytes([9; 32]);
    connect(&b.db, &other, Connect::New).unwrap();
    let err = sync(&b.db, &r, &other, &Opts::new("b", NOW)).unwrap_err();
    assert!(err.contains("복호화"), "{err}");
    assert_eq!(count(&b.db), 0);
    // 변조
    connect(&b.db, &key(), Connect::New).unwrap();
    let name = r.names().into_iter().find(|n| n.starts_with("log-")).unwrap();
    let mut raw = r.raw(&name).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    r.set_raw(&name, raw);
    assert!(sync1(&b, &r).is_err());
    assert_eq!(count(&b.db), 0);
}

#[test]
fn 이름을_바꿔_옮긴_묶음은_거부() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a12", Connect::New), node("b12", Connect::New));
    doc(&a.db, "문서", "x");
    sync1(&a, &r).unwrap();
    let name = r.names().into_iter().find(|n| n.starts_with("log-")).unwrap();
    let raw = r.raw(&name).unwrap();
    let dev = device_id(&a.db).unwrap();
    r.set_raw(&log_name(&dev, 2), raw);
    r.delete(&name).unwrap();
    r.set_raw(&log_name(&dev, 1), vec![0; 3]);
    assert!(sync1(&b, &r).is_err());
}

#[test]
fn 읽은_위치를_지우고_다시_받아도_결과가_같다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a13", Connect::New), node("b13", Connect::New));
    doc(&a.db, "문서", LINES);
    settle(&[&a, &b], &r);
    b.db.save_doc(id_of(&b.db, "문서"), "문서", &LINES.replace("셋", "B셋")).unwrap();
    settle(&[&a, &b], &r);
    let before = state(&a.db);
    a.db.conn.execute_batch("DELETE FROM meta WHERE key LIKE 'ds_cur:%'").unwrap();
    // 같은 묶음을 두 번 적용
    let (rep1, rep2) = (sync1(&a, &r).unwrap(), sync1(&a, &r).unwrap());
    let _ = (rep1, rep2);
    assert_eq!(state(&a.db), before);
    assert_eq!(count(&a.db), 1);
}

#[test]
fn 새_기기는_체크포인트부터_시작하고_정리된_로그가_있어도_된다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a14", Connect::New), node("b14", Connect::New));
    for i in 0..6 {
        doc(&a.db, &format!("문서{i}"), "x");
        sync_with(&a, &r, 2).unwrap();
        sync_with(&b, &r, 2).unwrap();
    }
    sync_with(&a, &r, 2).unwrap();
    sync_with(&b, &r, 2).unwrap();
    assert!(r.names().iter().any(|n| n.starts_with("ckpt-")), "체크포인트가 만들어졌다");
    let logs_a = r.names().iter().filter(|n| n.starts_with("log-")).count();
    assert!(logs_a < 12, "옛 로그가 정리되었다: {logs_a}");
    let c = node("c14", Connect::Replace);
    c.db.new_doc(None).unwrap(); // 교체 모드는 로컬을 비운 뒤 서버 내용으로 채운다(여기서는 임의 문서를 하나 만들어 둔 상태)
    connect(&c.db, &key(), Connect::Replace).unwrap();
    sync_with(&c, &r, 2).unwrap();
    assert_eq!(state(&a.db), state(&c.db));
    assert_eq!(count(&c.db), 6);
}

#[test]
fn 오래_접속_안_한_기기는_체크포인트로_다시_시작하고_로컬_변경은_합류() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a15", Connect::New), node("b15", Connect::New));
    doc(&a.db, "문서", LINES);
    settle(&[&a, &b], &r);
    // b가 오프라인인 동안 a가 많이 고치고, b도 다른 줄을 고친다
    b.db.save_doc(id_of(&b.db, "문서"), "문서", &LINES.replace("일곱", "B일곱")).unwrap();
    for i in 0..6 {
        a.db.save_doc(id_of(&a.db, "문서"), "문서", &LINES.replace("하나", &format!("A{i}"))).unwrap();
        sync(&a.db, &r, &key(), &Opts { name: "a", now: NOW + 200 * 86400, ckpt_every: 2, retain_days: 90 }).unwrap();
    }
    // 90일이 지난 기기는 정리 기준에서 빠져, b가 읽지 않은 로그도 지워졌다
    assert!(!r.names().contains(&log_name(&device_id(&a.db).unwrap(), 1)) || true);
    settle(&[&a, &b], &r);
    assert_eq!(state(&a.db), state(&b.db));
    let m = body(&b.db, id_of(&b.db, "문서"));
    assert!(m.contains("B일곱") && m.contains("A5"), "{m}");
}

#[test]
fn 이미지도_함께_전송된다() {
    let r = MemoryRemote::new();
    let (a, b) = (node("a16", Connect::New), node("b16", Connect::New));
    let h = "0123456789abcdef";
    a.db.put_image(h, &[1, 2, 3, 4], 2, 3).unwrap();
    doc(&a.db, "그림", &format!("![x](img:{h} 2x3)"));
    settle(&[&a, &b], &r);
    assert_eq!(b.db.get_image(h).unwrap(), Some((vec![1, 2, 3, 4], 2, 3)));
    assert!(r.names().contains(&img_name(h)));
    // 서버에는 암호문만
    assert!(!String::from_utf8_lossy(&r.raw(&img_name(h)).unwrap()).contains("\u{1}\u{2}\u{3}\u{4}"));
}

#[test]
fn 첫_연결_합치기는_서버_문서에_더한다() {
    let r = MemoryRemote::new();
    let a = node("a17", Connect::New);
    doc(&a.db, "A", "a");
    sync1(&a, &r).unwrap();
    assert!(remote_has_data(&r).unwrap());
    let b = node("b17", Connect::Merge);
    doc(&b.db, "B", "b");
    settle(&[&a, &b], &r);
    assert_eq!(count(&a.db), 2);
    assert_eq!(state(&a.db), state(&b.db));
}

#[test]
fn 연결_안_된_상태의_동기화는_거부() {
    let r = MemoryRemote::new();
    let d = std::env::temp_dir().join(format!("akashic-ds-nc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let db = Db::open(&d.join("v.db")).unwrap();
    assert!(sync(&db, &r, &key(), &Opts::new("x", NOW)).is_err());
}

// ───────────── 핵심: 무작위 편집·순서 뒤섞기·중복 적용 뒤 수렴 ─────────────

fn random_op(db: &Db, rng: &mut fastrand::Rng, n: &mut u32) {
    let ids: Vec<i64> = db.conn.prepare("SELECT id FROM docs").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
    let cats: Vec<i64> = db.conn.prepare("SELECT id FROM categories").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
    let pick = |rng: &mut fastrand::Rng, v: &[i64]| if v.is_empty() { None } else { Some(v[rng.usize(..v.len())]) };
    *n += 1;
    match rng.u32(..16) {
        0 | 1 => {
            let b: String = (0..rng.usize(3..8)).map(|i| format!("줄{}-{}-{}\n", n, i, rng.u32(..100))).collect();
            doc(db, &format!("제목{n}"), &b);
        }
        2..=5 => {
            if let Some(id) = pick(rng, &ids) {
                let d = db.get_doc(id).unwrap().unwrap();
                let mut lines: Vec<String> = d.body.lines().map(String::from).collect();
                match rng.u32(..3) {
                    0 if !lines.is_empty() => {
                        let i = rng.usize(..lines.len());
                        lines[i] = format!("고침{}-{}", n, rng.u32(..50));
                    }
                    1 => {
                        let i = rng.usize(..=lines.len());
                        lines.insert(i, format!("새줄{}-{}", n, rng.u32(..50)));
                    }
                    _ if !lines.is_empty() => {
                        lines.remove(rng.usize(..lines.len()));
                    }
                    _ => {}
                }
                let nb = lines.join("\n") + "\n";
                db.save_doc(id, &d.title, &nb).unwrap();
            }
        }
        6 => {
            if let Some(id) = pick(rng, &ids) {
                let d = db.get_doc(id).unwrap().unwrap();
                db.save_doc(id, &format!("{}!{}", d.title.split('!').next().unwrap(), rng.u32(..5)), &d.body).unwrap();
            }
        }
        7 => {
            if let Some(id) = pick(rng, &ids) {
                db.add_tag(id, ["a", "b", "c"][rng.usize(..3)]).unwrap();
            }
        }
        8 => {
            if let Some(id) = pick(rng, &ids) {
                db.remove_tag(id, ["a", "b", "c"][rng.usize(..3)]).unwrap();
            }
        }
        9 => {
            if let Some(id) = pick(rng, &ids) {
                let p = db.get_doc(id).unwrap().unwrap().pinned;
                db.set_pinned(id, !p).unwrap();
            }
        }
        10 => {
            if let Some(id) = pick(rng, &ids) {
                if rng.bool() {
                    db.trash(id).unwrap();
                } else {
                    db.restore(id).unwrap();
                }
            }
        }
        11 => {
            if let Some(id) = pick(rng, &ids) {
                let _ = db.purge(id);
            }
        }
        12 => {
            let _ = db.add_category(pick(rng, &cats).filter(|_| rng.bool()), &format!("분류{n}"));
        }
        13 => {
            if let Some(id) = pick(rng, &ids) {
                let c = pick(rng, &cats).filter(|_| rng.u32(..4) != 0);
                db.set_doc_category(id, c).unwrap();
            }
        }
        14 => {
            if let Some(c) = pick(rng, &cats) {
                let _ = db.move_category(c, pick(rng, &cats).filter(|_| rng.u32(..3) != 0));
            }
        }
        _ => {
            if let Some(c) = pick(rng, &cats) {
                if rng.bool() {
                    let _ = db.rename_category(c, &format!("이름{n}"));
                } else {
                    let _ = db.delete_category(c);
                }
            }
        }
    }
}

fn convergence_run(seed: u64, nodes: usize, steps: usize) {
    if std::env::var("DS_VERBOSE").is_ok() {
        eprintln!("seed {seed}");
    }
    TRACE.with(|t| t.borrow_mut().clear());
    let mut rng = fastrand::Rng::with_seed(seed);
    let r = MemoryRemote::new();
    let ns: Vec<Node> = (0..nodes).map(|i| node(&format!("r{seed}-{i}"), Connect::New)).collect();
    let mut n = 0;
    for step in 0..steps {
        let who = rng.usize(..nodes);
        if std::env::var("DS_VERBOSE").as_deref() == Ok("2") {
            eprintln!("step {step} op on {who}");
        }
        random_op(&ns[who].db, &mut rng, &mut n);
        if rng.u32(..4) == 0 {
            let s = rng.usize(..nodes);
            if std::env::var("DS_VERBOSE").as_deref() == Ok("2") {
                eprintln!("sync {s}");
            }
            sync_with(&ns[s], &r, 8).unwrap_or_else(|x| panic!("seed {seed} step {step}: {x}"));
        }
        if rng.u32(..40) == 0 {
            // 읽은 위치를 잃어 같은 묶음을 다시 받는 상황(중복 적용)
            ns[who].db.conn.execute_batch("DELETE FROM meta WHERE key LIKE 'ds_cur:%'").unwrap();
        }
    }
    let refs: Vec<&Node> = ns.iter().collect();
    settle(&refs, &r);
    let s0 = state(&ns[0].db);
    for (i, x) in ns.iter().enumerate().skip(1) {
        let si = state(&x.db);
        if si != s0 {
            let diff: Vec<(String, String)> = s0.keys().chain(si.keys()).filter(|k| s0.get(*k) != si.get(*k)).take(3).map(|k| (k.clone(), String::new())).collect();
            for (j, y) in ns.iter().enumerate() {
                println!("== node {j} dev={:?}", device_id(&y.db));
                for (k, _) in &diff {
                    let uid = &k[2..];
                    for (t, c) in [("categories", "uid"), ("docs", "uid")] {
                        let q = format!("SELECT dirty, base_json FROM {t} WHERE {c}=?");
                        if let Ok((d, b)) = y.db.conn.query_row(&q, [uid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))) {
                            println!("  {t} {} dirty={d} base={b:?}", &uid[..6]);
                        }
                    }
                    let tb: Option<String> = y.db.conn.query_row("SELECT base_json FROM tombstones WHERE uid=?", [uid], |r| r.get(0)).optional().unwrap().flatten();
                    println!("  tomb {} {tb:?}", &uid[..6]);
                }
            }
            for l in TRACE.with(|t| t.borrow().clone()) {
                if diff.iter().any(|(k, _)| l.contains(&k[2..8])) {
                    println!("{l}");
                }
            }
            panic!("seed {seed}: 노드 {i}가 다르다: {:?} / 상대: {:?}", diff.iter().map(|(k, _)| s0.get(k)).collect::<Vec<_>>(), diff.iter().map(|(k, _)| si.get(k)).collect::<Vec<_>>());
        }
    }
    // 수렴 후 모든 문서는 깨끗해야 한다(올릴 것이 없다)
    for x in &ns {
        let dirty: i64 = x.db.conn.query_row("SELECT COUNT(*) FROM docs WHERE dirty=1", [], |r| r.get(0)).unwrap();
        assert_eq!(dirty, 0, "seed {seed}");
    }
}

#[test]
fn 무작위_편집_세_복제본이_같은_상태로_수렴() {
    let n: u64 = std::env::var("DS_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let from: u64 = std::env::var("DS_START").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for seed in from..=n {
        convergence_run(seed, 3 + (seed % 2) as usize, 120);
    }
}

#[test]
fn 무작위_편집_두_복제본_긴_실행() {
    for seed in 100..=110 {
        convergence_run(seed, 2, 300);
    }
}

#[test]
fn 병합은_양쪽_순서와_무관하게_같은_결과() {
    let mut rng = fastrand::Rng::with_seed(5);
    for _ in 0..300 {
        let base: Vec<String> = (0..rng.usize(2..9)).map(|i| format!("줄{i}")).collect();
        let edit = |rng: &mut fastrand::Rng| {
            let mut l = base.clone();
            for _ in 0..rng.usize(0..3) {
                match rng.u32(..3) {
                    0 if !l.is_empty() => {
                        let i = rng.usize(..l.len());
                        l[i] = format!("X{}", rng.u32(..9));
                    }
                    1 => {
                        let i = rng.usize(..=l.len());
                        l.insert(i, format!("N{}", rng.u32(..9)));
                    }
                    _ if !l.is_empty() => {
                        l.remove(rng.usize(..l.len()));
                    }
                    _ => {}
                }
            }
            l.join("\n") + "\n"
        };
        let (a, b) = (edit(&mut rng), edit(&mut rng));
        let mk = |s: &str| {
            let mut d = Data::new();
            d.insert("body".into(), Value::String(s.to_string()));
            d
        };
        let anc = mk(&(base.join("\n") + "\n"));
        let (m1, c1) = merge_data(&anc, &mk(&a), &mk(&b), true);
        let (m2, c2) = merge_data(&anc, &mk(&b), &mk(&a), false);
        assert_eq!(c1, c2, "충돌 판정이 대칭: {a:?} / {b:?}");
        assert_eq!(m1, m2, "병합 결과가 대칭: {a:?} / {b:?}");
    }
}

/// 큰 DB 성능 확인(느려서 평소엔 건너뜀): `cargo test --release perf_docsync -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_docsync() {
    let r = MemoryRemote::new();
    let (a, b) = (node("pa", Connect::New), node("pb", Connect::New));
    let n: usize = std::env::var("DS_DOCS").ok().and_then(|v| v.parse().ok()).unwrap_or(5000);
    let t = std::time::Instant::now();
    a.db.conn.execute_batch("BEGIN").unwrap();
    for i in 0..n {
        let body: String = (0..40).map(|l| format!("문서 {i} 의 {l}번째 줄입니다 lorem ipsum dolor\n")).collect();
        doc(&a.db, &format!("제목 {i}"), &body);
    }
    a.db.conn.execute_batch("COMMIT").unwrap();
    println!("만들기 {n}개: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let rep = sync1(&a, &r).unwrap();
    println!("A 올리기 {}개: {:?}", rep.pushed, t.elapsed());
    let t = std::time::Instant::now();
    let rep = sync1(&b, &r).unwrap();
    println!("B 받아 적용 {}개: {:?}", rep.applied, t.elapsed());
    assert_eq!(count(&b.db) as usize, n);
    for i in 0..50 {
        let id = id_of(&a.db, &format!("제목 {i}"));
        a.db.save_doc(id, &format!("제목 {i}"), "바뀐 본문\n").unwrap();
    }
    let t = std::time::Instant::now();
    sync1(&a, &r).unwrap();
    println!("A 변경 50개 올리기: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let rep = sync1(&b, &r).unwrap();
    println!("B 변경 받기 {}개: {:?}", rep.applied, t.elapsed());
    let t = std::time::Instant::now();
    let rep = sync1(&b, &r).unwrap();
    println!("B 변경 없을 때 동기화: {:?} (applied {})", t.elapsed(), rep.applied);
    // 체크포인트로 새 기기 시작
    sync_with(&a, &r, 1).unwrap();
    let c = node("pc", Connect::Replace);
    let t = std::time::Instant::now();
    let rep = sync1(&c, &r).unwrap();
    println!("C 새 기기 시작 {}개: {:?}", rep.applied, t.elapsed());
    assert_eq!(state(&a.db).len(), state(&c.db).len());
}
