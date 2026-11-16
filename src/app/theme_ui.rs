//! 테마(라이트/다크/레이저/사용자 지정)·영문/한글 글꼴 선택 UI.
#![allow(deprecated)]
use super::*;
use crate::theme::{CUSTOM_NAME, COLOR_COUNT, COLOR_LABELS, FamilyInfo};
use std::sync::{Mutex, Arc as StdArc};

// ---------------------------------------------------------------- 글꼴 목록 캐시
enum Cat {
    Idle,
    Loading,
    Ready(StdArc<Vec<FamilyInfo>>),
}
static CATALOG: Mutex<Cat> = Mutex::new(Cat::Idle);

/// 설치된 글꼴 목록. 처음 부르면 백그라운드 스레드로 훑기 시작하고 None, 끝나면 캐시를 돌려준다.
fn catalog(ctx: &egui::Context) -> Option<StdArc<Vec<FamilyInfo>>> {
    let mut g = CATALOG.lock().ok()?;
    match &*g {
        Cat::Ready(v) => Some(v.clone()),
        Cat::Loading => None,
        Cat::Idle => {
            *g = Cat::Loading;
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let list = theme::scan_families(&theme::font_dirs(), None);
                if let Ok(mut g) = CATALOG.lock() {
                    *g = Cat::Ready(StdArc::new(list));
                }
                ctx.request_repaint();
            });
            None
        }
    }
}

/// 시작할 때: 설정의 글꼴을 적용하고 테마를 정한다.
pub(super) fn startup_theme(ctx: &egui::Context, cfg: &mut Config) -> Theme {
    let rep = theme::install_fonts(ctx, (&cfg.font_latin, &cfg.font_latin_loc), (&cfg.font_korean, &cfg.font_korean_loc));
    // 위치를 새로 알게 됐으면 저장해 다음부터 그 파일만 읽는다. 못 찾은 글꼴은 알린다.
    if !cfg.font_latin.is_empty() && rep.latin != cfg.font_latin_loc && !rep.latin.path.is_empty() {
        cfg.font_latin_loc = rep.latin.clone();
        cfg.save();
    }
    if !cfg.font_korean.is_empty() && rep.korean != cfg.font_korean_loc && !rep.korean.path.is_empty() {
        cfg.font_korean_loc = rep.korean.clone();
        cfg.save();
    }
    for n in &rep.notes {
        crate::diag::log(n);
        crate::add_notice(n);
    }
    theme::resolve(&cfg.theme, &cfg.custom_theme)
}

// ---------------------------------------------------------------- 색 변환(HSV)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h.floor() as i32, h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i % 6 { 0 => (v, t, p), 1 => (q, v, p), 2 => (p, v, t), 3 => (p, q, v), 4 => (t, p, v), _ => (v, p, q) };
    let ch = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
    (ch(r) << 16) | (ch(g) << 8) | ch(b)
}

fn rgb_to_hsv(c: u32) -> (f32, f32, f32) {
    let (r, g, b) = (((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 { 0.0 } else if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 } / 6.0;
    (h, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}

fn col(v: u32) -> egui::Color32 {
    egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const RED: egui::Color32 = egui::Color32::from_rgb(0xd0, 0x30, 0x30);

fn editor_open_id() -> egui::Id { egui::Id::new("custom_theme_editor_open") }

impl App {
    /// 옵션 창의 테마·글꼴 줄들. 화면을 다시 적용해야 하면 true.
    pub(super) fn theme_options(&mut self, ui: &mut egui::Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("테마");
            egui::ComboBox::from_id_salt("theme").selected_text(self.cfg.theme.clone()).show_ui(ui, |ui| {
                for th in theme::themes() {
                    if ui.selectable_label(self.cfg.theme == th.name, th.name).clicked() {
                        self.cfg.theme = th.name.to_string();
                        changed = true;
                    }
                }
                if ui.selectable_label(self.cfg.theme == CUSTOM_NAME, CUSTOM_NAME).clicked() {
                    if theme::Theme::from_colors(&self.cfg.custom_theme).is_none() {
                        self.cfg.custom_theme = self.theme.colors().to_vec();
                    }
                    self.cfg.theme = CUSTOM_NAME.to_string();
                    ctx.data_mut(|d| d.insert_temp(editor_open_id(), true));
                    changed = true;
                }
            });
            if self.cfg.theme == CUSTOM_NAME && crate::icons::btn(ui, crate::icons::Icon::Edit, "편집…").clicked() {
                ctx.data_mut(|d| d.insert_temp(editor_open_id(), true));
            }
        });
        if changed {
            self.cfg.save();
        }

        // ---- 글꼴
        match catalog(&ctx) {
            None => {
                ui.horizontal(|ui| {
                    ui.label("글꼴");
                    ui.label(egui::RichText::new("설치된 글꼴 목록을 읽는 중…").color(self.theme.dim));
                });
            }
            Some(cat) => {
                // 목록에 없는 저장값은 기본값으로 되돌린다.
                for (name, need_hangul) in [(&mut self.cfg.font_latin, false), (&mut self.cfg.font_korean, true)] {
                    if !name.is_empty() && !cat.iter().any(|f| f.name.eq_ignore_ascii_case(name) && (if need_hangul { f.hangul } else { f.latin })) {
                        name.clear();
                    }
                }
                let mut font_changed = false;
                for (label, salt, hangul) in [("영문 글꼴", "font_latin", false), ("한글 글꼴", "font_korean", true)] {
                    let cur = if hangul { self.cfg.font_korean.clone() } else { self.cfg.font_latin.clone() };
                    let default_label = if hangul { "기본값(맑은 고딕)" } else { "기본값(Segoe UI)" };
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(salt)
                            .width(240.0)
                            .height(320.0)
                            .selected_text(egui::RichText::new(if cur.is_empty() { default_label } else { cur.as_str() }))
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.set_min_width(240.0);
                                ui.set_max_width(240.0);
                                let mut pick: Option<String> = None;
                                if ui.selectable_label(cur.is_empty(), default_label).clicked() {
                                    pick = Some(String::new());
                                }
                                for f in cat.iter().filter(|f| if hangul { f.hangul } else { f.latin }) {
                                    if ui.add(egui::Button::selectable(cur.eq_ignore_ascii_case(&f.name), f.name.as_str()).truncate()).clicked() {
                                        pick = Some(f.name.clone());
                                    }
                                }
                                if let Some(p) = pick {
                                    if hangul { self.cfg.font_korean = p } else { self.cfg.font_latin = p }
                                    font_changed = true;
                                }
                            });
                    });
                }
                if font_changed {
                    self.apply_fonts(&ctx, &cat);
                }
            }
        }
        changed
    }

    fn apply_fonts(&mut self, ctx: &egui::Context, cat: &[FamilyInfo]) {
        let find = |n: &str, hangul: bool| cat.iter().find(|f| !n.is_empty() && f.name.eq_ignore_ascii_case(n) && (if hangul { f.hangul } else { f.latin }));
        let (l, k) = (find(&self.cfg.font_latin, false), find(&self.cfg.font_korean, true));
        self.cfg.font_latin_loc = l.map(theme::FontPaths::from_family).unwrap_or_default();
        self.cfg.font_korean_loc = k.map(theme::FontPaths::from_family).unwrap_or_default();
        theme::install_with(ctx, l, k);
        self.galley_cache = None;
        self.cfg.save();
    }

    /// 설정에서 현재 테마를 만든다.
    pub(super) fn resolve_theme(&self) -> Theme {
        theme::resolve(&self.cfg.theme, &self.cfg.custom_theme)
    }

    fn set_theme_now(&mut self, ctx: &egui::Context) {
        self.theme = self.resolve_theme();
        theme::apply(ctx, &self.theme);
        self.last_theme = self.theme.name.to_string();
        self.galley_cache = None;
        self.cfg.save();
    }

    /// 매 프레임: Ctrl+Alt+D 빠른 전환, 사용자 지정 테마 편집 창.
    pub(super) fn theme_tick(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::D)) {
            self.cfg.theme = if self.theme.dark { "라이트" } else { "다크" }.to_string();
            self.set_theme_now(ctx);
        }
        let mut open = ctx.data(|d| d.get_temp::<bool>(editor_open_id())).unwrap_or(false);
        if !open {
            return;
        }
        if self.cfg.theme != CUSTOM_NAME {
            ctx.data_mut(|d| d.insert_temp(editor_open_id(), false));
            return;
        }
        let mut cols: Vec<u32> = if self.cfg.custom_theme.len() == COLOR_COUNT { self.cfg.custom_theme.clone() } else { theme::base_colors("라이트").to_vec() };
        let before = cols.clone();
        let mut bad = false;
        let dim = self.theme.dim;
        let accent = self.theme.accent;
        let border = self.theme.border;
        egui::Window::new("사용자 지정 테마").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("불러오기");
                for n in ["라이트", "다크", "레이저"] {
                    if ui.button(format!("{n}에서 시작")).clicked() {
                        cols = theme::base_colors(n).to_vec();
                    }
                }
            });
            ui.separator();
            let sel_id = egui::Id::new("custom_theme_sel");
            let mut selected: Option<usize> = ui.ctx().data(|d| d.get_temp(sel_id));
            egui::Grid::new("custom_theme_grid").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                for i in 0..COLOR_COUNT {
                    ui.label(COLOR_LABELS[i]);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 18.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 0.0, col(cols[i]));
                    let sc = if selected == Some(i) { accent } else { border };
                    ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, sc), egui::StrokeKind::Inside);
                    if resp.clicked() {
                        selected = if selected == Some(i) { None } else { Some(i) };
                    }
                    let id = egui::Id::new(("custom_theme_hex", i));
                    let editing: Option<String> = ui.ctx().data(|d| d.get_temp(id));
                    let mut buf = editing.clone().unwrap_or_else(|| theme::to_hex(cols[i]));
                    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(76.0).char_limit(7));
                    if r.has_focus() || r.changed() {
                        match theme::parse_hex(&buf) {
                            Some(v) => cols[i] = v,
                            None => bad = true,
                        }
                        ui.ctx().data_mut(|d| d.insert_temp(id, buf));
                    } else {
                        ui.ctx().data_mut(|d| d.remove_temp::<String>(id));
                    }
                    ui.end_row();
                }
            });
            if bad {
                ui.colored_label(RED, "잘못된 색 값입니다. #RRGGBB 형식(예: #1A6FD6)으로 입력하세요. 이전 값이 유지됩니다.");
            }
            if let Some(i) = selected {
                ui.separator();
                ui.label(egui::RichText::new(format!("{} 색 고르기", COLOR_LABELS[i])).color(dim));
                let hid = egui::Id::new("custom_theme_hsv");
                let (mut h, mut s, mut v) = ui.ctx().data(|d| d.get_temp::<(usize, f32, f32, f32)>(hid)).filter(|&(k, h, s, v)| k == i && hsv_to_rgb(h, s, v) == cols[i]).map(|(_, h, s, v)| (h, s, v)).unwrap_or_else(|| rgb_to_hsv(cols[i]));
                let (sv_w, sv_h, n) = (192.0, 120.0, (48usize, 30usize));
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(sv_w, sv_h), egui::Sense::click_and_drag());
                let (cw, ch) = (sv_w / n.0 as f32, sv_h / n.1 as f32);
                for gx in 0..n.0 {
                    for gy in 0..n.1 {
                        let c = col(hsv_to_rgb(h, (gx as f32 + 0.5) / n.0 as f32, 1.0 - (gy as f32 + 0.5) / n.1 as f32));
                        let r = egui::Rect::from_min_size(rect.min + egui::vec2(gx as f32 * cw, gy as f32 * ch), egui::vec2(cw.ceil(), ch.ceil()));
                        ui.painter().rect_filled(r.intersect(rect), 0.0, c);
                    }
                }
                if (resp.dragged() || resp.clicked()) && let Some(p) = resp.interact_pointer_pos() {
                    s = ((p.x - rect.left()) / sv_w).clamp(0.0, 1.0);
                    v = 1.0 - ((p.y - rect.top()) / sv_h).clamp(0.0, 1.0);
                }
                let m = egui::pos2(rect.left() + s * sv_w, rect.top() + (1.0 - v) * sv_h);
                ui.painter().rect_stroke(egui::Rect::from_center_size(m, egui::vec2(7.0, 7.0)), 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
                ui.painter().rect_stroke(egui::Rect::from_center_size(m, egui::vec2(9.0, 9.0)), 0.0, egui::Stroke::new(1.0, egui::Color32::BLACK), egui::StrokeKind::Outside);
                let (hrect, hresp) = ui.allocate_exact_size(egui::vec2(sv_w, 14.0), egui::Sense::click_and_drag());
                let hn = 96usize;
                for k in 0..hn {
                    let x0 = hrect.left() + sv_w * k as f32 / hn as f32;
                    let r = egui::Rect::from_min_size(egui::pos2(x0, hrect.top()), egui::vec2(sv_w / hn as f32 + 1.0, 14.0)).intersect(hrect);
                    ui.painter().rect_filled(r, 0.0, col(hsv_to_rgb((k as f32 + 0.5) / hn as f32, 1.0, 1.0)));
                }
                if (hresp.dragged() || hresp.clicked()) && let Some(p) = hresp.interact_pointer_pos() {
                    h = ((p.x - hrect.left()) / sv_w).clamp(0.0, 0.999);
                }
                let hx = hrect.left() + h * sv_w;
                ui.painter().rect_stroke(egui::Rect::from_min_max(egui::pos2(hx - 2.0, hrect.top()), egui::pos2(hx + 2.0, hrect.bottom())), 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
                let nv = hsv_to_rgb(h, s, v);
                if resp.dragged() || resp.clicked() || hresp.dragged() || hresp.clicked() {
                    cols[i] = nv;
                    ui.ctx().data_mut(|d| d.remove_temp::<String>(egui::Id::new(("custom_theme_hex", i))));
                }
                ui.ctx().data_mut(|d| d.insert_temp(hid, (i, h, s, v)));
            }
            match selected {
                Some(i) => ui.ctx().data_mut(|d| { d.insert_temp(sel_id, i); }),
                None => ui.ctx().data_mut(|d| { d.remove_temp::<usize>(sel_id); }),
            }
            ui.separator();
            ui.label(egui::RichText::new(format!("밝기: {} (배경색으로 자동 판정)", if theme::is_dark_color(cols[0]) { "어두움" } else { "밝음" })).color(dim));
        });
        ctx.data_mut(|d| d.insert_temp(editor_open_id(), open));
        if cols != before {
            self.cfg.custom_theme = cols;
            self.set_theme_now(ctx);
        }
    }
}
