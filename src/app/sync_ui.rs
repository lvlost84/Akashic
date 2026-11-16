//! 동기화 저장소 UI·자동 동기화 훅.
use super::*;
use crate::{db, sync};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

type JobResult = Result<sync::WriteStats, String>;

#[derive(Default)]
struct St {
    rx: Option<Receiver<JobResult>>,
    last_check: Option<Instant>,
    /// 충돌로 자동 동기화를 건너뛴 manifest 의 saved_at (같은 알림 반복 방지)
    skipped_for: i64,
    ctx: Option<egui::Context>,
}

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
    fn sync_busy(&self) -> bool {
        ST.with(|s| s.borrow().rx.is_some())
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

    /// 올리기 전 충돌 확인. `Ok(true)` 진행, `Ok(false)` 사용자가 취소, `Err` 자동 동기화 보류(빈 글이면 알리지 않음).
    fn sync_conflict(&mut self, dir: &Path, dev: &str, auto: bool) -> Result<bool, String> {
        let Ok(Some(m)) = sync::read_manifest(dir) else { return Ok(true) };
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

    /// 스냅숏(읽기 전용 연결)을 만들고 조각을 쓴다. 화면 스레드가 아니어도 된다.
    fn sync_job(src: &Path, dir: &Path, dev: &str, now: i64) -> JobResult {
        let snap = std::env::temp_dir().join(format!("akashic-sync-{}.db", std::process::id()));
        let r = db::Db::snapshot_file(src, &snap)
            .map_err(|x| format!("스냅숏 실패: {x}"))
            .and_then(|_| sync::push_file(&snap, dir, dev, now, sync::CHUNK));
        let _ = std::fs::remove_file(&snap);
        r
    }

    /// 마지막 동기화 결과를 설정에 남긴다(상태바 툴팁·옵션에서 다시 볼 수 있게).
    fn sync_record(&mut self, r: &JobResult) {
        self.cfg.sync_result_at = db::now();
        match r {
            Ok(st) => {
                self.cfg.last_sync = st.saved_at;
                self.cfg.sync_failed = false;
                self.cfg.sync_skip.clear();
                self.cfg.sync_result = format!("올리기 완료 · 조각 {}개 중 {}개 갱신", st.total, st.written);
            }
            Err(x) => {
                self.cfg.sync_failed = true;
                self.cfg.sync_result = format!("올리기 실패 · {x}");
            }
        }
        self.cfg.save();
    }

    /// 충돌을 확인하고 쓰기를 별도 스레드로 넘긴다(스냅숏도 그 스레드에서 만든다). `Ok(true)` 시작됨, `Ok(false)` 취소.
    fn sync_start(&mut self, auto: bool) -> Result<bool, String> {
        if self.sync_busy() {
            return Err("이미 동기화 중입니다".into());
        }
        let dir = self.sync_dir_path()?;
        let dev = sync::device_id();
        if !self.sync_conflict(&dir, &dev, auto)? {
            return Ok(false);
        }
        self.flush();
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
        ST.with(|s| s.borrow_mut().rx = Some(rx));
        Ok(true)
    }

    /// 닫을 때: 마지막 동기화 이후 바뀐 것이 있으면 바로 올린다(바뀐 조각만 쓰므로 짧다). 충돌이면 올리지 않는다.
    pub(super) fn sync_on_close(&mut self) {
        if !(self.cfg.sync_auto && self.cfg.sync_on_close) || self.sync_busy() || self.db.latest_change() <= self.cfg.last_sync {
            return;
        }
        let Ok(dir) = self.sync_dir_path() else { return };
        let dev = sync::device_id();
        if !matches!(self.sync_conflict(&dir, &dev, true), Ok(true)) {
            return;
        }
        let src = self.db.path.clone();
        let r = Self::sync_job(&src, &dir, &dev, db::now());
        self.sync_record(&r);
    }

    fn sync_restore(&mut self) -> Result<PathBuf, String> {
        if self.sync_busy() {
            return Err("동기화 중입니다. 잠시 뒤에 다시 시도하세요".into());
        }
        let dir = self.sync_dir_path()?;
        self.flush();
        let (bak, at) = sync::restore_from_dir(&mut self.db, &dir)?;
        // 방금 불러온 저장본을 기준으로 삼아야 다음 올리기를 "다른 기기가 올린 새 내용"으로 오해하지 않는다.
        self.cfg.last_sync = at;
        self.cfg.sync_skip.clear();
        self.cfg.sync_failed = false;
        self.cfg.sync_result = "불러오기 완료".into();
        self.cfg.sync_result_at = db::now();
        self.cfg.save();
        self.list_dirty = true;
        self.graph_stale = true;
        // 현재 문서를 다시 연다(없어졌으면 비움)
        if let Some(id) = self.cur {
            self.last_edit = None;
            if matches!(self.db.get_doc(id), Ok(Some(_))) {
                self.open_doc(id, false);
            } else {
                self.cur = None;
                self.title.clear();
                self.body.clear();
            }
        }
        Ok(bak)
    }

    /// 옵션 창의 "동기화 저장소" 구역.
    pub(super) fn sync_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        ST.with(|s| s.borrow_mut().ctx = Some(ui.ctx().clone()));
        ui.separator();
        ui.label(egui::RichText::new("동기화 저장소").strong());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("폴더에 조각으로 저장해 다른 PC와 맞춥니다").color(self.theme.dim));
            if crate::icons::icon_only(ui, crate::icons::Icon::Help).on_hover_text("동기화 사용법 (도움말)").clicked() {
                self.show_help = true;
                self.help_topic = super::help_ui::topic_index("동기화 저장소");
            }
        });
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
                    self.cfg.save();
                    self.fs_invalidate(true);
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
                match self.sync_start(false) {
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
        });
        // 폴더의 저장본 정보는 백그라운드에서 읽어 둔 값을 쓴다.
        let remote = self.storage.remote.as_ref().map(|(at, dev)| format!(" · 폴더의 저장본: {} ({})", fmt_ts(*at), dev)).unwrap_or_default();
        ui.label(format!("마지막 저장: {}{}", fmt_ts(self.cfg.last_sync), remote));
        if !self.cfg.sync_result.is_empty() {
            let c = if self.cfg.sync_failed { self.theme.code_fg } else { self.theme.dim };
            ui.label(egui::RichText::new(format!("마지막 결과 ({}): {}", fmt_ts(self.cfg.sync_result_at), self.cfg.sync_result)).color(c));
        }
        if !self.cfg.sync_skip.is_empty() {
            ui.label(egui::RichText::new(&self.cfg.sync_skip).color(self.theme.code_fg));
        }
    }

    fn sync_due(&self) -> bool {
        self.cfg.sync_auto
            && !self.cfg.sync_dir.trim().is_empty()
            && db::now() - self.cfg.last_sync >= self.cfg.sync_hours.max(1) as i64 * 3600
    }

    fn sync_auto_try(&mut self) {
        if !self.sync_due() || self.sync_busy() {
            return;
        }
        match self.sync_start(true) {
            Ok(_) => self.say("자동 동기화를 시작했습니다"),
            Err(x) if x.is_empty() => {}
            Err(x) => self.say(&x),
        }
    }

    /// 상태바용 동기화 요약: (짧은 글, 자세한 설명, 주의 여부).
    pub(super) fn sync_status(&self) -> (String, String, bool) {
        let hhmm = |ts: i64| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default()
        };
        if self.sync_busy() {
            return ("동기화 중…".into(), "동기화 폴더에 바뀐 조각을 쓰는 중입니다".into(), false);
        }
        if self.cfg.sync_dir.trim().is_empty() {
            return ("동기화 꺼짐".into(), "동기화 폴더가 없습니다.\n클릭: 옵션 > 백업·동기화에서 폴더를 고르세요".into(), false);
        }
        // 폴더 접근 여부는 백그라운드에서 읽어 둔 값만 쓴다(프레임마다 폴더를 열지 않는다).
        if self.storage.sync_ok == Some(false) {
            return ("동기화 폴더 없음".into(), format!("접근할 수 없습니다: {}\n클릭: 옵션 > 백업·동기화", self.cfg.sync_dir), true);
        }
        let last = self.cfg.last_sync;
        let result = if self.cfg.sync_result.is_empty() { String::new() } else { format!("\n마지막 결과 ({}): {}", fmt_ts(self.cfg.sync_result_at), self.cfg.sync_result) };
        if !self.cfg.sync_skip.is_empty() {
            return ("동기화 보류".into(), format!("{}\n옵션 > 백업·동기화에서 '이 폴더에서 불러오기'로 먼저 받거나, '지금 동기화'로 덮어쓸지 정하세요{result}", self.cfg.sync_skip), true);
        }
        if self.cfg.sync_failed {
            return ("동기화 실패".into(), format!("{}\n클릭: 옵션 > 백업·동기화", self.cfg.sync_result), true);
        }
        let hours = self.cfg.sync_hours.max(1) as i64;
        let next_short = if !self.cfg.sync_auto {
            "수동".to_string()
        } else if last <= 0 {
            "곧".to_string()
        } else {
            let due = last + hours * 3600 - db::now();
            if due <= 0 {
                "곧".to_string()
            } else if due < 3600 {
                format!("{}분 후", (due + 59) / 60)
            } else {
                format!("{}시간 후", (due + 1799) / 3600)
            }
        };
        let short = if last > 0 { format!("동기화 {} · {}", hhmm(last), next_short) } else { format!("동기화 안 함 · {next_short}") };
        let tip = format!(
            "폴더: {}\n마지막 저장: {}\n자동 동기화: {}{}{}\n\n클릭: 옵션 > 백업·동기화",
            self.cfg.sync_dir,
            fmt_ts(last),
            if self.cfg.sync_auto { "켜짐" } else { "꺼짐" },
            if self.cfg.sync_auto { format!(" ({hours}시간마다)") } else { String::new() },
            result
        );
        (short, tip, false)
    }

    /// 프로그램 시작 직후 한 번 호출된다.
    pub(super) fn sync_on_start(&mut self) {
        ST.with(|s| s.borrow_mut().last_check = Some(Instant::now()));
        self.sync_auto_try();
    }

    /// 매 프레임 호출된다(가벼워야 함).
    pub(super) fn sync_tick(&mut self) {
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
        match res {
            Some(r) => {
                self.sync_record(&r);
                self.fs_invalidate(true);
                match r {
                    Ok(st) => self.say(&format!("동기화 완료: 조각 {}개 중 {}개 갱신", st.total, st.written)),
                    Err(x) => self.say(&format!("동기화 실패: {x}")),
                }
            }
            None => {}
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
