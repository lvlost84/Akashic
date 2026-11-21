//! "변경 이력" 창: 문서마다 최근 10개의 이전 내용을 보고, 현재 본문과 줄 단위로 비교하고, 되돌린다.
use super::*;
use diffy::Line;

/// 오른쪽에 보여 줄 내용 한 줄: 0 그대로, 1 되돌리면 사라질 줄, 2 되돌리면 생길 줄, 3 생략 표시.
type ViewLine = (u8, String);

#[derive(Default)]
pub(super) struct Hist {
    pub open: bool,
    doc: Option<i64>,
    list: Vec<crate::db::Version>,
    sel: Option<i64>,
    /// true면 차이 대신 그 시점 본문 전체를 보인다
    full: bool,
    confirm_clear: bool,
    /// (이력 번호, 현재 본문 지문, 전체 보기, 줄들) — 바뀔 때만 다시 만든다
    view: Option<(i64, u64, bool, Vec<ViewLine>)>,
}

fn shown(text: &str) -> String {
    crate::textcrypt::mask_for_display(text, None).unwrap_or_else(|| text.to_string())
}

fn clock_full(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

/// 현재 본문 `cur`을 이력 `old`로 되돌릴 때 바뀌는 줄(앞뒤 문맥 3줄 포함).
fn diff_lines(cur: &str, old: &str) -> Vec<ViewLine> {
    let patch = diffy::create_patch(cur, old);
    let mut out: Vec<ViewLine> = Vec::new();
    for (n, h) in patch.hunks().iter().enumerate() {
        if n > 0 {
            out.push((3, "⋯".into()));
        }
        for l in h.lines() {
            let (k, t) = match l {
                Line::Context(t) => (0, t),
                Line::Delete(t) => (1, t),
                Line::Insert(t) => (2, t),
            };
            out.push((k, t.trim_end_matches('\n').to_string()));
        }
    }
    out
}

impl App {
    /// 현재 문서의 변경 이력 창을 연다(먼저 저장해서 최신 상태를 이력에 반영).
    pub(super) fn hist_open(&mut self) {
        if !self.flush() {
            return;
        }
        let Some(id) = self.cur else { return };
        self.hist.doc = Some(id);
        self.hist.sel = None;
        self.hist.confirm_clear = false;
        self.hist.view = None;
        self.hist_reload();
        self.hist.open = true;
    }

    fn hist_reload(&mut self) {
        let Some(id) = self.hist.doc else { return };
        self.hist.list = self.db.versions(id).unwrap_or_default();
        if !self.hist.list.iter().any(|v| Some(v.id) == self.hist.sel) {
            self.hist.sel = self.hist.list.first().map(|v| v.id);
        }
        self.hist.view = None;
    }

    /// 이력 `vid`를 현재 문서에 되돌린다.
    fn hist_restore(&mut self, vid: i64) {
        let Some(id) = self.cur else { return };
        if !self.flush() {
            return;
        }
        match self.db.restore_version(id, vid) {
            Ok(true) => {
                if let Ok(Some(d)) = self.db.get_doc(id) {
                    self.title = d.title;
                    self.body = self.reveal_body(&d.body);
                    self.cursor_line = None;
                    self.last_edit = None;
                    self.last_saved_ts = now();
                    self.list_dirty = true;
                    self.ref_rev += 1;
                    self.compute_links();
                    self.say("이력으로 되돌렸습니다(되돌리기 전 내용도 이력에 남았습니다)");
                }
            }
            Ok(false) => self.say("그 이력을 찾을 수 없습니다"),
            Err(x) => self.say(&format!("되돌리기 실패: {x}")),
        }
        self.hist_reload();
    }

    pub(super) fn hist_window(&mut self, ctx: &egui::Context) {
        if self.hist.doc != self.cur {
            self.hist.open = false; // 다른 문서로 옮기면 닫는다
            return;
        }
        let key = crate::render::cache_key(&self.body);
        // 선택한 이력의 표시 내용을 (필요할 때만) 만든다.
        if let Some(sid) = self.hist.sel {
            let stale = self.hist.view.as_ref().map_or(true, |v| v.0 != sid || v.1 != key || v.2 != self.hist.full);
            if stale {
                if let Some(v) = self.hist.list.iter().find(|v| v.id == sid).cloned() {
                    let old = shown(&self.reveal_body(&v.body));
                    let lines = if self.hist.full {
                        old.lines().map(|l| (0u8, l.to_string())).collect()
                    } else {
                        diff_lines(&shown(&self.body), &old)
                    };
                    self.hist.view = Some((sid, key, self.hist.full, lines));
                }
            }
        }
        let dim = self.theme.dim;
        let cur_title = self.title.clone();
        let mut open = self.hist.open;
        let mut restore: Option<i64> = None;
        let mut del: Option<i64> = None;
        let mut clear = false;
        let h = &mut self.hist;
        let mut copy = false;
        let mut builder = egui::ViewportBuilder::default().with_title("아카식 변경 이력").with_inner_size([860.0, 560.0]).with_min_inner_size([520.0, 320.0]);
        if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon-128.png")) {
            builder = builder.with_icon(icon);
        }
        // 본창 안의 떠 있는 창이 아니라 별도 OS 창이라 본창 밖으로 옮기고 크기를 마음대로 바꿀 수 있다.
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("akashic_history"), builder, |ui, _class| {
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                open = false;
            }
            egui::Panel::top("hist_top").show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("저장할 때마다 바뀌기 전 내용을 문서당 최근 10개까지 이 PC에 남깁니다(동기화되지 않음).").size(crate::theme::ui(0.0)).color(dim));
                ui.add_space(2.0);
            });
            if h.list.is_empty() {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("아직 이력이 없습니다. 내용을 고치고 저장하면 생깁니다.");
                });
                return;
            }
            egui::Panel::bottom("hist_btns").show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has = h.sel.is_some();
                    if ui.add_enabled(has, egui::Button::new("이 버전으로 되돌리기")).on_hover_text("되돌리기 전의 현재 내용도 이력에 남습니다").clicked() {
                        restore = h.sel;
                    }
                    if ui.add_enabled(has, egui::Button::new("이 버전 내용 복사")).on_hover_text("그 시점의 본문 전체를 클립보드에 복사합니다").clicked() {
                        copy = true;
                    }
                    if ui.add_enabled(has, egui::Button::new("이 이력 지우기")).clicked() {
                        del = h.sel;
                    }
                    if h.confirm_clear {
                        if ui.button("정말 모두 지우기").clicked() {
                            clear = true;
                            h.confirm_clear = false;
                        }
                        if ui.button("취소").clicked() {
                            h.confirm_clear = false;
                        }
                    } else if ui.button("이 문서 이력 모두 지우기").clicked() {
                        h.confirm_clear = true;
                    }
                });
                ui.add_space(4.0);
            });
            egui::Panel::left("hist_list").default_size(210.0).size_range(150.0..=420.0).resizable(true).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("hist_list").auto_shrink([false, false]).show(ui, |ui| {
                    for v in &h.list {
                        let t = format!("{}  {}", clock_full(v.saved_at), v.title.trim());
                        if ui.selectable_label(h.sel == Some(v.id), t).clicked() {
                            h.sel = Some(v.id);
                        }
                    }
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut h.full, false, "현재와 차이");
                    ui.radio_value(&mut h.full, true, "그 시점 전체");
                });
                if !h.full {
                    ui.label(egui::RichText::new("− 빨강: 되돌리면 사라질 줄    + 초록: 되돌리면 생길 줄").size(crate::theme::ui(0.0)).color(dim));
                }
                if let Some(v) = h.sel.and_then(|s| h.list.iter().find(|v| v.id == s)) {
                    if v.title != cur_title {
                        ui.label(egui::RichText::new(format!("제목: {} → {}", cur_title.trim(), v.title.trim())).color(dim));
                    }
                }
                ui.separator();
                egui::ScrollArea::both().id_salt("hist_view").auto_shrink([false, false]).show(ui, |ui| {
                    if let Some((_, _, _, lines)) = &h.view {
                        if lines.is_empty() {
                            ui.label("현재 본문과 같습니다(제목만 다를 수 있습니다).");
                        }
                        for (k, t) in lines {
                            let (pre, col) = match k {
                                1 => ("− ", Some(egui::Color32::from_rgb(210, 80, 80))),
                                2 => ("+ ", Some(egui::Color32::from_rgb(70, 160, 90))),
                                3 => ("", Some(dim)),
                                _ => (if h.full { "" } else { "  " }, None),
                            };
                            let txt = egui::RichText::new(format!("{pre}{t}")).monospace();
                            ui.label(match col {
                                Some(c) => txt.color(c),
                                None => txt,
                            });
                        }
                    }
                });
            });
        });
        if copy {
            if let Some(v) = self.hist.sel.and_then(|s| self.hist.list.iter().find(|v| v.id == s)).cloned() {
                let plain = self.reveal_body(&v.body);
                ctx.copy_text(plain);
                self.say("그 시점의 본문을 복사했습니다");
            }
        }
        self.hist.open = open;
        if let Some(v) = restore {
            self.hist_restore(v);
        }
        if let Some(v) = del {
            let _ = self.db.delete_version(v);
            self.hist_reload();
        }
        if clear {
            if let Some(id) = self.hist.doc {
                let _ = self.db.clear_versions(id);
            }
            self.hist_reload();
        }
    }
}
