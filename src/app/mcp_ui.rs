//! MCP 서버 연동 UI·상태: 옵션(HTTP 서버 가동·쓰기 허용), 서버 시작/중지, 다른 곳이 바꾼 DB 반영. 프로토콜은 mcp.rs.
use super::*;
use std::sync::atomic::Ordering;

impl App {
    /// 매 프레임: 옵션에 맞게 HTTP 서버를 켜고 끄고, 쓰기를 허용하면 다른 곳(MCP)의 변경을 화면에 반영한다.
    pub(super) fn mcp_tick(&mut self, ctx: &egui::Context) {
        self.mcp_write_flag.store(self.cfg.mcp_write, Ordering::Relaxed);
        match (&self.mcp_http, self.cfg.mcp_http) {
            (None, true) => self.mcp_start(),
            (Some(_), false) => self.mcp_http = None,
            // 포트 입력 중(포커스·드래그)에는 기다렸다가 끝나면 바꾼다
            (Some(h), true) if h.port != self.cfg.mcp_port => {
                if !ctx.input(|i| i.pointer.any_down()) && ctx.memory(|m| m.focused().is_none()) {
                    self.mcp_http = None;
                    self.mcp_start();
                }
            }
            _ => {}
        }
        if self.cfg.mcp_write {
            self.mcp_watch(ctx);
        }
    }

    fn mcp_start(&mut self) {
        if self.cfg.mcp_token.len() < 16 {
            match crate::mcp::new_token() {
                Ok(t) => self.cfg.mcp_token = t,
                Err(x) => {
                    self.cfg.mcp_http = false;
                    self.say(&format!("MCP 서버를 켜지 못했습니다: {x}"));
                    return;
                }
            }
            self.cfg.save();
        }
        let port = self.cfg.mcp_port.max(1024);
        match crate::mcp::start_http(port, self.cfg.mcp_token.clone(), self.db.path.clone(), self.mcp_write_flag.clone()) {
            Ok(h) => {
                self.mcp_http = Some(h);
                self.say(&format!("MCP 서버를 켰습니다 (127.0.0.1:{port})"));
            }
            Err(x) => {
                self.cfg.mcp_http = false; // 다시 시도를 반복하지 않도록 끈다
                self.say(&format!("MCP 서버를 켜지 못했습니다: {x}"));
            }
        }
    }

    /// 3초마다 DB 변경 번호를 보고, 바뀌었으면 목록·그래프를 다시 읽고 열린 문서가 바뀌었는지 살핀다.
    fn mcp_watch(&mut self, ctx: &egui::Context) {
        const EVERY: Duration = Duration::from_secs(3);
        ctx.request_repaint_after(EVERY);
        if self.mcp_polled.elapsed() < EVERY {
            return;
        }
        self.mcp_polled = Instant::now();
        let rev = self.db.rev();
        if rev == self.mcp_seen_rev {
            return;
        }
        let first = self.mcp_seen_rev == 0;
        self.mcp_seen_rev = rev;
        if first {
            return;
        }
        self.list_dirty = true;
        self.graph_stale = true;
        self.ref_rev += 1;
        let Some(id) = self.cur else { return };
        let Ok(Some(d)) = self.db.get_doc(id) else { return };
        if self.last_edit.is_none() {
            // 편집 중이 아니면: 저장본과 화면이 다를 때만 다시 불러온다(내가 방금 저장한 경우는 같다).
            match self.conceal_body() {
                Ok(stored) if stored == d.body && self.title == d.title && self.cur_trashed == d.trashed => {}
                Ok(_) => self.mcp_reload_cur(ctx, d),
                Err(_) => {}
            }
        } else if d.updated > self.last_saved_ts.max(self.mcp_since) && d.updated != self.mcp_warned {
            // 편집 중인 문서가 밖에서 바뀐 듯하다. 덮어쓰지는 않고 알린다(저장하면 이 편집이 이기고, 이전 내용은 변경 이력에 남는다).
            self.mcp_warned = d.updated;
            self.say("이 문서가 다른 곳(MCP)에서 바뀐 것 같습니다. 지금 저장하면 편집 중인 내용이 우선하며, 이전 내용은 변경 이력에서 되살릴 수 있습니다");
        }
    }

    /// 편집하지 않던 열린 문서를 DB의 새 내용으로 바꿔 보인다.
    fn mcp_reload_cur(&mut self, ctx: &egui::Context, d: crate::db::Doc) {
        let id = d.id;
        self.title = d.title;
        self.body = self.reveal_body(&d.body);
        self.cur_trashed = d.trashed;
        self.cur_pinned = d.pinned;
        self.cur_cat = d.category;
        self.doc_tags = self.db.doc_tags(id).unwrap_or_default();
        self.last_edit = None;
        self.cursor_idx = self.cursor_idx.min(self.body.chars().count());
        self.compute_links();
        use egui::text::{CCursor, CCursorRange};
        #[allow(deprecated)]
        let eid = egui::Id::new("body_edit");
        let mut st = egui::TextEdit::load_state(ctx, eid).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(self.cursor_idx))));
        st.clear_undoer();
        st.store(ctx, eid);
        ctx.request_repaint();
        self.say("다른 곳(MCP)에서 바뀐 내용을 열려 있는 문서에 반영했습니다 (되돌리기 기록이 초기화되었습니다)");
    }

    /// 옵션 > 백업·동기화 탭의 MCP 구역.
    pub(super) fn mcp_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        let dim = self.theme.dim;
        let small = |t: &str| egui::RichText::new(t).size(crate::theme::ui(0.0)).color(dim);
        ui.separator();
        ui.label(egui::RichText::new("MCP 서버 (AI 연동)").strong());
        ui.label(small("Claude Code 같은 MCP 클라이언트가 이 문서 저장소를 읽게 합니다. 암호화한 글은 '암호화됨'으로 가려서 보냅니다. 아카식은 API를 호출하지 않아 이 기능 자체에는 비용이 들지 않습니다(AI 쪽 사용량은 해당 서비스 요금제를 따릅니다)."));
        ui.checkbox(&mut self.cfg.mcp_write, "쓰기 도구 허용 (문서 만들기·고치기·덧붙이기)")
            .on_hover_text("끄면 읽기 전용입니다. 고친 문서의 이전 내용은 변경 이력(문서당 10개)에 남습니다");
        ui.separator();
        ui.checkbox(&mut self.cfg.mcp_http, "앱이 켜져 있는 동안 서버 가동 (HTTP, 이 PC 안에서만)");
        if self.cfg.mcp_http {
            ui.horizontal(|ui| {
                ui.label("포트");
                ui.add(egui::DragValue::new(&mut self.cfg.mcp_port).range(1024..=65535));
            });
            let port = self.cfg.mcp_port;
            let url = format!("http://127.0.0.1:{port}/mcp");
            ui.label(small(&match &self.mcp_http {
                Some(_) => format!("가동 중 · {url}"),
                None => "멈춤(포트를 열 수 없으면 알림이 뜹니다)".to_string(),
            }));
            ui.horizontal_wrapped(|ui| {
                if ui.button("등록 명령 복사 (Claude Code)").on_hover_text("토큰이 들어 있으니 다른 사람에게 보이지 마세요").clicked() {
                    ui.ctx().copy_text(format!(
                        "claude mcp add --transport http akashic {url} --header \"Authorization: Bearer {}\"",
                        self.cfg.mcp_token
                    ));
                    notes.push("등록 명령을 복사했습니다(토큰 포함)".into());
                }
                if ui.button("토큰 새로 만들기").on_hover_text("이전 토큰을 쓰던 클라이언트는 다시 등록해야 합니다").clicked() {
                    if let Ok(t) = crate::mcp::new_token() {
                        self.cfg.mcp_token = t;
                        self.mcp_http = None; // 새 토큰으로 다시 시작
                        notes.push("토큰을 새로 만들었습니다. 클라이언트를 다시 등록하세요".into());
                    }
                }
            });
        }
        ui.separator();
        ui.label(small("앱을 켜 두지 않고 쓰려면(클라이언트가 아카식을 직접 실행):"));
        let exe = std::env::current_exe().map(|p| crate::update::original_path(&p)).map(|p| p.display().to_string()).unwrap_or_else(|_| "Akashic.exe".into());
        let cmd = format!("claude mcp add akashic -- \"{exe}\" --mcp");
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&cmd).monospace().size(crate::theme::ui(-1.0)));
            if ui.button("복사").clicked() {
                ui.ctx().copy_text(cmd.clone());
                notes.push("등록 명령을 복사했습니다".into());
            }
        });
    }
}
