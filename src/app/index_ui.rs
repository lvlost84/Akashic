//! 인덱스 문서 지정과 "인덱스 정리" 창. 규칙은 docindex.rs, 여기는 화면과 동작만.
use super::*;
use crate::docindex::IndexScan;

#[derive(Default)]
pub(super) struct IndexWin {
    pub open: bool,
    scan: IndexScan,
    add_sel: Vec<bool>,
    del_sel: Vec<bool>,
}

impl App {
    /// 문서를 인덱스로 지정/해제한다(이 PC 전용).
    pub(super) fn index_set(&mut self, doc: i64, on: bool) {
        match self.db.set_index_doc(on.then_some(doc)) {
            Ok(()) => {
                self.index_win.open = false;
                self.graph_stale = true; // 인덱스 문서는 그래프·백링크에서 빠지므로 다시 만든다
                self.compute_links();
                self.say(if on {
                    "이 문서를 인덱스로 지정했습니다. 새 문서에 제목이 생기면 알맞은 섹션에 [[링크]]를 덧붙입니다"
                } else {
                    "인덱스 지정을 해제했습니다"
                });
            }
            Err(x) => self.say(&format!("인덱스 지정 실패: {x}")),
        }
    }

    /// "인덱스 정리…": 빠진 문서와 끊긴 링크를 찾아 창을 연다.
    pub(super) fn index_open(&mut self) {
        if !self.flush() {
            return;
        }
        match self.db.index_scan() {
            Ok(None) => self.say("인덱스 문서가 없습니다. 문서를 우클릭해 먼저 지정해 주세요"),
            Ok(Some(s)) if s.missing.is_empty() && s.broken.is_empty() => self.say("정리할 것이 없습니다"),
            Ok(Some(s)) => {
                self.index_win = IndexWin { open: true, add_sel: vec![true; s.missing.len()], del_sel: vec![false; s.broken.len()], scan: s };
            }
            Err(x) => self.say(&format!("인덱스 검사 실패: {x}")),
        }
    }

    fn index_apply_sel(&mut self) {
        // 창을 연 뒤 인덱스 문서를 고쳤을 수 있으니 먼저 저장해 DB와 화면을 맞춘다.
        if !self.flush() {
            return;
        }
        let w = &self.index_win;
        let ids: Vec<i64> = w.scan.missing.iter().zip(&w.add_sel).filter(|(_, s)| **s).map(|(m, _)| m.id).collect();
        let del: Vec<String> = w.scan.broken.iter().zip(&w.del_sel).filter(|(_, s)| **s).map(|(t, _)| t.clone()).collect();
        match self.db.index_apply(&ids, &del) {
            Ok(true) => {
                self.list_dirty = true;
                self.ref_rev += 1;
                self.index_reload_cur();
                self.say(&format!("인덱스에 {}건 추가, {}건 삭제했습니다(변경 이력에서 되돌릴 수 있습니다)", ids.len(), del.len()));
            }
            Ok(false) => self.say("바뀐 것이 없습니다"),
            Err(x) => self.say(&format!("인덱스 정리 실패: {x}")),
        }
        self.index_win.open = false;
    }

    /// 편집하지 않는 채 열려 있는 문서가 인덱스인데 DB 내용이 달라졌으면(가져오기 등이 링크를 덧붙임) 다시 불러온다.
    /// 그대로 두면 다음 저장 때 화면의 옛 내용이 덧붙은 링크를 덮어쓴다.
    pub(super) fn index_reload_if_stale(&mut self) {
        let Some(id) = self.cur else { return };
        if self.last_edit.is_some() || self.db.index_doc() != Some(id) {
            return;
        }
        let Ok(Some(d)) = self.db.get_doc(id) else { return };
        if matches!(self.conceal_body(), Ok(stored) if stored != d.body) {
            self.index_reload_cur();
        }
    }

    /// 지금 열려 있는 문서가 인덱스면 DB의 새 내용으로 다시 불러온다.
    fn index_reload_cur(&mut self) {
        let Some(id) = self.cur else { return };
        if self.db.index_doc() != Some(id) {
            return;
        }
        if let Ok(Some(d)) = self.db.get_doc(id) {
            self.title = d.title;
            self.body = self.reveal_body(&d.body);
            self.cursor_idx = self.cursor_idx.min(self.body.chars().count());
            self.cursor_line = None;
            self.last_edit = None;
            self.last_saved_ts = now();
            self.compute_links();
        }
    }

    pub(super) fn index_window(&mut self, ctx: &egui::Context) {
        let dim = self.theme.dim;
        let mut open = self.index_win.open;
        let mut apply = false;
        let mut cancel = false;
        let IndexWin { scan, add_sel, del_sel, .. } = &mut self.index_win;
        egui::Window::new("인덱스 정리").open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            ui.label(egui::RichText::new("체크한 항목만 인덱스 문서에 반영합니다. 인덱스 문서는 변경 이력(최근 10개)에 남아 되돌릴 수 있습니다.").size(crate::theme::ui(0.0)).color(dim));
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("인덱스에 빠진 문서 ({})", scan.missing.len())).strong());
                    if ui.small_button("전체 선택").clicked() {
                        add_sel.iter_mut().for_each(|s| *s = true);
                    }
                    if ui.small_button("전체 해제").clicked() {
                        add_sel.iter_mut().for_each(|s| *s = false);
                    }
                });
                if scan.missing.is_empty() {
                    ui.label(egui::RichText::new("없음").color(dim));
                }
                for (m, sel) in scan.missing.iter().zip(add_sel.iter_mut()) {
                    ui.checkbox(sel, format!("{}   →  {}", m.title, m.section));
                }
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("끊긴 링크 ({})", scan.broken.len())).strong());
                    if ui.small_button("전체 선택").clicked() {
                        del_sel.iter_mut().for_each(|s| *s = true);
                    }
                    if ui.small_button("전체 해제").clicked() {
                        del_sel.iter_mut().for_each(|s| *s = false);
                    }
                });
                ui.label(egui::RichText::new("제목이 같은 문서가 없는 링크입니다. 오타나 지운 문서일 수 있어 기본은 선택하지 않습니다. 체크한 것만 인덱스에서 지웁니다.").size(crate::theme::ui(0.0)).color(dim));
                if scan.broken.is_empty() {
                    ui.label(egui::RichText::new("없음").color(dim));
                }
                for (t, sel) in scan.broken.iter().zip(del_sel.iter_mut()) {
                    ui.checkbox(sel, format!("[[{t}]]"));
                }
            });
            ui.separator();
            let na = add_sel.iter().filter(|s| **s).count();
            let nd = del_sel.iter().filter(|s| **s).count();
            ui.horizontal(|ui| {
                if ui.add_enabled(na + nd > 0, egui::Button::new(format!("적용 (추가 {na} · 삭제 {nd})"))).clicked() {
                    apply = true;
                }
                if ui.button("취소").clicked() {
                    cancel = true;
                }
            });
        });
        self.index_win.open = open && !cancel;
        if apply {
            self.index_apply_sel();
        }
    }
}
