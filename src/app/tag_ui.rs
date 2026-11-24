//! 문서 머리의 태그 입력칸과 기존 태그 자동완성 팝업.

use super::*;

impl App {
    /// 태그 입력칸을 그린다. 마지막으로 치는 조각에 맞는 기존 태그가 후보로 뜨고, ↑↓로 고른 뒤 Enter·Tab·클릭으로 넣는다(Esc로 닫음).
    pub(super) fn tag_input(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, acts: &mut Vec<Act>) {
        #[allow(deprecated)]
        let eid = egui::Id::new("tag_edit");
        let editable = !self.cur_trashed;
        // 한글 조합 상태 추적(조합 중에는 ↑↓·Enter·Tab·Esc를 후보가 쓰지 않는다)
        let composing_before = self.tag_composing;
        let mut ime_this_frame = false;
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Ime(ev) = e {
                    match ev {
                        egui::ImeEvent::Preedit { text, .. } => self.tag_composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => {
                            self.tag_composing = false;
                            ime_this_frame = true;
                        }
                        _ => {}
                    }
                }
            }
        });
        let ime_busy = composing_before || self.tag_composing || ime_this_frame;
        // 팝업에서 고른 태그를 입력칸에 넣고 입력칸으로 포커스를 되돌린다(그리기 전에 적용).
        let mut enter_add = false;
        if let Some(tag) = self.tag_pick.take() {
            self.new_tag = crate::tagsuggest::complete(&self.new_tag, &tag);
            self.tag_sel = None;
            ctx.memory_mut(|m| m.request_focus(eid));
            self.tag_cursor_end(ctx, eid);
        }
        let focused = ctx.memory(|m| m.has_focus(eid));
        // 키: 목록이 떠 있을 때만 쓴다(그리기 전에 소비해 입력칸이 같은 키를 쓰지 않게).
        if focused && editable && !ime_busy && !self.tag_items.is_empty() {
            let n = self.tag_items.len();
            if key_exact(ctx, Modifiers::NONE, Key::ArrowDown) {
                self.tag_sel = Some(self.tag_sel.map_or(0, |s| (s + 1) % n));
            } else if key_exact(ctx, Modifiers::NONE, Key::ArrowUp) {
                self.tag_sel = Some(self.tag_sel.map_or(n - 1, |s| (s + n - 1) % n));
            } else if key_exact(ctx, Modifiers::NONE, Key::Tab) {
                let tag = self.tag_items[self.tag_sel.unwrap_or(0).min(n - 1)].clone();
                self.tag_pick = Some(tag);
                ctx.request_repaint();
            } else if self.tag_sel.is_some() && key_exact(ctx, Modifiers::NONE, Key::Enter) {
                // 고른 항목이 있을 때만 Enter로 그 태그를 넣는다(고르지 않았으면 쓴 글 그대로 추가).
                let tag = self.tag_items[self.tag_sel.unwrap_or(0).min(n - 1)].clone();
                self.new_tag = crate::tagsuggest::complete(&self.new_tag, &tag);
                enter_add = true;
            } else if key_exact(ctx, Modifiers::NONE, Key::Escape) {
                self.tag_esc = Some(self.new_tag.clone());
                self.tag_items.clear();
                self.tag_sel = None;
            }
        }
        let r = ui.add_enabled(editable, egui::TextEdit::singleline(&mut self.new_tag).id(eid).hint_text("태그 추가").desired_width(90.0));
        if enter_add && !self.new_tag.trim().is_empty() {
            acts.push(Act::AddTag);
            r.request_focus();
        } else if r.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) && !self.new_tag.trim().is_empty() {
            acts.push(Act::AddTag);
            r.request_focus();
        }
        if r.changed() {
            self.tag_sel = None;
            self.tag_esc = None;
        }
        // 팝업을 누르는 순간 입력칸이 포커스를 잃으므로, 눌린 채 팝업 안에 있으면 후보를 유지한다.
        let in_popup = self.tag_popup_rect.is_some_and(|p| ctx.input(|i| i.pointer.any_down() && i.pointer.interact_pos().is_some_and(|q| p.contains(q))));
        if !editable || (!focused && !r.has_focus() && !in_popup) {
            self.tag_items.clear();
            self.tag_sel = None;
            self.tag_popup_rect = None;
            return;
        }
        if r.has_focus() || focused {
            let rev = self.db.rev();
            if self.tag_cache.0 != rev {
                self.tag_cache = (rev, self.db.all_tags().unwrap_or_default());
            }
            self.tag_items = if self.tag_esc.as_deref() == Some(self.new_tag.as_str()) {
                Vec::new()
            } else {
                crate::tagsuggest::candidates(&self.tag_cache.1, &self.doc_tags, &self.new_tag, crate::tagsuggest::MAX_ITEMS)
            };
            if let Some(s) = self.tag_sel {
                if s >= self.tag_items.len() {
                    self.tag_sel = None;
                }
            }
        }
        if self.tag_items.is_empty() {
            self.tag_popup_rect = None;
            return;
        }
        let pressed = ctx.input(|i| i.pointer.primary_pressed());
        let mut pick: Option<String> = None;
        #[allow(deprecated)]
        let aid = egui::Id::new("tag_suggest");
        let items = self.tag_items.clone();
        let sel = self.tag_sel;
        let area = egui::Area::new(aid).order(egui::Order::Foreground).fixed_pos(r.rect.left_bottom() + egui::vec2(0.0, 2.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(260.0);
                for (i, t) in items.iter().enumerate() {
                    let b = ui.add(egui::Button::selectable(sel == Some(i), t.as_str()).truncate());
                    if b.clicked() || (pressed && b.hovered()) {
                        pick = Some(t.clone());
                    }
                }
            });
        });
        self.tag_popup_rect = Some(area.response.rect);
        if let Some(t) = pick {
            self.tag_pick = Some(t);
            ctx.request_repaint();
        }
    }

    /// 입력칸 커서를 글 끝으로 옮긴다.
    fn tag_cursor_end(&self, ctx: &egui::Context, id: egui::Id) {
        let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        let n = self.new_tag.chars().count();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
        st.store(ctx, id);
    }
}
