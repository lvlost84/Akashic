//! 자동 업데이트 UI·백그라운드 작업(확인 → 내려받기 → 해시 확인 → 교체). 실제 처리는 update.rs.
use super::*;
use crate::db;
use crate::update::{self, Release};
use std::cell::RefCell;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

enum Msg {
    Checked(Result<Release, String>),
    Installed(Result<(), String>, String),
}

#[derive(Default)]
struct St {
    rx: Option<Receiver<Msg>>,
    ctx: Option<egui::Context>,
    /// 알려진 최신 릴리스(새 버전일 때만)
    available: Option<Release>,
    /// 설치를 마친 버전(다시 시작하면 적용)
    installed: Option<String>,
    /// 화면에 보일 한 줄 상태
    status: String,
    /// 확인이 자동(시작 때)인지: 자동이면 "최신 버전" 같은 결과는 알리지 않는다
    auto: bool,
    restart: bool,
}

thread_local! {
    static ST: RefCell<St> = RefCell::new(St::default());
}

const DAY: i64 = 24 * 3600;

fn running_exe() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().map(|p| update::original_path(&p))
}

impl App {
    fn update_busy(&self) -> bool {
        ST.with(|s| s.borrow().rx.is_some())
    }

    fn update_check_start(&mut self, auto: bool) {
        if self.update_busy() || update::REPO.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(update::fetch_latest()));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.rx = Some(rx);
            s.auto = auto;
            s.status = "확인하는 중…".into();
        });
    }

    fn update_install_start(&mut self, rel: Release) {
        let Some(exe) = running_exe() else { return };
        if update::is_dev_build(&exe) {
            ST.with(|s| s.borrow_mut().status = format!("새 버전 v{}이 있습니다(개발 빌드라 설치하지 않음)", update::fmt_ver(rel.version)));
            return;
        }
        let (tx, rx) = channel();
        let ctx = ST.with(|s| s.borrow().ctx.clone());
        let tag = rel.tag.clone();
        std::thread::spawn(move || {
            let r = update::install(&rel, &exe).map(|_| ());
            let _ = tx.send(Msg::Installed(r, tag));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        ST.with(|s| {
            let mut s = s.borrow_mut();
            s.rx = Some(rx);
            s.status = "새 버전을 내려받는 중…".into();
        });
    }

    /// 프로그램 시작 직후 한 번 호출된다.
    pub(super) fn update_on_start(&mut self, ctx: &egui::Context) {
        ST.with(|s| s.borrow_mut().ctx = Some(ctx.clone()));
        if let Some(exe) = running_exe() {
            update::cleanup(&exe);
        }
        if self.cfg.update_check && !update::REPO.is_empty() && db::now() - self.cfg.update_last >= DAY {
            self.update_check_start(true);
        }
    }

    /// 매 프레임 호출된다(가벼워야 함).
    pub(super) fn update_tick(&mut self) {
        let msg = ST.with(|s| {
            let mut s = s.borrow_mut();
            match s.rx.as_ref().map(|r| r.try_recv()) {
                Some(Ok(m)) => {
                    s.rx = None;
                    Some(m)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    s.rx = None;
                    s.status = "업데이트 작업이 중단되었습니다".into();
                    None
                }
                _ => None,
            }
        });
        match msg {
            Some(Msg::Checked(Ok(rel))) => {
                self.cfg.update_last = db::now();
                self.cfg.save();
                // 이미 설치를 마친 버전(재시작 전)은 실행 중인 프로그램보다 새것이어도 다시 설치하지 않는다.
                let already = ST.with(|s| s.borrow().installed.as_deref() == Some(rel.tag.as_str()));
                if update::is_newer(&rel) && !already {
                    let v = update::fmt_ver(rel.version);
                    ST.with(|s| {
                        let mut s = s.borrow_mut();
                        s.available = Some(rel.clone());
                        s.status = format!("새 버전 v{v}이 있습니다");
                    });
                    if self.cfg.update_auto {
                        self.update_install_start(rel);
                    } else {
                        self.say(&format!("새 버전 v{v}이 있습니다 (옵션 > 정보에서 설치)"));
                    }
                } else {
                    let auto = ST.with(|s| {
                        let mut s = s.borrow_mut();
                        s.available = None;
                        s.status = if already {
                            format!("{} 설치 완료 — 다시 시작하면 적용됩니다", rel.tag)
                        } else {
                            format!("최신 버전입니다 (v{})", update::fmt_ver(update::current_version()))
                        };
                        s.auto
                    });
                    let _ = auto;
                }
            }
            Some(Msg::Checked(Err(e))) => {
                let auto = ST.with(|s| {
                    let mut s = s.borrow_mut();
                    s.status = format!("확인하지 못했습니다: {e}");
                    s.auto
                });
                // 오프라인 등으로 자동 확인이 실패하면 조용히 넘어간다(상태는 옵션 > 정보에서 볼 수 있다).
                let _ = auto;
            }
            Some(Msg::Installed(Ok(()), tag)) => {
                ST.with(|s| {
                    let mut s = s.borrow_mut();
                    s.installed = Some(tag.clone());
                    s.available = None;
                    s.status = format!("{tag} 설치 완료 — 다시 시작하면 적용됩니다");
                });
                self.say(&format!("새 버전 {tag}을 설치했습니다. 다시 시작하면 적용됩니다 (옵션 > 정보)"));
            }
            Some(Msg::Installed(Err(e), _)) => {
                ST.with(|s| s.borrow_mut().status = format!("설치하지 못했습니다: {e}"));
                self.say(&format!("업데이트 설치 실패: {e}"));
            }
            None => {}
        }
    }

    /// 닫을 때 호출: "지금 다시 시작"을 눌렀다면 새 프로그램을 예약 실행한다.
    pub(super) fn update_on_close(&mut self) {
        if ST.with(|s| std::mem::take(&mut s.borrow_mut().restart)) {
            if let Some(exe) = running_exe() {
                update::relaunch_later(&exe);
            }
        }
    }

    /// 옵션 > 정보 탭의 업데이트 영역.
    pub(super) fn update_section(&mut self, ui: &mut egui::Ui) {
        let dim = self.theme.dim;
        ui.label(egui::RichText::new("업데이트").strong());
        if update::REPO.is_empty() {
            ui.label(egui::RichText::new("이 빌드에는 업데이트 저장소가 설정되어 있지 않습니다.").color(dim));
            return;
        }
        let (status, available, installed, busy) = ST.with(|s| {
            let s = s.borrow();
            (s.status.clone(), s.available.clone(), s.installed.clone(), s.rx.is_some())
        });
        if ui.checkbox(&mut self.cfg.update_check, "시작할 때 하루 한 번 새 버전 확인").changed() {
            self.cfg.save();
        }
        ui.add_enabled_ui(self.cfg.update_check, |ui| {
            if ui.checkbox(&mut self.cfg.update_auto, "새 버전이 있으면 자동으로 내려받아 설치 (다시 시작하면 적용)").changed() {
                self.cfg.save();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("지금 확인")).clicked() {
                self.update_check_start(false);
            }
            if let (Some(rel), false) = (&available, busy) {
                if ui.button(format!("v{} 설치", update::fmt_ver(rel.version))).clicked() {
                    self.update_install_start(rel.clone());
                }
            }
            if installed.is_some() && ui.button("지금 다시 시작").on_hover_text("저장하고 닫은 뒤 새 버전으로 다시 엽니다").clicked() {
                ST.with(|s| s.borrow_mut().restart = true);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if !status.is_empty() {
            ui.label(egui::RichText::new(status).color(dim));
        }
        ui.label(egui::RichText::new(format!("출처: github.com/{} (내려받은 파일이 손상되지 않았는지 릴리스의 SHA256 해시로 확인한 뒤 설치합니다)", update::REPO)).color(dim).size(crate::theme::ui(-1.0)));
    }
}
