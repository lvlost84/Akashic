//! 참고 창: 지금 문서를 별도 OS 창에 읽기 전용으로 띄워 두고 보며 작업한다. 여러 개를 열 수 있다.
//! 본문은 본창과 같은 방식(md::build + render::paint)으로 그리되 편집은 막는다.
//! 내용 갱신: 그 문서가 본창에 열려 있으면 본창의 현재 본문(저장 전 글자까지), 아니면 DB 저장본을 쓴다.
use super::*;

/// 한꺼번에 열 수 있는 참고 창 수.
pub const MAX_REF_WINDOWS: usize = 8;

/// 참고 창 하나의 상태(순수 데이터).
#[derive(Clone, Debug, PartialEq)]
pub struct RefWin {
    /// 창을 구별하는 번호(뷰포트 id에 쓴다)
    pub serial: u64,
    pub doc: i64,
    /// 이 창에서 링크로 건너오기 전의 문서들(뒤로 가기용)
    pub back: Vec<i64>,
}

/// 참고 창 목록과 그 상태 전환 규칙.
#[derive(Default, Debug)]
pub struct RefWins {
    pub wins: Vec<RefWin>,
    next_serial: u64,
}

#[derive(Debug, PartialEq)]
pub enum OpenResult {
    /// 새 창을 열었다(serial)
    Opened(u64),
    /// 이미 그 문서를 보여 주는 창이 있어 그 창을 앞으로 가져온다(serial)
    Existing(u64),
    /// 상한에 걸려 열지 못했다
    Full,
}

impl RefWins {
    pub fn open(&mut self, doc: i64) -> OpenResult {
        if let Some(w) = self.wins.iter().find(|w| w.doc == doc) {
            return OpenResult::Existing(w.serial);
        }
        if self.wins.len() >= MAX_REF_WINDOWS {
            return OpenResult::Full;
        }
        self.next_serial += 1;
        self.wins.push(RefWin { serial: self.next_serial, doc, back: Vec::new() });
        OpenResult::Opened(self.next_serial)
    }

    /// 창 안 링크로 다른 문서를 본다(그 창만 바뀐다). 같은 문서면 아무 일도 하지 않는다.
    pub fn navigate(&mut self, serial: u64, doc: i64) {
        if let Some(w) = self.wins.iter_mut().find(|w| w.serial == serial) {
            if w.doc != doc {
                w.back.push(w.doc);
                w.doc = doc;
            }
        }
    }

    /// 창 안 "뒤로". 돌아갈 곳이 없으면 false.
    pub fn go_back(&mut self, serial: u64) -> bool {
        match self.wins.iter_mut().find(|w| w.serial == serial) {
            Some(w) => match w.back.pop() {
                Some(d) => {
                    w.doc = d;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    pub fn close(&mut self, serial: u64) {
        self.wins.retain(|w| w.serial != serial);
    }
}

/// 창마다 따로 가지는 그리기 캐시.
#[derive(Default)]
pub struct RefView {
    loaded: Option<(u64, i64, String, String, bool, bool)>, // (rev, doc, title, body, trashed, found)
    galley: Option<(u64, Arc<egui::Galley>)>,
}

impl App {
    /// 문서를 참고 창으로 연다(목록 우클릭·도구줄 단추 공용).
    pub(super) fn open_ref_window(&mut self, doc: i64) {
        match self.refs.open(doc) {
            OpenResult::Opened(_) => {}
            OpenResult::Existing(s) => {
                self.ref_focus = Some(s);
                self.say("이미 참고 창으로 열려 있습니다");
            }
            OpenResult::Full => self.say(&format!("참고 창은 {MAX_REF_WINDOWS}개까지 열 수 있습니다")),
        }
    }

    /// 창이 보여 줄 문서의 (제목, 휴지통 여부, 존재 여부). 본창에 열린 문서는 본창 값, 아니면 DB 저장본(변경 번호가 바뀔 때만 다시 읽음).
    fn ref_loaded(&self, view: &mut RefView, doc: i64) -> (String, bool, bool) {
        if Some(doc) == self.cur {
            return (self.title.clone(), self.cur_trashed, true);
        }
        let stale = view.loaded.as_ref().is_none_or(|l| l.0 != self.ref_rev || l.1 != doc);
        if stale {
            view.loaded = Some(match self.db.get_doc(doc) {
                Ok(Some(d)) => (self.ref_rev, doc, d.title, d.body, d.trashed, true),
                _ => (self.ref_rev, doc, String::new(), String::new(), false, false),
            });
        }
        let l = view.loaded.as_ref().unwrap();
        (l.2.clone(), l.4, l.5)
    }

    /// 열려 있는 모든 참고 창을 그린다(매 프레임). 본창의 `acts`는 쓰지 않는다 — 참고 창은 본창을 바꾸지 않는다.
    #[allow(deprecated)]
    pub(super) fn ref_windows(&mut self, ctx: &egui::Context) {
        if self.refs.wins.is_empty() {
            self.ref_views.clear();
            return;
        }
        let wins = self.refs.wins.clone();
        let mut closes: Vec<u64> = Vec::new();
        let mut navs: Vec<(u64, i64)> = Vec::new();
        let mut backs: Vec<u64> = Vec::new();
        let mut say: Option<String> = None;
        let t = self.theme.clone();
        let (size, wrap_on) = (self.cfg.font_size, self.cfg.wrap);
        let focus = self.ref_focus.take();
        self.ref_views.retain(|s, _| wins.iter().any(|w| w.serial == *s));
        for w in &wins {
            let mut view = self.ref_views.remove(&w.serial).unwrap_or_default();
            let vid = egui::ViewportId::from_hash_of(("akashic_ref", w.serial));
            // 제목·본문 준비
            let (title, trashed, found) = self.ref_loaded(&mut view, w.doc);
            let live = Some(w.doc) == self.cur;
            let shown = if title.trim().is_empty() { "새 문서".to_string() } else { title.trim().to_string() };
            let mut builder = egui::ViewportBuilder::default()
                .with_title(format!("{shown} — 아카식 참고"))
                .with_inner_size([720.0, 640.0])
                .with_min_inner_size([360.0, 240.0]);
            if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon-128.png")) {
                builder = builder.with_icon(icon);
            }
            if focus == Some(w.serial) {
                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Focus);
            }
            let body: &str = if live { &self.body } else { view.loaded.as_ref().map_or("", |l| l.3.as_str()) };
            let (img, db) = (&mut self.img, &self.db);
            let galley_cache = &mut view.galley;
            let can_back = !w.back.is_empty();
            let serial = w.serial;
            ctx.show_viewport_immediate(vid, builder, |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested() || (i.key_pressed(Key::Escape) && !egui::Popup::is_any_open(ui.ctx()))) {
                    closes.push(serial);
                    return;
                }
                if ui.ctx().input(|i| i.modifiers.alt && i.key_pressed(Key::ArrowLeft)) && can_back {
                    backs.push(serial);
                }
                egui::Panel::top(egui::Id::new(("ref_top", serial))).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if crate::icons::btn_ex(ui, crate::icons::Icon::Back, "뒤로", can_back, None).on_hover_text("이 창에서 링크로 건너오기 전 문서로 (Alt+←)").clicked() {
                            backs.push(serial);
                        }
                        ui.label(egui::RichText::new(&shown).font(FontId::new(crate::theme::ui(2.0), egui::FontFamily::Name("bold".into()))));
                        ui.label(egui::RichText::new("읽기 전용 · [[링크]]를 누르면 이 창의 문서가 바뀝니다").size(crate::theme::ui(-1.0)).color(t.dim));
                    });
                    if trashed {
                        ui.colored_label(t.accent, "휴지통에 있는 문서입니다");
                    }
                });
                egui::CentralPanel::default().show(ui, |ui| {
                    if !found {
                        ui.label(egui::RichText::new("문서를 찾을 수 없습니다").color(t.dim));
                        return;
                    }
                    let blocks = crate::render::blocks_cached(ui.ctx(), body);
                    let bg = ui.visuals().extreme_bg_color;
                    let avail_h = ui.available_height();
                    let ctx2 = ui.ctx().clone();
                    let area = if wrap_on { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
                    let mut text: &str = body;
                    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, width: f32| -> Arc<egui::Galley> {
                        let text = buf.as_str();
                        let wd = if wrap_on { width } else { f32::INFINITY };
                        let mut h = DefaultHasher::new();
                        crate::render::cache_key(text).hash(&mut h);
                        (wd.to_bits(), size.to_bits(), t.name).hash(&mut h);
                        { let (a, b, c) = md::spacing(); (a.to_bits(), b.to_bits(), c.to_bits()).hash(&mut h); }
                        let key = h.finish();
                        if let Some((k, g)) = galley_cache.as_ref() {
                            if *k == key {
                                return g.clone();
                            }
                        }
                        let g = layout_doc(ui, text, wd, size, &t);
                        *galley_cache = Some((key, g.clone()));
                        g
                    };
                    area.auto_shrink([false, false]).show(ui, |ui| {
                        let slot = ui.painter().add(egui::Shape::Noop);
                        let out = egui::TextEdit::multiline(&mut text)
                            .id(egui::Id::new(("ref_body", serial)))
                            .interactive(false)
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .min_size(egui::vec2(0.0, (avail_h - 6.0).max(60.0)))
                            .background_color(egui::Color32::TRANSPARENT)
                            .layouter(&mut layouter)
                            .show(ui);
                        let mut imgs = |h: &str| img.texture(&ctx2, db, h);
                        Self::paint_blocks_behind(ui, slot, &out, &blocks, bg, &t, size, true, None, None, &mut imgs);
                        // 글자를 직접 눌러 [[문서 링크]]를 따라간다(편집창이 아니라 클릭을 직접 감지)
                        let resp = ui.interact(out.response.rect, egui::Id::new(("ref_click", serial)), Sense::click());
                                                if let Some(p) = resp.hover_pos() {
                            let idx: usize = out.galley.cursor_from_pos(p - out.galley_pos).index.into();
                            if md::wiki_at(text, idx).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                if resp.clicked() {
                                    if let Some(title) = md::wiki_at(text, idx) {
                                        match db.find_by_title(&title) {
                                            Ok(Some(id)) => navs.push((serial, id)),
                                            _ => say = Some(format!("\"{title}\" 문서가 없습니다")),
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
            });
            self.ref_views.insert(w.serial, view);
        }
        if !navs.is_empty() || !backs.is_empty() || !closes.is_empty() {
            ctx.request_repaint();
        }
        for (s, d) in navs {
            self.refs.navigate(s, d);
        }
        for s in backs {
            self.refs.go_back(s);
        }
        for s in closes {
            self.refs.close(s);
        }
        if let Some(m) = say {
            self.say(&m);
        }
    }
}

/// 문서 본문을 서식이 입혀진 글자 배치로 만든다(목록 줄이 줄바꿈될 때의 들여쓰기 포함, 서식 기호는 숨김·커서 없음).
pub(super) fn layout_doc(ui: &egui::Ui, text: &str, w: f32, size: f32, t: &Theme) -> Arc<egui::Galley> {
    let (hangs, maxm) = if w.is_finite() { md::hang_info(text) } else { (Vec::new(), 0) };
    let reserve = if maxm > 0 { size * (md::LIST_INDENT + 0.6 * maxm as f32) } else { 0.0 };
    let job = md::build(text, None, size, t, (w - reserve).max(60.0), true);
    let mut g = ui.ctx().fonts_mut(|f| f.layout_job(job));
    if maxm > 0 {
        let actual = md::max_hang_shift(&g, &hangs);
        if actual > reserve + 0.5 {
            let job = md::build(text, None, size, t, (w - actual).max(60.0), true);
            g = ui.ctx().fonts_mut(|f| f.layout_job(job));
        }
        g = Arc::new(md::apply_hang(&g, &hangs));
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_assigns_distinct_serials() {
        let mut r = RefWins::default();
        let (OpenResult::Opened(a), OpenResult::Opened(b)) = (r.open(1), r.open(2)) else { panic!() };
        assert_ne!(a, b);
        assert_eq!(r.wins.len(), 2);
    }

    #[test]
    fn open_same_doc_reuses_window() {
        let mut r = RefWins::default();
        let OpenResult::Opened(a) = r.open(5) else { panic!() };
        assert_eq!(r.open(5), OpenResult::Existing(a));
        assert_eq!(r.wins.len(), 1);
    }

    #[test]
    fn open_caps_at_max() {
        let mut r = RefWins::default();
        for i in 0..MAX_REF_WINDOWS as i64 {
            assert!(matches!(r.open(i), OpenResult::Opened(_)));
        }
        assert_eq!(r.open(999), OpenResult::Full);
        assert_eq!(r.wins.len(), MAX_REF_WINDOWS);
        // 닫으면 다시 열 수 있다
        let s = r.wins[0].serial;
        r.close(s);
        assert!(matches!(r.open(999), OpenResult::Opened(_)));
    }

    #[test]
    fn navigate_changes_only_that_window_and_records_history() {
        let mut r = RefWins::default();
        let (OpenResult::Opened(a), OpenResult::Opened(b)) = (r.open(1), r.open(2)) else { panic!() };
        r.navigate(a, 7);
        assert_eq!(r.wins.iter().find(|w| w.serial == a).unwrap().doc, 7);
        assert_eq!(r.wins.iter().find(|w| w.serial == b).unwrap().doc, 2);
        assert_eq!(r.wins.iter().find(|w| w.serial == a).unwrap().back, vec![1]);
    }

    #[test]
    fn navigate_to_same_doc_adds_no_history() {
        let mut r = RefWins::default();
        let OpenResult::Opened(a) = r.open(1) else { panic!() };
        r.navigate(a, 1);
        assert!(r.wins[0].back.is_empty());
    }

    #[test]
    fn go_back_walks_history_then_stops() {
        let mut r = RefWins::default();
        let OpenResult::Opened(a) = r.open(1) else { panic!() };
        r.navigate(a, 2);
        r.navigate(a, 3);
        assert!(r.go_back(a));
        assert_eq!(r.wins[0].doc, 2);
        assert!(r.go_back(a));
        assert_eq!(r.wins[0].doc, 1);
        assert!(!r.go_back(a));
        assert_eq!(r.wins[0].doc, 1);
    }

    #[test]
    fn close_removes_and_unknown_serial_is_ignored() {
        let mut r = RefWins::default();
        let OpenResult::Opened(a) = r.open(1) else { panic!() };
        r.navigate(999, 5);
        assert!(!r.go_back(999));
        r.close(999);
        assert_eq!(r.wins.len(), 1);
        r.close(a);
        assert!(r.wins.is_empty());
    }
}
