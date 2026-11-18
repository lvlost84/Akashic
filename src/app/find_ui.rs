//! 문서 안 찾기(Ctrl+F): 찾기 막대, 일치 목록, 이전/다음 이동. 일치 표시는 본문 뒤에 깔린다(`paint_blocks_behind`).
use super::*;
use egui::text::{CCursor, CCursorRange};

#[derive(Default)]
pub(super) struct Find {
    pub open: bool,
    pub query: String,
    pub case: bool,
    /// 일치 위치(char 번호 시작, 끝)
    pub matches: Vec<(usize, usize)>,
    pub cur: usize,
    /// 본문·검색어·옵션 지문(바뀔 때만 다시 찾는다)과 검색어 지문
    sig: u64,
    qsig: u64,
    want_focus: bool,
    /// 한글 등 IME 조합 중인지(조합을 끝내는 Enter는 "다음"으로 보지 않는다)
    composing: bool,
    /// 다음 그리기에서 현재 일치로 이동
    pub jump: bool,
    /// 찾아 바꾸기(Ctrl+H) 줄 표시, 바꿀 글
    pub replace_open: bool,
    pub repl: String,
    /// 하나 바꾼 직후: 이 글자 위치 뒤의 첫 일치로 이동(다음 갱신 때 한 번)
    after_replace: Option<usize>,
}

#[allow(deprecated)]
fn repl_id() -> egui::Id {
    egui::Id::new("find_repl")
}

#[allow(deprecated)]
fn edit_id() -> egui::Id {
    egui::Id::new("find_edit")
}

impl App {
    /// 단축키: Ctrl+F 열기, F3/Shift+F3 다음/이전, Esc 닫기.
    pub(super) fn find_keys(&mut self, ctx: &egui::Context, body_id: egui::Id) {
        use egui::{Key, Modifiers};
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            // 본문에서 한 줄짜리 글자를 선택해 두었으면 그것으로 미리 채운다.
            if let Some(r) = egui::TextEdit::load_state(ctx, body_id).and_then(|s| s.cursor.char_range()) {
                let (a, b) = (usize::from(r.primary.index), usize::from(r.secondary.index));
                let (lo, hi) = (a.min(b), a.max(b));
                if hi > lo && hi - lo <= 100 {
                    let s: String = self.body.chars().skip(lo).take(hi - lo).collect();
                    if !s.contains('\n') {
                        self.find.query = s;
                    }
                }
            }
            self.find.open = true;
            self.find.want_focus = true;
        }
        if !self.cur_trashed && super::key_exact(ctx, Modifiers::COMMAND, Key::H) {
            // Ctrl+H: 찾기 막대를 바꾸기 줄과 함께 연다(선택한 글이 있으면 검색어로 채운다)
            if let Some(r) = egui::TextEdit::load_state(ctx, body_id).and_then(|s| s.cursor.char_range()) {
                let (a, b) = (usize::from(r.primary.index), usize::from(r.secondary.index));
                let (lo, hi) = (a.min(b), a.max(b));
                if hi > lo && hi - lo <= 100 {
                    let t: String = self.body.chars().skip(lo).take(hi - lo).collect();
                    if !t.contains('\n') {
                        self.find.query = t;
                    }
                }
            }
            self.find.open = true;
            self.find.replace_open = true;
            self.find.want_focus = true;
        }
        if !self.find.open {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::F3)) {
            self.find_step(false);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F3)) {
            self.find_step(true);
        }
        // 한글 조합 중의 Esc는 조합 취소용이므로 찾기 막대를 닫지 않는다.
        if !self.find.composing && !self.show_options && !self.show_help && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.find_close(ctx, body_id);
        }
    }

    /// 다른 문서로 옮길 때: 이전 문서의 일치 위치·번호를 버린다(검색어는 유지).
    pub(super) fn find_reset_for_doc(&mut self) {
        self.find.sig = 0;
        self.find.cur = 0;
        self.find.matches.clear();
        self.find.jump = false;
    }

    fn find_close(&mut self, ctx: &egui::Context, body_id: egui::Id) {
        self.find.open = false;
        self.find.jump = false;
        self.find.composing = false;
        ctx.memory_mut(|m| m.request_focus(body_id));
    }

    /// 본문·검색어·옵션이 바뀌었을 때만 일치 목록을 다시 만든다.
    fn find_refresh(&mut self) {
        let mut h = DefaultHasher::new();
        (&self.find.query, self.find.case).hash(&mut h);
        let qs = h.finish();
        crate::render::cache_key(&self.body).hash(&mut h);
        let sig = h.finish();
        if sig == self.find.sig {
            return;
        }
        let qchanged = qs != self.find.qsig;
        self.find.sig = sig;
        self.find.qsig = qs;
        self.find.matches = md::find_all(&self.body, &self.find.query, self.find.case);
        let n = self.find.matches.len();
        if qchanged {
            // 검색어를 바꾸면 커서 뒤의 첫 일치로 간다(없으면 처음).
            let from = self.cursor_idx;
            self.find.cur = self.find.matches.iter().position(|m| m.0 >= from).unwrap_or(0);
            self.find.jump = n > 0;
        } else {
            self.find.cur = self.find.cur.min(n.saturating_sub(1));
        }
        if let Some(p) = self.find.after_replace.take() {
            // 하나 바꾼 뒤: 바꾼 글 바로 뒤의 첫 일치(없으면 처음)로 간다.
            self.find.cur = self.find.matches.iter().position(|m| m.0 >= p).unwrap_or(0);
            self.find.jump = n > 0;
        }
    }

    /// 바꾸기(현재 일치 하나) 또는 모두 바꾸기. 편집창을 그리기 전에 본문을 바꾸므로 Ctrl+Z 한 번으로 되돌아간다.
    fn find_replace(&mut self, ctx: &egui::Context, body_id: egui::Id, all: bool) {
        if self.cur_trashed || self.find.query.is_empty() {
            return;
        }
        // 이 프레임에 본문이 바뀌었을 수 있으니 일치 위치를 새로 구한다.
        let matches = md::find_all(&self.body, &self.find.query, self.find.case);
        if matches.is_empty() {
            self.say("바꿀 글이 없습니다");
            return;
        }
        let repl = self.find.repl.clone();
        let rl = repl.chars().count();
        if all {
            let (text, n, cur) = crate::editkeys::replace_matches(&self.body, &matches, &repl, self.cursor_idx);
            self.apply_body_edit(ctx, body_id, text, (cur, cur));
            self.find.jump = false;
            self.say(&format!("{n}건을 바꿨습니다"));
        } else {
            let (s, e) = matches[self.find.cur.min(matches.len() - 1)];
            let (text, _, _) = crate::editkeys::replace_matches(&self.body, &[(s, e)], &repl, s);
            self.apply_body_edit(ctx, body_id, text, (s + rl, s + rl));
            self.find.after_replace = Some(s + rl);
        }
    }

    pub(super) fn find_step(&mut self, forward: bool) {
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        self.find.cur = if forward { (self.find.cur + 1) % n } else { (self.find.cur + n - 1) % n };
        self.find.jump = true;
    }

    /// 현재 일치로 커서를 옮기고 스크롤할 글자 위치를 돌려준다(편집창을 그리기 전에 호출).
    pub(super) fn find_apply_jump(&mut self, ctx: &egui::Context, body_id: egui::Id) -> Option<usize> {
        if !std::mem::take(&mut self.find.jump) || !self.find.open {
            return None;
        }
        let &(s, e) = self.find.matches.get(self.find.cur)?;
        let mut state = egui::TextEdit::load_state(ctx, body_id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(s), CCursor::new(e))));
        state.store(ctx, body_id);
        self.cursor_idx = s;
        self.cursor_line = Some(md::line_of_char(&self.body, s));
        Some(s)
    }

    /// 찾기 막대(열려 있을 때만).
    pub(super) fn find_bar(&mut self, ui: &mut egui::Ui, body_id: egui::Id) {
        use egui::Key;
        if !self.find.open {
            return;
        }
        let ctx = ui.ctx().clone();
        self.find_refresh();
        let composing_before = self.find.composing;
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Ime(ev) = e {
                    match ev {
                        egui::ImeEvent::Preedit { text, .. } => self.find.composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => self.find.composing = false,
                        _ => {}
                    }
                }
            }
        });
        let (enter, shift) = ctx.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift));
        let dim = self.theme.dim;
        let mut close = false;
        let mut step: Option<bool> = None;
        let mut rep: Option<bool> = None;
        if self.find.want_focus {
            // 열 때 기존 검색어를 전부 선택해 바로 덮어쓸 수 있게 한다.
            let mut st = egui::TextEdit::load_state(&ctx, edit_id()).unwrap_or_default();
            let len = self.find.query.chars().count();
            st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
            st.store(&ctx, edit_id());
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.find.query).id(edit_id()).hint_text("문서에서 찾기").desired_width(220.0));
            if self.find.want_focus {
                // 포커스는 처음 한 번만 요청한다(매 프레임 요청하면 한글 조합이 끊길 수 있다).
                r.request_focus();
                self.find.want_focus = false;
            }
            if r.lost_focus() && enter && !composing_before && !self.find.composing {
                step = Some(!shift);
                r.request_focus();
            }
            // 입력칸이 포커스를 잃었으면 조합 상태가 남지 않게 푼다(Esc·Enter가 영영 막히는 것 방지).
            if !r.has_focus() && !r.lost_focus() {
                self.find.composing = false;
            }
            let n = self.find.matches.len();
            let label = if self.find.query.is_empty() {
                String::new()
            } else if n == 0 {
                "없음".to_string()
            } else {
                format!("{}/{}", self.find.cur + 1, n)
            };
            ui.label(egui::RichText::new(label).color(dim));
            if ui.button("▲").on_hover_text("이전 (Shift+Enter · Shift+F3)").clicked() {
                step = Some(false);
            }
            if ui.button("▼").on_hover_text("다음 (Enter · F3)").clicked() {
                step = Some(true);
            }
            if ui.selectable_label(self.find.case, "Aa").on_hover_text("대소문자 구분").clicked() {
                self.find.case = !self.find.case;
            }
            if ui.button("닫기").on_hover_text("Esc").clicked() {
                close = true;
            }
        });
        if self.find.replace_open {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.find.repl).id(repl_id()).hint_text("바꿀 글").desired_width(220.0));
                let on = !self.cur_trashed && !self.find.query.is_empty() && !self.find.matches.is_empty();
                if ui.add_enabled(on, egui::Button::new("바꾸기")).on_hover_text("현재 일치 하나를 바꾸고 다음으로").clicked() {
                    rep = Some(false);
                }
                if ui.add_enabled(on, egui::Button::new("모두 바꾸기")).on_hover_text("일치를 전부 바꿉니다 (Ctrl+Z 한 번으로 되돌림)").clicked() {
                    rep = Some(true);
                }
            });
        }
        ui.add_space(2.0);
        if let Some(all) = rep {
            self.find_replace(&ctx, body_id, all);
        }
        if let Some(fwd) = step {
            self.find_step(fwd);
        }
        if close {
            self.find_close(&ctx, body_id);
        }
    }

    /// 화면에 보이는 일치만 사각형으로 그린다(본문 뒤). 현재 일치는 진하게.
    pub(super) fn find_highlights(out: &egui::text_edit::TextEditOutput, matches: &[(usize, usize)], cur: usize, t: &theme::Theme) -> Vec<egui::Shape> {
        let mut shapes = vec![];
        if matches.is_empty() {
            return shapes;
        }
        let clip = out.text_clip_rect;
        let to_local = |y: f32| egui::vec2(0.0, y - out.galley_pos.y);
        let lo: usize = out.galley.cursor_from_pos(to_local(clip.min.y)).index.into();
        let hi: usize = out.galley.cursor_from_pos(to_local(clip.max.y)).index.into();
        let first = matches.partition_point(|m| m.1 < lo);
        let off = out.galley_pos.to_vec2();
        for (k, m) in matches.iter().enumerate().skip(first) {
            if m.0 > hi {
                break;
            }
            let a = out.galley.pos_from_cursor(CCursor::new(m.0)).translate(off);
            let b = out.galley.pos_from_cursor(CCursor::new(m.1)).translate(off);
            let col = if k == cur { t.accent.gamma_multiply(0.55) } else { t.accent.gamma_multiply(0.25) };
            if (a.top() - b.top()).abs() < 1.0 {
                shapes.push(egui::Shape::rect_filled(egui::Rect::from_min_max(egui::pos2(a.left(), a.top()), egui::pos2(b.left().max(a.left() + 2.0), a.bottom())), egui::CornerRadius::ZERO, col));
            } else {
                // 줄바꿈에 걸친 일치: 첫 줄은 끝까지, 마지막 줄은 처음부터
                let right = out.galley_pos.x + out.galley.rect.right();
                shapes.push(egui::Shape::rect_filled(egui::Rect::from_min_max(egui::pos2(a.left(), a.top()), egui::pos2(right, a.bottom())), egui::CornerRadius::ZERO, col));
                let left = out.galley_pos.x + out.galley.rect.left();
                shapes.push(egui::Shape::rect_filled(egui::Rect::from_min_max(egui::pos2(left, b.top()), egui::pos2(b.left(), b.bottom())), egui::CornerRadius::ZERO, col));
            }
        }
        shapes
    }
}
