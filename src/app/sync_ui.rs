//! 동기화 저장소 UI·자동 동기화 훅. 폴더 방식(기존)과 WebDAV 암호화 방식을 모두 다룬다.
use super::*;
use crate::vault::{self, VaultError, VaultKey};
use crate::webdav::{self, WebDavRemote};
use crate::{db, sync};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// 올리기 작업의 결과. `Foreign` 은 (WebDAV) 올리기 전에 서버를 보니 다른 기기가 먼저 올린 내용이 있었다는 뜻이다.
enum Job {
    Done { total: usize, written: usize, saved_at: i64, etag: String },
    Foreign { device: String, saved_at: i64 },
}
type JobResult = Result<Job, String>;
/// WebDAV 불러오기(내려받기) 결과: (받아 둔 임시 DB 파일, 저장본 시각, ETag)
type RestoreMsg = Result<(PathBuf, i64, String), String>;

/// WebDAV 설정 화면의 백그라운드 작업 결과
enum WdMsg {
    Test(Result<String, String>),
    Connect { key: String, res: Result<String, String> },
}

/// 올리는 순간 다른 기기가 먼저 올린 것이 드러났을 때(412)의 안내 문구
const CONFLICT_TEXT: &str = "다른 기기가 먼저 올려서 올리지 않았습니다. 먼저 '서버에서 받아오기'로 받은 뒤 다시 올리세요";

/// 암호 저장소 오류를 사용자에게 구분되게 보여 줄 한국어 문구로 바꾼다(충돌 / 키 불일치 / 네트워크 / 손상).
fn vault_err(e: VaultError) -> String {
    match e {
        VaultError::Conflict => CONFLICT_TEXT.to_string(),
        VaultError::WrongKey => "키가 다릅니다: 복구 문자열이 이 저장소의 것이 아닙니다 (서버에는 아무것도 쓰지 않았습니다)".to_string(),
        VaultError::Corrupt(s) => format!("서버의 저장본이 손상되었거나 바뀌었습니다: {s}"),
        VaultError::Remote(s) => format!("서버·네트워크 오류: {s}"),
        VaultError::Io(s) => format!("파일 오류: {s}"),
    }
}

#[derive(Default)]
struct St {
    rx: Option<Receiver<JobResult>>,
    last_check: Option<Instant>,
    /// 충돌로 자동 동기화를 건너뛴 manifest 의 saved_at (같은 알림 반복 방지)
    skipped_for: i64,
    ctx: Option<egui::Context>,
    /// 지금 진행 중인 올리기를 시작할 때의 변경 카운터(성공하면 cfg.sync_rev로 기록)
    pending_rev: i64,
    /// 올리기를 시작한 시각(너무 오래 끝나지 않으면 멈춘 것으로 보고 포기)
    started: Option<Instant>,
    /// 자동 동기화가 실패했을 때 이 시각까지는 다시 시도하지 않는다
    retry_after: Option<Instant>,
    /// 진행 중인 올리기가 자동 동기화로 시작된 것인가(다른 기기 내용을 만났을 때 묻지 않고 보류)
    auto_job: bool,
    /// WebDAV 불러오기(내려받기) 진행
    rrx: Option<Receiver<RestoreMsg>>,
    /// WebDAV 설정 화면의 연결 시험·기존 저장소 연결 진행
    wrx: Option<Receiver<WdMsg>>,
    wd_msg: String,
    wd_msg_err: bool,
    /// "기존 저장소 연결" 입력칸
    wd_input: String,
    /// 복구 문자열 창 표시
    show_key: bool,
    /// 주소·계정 입력칸을 고치는 중(포커스를 잃으면 저장)
    wd_dirty: bool,
}

/// 올리기가 이 시간 안에 끝나지 않으면(네트워크 폴더가 멈춘 경우 등) 멈춘 것으로 본다.
const STUCK: Duration = Duration::from_secs(15 * 60);
/// 자동 동기화 실패 뒤 재시도 간격
const RETRY: Duration = Duration::from_secs(10 * 60);

thread_local! {
    static ST: RefCell<St> = RefCell::new(St::default());
}

fn fmt_ts(ts: i64) -> String {
    if ts <= 0 {
        return "아직 없음".into();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "?".into())
}

impl App {
    pub(super) fn sync_busy(&self) -> bool {
        ST.with(|s| {
            let s = s.borrow();
            s.rx.is_some() || s.rrx.is_some()
        }) || Self::ds_busy()
    }

    /// WebDAV 연결 설정을 쓰는 방식인가(전체 스냅숏 "webdav" 또는 문서 단위 "webdoc").
    fn is_wd(&self) -> bool {
        matches!(self.cfg.sync_mode.as_str(), "webdav" | "webdoc")
    }

    /// 문서 단위 동기화(WebDAV)인가.
    pub(super) fn is_doc(&self) -> bool {
        self.cfg.sync_mode == "webdoc"
    }

    /// 동기화 대상이 설정되어 있는가(폴더 방식: 폴더, WebDAV: 주소·사용자·복구 문자열).
    fn sync_configured(&self) -> bool {
        if self.is_wd() {
            !self.cfg.wd_url.trim().is_empty() && !self.cfg.wd_user.trim().is_empty() && !self.cfg.wd_key.trim().is_empty()
        } else {
            !self.cfg.sync_dir.trim().is_empty()
        }
    }

    /// 설정으로 WebDAV 전송과 키를 만든다.
    pub(super) fn wd_remote(&self) -> Result<(WebDavRemote, VaultKey), String> {
        if self.cfg.wd_key.trim().is_empty() {
            return Err("저장소가 없습니다. '새 저장소 만들기' 또는 '기존 저장소 연결'을 먼저 하세요".into());
        }
        let key = VaultKey::from_recovery(&self.cfg.wd_key)?;
        let folder = if self.is_doc() { super::docsync_ui::doc_folder(&key) } else { key.folder_name() };
        let remote = WebDavRemote::new(&self.cfg.wd_url, self.cfg.wd_user.trim(), &self.cfg.wd_pass, &folder)?;
        Ok((remote, key))
    }

    /// 동기화 대상이 바뀌었을 때: 이전 대상 기준의 시각·결과로 새 대상을 판단하면 충돌을 놓치므로 처음부터 다시 시작한다.
    fn sync_reset_state(&mut self) {
        self.cfg.last_sync = 0;
        self.cfg.sync_rev = 0;
        self.cfg.wd_etag.clear();
        self.cfg.sync_skip.clear();
        self.cfg.sync_result.clear();
        self.cfg.sync_failed = false;
        self.cfg.save();
        self.fs_invalidate(true);
    }

    fn sync_dir_path(&self) -> Result<PathBuf, String> {
        if self.cfg.sync_dir.trim().is_empty() {
            return Err("동기화 폴더가 설정되지 않았습니다".into());
        }
        let p = PathBuf::from(&self.cfg.sync_dir);
        crate::diag::count("sync_dir_action");
        if !p.is_dir() {
            return Err(format!("동기화 폴더에 접근할 수 없습니다: {}", p.display()));
        }
        Ok(p)
    }

    /// 올리기 전 충돌 확인(폴더 방식). `Ok(true)` 진행, `Ok(false)` 사용자가 취소, `Err` 자동 동기화 보류(빈 글이면 알리지 않음).
    fn sync_conflict(&mut self, dir: &Path, dev: &str, auto: bool) -> Result<bool, String> {
        let m = match sync::read_manifest(dir) {
            Ok(None) => return Ok(true), // 폴더에 저장본이 아직 없음
            Ok(Some(m)) => m,
            // 읽지 못한 것을 "충돌 없음"으로 보면 다른 기기의 내용을 덮어쓸 수 있으므로 올리지 않는다.
            Err(x) => return Err(format!("동기화 폴더의 저장본을 읽을 수 없어 올리지 않았습니다: {x}")),
        };
        if !sync::foreign_newer(&m, dev, self.cfg.last_sync) {
            return Ok(true);
        }
        if auto {
            let first = ST.with(|s| {
                let mut s = s.borrow_mut();
                let first = s.skipped_for != m.saved_at;
                s.skipped_for = m.saved_at;
                first
            });
            let note = format!("다른 기기({})가 {}에 올린 내용이 있어 자동 동기화를 보류했습니다", m.device, fmt_ts(m.saved_at));
            if self.cfg.sync_skip != note {
                self.cfg.sync_skip = note;
                self.cfg.save();
            }
            return Err(if first { format!("다른 기기({})가 올린 내용이 있어 자동 동기화를 건너뜁니다", m.device) } else { String::new() });
        }
        let ask = rfd::MessageDialog::new()
            .set_title("동기화")
            .set_description(format!(
                "다른 곳에서 바뀐 것 같습니다 ({}, {}).\n지금 올리면 그 내용을 덮어씁니다. 그래도 올릴까요?",
                m.device,
                fmt_ts(m.saved_at)
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        Ok(ask == rfd::MessageDialogResult::Yes)
    }

    /// 스냅숏(읽기 전용 연결)을 만들고 조각을 쓴다(폴더 방식). 화면 스레드가 아니어도 된다.
    fn sync_job(src: &Path, dir: &Path, dev: &str, now: i64) -> JobResult {
        let snap = std::env::temp_dir().join(format!("akashic-sync-{}.db", std::process::id()));
        let r = db::Db::snapshot_file(src, &snap)
            .map_err(|x| format!("스냅숏 실패: {x}"))
            .and_then(|_| sync::push_file(&snap, dir, dev, now, sync::CHUNK));
        let _ = std::fs::remove_file(&snap);
        r.map(|st| Job::Done { total: st.total, written: st.written, saved_at: st.saved_at, etag: String::new() })
    }

    /// WebDAV 올리기. 서버의 manifest 를 먼저 읽어 키를 확인하고(틀리면 아무것도 쓰지 않는다),
    /// 마지막으로 올리거나 받은 뒤 다른 기기가 올렸으면 `force` 가 아닌 한 올리지 않고 알린다.
    fn wd_job(src: &Path, remote: &WebDavRemote, key: &VaultKey, dev: &str, now: i64, known_etag: &str, force: bool) -> JobResult {
        let state = vault::read_state(remote, key).map_err(vault_err)?;
        if let Some(s) = &state {
            if s.etag != known_etag && s.manifest.device != dev && !force {
                return Ok(Job::Foreign { device: s.manifest.device.clone(), saved_at: s.manifest.saved_at });
            }
        }
        let snap = std::env::temp_dir().join(format!("akashic-sync-wd-{}.db", std::process::id()));
        let r = db::Db::snapshot_file(src, &snap)
            .map_err(|x| format!("스냅숏 실패: {x}"))
            .and_then(|_| vault::push_snapshot(remote, key, &snap, state.as_ref(), dev, now, vault::CHUNK).map_err(vault_err));
        let _ = std::fs::remove_file(&snap);
        r.map(|st| Job::Done { total: st.total, written: st.uploaded, saved_at: now, etag: st.etag })
    }

    /// 마지막 동기화 결과를 설정에 남긴다(상태바 툴팁·옵션에서 다시 볼 수 있게).
    fn sync_record(&mut self, r: &JobResult, rev: i64) {
        self.cfg.sync_result_at = db::now();
        match r {
            Ok(Job::Done { total, written, saved_at, etag }) => {
                self.cfg.last_sync = *saved_at;
                self.cfg.sync_rev = rev;
                self.cfg.sync_failed = false;
                self.cfg.sync_skip.clear();
                if self.is_wd() {
                    self.cfg.wd_etag = etag.clone();
                }
                let place = if self.is_wd() { "WebDAV 서버" } else { "동기화 폴더" };
                self.cfg.sync_result = format!("{place}에 올리기 완료 · 조각 {total}개 중 {written}개 갱신");
            }
            Ok(Job::Foreign { .. }) => {} // 호출한 쪽(sync_tick)이 따로 처리한다
            Err(x) => {
                if x == CONFLICT_TEXT {
                    self.cfg.sync_skip = x.clone(); // 충돌은 "실패"가 아니라 "보류"로 보인다
                } else {
                    self.cfg.sync_failed = true;
                }
                let place = if self.is_wd() { "WebDAV 서버" } else { "동기화 폴더" };
                self.cfg.sync_result = format!("{place} 올리기 실패 · {x}");
            }
        }
        self.cfg.save();
    }

    /// 충돌을 확인하고 쓰기를 별도 스레드로 넘긴다(스냅숏도 그 스레드에서 만든다). `Ok(true)` 시작됨, `Ok(false)` 취소.
    fn sync_start(&mut self, auto: bool, force: bool) -> Result<bool, String> {
        if self.sync_busy() {
            return Err("이미 동기화 중입니다".into());
        }
        if self.is_doc() {
            return self.ds_start(auto, None);
        }
        if self.is_wd() {
            return self.wd_start(auto, force);
        }
        let dir = self.sync_dir_path()?;
        let dev = sync::device_id();
        if !self.sync_conflict(&dir, &dev, auto)? {
            return Ok(false);
        }
        self.flush();
        let rev = self.db.rev();
        let src = self.db.path.clone();
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        let now = db::now();
        std::thread::spawn(move || {
            let _ = tx.send(Self::sync_job(&src, &dir, &dev, now));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.rx = Some(rx);
            s.pending_rev = rev;
            s.started = Some(Instant::now());
            s.auto_job = auto;
        });
        Ok(true)
    }

    /// WebDAV 올리기를 별도 스레드로 시작한다(서버 확인·스냅숏·업로드 모두 그 스레드에서).
    fn wd_start(&mut self, auto: bool, force: bool) -> Result<bool, String> {
        let (remote, key) = self.wd_remote()?;
        self.flush();
        let rev = self.db.rev();
        let src = self.db.path.clone();
        let dev = sync::device_id();
        let known = self.cfg.wd_etag.clone();
        let now = db::now();
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Self::wd_job(&src, &remote, &key, &dev, now, &known, force));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.rx = Some(rx);
            s.pending_rev = rev;
            s.started = Some(Instant::now());
            s.auto_job = auto;
        });
        Ok(true)
    }

    /// 올리기 전에 서버에서 다른 기기의 새 내용을 만났을 때. 자동이면 보류하고, 수동이면 덮어쓸지 묻는다.
    fn sync_foreign(&mut self, device: &str, saved_at: i64, auto: bool) {
        if auto {
            let first = ST.with(|s| {
                let mut s = s.borrow_mut();
                let first = s.skipped_for != saved_at;
                s.skipped_for = saved_at;
                s.retry_after = Some(Instant::now() + RETRY);
                first
            });
            let note = format!("다른 기기({device})가 {}에 올린 내용이 있어 자동 동기화를 보류했습니다", fmt_ts(saved_at));
            if self.cfg.sync_skip != note {
                self.cfg.sync_skip = note;
                self.cfg.save();
            }
            if first {
                self.say(&format!("다른 기기({device})가 올린 내용이 있어 자동 동기화를 건너뜁니다"));
            }
            return;
        }
        let ask = rfd::MessageDialog::new()
            .set_title("동기화")
            .set_description(format!(
                "다른 기기가 서버에 먼저 올린 내용이 있습니다 ({device}, {}).\n지금 올리면 그 내용을 덮어씁니다. 그래도 올릴까요?\n(아니오를 고르면 '서버에서 받아오기'로 먼저 받을 수 있습니다)",
                fmt_ts(saved_at)
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if ask == rfd::MessageDialogResult::Yes {
            match self.sync_start(false, true) {
                Ok(_) => self.say("동기화를 시작했습니다"),
                Err(x) => self.say(&x),
            }
        } else {
            self.say("올리지 않았습니다");
        }
    }

    /// 닫을 때: 마지막 동기화 이후 바뀐 것이 있으면 바로 올린다(바뀐 조각만 쓰므로 짧다). 충돌이면 올리지 않는다.
    pub(super) fn sync_on_close(&mut self) {
        if !(self.cfg.sync_auto && self.cfg.sync_on_close) || self.sync_busy() || self.db.rev() == self.cfg.sync_rev {
            return;
        }
        if self.is_doc() {
            self.ds_on_close();
            return;
        }
        let dev = sync::device_id();
        let src = self.db.path.clone();
        let rev = self.db.rev();
        let r = if self.is_wd() {
            let Ok((remote, key)) = self.wd_remote() else { return };
            let known = self.cfg.wd_etag.clone();
            match Self::wd_job(&src, &remote, &key, &dev, db::now(), &known, false) {
                Ok(Job::Foreign { .. }) => return, // 다른 기기 내용이 있으면 닫을 때는 건드리지 않는다
                r => r,
            }
        } else {
            let Ok(dir) = self.sync_dir_path() else { return };
            if !matches!(self.sync_conflict(&dir, &dev, true), Ok(true)) {
                return;
            }
            Self::sync_job(&src, &dir, &dev, db::now())
        };
        self.sync_record(&r, rev);
    }

    /// 불러온 뒤의 공통 처리: 기준 시각·ETag 를 맞추고 화면·캐시를 새 DB 기준으로 다시 읽는다.
    fn after_restore(&mut self, at: i64, etag: Option<String>) {
        // 방금 불러온 저장본을 기준으로 삼아야 다음 올리기를 "다른 기기가 올린 새 내용"으로 오해하지 않는다.
        self.cfg.last_sync = at;
        self.cfg.sync_rev = self.db.rev(); // 불러온 내용은 이미 폴더에 있는 것이므로 "바뀐 것 없음"으로 시작한다
        if let Some(e) = etag {
            self.cfg.wd_etag = e;
        }
        self.cfg.sync_skip.clear();
        self.cfg.sync_failed = false;
        self.cfg.sync_result = format!("{}에서 불러오기 완료", if self.is_wd() { "WebDAV 서버" } else { "동기화 폴더" });
        self.cfg.sync_result_at = db::now();
        self.cfg.save();
        self.img.clear(); // 이미지 캐시도 새 DB 기준으로 다시 읽는다
        self.list_dirty = true;
        self.graph_stale = true;
        self.fs_invalidate(true); // 상태바의 DB 크기·저장본 정보를 다시 읽는다
        // 현재 문서를 다시 연다(없어졌으면 비움)
        if let Some(id) = self.cur {
            self.last_edit = None;
            if matches!(self.db.get_doc(id), Ok(Some(_))) {
                self.open_doc(id, false);
            } else {
                self.cur = None;
                self.cur_trashed = false;
                self.title.clear();
                self.body.clear();
            }
        }
    }

    fn sync_restore(&mut self) -> Result<PathBuf, String> {
        if self.sync_busy() {
            return Err("동기화 중입니다. 잠시 뒤에 다시 시도하세요".into());
        }
        let dir = self.sync_dir_path()?;
        self.flush();
        let (bak, at) = sync::restore_from_dir(&mut self.db, &dir)?;
        self.after_restore(at, None);
        Ok(bak)
    }

    /// WebDAV 불러오기: 내려받기는 별도 스레드, 검증·반영은 받은 뒤 화면 스레드에서(sync_tick).
    fn wd_restore_start(&mut self) -> Result<(), String> {
        if self.sync_busy() {
            return Err("동기화 중입니다. 잠시 뒤에 다시 시도하세요".into());
        }
        let (remote, key) = self.wd_remote()?;
        self.flush();
        let base = self.db.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        std::thread::spawn(move || {
            let r: RestoreMsg = (|| {
                let st = vault::read_state(&remote, &key).map_err(vault_err)?.ok_or_else(|| "서버에 저장본이 아직 없습니다".to_string())?;
                std::fs::create_dir_all(&base).map_err(|x| x.to_string())?;
                let tmp = base.join("sync-restore.tmp.db");
                vault::fetch_snapshot(&remote, &key, &st.manifest, &tmp).map_err(vault_err)?;
                Ok((tmp, st.manifest.saved_at, st.etag))
            })();
            let _ = tx.send(r);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.rrx = Some(rx);
            s.started = Some(Instant::now());
        });
        Ok(())
    }

    /// 연결 시험·기존 저장소 연결을 별도 스레드로 시작한다.
    fn wd_op_start(&mut self, connect_key: Option<String>) {
        if ST.with(|s| s.borrow().wrx.is_some()) {
            return;
        }
        let (url, user, pass) = (self.cfg.wd_url.clone(), self.cfg.wd_user.trim().to_string(), self.cfg.wd_pass.clone());
        let doc = self.is_doc();
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        std::thread::spawn(move || {
            let msg = match connect_key {
                None => WdMsg::Test(webdav::test_connection(&url, &user, &pass)),
                Some(k) => {
                    let res = (|| -> Result<String, String> {
                        let key = VaultKey::from_recovery(&k)?;
                        if doc {
                            // 문서 단위는 스냅숏과 다른 서버 폴더(akashic2-…)를 쓴다.
                            let remote = WebDavRemote::new(&url, &user, &pass, &super::docsync_ui::doc_folder(&key))?;
                            return if crate::docsync::remote_has_data(&remote)? {
                                Ok("연결했습니다 · 서버에 문서 단위 저장소가 있습니다".to_string())
                            } else {
                                Err("서버에 이 복구 문자열의 문서 단위 저장소가 없습니다. 서버 주소·계정이 맞는지, 다른 PC에서 [지금 동기화]를 한 번 했는지 확인하세요(스냅숏 방식으로 만든 저장소는 문서 단위에서 보이지 않습니다)".into())
                            };
                        }
                        let remote = WebDavRemote::new(&url, &user, &pass, &key.folder_name())?;
                        match vault::read_state(&remote, &key).map_err(vault_err)? {
                            Some(s) => Ok(format!("연결했습니다 · 서버의 저장본: {} ({})", fmt_ts(s.manifest.saved_at), s.manifest.device)),
                            None => Err("서버에 이 복구 문자열의 저장소가 없습니다. 서버 주소·계정이 맞는지 확인하거나, 새로 시작하려면 '새 저장소 만들기'를 쓰세요".into()),
                        }
                    })();
                    WdMsg::Connect { key: k, res }
                }
            };
            let _ = tx.send(msg);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.wrx = Some(rx);
            s.wd_msg = "서버에 확인하는 중…".into();
            s.wd_msg_err = false;
        });
    }

    /// 새 저장소: 무작위 키를 만들어 설정에 저장하고 복구 문자열 창을 띄운다. 서버에는 첫 올리기 때 폴더가 생긴다.
    fn wd_create_vault(&mut self) -> Result<(), String> {
        let key = VaultKey::generate()?;
        self.cfg.wd_key = key.recovery_string();
        self.sync_reset_state();
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.show_key = true;
            s.wd_msg = "새 저장소를 만들었습니다. 아래 복구 문자열을 꼭 보관하세요".into();
            s.wd_msg_err = false;
        });
        Ok(())
    }

    /// 복구 문자열 창(만든 직후 크게 표시 · 복사). 옵션 창 안에서 그려지며 [복구 문자열 보기]로 다시 열 수 있다.
    fn wd_key_window(&mut self, ctx: &egui::Context) {
        let mut open = ST.with(|s| s.borrow().show_key);
        if !open {
            return;
        }
        let key = self.cfg.wd_key.clone();
        if key.is_empty() {
            ST.with(|s| s.borrow_mut().show_key = false);
            return;
        }
        let mut close = false;
        egui::Window::new("저장소 복구 문자열").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(egui::RichText::new("이 문자열을 잃으면 서버의 데이터를 복구할 수 없습니다.").strong().color(self.theme.code_fg));
            ui.label("다른 PC를 연결할 때도 필요합니다. 비밀번호 관리자나 종이에 적어 안전한 곳에 보관하세요. 서버 관리자도 이 문자열 없이는 내용을 볼 수 없습니다.");
            ui.add_space(8.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(&key).monospace().size(20.0).strong());
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if crate::icons::btn(ui, crate::icons::Icon::Copy, "복사").clicked() {
                    ui.ctx().copy_text(key.clone());
                }
                if crate::icons::btn(ui, crate::icons::Icon::Check, "보관했습니다").clicked() {
                    close = true;
                }
            });
        });
        if close {
            open = false;
        }
        ST.with(|s| s.borrow_mut().show_key = open);
    }

    /// 옵션 창의 "동기화 저장소" 구역.
    pub(super) fn sync_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        ST.with(|s| s.borrow_mut().ctx = Some(ui.ctx().clone()));
        Self::ds_set_ctx(ui.ctx());
        ui.separator();
        ui.label(egui::RichText::new("동기화 저장소").strong());
        ui.horizontal(|ui| {
            let sub = if self.is_doc() {
                "바뀐 문서만 암호화해서 주고받고, 서로 다른 문서의 동시 편집은 자동으로 합칩니다"
            } else if self.is_wd() {
                "NAS의 WebDAV 서버에 암호화해서 올려 다른 PC와 맞춥니다"
            } else {
                "폴더에 조각으로 저장해 다른 PC와 맞춥니다"
            };
            ui.label(egui::RichText::new(sub).color(self.theme.dim));
            if crate::icons::icon_only(ui, crate::icons::Icon::Help).on_hover_text("동기화 사용법 (도움말)").clicked() {
                self.show_help = true;
                self.help_topic = super::help_ui::topic_index("동기화 저장소");
            }
        });
        ui.horizontal(|ui| {
            ui.label("방식:");
            let was = self.cfg.sync_mode.clone();
            let mut m = if was == "webdav" || was == "webdoc" { was.clone() } else { "folder".to_string() };
            ui.radio_value(&mut m, "folder".to_string(), "폴더 (동기화 프로그램 사용)");
            ui.radio_value(&mut m, "webdoc".to_string(), "문서 단위 (WebDAV, 권장)");
            ui.radio_value(&mut m, "webdav".to_string(), "전체 스냅숏 (WebDAV)");
            if m != was {
                self.cfg.sync_mode = m;
                self.sync_reset_state();
            }
        });
        if self.is_wd() {
            self.wd_options(ui, notes);
        } else {
            self.folder_options(ui, notes);
        }
        if ui.checkbox(&mut self.cfg.sync_auto, "자동 동기화").changed() {
            self.cfg.save();
        }
        ui.add_enabled_ui(self.cfg.sync_auto, |ui| {
            if ui.checkbox(&mut self.cfg.sync_on_close, "닫을 때 바뀐 것이 있으면 한 번 더 올리기").changed() {
                self.cfg.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("주기");
            if self.is_doc() {
                // 문서 단위: 다른 PC의 변경을 이 주기로 확인하고, 이 PC에서 고친 것은 2분 뒤에 올린다.
                let cur = self.cfg.sync_mins;
                egui::ComboBox::from_id_salt("sync_mins").selected_text(mins_text(cur)).show_ui(ui, |ui| {
                    for m in [5u32, 15, 60] {
                        if ui.selectable_label(cur == m, mins_text(m)).clicked() {
                            self.cfg.sync_mins = m;
                            self.cfg.save();
                            self.fs_invalidate(true);
                            ui.close();
                        }
                    }
                });
            } else {
                let cur = self.cfg.sync_hours;
                egui::ComboBox::from_id_salt("sync_hours").selected_text(format!("{cur}시간")).show_ui(ui, |ui| {
                    for h in [1u32, 3, 6, 24] {
                        if ui.selectable_label(cur == h, format!("{h}시간")).clicked() {
                            self.cfg.sync_hours = h;
                            self.cfg.save();
                            self.fs_invalidate(true);
                            ui.close();
                        }
                    }
                });
            }
        });
        // 저장본 정보는 백그라운드에서 읽어 둔 값을 쓴다.
        let place = if self.is_wd() { "서버" } else { "폴더" };
        let remote = self.storage.remote.as_ref().map(|(at, dev)| format!(" · {place}의 저장본: {} ({})", fmt_ts(*at), dev)).unwrap_or_default();
        ui.label(format!("마지막 저장: {}{}", fmt_ts(self.cfg.last_sync), remote));
        if !self.cfg.sync_result.is_empty() {
            let c = if self.cfg.sync_failed { self.theme.code_fg } else { self.theme.dim };
            ui.label(egui::RichText::new(format!("마지막 결과 ({}): {}", fmt_ts(self.cfg.sync_result_at), self.cfg.sync_result)).color(c));
        }
        if !self.cfg.sync_skip.is_empty() {
            ui.label(egui::RichText::new(&self.cfg.sync_skip).color(self.theme.code_fg));
        }
        let ctx = ui.ctx().clone();
        self.wd_key_window(&ctx);
    }

    /// 폴더 방식 입력·단추(기존 동작 그대로).
    fn folder_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        ui.horizontal_wrapped(|ui| {
            ui.label("폴더:");
            if self.cfg.sync_dir.is_empty() {
                ui.label("(선택 안 됨)");
            } else {
                ui.monospace(&self.cfg.sync_dir);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if crate::icons::btn(ui, crate::icons::Icon::Folder, "폴더 선택…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.cfg.sync_dir = d.to_string_lossy().into_owned();
                    // 이전 폴더 기준의 시각·결과로 새 폴더를 판단하면 충돌을 놓친다 → 처음부터 다시 시작
                    self.sync_reset_state();
                    notes.push(format!("동기화 폴더: {}", self.cfg.sync_dir));
                }
            }
            if crate::icons::btn(ui, crate::icons::Icon::Folder, "폴더 열기").clicked() {
                match self.sync_dir_path() {
                    Ok(p) => {
                        let _ = std::process::Command::new("explorer").arg(p).spawn();
                    }
                    Err(x) => notes.push(x),
                }
            }
            if crate::icons::btn(ui, crate::icons::Icon::Sync, "지금 동기화").clicked() {
                match self.sync_start(false, false) {
                    Ok(true) => notes.push("동기화를 시작했습니다 (끝나면 상태바에 표시됩니다)".into()),
                    Ok(false) => notes.push("동기화를 취소했습니다".into()),
                    Err(x) => notes.push(x),
                }
            }
            if crate::icons::btn(ui, crate::icons::Icon::Import, "이 폴더에서 불러오기…").clicked() {
                match self.sync_dir_path() {
                    Err(x) => notes.push(x),
                    Ok(_) => {
                        let ask = rfd::MessageDialog::new()
                            .set_title("불러오기")
                            .set_description("동기화 폴더의 내용으로 현재 문서 전체를 바꿉니다.\n바꾸기 전 현재 DB는 backup 폴더에 보관됩니다.\n계속할까요?")
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show();
                        if ask == rfd::MessageDialogResult::Yes {
                            match self.sync_restore() {
                                Ok(b) => notes.push(format!("불러왔습니다. 이전 DB 보관: {}", b.display())),
                                Err(x) => notes.push(format!("불러오기 중단: {x}")),
                            }
                        }
                    }
                }
            }
        });
    }

    /// WebDAV 방식 입력·단추.
    fn wd_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        ui.label(egui::RichText::new("서버에는 암호화된 조각만 올라갑니다. 주소는 https:// 여야 합니다.").color(self.theme.dim));
        if self.is_doc() {
            ui.label(
                egui::RichText::new(format!(
                    "문서 단위: 바뀐 문서·분류·태그만 주고받습니다. 두 PC가 같은 문서를 같은 시간대에 고치면 줄 단위로 자동 병합하고, 같은 줄이 겹치면 한쪽을 '제목 (충돌 · PC · 시각)' 사본(태그 '{}')으로 남깁니다. 삭제와 수정이 겹치면 수정이 남습니다.",
                    crate::docsync::CONFLICT_TAG
                ))
                .color(self.theme.dim)
                .small(),
            );
        }
        // 입력칸: 고치는 동안은 표시만 하고, 포커스를 잃을 때 저장한다(글자마다 파일을 쓰지 않도록).
        let mut dirty = ST.with(|s| s.borrow().wd_dirty);
        let mut url_edited = false;
        egui::Grid::new("wd_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("서버 주소");
            let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.wd_url).hint_text("https://nas.example.com/dav").desired_width(320.0));
            if r.changed() {
                dirty = true;
                url_edited = true;
            }
            if r.lost_focus() && dirty {
                self.cfg.wd_url = self.cfg.wd_url.trim().to_string();
                self.sync_reset_state(); // 다른 서버가 될 수 있으므로 기준을 다시 잡는다
                dirty = false;
            }
            ui.end_row();
            ui.label("사용자");
            let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.wd_user).desired_width(320.0));
            if r.changed() {
                dirty = true;
            }
            if r.lost_focus() && dirty && !url_edited {
                self.cfg.save();
                dirty = false;
            }
            ui.end_row();
            ui.label("비밀번호");
            let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.wd_pass).password(true).desired_width(320.0));
            if r.changed() {
                dirty = true;
            }
            if r.lost_focus() && dirty && !url_edited {
                self.cfg.save();
                dirty = false;
            }
            ui.end_row();
        });
        ST.with(|s| s.borrow_mut().wd_dirty = dirty);
        ui.label(egui::RichText::new("비밀번호와 복구 문자열은 설정 파일(config.json)에 이 PC·이 Windows 계정에서만 풀리게 암호화되어 저장됩니다.").color(self.theme.dim).small());

        let busy_op = ST.with(|s| s.borrow().wrx.is_some());
        let have_acct = !self.cfg.wd_url.trim().is_empty() && !self.cfg.wd_user.trim().is_empty();
        ui.horizontal_wrapped(|ui| {
            if crate::icons::btn_ex(ui, crate::icons::Icon::Check, "연결 시험", have_acct && !busy_op, None).clicked() {
                self.wd_op_start(None);
            }
            if crate::icons::btn_ex(ui, crate::icons::Icon::Sync, "새 저장소 만들기", !busy_op, None).clicked() {
                let go = self.cfg.wd_key.trim().is_empty()
                    || rfd::MessageDialog::new()
                        .set_title("새 저장소")
                        .set_description("지금 연결된 저장소 대신 새 저장소를 만듭니다.\n기존 복구 문자열을 따로 보관하지 않았다면 그 저장소는 다시 열 수 없습니다. 서버의 기존 데이터는 그대로 남습니다.\n계속할까요?")
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::Yes;
                if go {
                    if let Err(x) = self.wd_create_vault() {
                        notes.push(format!("새 저장소를 만들지 못했습니다: {x}"));
                    }
                }
            }
            if !self.cfg.wd_key.trim().is_empty() && crate::icons::btn(ui, crate::icons::Icon::Copy, "복구 문자열 보기").clicked() {
                ST.with(|s| s.borrow_mut().show_key = true);
            }
        });
        if let Ok(k) = VaultKey::from_recovery(&self.cfg.wd_key) {
            let folder = if self.is_doc() { super::docsync_ui::doc_folder(&k) } else { k.folder_name() };
            ui.label(egui::RichText::new(format!("연결된 저장소의 서버 폴더: {folder} (폴더 이름일 뿐 복구 문자열이 아닙니다)")).color(self.theme.dim).small());
        } else {
            ui.label(egui::RichText::new("연결된 저장소: 없음 — 새로 만들거나 아래에 복구 문자열을 넣어 기존 저장소에 연결하세요").color(self.theme.dim).small());
        }
        // 기존 저장소 연결(다른 PC에서 만든 저장소)
        ui.horizontal_wrapped(|ui| {
            ui.label("기존 저장소 연결:");
            let mut input = ST.with(|s| s.borrow().wd_input.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut input).hint_text("AKS1-XXXX-XXXX-…").desired_width(300.0));
            let changed = r.changed();
            let valid = VaultKey::from_recovery(&input).is_ok();
            let can = have_acct && valid && !busy_op;
            if crate::icons::btn_ex(ui, crate::icons::Icon::Import, "연결", can, None).clicked() {
                self.wd_op_start(Some(input.trim().to_string()));
            }
            if changed {
                ST.with(|s| s.borrow_mut().wd_input = input.clone());
            }
            if !input.trim().is_empty() && !valid {
                let msg = if input.trim().to_ascii_lowercase().starts_with("akashic-") {
                    "이 값은 서버 폴더 이름입니다. 첫 PC의 [복구 문자열 보기]에 나오는 AKS1-로 시작하는 문자열을 넣으세요"
                } else {
                    "복구 문자열이 올바르지 않습니다(AKS1-로 시작, 0·1·I·O는 쓰이지 않음 · 오타·체크섬)"
                };
                ui.label(egui::RichText::new(msg).color(self.theme.code_fg).small());
            }
        });
        let (msg, err) = ST.with(|s| {
            let s = s.borrow();
            (s.wd_msg.clone(), s.wd_msg_err)
        });
        if !msg.is_empty() {
            ui.label(egui::RichText::new(msg).color(if err { self.theme.code_fg } else { self.theme.dim }));
        }
        ui.add_space(4.0);
        let busy = self.sync_busy();
        ui.horizontal_wrapped(|ui| {
            let ready = self.sync_configured();
            if crate::icons::btn_ex(ui, crate::icons::Icon::Sync, "지금 동기화", ready && !busy, None).clicked() {
                match self.sync_start(false, false) {
                    Ok(true) => notes.push("동기화를 시작했습니다 (끝나면 상태바에 표시됩니다)".into()),
                    Ok(false) => notes.push("동기화를 취소했습니다".into()),
                    Err(x) => notes.push(x),
                }
            }
            if !self.is_doc() && crate::icons::btn_ex(ui, crate::icons::Icon::Import, "서버에서 받아오기…", ready && !busy, None).clicked() {
                let ask = rfd::MessageDialog::new()
                    .set_title("받아오기")
                    .set_description("서버의 저장본으로 현재 문서 전체를 바꿉니다.\n바꾸기 전 현재 DB는 backup 폴더에 보관됩니다.\n계속할까요?")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if ask == rfd::MessageDialogResult::Yes {
                    match self.wd_restore_start() {
                        Ok(()) => notes.push("서버에서 받는 중입니다 (끝나면 상태바에 표시됩니다)".into()),
                        Err(x) => notes.push(format!("받아오기 중단: {x}")),
                    }
                }
            }
        });
    }

    fn sync_due(&self) -> bool {
        let waited = db::now() - self.cfg.last_sync;
        let period = if self.is_doc() { self.cfg.sync_mins.max(1) as i64 * 60 } else { self.cfg.sync_hours.max(1) as i64 * 3600 };
        let by_hours = waited >= period;
        // 문서 단위는 이 PC에 바뀐 것이 있으면 주기를 기다리지 않고 2분 뒤에 올린다.
        let by_edit = self.is_doc() && waited >= 120 && self.db.rev() != self.cfg.sync_rev;
        self.cfg.sync_auto && self.sync_configured() && (by_hours || by_edit)
    }

    fn sync_auto_try(&mut self) {
        if !self.sync_due() || self.sync_busy() {
            return;
        }
        if ST.with(|s| s.borrow().retry_after.map_or(false, |t| Instant::now() < t)) {
            return; // 방금 실패했으므로 한동안 쉰다(1분마다 같은 오류·설정 저장이 반복되지 않도록)
        }
        match self.sync_start(true, false) {
            Ok(_) => self.say("자동 동기화를 시작했습니다"),
            Err(x) => {
                ST.with(|s| s.borrow_mut().retry_after = Some(Instant::now() + RETRY));
                if !x.is_empty() {
                    // 충돌로 보류한 경우는 sync_skip이 따로 표시하므로 "실패"로 칠하지 않는다.
                    self.cfg.sync_failed = self.cfg.sync_skip.is_empty();
                    self.cfg.sync_result = format!("자동 동기화 못 함 · {x}");
                    self.cfg.sync_result_at = db::now();
                    self.cfg.save();
                    self.say(&x);
                }
            }
        }
    }

    /// 상태바용 동기화 요약: (짧은 글, 자세한 설명, 주의 여부).
    pub(super) fn sync_status(&self) -> (String, String, bool) {
        let hhmm = |ts: i64| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default()
        };
        let wd = self.is_wd();
        if self.sync_busy() {
            let tip = if self.is_doc() { "바뀐 문서를 서버와 주고받는 중입니다" } else if wd { "서버와 주고받는 중입니다" } else { "동기화 폴더에 바뀐 조각을 쓰는 중입니다" };
            return ("동기화 중…".into(), tip.into(), false);
        }
        if !self.sync_configured() {
            let tip = if wd {
                "WebDAV 서버 주소·사용자·저장소가 아직 없습니다.\n클릭: 옵션 > 백업·동기화에서 설정하세요"
            } else {
                "동기화 폴더가 없습니다.\n클릭: 옵션 > 백업·동기화에서 폴더를 고르세요"
            };
            return ("동기화 꺼짐".into(), tip.into(), false);
        }
        // 폴더·서버 접근 여부는 백그라운드에서 읽어 둔 값만 쓴다(프레임마다 폴더를 열거나 서버에 묻지 않는다).
        if self.storage.sync_ok == Some(false) {
            return if wd {
                ("동기화 서버 접속 불가".into(), format!("서버({})에 접속하지 못했거나 키가 맞지 않습니다.\n클릭: 옵션 > 백업·동기화", self.cfg.wd_url), true)
            } else {
                ("동기화 폴더 없음".into(), format!("접근할 수 없습니다: {}\n클릭: 옵션 > 백업·동기화", self.cfg.sync_dir), true)
            };
        }
        let last = self.cfg.last_sync;
        let result = if self.cfg.sync_result.is_empty() { String::new() } else { format!("\n마지막 결과 ({}): {}", fmt_ts(self.cfg.sync_result_at), self.cfg.sync_result) };
        if !self.cfg.sync_skip.is_empty() {
            let how = if wd { "'서버에서 받아오기'로 먼저 받거나, '지금 동기화'로 덮어쓸지 정하세요" } else { "'이 폴더에서 불러오기'로 먼저 받거나, '지금 동기화'로 덮어쓸지 정하세요" };
            return ("동기화 보류".into(), format!("{}\n옵션 > 백업·동기화에서 {how}{result}", self.cfg.sync_skip), true);
        }
        if self.cfg.sync_failed {
            return ("동기화 실패".into(), format!("{}\n클릭: 옵션 > 백업·동기화", self.cfg.sync_result), true);
        }
        let period_secs = if self.is_doc() { self.cfg.sync_mins.max(1) as i64 * 60 } else { self.cfg.sync_hours.max(1) as i64 * 3600 };
        let next_short = if !self.cfg.sync_auto {
            "수동".to_string()
        } else if last <= 0 {
            "곧".to_string()
        } else {
            let due = last + period_secs - db::now();
            if due <= 0 {
                "곧".to_string()
            } else if due < 3600 {
                format!("{}분 후", (due + 59) / 60)
            } else {
                format!("{}시간 후", (due + 1799) / 3600)
            }
        };
        let short = if last > 0 { format!("동기화 {} · {}", hhmm(last), next_short) } else { format!("동기화 안 함 · {next_short}") };
        let target = if self.is_doc() { format!("서버: {} (암호화 · 문서 단위)", self.cfg.wd_url) } else if wd { format!("서버: {} (암호화)", self.cfg.wd_url) } else { format!("폴더: {}", self.cfg.sync_dir) };
        let tip = format!(
            "{target}\n마지막 저장: {}\n자동 동기화: {}{}{}\n\n클릭: 옵션 > 백업·동기화",
            fmt_ts(last),
            if self.cfg.sync_auto { "켜짐" } else { "꺼짐" },
            if !self.cfg.sync_auto {
                String::new()
            } else if self.is_doc() {
                format!(" ({}마다 확인 · 고친 것은 2분 뒤 올림)", mins_text(self.cfg.sync_mins.max(1)))
            } else {
                format!(" ({}시간마다)", self.cfg.sync_hours.max(1))
            },
            result
        );
        (short, tip, false)
    }

    /// 프로그램 시작 직후 한 번 호출된다.
    pub(super) fn sync_on_start(&mut self, ctx: &egui::Context) {
        // 옵션 창을 한 번도 열지 않아도 백그라운드 결과가 바로 화면에 반영되도록 처음부터 보관한다.
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.ctx = Some(ctx.clone());
            s.last_check = Some(Instant::now());
        });
        Self::ds_set_ctx(ctx);
        self.sync_auto_try();
    }

    /// 설정 화면의 연결 시험·기존 저장소 연결 결과를 반영한다.
    fn wd_poll_ops(&mut self) {
        let msg = ST.with(|s| {
            let mut s = s.borrow_mut();
            match s.wrx.as_ref().map(|r| r.try_recv()) {
                Some(Ok(m)) => {
                    s.wrx = None;
                    Some(m)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    s.wrx = None;
                    Some(WdMsg::Test(Err("작업이 중단되었습니다".into())))
                }
                _ => None,
            }
        });
        let Some(m) = msg else { return };
        let (text, err) = match m {
            WdMsg::Test(Ok(t)) => (t, false),
            WdMsg::Test(Err(e)) => (format!("연결 실패: {e}"), true),
            WdMsg::Connect { key, res: Ok(t) } => {
                self.cfg.wd_key = key;
                self.sync_reset_state();
                ST.with(|s| s.borrow_mut().wd_input.clear());
                (
                    if self.is_doc() {
                        format!("{t}. [지금 동기화]를 누르면 서버에 이미 문서가 있을 때 첫 연결 방식(서버 내용으로 교체 / 합치기)을 고르게 됩니다")
                    } else {
                        format!("{t}. 이 PC의 문서와 합치려면 '서버에서 받아오기'를 먼저 하세요")
                    },
                    false,
                )
            }
            WdMsg::Connect { res: Err(e), .. } => (format!("연결 실패: {e}"), true),
        };
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.wd_msg = text;
            s.wd_msg_err = err;
        });
    }

    /// 매 프레임 호출된다(가벼워야 함).
    pub(super) fn sync_tick(&mut self) {
        self.wd_poll_ops();
        self.ds_poll();
        // WebDAV 불러오기: 내려받기가 끝나면 화면 스레드에서 검증·반영한다.
        let restored = ST.with(|s| {
            let mut s = s.borrow_mut();
            match s.rrx.as_ref().map(|r| r.try_recv()) {
                Some(Ok(r)) => {
                    s.rrx = None;
                    Some(r)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    s.rrx = None;
                    Some(Err("받아오기 작업이 중단되었습니다".to_string()))
                }
                _ => None,
            }
        });
        if let Some(r) = restored {
            match r {
                Ok((tmp, at, etag)) => {
                    let res = sync::restore_from_file(&mut self.db, &tmp);
                    let _ = std::fs::remove_file(&tmp);
                    match res {
                        Ok(bak) => {
                            self.after_restore(at, Some(etag));
                            self.say(&format!("서버에서 받아왔습니다. 이전 DB 보관: {}", bak.display()));
                        }
                        Err(x) => self.say(&format!("받아오기 중단: {x}")),
                    }
                }
                Err(x) => {
                    self.cfg.sync_result = format!("{} 받아오기 실패 · {x}", if self.is_wd() { "WebDAV 서버" } else { "동기화 폴더" });
                    self.cfg.sync_result_at = db::now();
                    self.cfg.sync_failed = true;
                    self.cfg.save();
                    self.say(&format!("받아오기 실패: {x}"));
                }
            }
            self.fs_invalidate(true);
        }
        let res = ST.with(|s| {
            let mut s = s.borrow_mut();
            match s.rx.as_ref().map(|r| r.try_recv()) {
                Some(Ok(r)) => {
                    s.rx = None;
                    Some(r)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    s.rx = None;
                    Some(Err("동기화 작업이 중단되었습니다".into()))
                }
                _ => None,
            }
        });
        // 너무 오래 끝나지 않는 올리기(멈춘 네트워크 폴더 등)는 포기해서 이후 동기화가 막히지 않게 한다.
        let stuck = ST.with(|s| {
            let mut s = s.borrow_mut();
            let stuck = (s.rx.is_some() || s.rrx.is_some()) && s.started.map_or(false, |t| t.elapsed() > STUCK);
            if stuck {
                s.rx = None;
                s.rrx = None;
                s.retry_after = Some(Instant::now() + RETRY);
            }
            stuck
        });
        let res = if stuck { Some(Err("올리기가 15분 넘게 끝나지 않아 중단했습니다(폴더·서버 접근을 확인하세요)".to_string())) } else { res };
        let (rev, auto) = ST.with(|s| {
            let s = s.borrow();
            (s.pending_rev, s.auto_job)
        });
        if let Some(r) = res {
            if let Ok(Job::Foreign { device, saved_at }) = &r {
                self.sync_foreign(device, *saved_at, auto);
            } else {
                self.sync_record(&r, rev);
                self.fs_invalidate(true);
                match r {
                    Ok(Job::Done { total, written, .. }) => self.say(&format!("동기화 완료: 조각 {total}개 중 {written}개 갱신")),
                    Ok(Job::Foreign { .. }) => {}
                    Err(x) => self.say(&format!("동기화 실패: {x}")),
                }
            }
        }
        let check = ST.with(|s| {
            let mut s = s.borrow_mut();
            let due = s.last_check.map_or(true, |t| t.elapsed() >= Duration::from_secs(60));
            if due {
                s.last_check = Some(Instant::now());
            }
            due
        });
        if check {
            self.sync_auto_try();
        }
    }
}

/// 문서 단위 동기화 주기 표시("5분", "1시간").
fn mins_text(m: u32) -> String {
    if m >= 60 && m % 60 == 0 { format!("{}시간", m / 60) } else { format!("{m}분") }
}
