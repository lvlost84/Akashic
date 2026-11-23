//! 문서 단위 동기화("webdoc") UI: 백그라운드 실행, 첫 연결 선택, 결과 문구, 열려 있는 문서 반영.
//! 동기화 자체는 `crate::docsync`(UI와 무관한 순수 로직)가 한다.
use super::*;
use crate::db;
use crate::docsync::{self, Connect, Opts};
use crate::vault::VaultKey;
use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// 작업 스레드가 돌려주는 결과.
enum DsMsg {
    /// 서버에 이미 문서가 있어 첫 연결 방식을 사용자가 골라야 한다.
    NeedChoice,
    Done(Result<docsync::Report, String>),
}

/// 시작할 때의 열린 문서(id, 제목, 본문): 동기화 중 편집한 내용과 원격 변경을 3-way로 합칠 때 공통 조상.
type OpenSnap = Option<(i64, String, String)>;

#[derive(Default)]
struct Ds {
    rx: Option<Receiver<DsMsg>>,
    started: Option<Instant>,
    auto: bool,
    snap: OpenSnap,
    ctx: Option<egui::Context>,
}

thread_local! {
    static DS: RefCell<Ds> = RefCell::new(Ds::default());
}

const STUCK: Duration = Duration::from_secs(20 * 60);
/// 첫 연결 선택 단추 이름
const BTN_REPLACE: &str = "서버 내용으로 교체";
const BTN_MERGE: &str = "합치기";

/// 서버 폴더 이름(스냅숏 방식 폴더와 섞이지 않게 따로 둔다).
pub(super) fn doc_folder(key: &VaultKey) -> String {
    format!("akashic2-{}", key.vault_id())
}

fn device_label() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "PC".into())
}

/// 결과 한 줄 요약(상태바·옵션에 표시).
pub(super) fn report_text(r: &docsync::Report) -> String {
    let mut s = format!("WebDAV 서버(문서 단위) 동기화 완료 · 받은 변경 {}개 · 올린 변경 {}개", r.applied, r.pushed);
    if r.merged > 0 {
        s += &format!(" · 자동 병합 {}건", r.merged);
    }
    if !r.conflicts.is_empty() {
        s += &format!(" · 충돌 사본 {}개 만듦(태그 '{}')", r.conflicts.len(), docsync::CONFLICT_TAG);
    }
    if r.images_up + r.images_down > 0 {
        s += &format!(" · 이미지 ↑{} ↓{}", r.images_up, r.images_down);
    }
    if r.pruned > 0 {
        s += &format!(" · 옛 로그 {}개 정리", r.pruned);
    }
    s
}

impl App {
    pub(super) fn ds_busy() -> bool {
        DS.with(|d| d.borrow().rx.is_some())
    }

    pub(super) fn ds_started() -> Option<Instant> {
        DS.with(|d| {
            let d = d.borrow();
            if d.rx.is_some() { d.started } else { None }
        })
    }

    pub(super) fn ds_set_ctx(ctx: &egui::Context) {
        DS.with(|d| d.borrow_mut().ctx = Some(ctx.clone()));
    }

    /// 문서 단위 동기화를 별도 스레드로 시작한다. `connect`는 첫 연결 방식(이미 연결돼 있으면 쓰이지 않는다).
    pub(super) fn ds_start(&mut self, auto: bool, connect: Option<Connect>) -> Result<bool, String> {
        if Self::ds_busy() || self.sync_busy() {
            return Err("이미 동기화 중입니다".into());
        }
        let (remote, key) = self.wd_remote()?;
        if !self.flush() {
            return Err("편집 중인 문서를 저장하지 못해 동기화를 시작하지 않았습니다".into());
        }
        let snap: OpenSnap = self.cur.filter(|_| !self.cur_trashed).map(|id| (id, self.title.clone(), self.body.clone()));
        let path = self.db.path.clone();
        let name = device_label();
        let (tx, rx) = channel();
        let ctx = DS.with(|d| d.borrow().ctx.clone());
        std::thread::spawn(move || {
            let r = (|| -> Result<DsMsg, String> {
                let db = crate::db::Db::open(&path)?;
                if !docsync::is_connected(&db, &key) {
                    match connect {
                        None => {
                            if docsync::remote_has_data(&remote)? {
                                return Ok(DsMsg::NeedChoice);
                            }
                            docsync::connect(&db, &key, Connect::New)?;
                        }
                        Some(m) => docsync::connect(&db, &key, m)?,
                    }
                }
                docsync::sync(&db, &remote, &key, &Opts::new(&name, crate::db::now())).map(|r| DsMsg::Done(Ok(r)))
            })();
            let _ = tx.send(r.unwrap_or_else(|x| DsMsg::Done(Err(x))));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        DS.with(|d| {
            let mut d = d.borrow_mut();
            d.rx = Some(rx);
            d.started = Some(Instant::now());
            d.auto = auto;
            d.snap = snap;
        });
        Ok(true)
    }

    /// 닫을 때: 바뀐 것이 있으면 한 번 동기화한다(화면 스레드에서 끝날 때까지 기다린다).
    pub(super) fn ds_on_close(&mut self) {
        if Self::ds_busy() || self.db.rev() == self.cfg.sync_rev {
            return;
        }
        let Ok((remote, key)) = self.wd_remote() else { return };
        self.flush();
        let Ok(db) = crate::db::Db::open(&self.db.path) else { return };
        if !docsync::is_connected(&db, &key) {
            return; // 첫 연결은 닫을 때 묻지 않는다
        }
        let r = docsync::sync(&db, &remote, &key, &Opts::new(&device_label(), crate::db::now()));
        self.ds_record(&r);
    }

    fn ds_record(&mut self, r: &Result<docsync::Report, String>) {
        self.cfg.sync_result_at = db::now();
        match r {
            Ok(rep) => {
                self.cfg.last_sync = db::now();
                self.cfg.sync_rev = self.db.rev();
                self.cfg.sync_failed = false;
                self.cfg.sync_skip.clear();
                self.cfg.sync_result = report_text(rep);
            }
            Err(x) => {
                self.cfg.sync_failed = true;
                self.cfg.sync_result = format!("WebDAV 서버(문서 단위) 동기화 실패 · {x}");
            }
        }
        self.cfg.save();
    }

    /// 첫 연결 선택 대화상자(서버에 이미 문서 단위 저장소가 있을 때).
    fn ds_choose_connect(&mut self) {
        let ans = rfd::MessageDialog::new()
            .set_title("문서 단위 동기화 · 첫 연결")
            .set_description(format!(
                "서버에 이미 다른 PC가 올린 문서가 있습니다.\n\n• [예] = {BTN_REPLACE}(권장): 이 PC의 문서를 모두 비우고 서버 내용으로 채웁니다. 지금 DB는 backup 폴더에 보관됩니다.\n• [아니요] = {BTN_MERGE}: 이 PC의 문서를 그대로 두고 서버 문서를 더합니다. 두 PC에 같은 문서가 있으면 중복됩니다.\n• [취소] = 연결하지 않습니다."
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(BTN_REPLACE.into(), BTN_MERGE.into(), "취소".into()))
            .show();
        let mode = match ans {
            rfd::MessageDialogResult::Custom(s) if s == BTN_REPLACE => Connect::Replace,
            rfd::MessageDialogResult::Custom(s) if s == BTN_MERGE => Connect::Merge,
            rfd::MessageDialogResult::Yes => Connect::Replace,
            rfd::MessageDialogResult::No => Connect::Merge,
            _ => {
                self.say("첫 연결을 취소했습니다");
                return;
            }
        };
        if mode == Connect::Replace {
            let dir = self.db.path.parent().unwrap_or(Path::new(".")).join("backup");
            let bak = dir.join(format!("before-docsync-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S%3f")));
            if let Err(x) = self.flush().then_some(()).ok_or_else(|| "편집 중인 문서를 저장하지 못했습니다".to_string()).and_then(|_| self.db.snapshot_to(&bak)) {
                self.say(&format!("교체 전 백업에 실패해 중단했습니다: {x}"));
                return;
            }
            self.say(&format!("현재 DB를 보관했습니다: {}", bak.display()));
        }
        match self.ds_start(false, Some(mode)) {
            Ok(_) => self.say("첫 연결 후 동기화를 시작했습니다"),
            Err(x) => self.say(&x),
        }
    }

    /// 매 프레임: 작업 결과를 받아 반영한다.
    pub(super) fn ds_poll(&mut self) {
        let (msg, stuck) = DS.with(|d| {
            let mut d = d.borrow_mut();
            let stuck = d.rx.is_some() && d.started.is_some_and(|t| t.elapsed() > STUCK);
            let m = match d.rx.as_ref().map(|r| r.try_recv()) {
                Some(Ok(m)) => Some(m),
                Some(Err(TryRecvError::Disconnected)) => Some(DsMsg::Done(Err("동기화 작업이 중단되었습니다".into()))),
                _ => None,
            };
            if m.is_some() || stuck {
                d.rx = None;
            }
            (m, stuck)
        });
        let msg = if stuck { Some(DsMsg::Done(Err("동기화가 20분 넘게 끝나지 않아 중단했습니다(서버 접근을 확인하세요)".into()))) } else { msg };
        let Some(msg) = msg else { return };
        let (auto, snap) = DS.with(|d| {
            let mut d = d.borrow_mut();
            (d.auto, d.snap.take())
        });
        match msg {
            DsMsg::NeedChoice => {
                if auto {
                    self.cfg.sync_skip = "문서 단위 동기화: 서버에 이미 문서가 있어 첫 연결 방식을 골라야 합니다. '지금 동기화'를 눌러 선택하세요".into();
                    self.cfg.save();
                } else {
                    self.ds_choose_connect();
                }
            }
            DsMsg::Done(r) => {
                self.ds_record(&r);
                match &r {
                    Ok(rep) => {
                        self.cfg.sync_skip.clear();
                        self.ds_after(rep, snap);
                        self.say(&report_text(rep));
                    }
                    Err(x) => self.say(&format!("동기화 실패: {x}")),
                }
                self.fs_invalidate(true);
            }
        }
    }

    /// 동기화가 끝난 뒤 화면 상태를 새 DB에 맞춘다(목록·그래프·이미지 캐시·열려 있는 문서).
    fn ds_after(&mut self, rep: &docsync::Report, snap: OpenSnap) {
        self.list_dirty = true;
        self.graph_stale = true;
        self.ref_rev += 1;
        if rep.images_down > 0 {
            self.img.clear();
        }
        let Some(id) = self.cur else { return };
        let gone = rep.deleted_docs.contains(&id) || matches!(self.db.get_doc(id), Ok(None));
        if gone {
            self.cur = None;
            self.cur_trashed = false;
            self.title.clear();
            self.body.clear();
            self.last_edit = None;
            self.say("열려 있던 문서가 다른 PC에서 삭제되어 닫았습니다");
            return;
        }
        if !rep.changed_docs.contains(&id) {
            return;
        }
        let Ok(Some(d)) = self.db.get_doc(id) else { return };
        let (anc_title, anc_body) = match snap {
            Some((sid, t, b)) if sid == id => (t, b),
            _ => (self.title.clone(), self.body.clone()),
        };
        let edited = self.body != anc_body || self.title != anc_title;
        let mut new_body = d.body.clone();
        let mut new_title = d.title.clone();
        let mut note = "다른 PC의 변경을 열려 있는 문서에 반영했습니다 (되돌리기 기록이 초기화되었습니다)";
        if edited {
            // 동기화하는 동안 편집한 내용: 원격 변경과 줄 단위로 합친다. 겹치면 내 편집을 두고 원격 쪽은 사본 문서로 남긴다.
            new_body = match diffy::merge(&anc_body, &self.body, &d.body) {
                Ok(m) => m,
                Err(_) => {
                    if let Ok(nid) = self.db.new_doc(d.category) {
                        let t = format!("{} (충돌 · 원격 · {})", d.title, chrono::Local::now().format("%Y-%m-%d %H:%M"));
                        let _ = self.db.save_doc_plain(nid, &t, &d.body);
                        let _ = self.db.add_tag(nid, docsync::CONFLICT_TAG);
                    }
                    note = "편집 중이던 내용과 다른 PC의 변경이 같은 줄에서 겹쳐 내 편집을 유지하고, 다른 PC 쪽은 '충돌' 사본 문서로 남겼습니다";
                    self.body.clone()
                }
            };
            if self.title != anc_title {
                new_title = self.title.clone();
            }
        }
        // 커서는 (줄, 칸)을 기억해 새 본문의 같은 자리로
        let line = md::line_of_char(&self.body, self.cursor_idx);
        let col = self.cursor_idx.saturating_sub(md::char_of_line(&self.body, line));
        let nl = line.min(new_body.lines().count().saturating_sub(1));
        let line_len = new_body.lines().nth(nl).map_or(0, |l| l.chars().count());
        let new_idx = md::char_of_line(&new_body, nl) + col.min(line_len);
        self.body = new_body;
        self.title = new_title;
        self.cur_trashed = d.trashed;
        self.cur_pinned = d.pinned;
        self.cur_cat = d.category;
        self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
        self.cursor_idx = new_idx;
        self.last_edit = if edited { Some(Instant::now()) } else { None };
        self.compute_links();
        if let Some(ctx) = DS.with(|d| d.borrow().ctx.clone()) {
            use egui::text::{CCursor, CCursorRange};
            #[allow(deprecated)]
            let eid = egui::Id::new("body_edit");
            let mut st = egui::TextEdit::load_state(&ctx, eid).unwrap_or_default();
            st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(new_idx))));
            st.clear_undoer();
            st.store(&ctx, eid);
            ctx.request_repaint();
        }
        self.say(note);
    }
}
