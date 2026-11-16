//! 전체 내보내기 UI: 옵션 창의 "내보내기" 구역과 백그라운드 쓰기 결과 수신.
use super::*;
use crate::export::{self, ExportResult};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

/// 진행 중인 내보내기의 결과 수신기(앱 구조체를 건드리지 않으려고 모듈 전역으로 둔다).
static RUNNING: Mutex<Option<Receiver<Result<ExportResult, String>>>> = Mutex::new(None);
/// 마지막으로 끝난 내보내기 폴더.
static LAST_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

impl App {
    /// 옵션 창의 "내보내기" 구역.
    pub(super) fn export_options(&mut self, ui: &mut egui::Ui, _notes: &mut Vec<String>) {
        ui.separator();
        ui.label(egui::RichText::new("내보내기").strong());
        let running = RUNNING.lock().map(|g| g.is_some()).unwrap_or(false);
        let last = LAST_DIR.lock().ok().and_then(|g| g.clone());
        ui.horizontal_wrapped(|ui| {
            let b = crate::icons::btn_ex(ui, crate::icons::Icon::Export, if running { "내보내는 중…" } else { "전체를 분류별 폴더로 내보내기…" }, !running, None)
                .on_hover_text("휴지통을 뺀 모든 문서를 분류 폴더 구조의 .md 파일로 저장합니다");
            if b.clicked() {
                self.start_export(ui.ctx());
            }
            if let Some(d) = last {
                if crate::icons::btn(ui, crate::icons::Icon::Folder, "마지막 내보내기 폴더 열기").clicked() {
                    let _ = std::process::Command::new("explorer").arg(d).spawn();
                }
            }
        });
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        let Some(parent) = rfd::FileDialog::new().set_title("내보낼 위치 선택").pick_folder() else { return };
        self.flush(); // 편집 중인 내용까지 저장한 뒤, 읽기·쓰기는 모두 별도 스레드에서 한다(큰 DB에서도 화면이 멈추지 않게)
        let src = self.db.path.clone();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = crate::db::Db::export_data_from_file(&src).map(|(docs, cats)| export::run_export(&parent, &stamp, &docs, &cats));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        if let Ok(mut g) = RUNNING.lock() {
            *g = Some(rx);
        }
        self.say("내보내는 중…");
    }

    /// 매 프레임 호출: 끝난 내보내기 결과를 상태바에 알린다.
    pub(super) fn poll_export(&mut self) {
        let mut crashed = false;
        let res = {
            let Ok(mut g) = RUNNING.lock() else { return };
            let Some(rx) = g.as_ref() else { return };
            match rx.try_recv() {
                Ok(r) => {
                    *g = None;
                    Some(r)
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    *g = None;
                    crashed = true; // 작업 스레드가 도중에 죽었다
                    None
                }
            }
        };
        if crashed {
            self.say("내보내기 실패: 내부 오류로 작업이 중단되었습니다");
        }
        let res = match res {
            Some(Ok(r)) => Some(r),
            Some(Err(x)) => {
                self.say(&format!("내보내기 실패: {x}"));
                None
            }
            None => None,
        };
        if let Some(r) = res {
            // 하나도 쓰지 못했다면(폴더 생성 실패 등) "마지막 내보내기 폴더"로 기억하지 않는다.
            if r.ok > 0 {
                if let Ok(mut l) = LAST_DIR.lock() {
                    *l = Some(r.dir.clone());
                }
            }
            let msg = if r.failed == 0 {
                format!("내보내기 완료: {}개 → {}", r.ok, r.dir.display())
            } else {
                format!("내보내기: {}개 완료, {}개 실패 ({}) → {}", r.ok, r.failed, r.first_error.unwrap_or_default(), r.dir.display())
            };
            self.say(&msg);
        }
    }
}
