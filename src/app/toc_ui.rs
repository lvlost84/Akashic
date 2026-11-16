//! 오른쪽 사이드바의 "목차" 모드: 본문의 제목(#)을 단계별로 보여 주고, 누르면 그 위치로 이동한다.
use super::*;

impl App {
    pub(super) fn toc_panel(&mut self, ui: &mut egui::Ui, _acts: &mut Vec<Act>) {
        // 제목 목록은 본문이 바뀔 때만 다시 만든다.
        let mut h = DefaultHasher::new();
        self.cur.hash(&mut h);
        self.body.hash(&mut h);
        let sig = h.finish();
        if sig != self.toc_sig {
            self.toc = md::headings(&self.body);
            self.toc_sig = sig;
        }
        let t = self.theme.clone();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("제목 {}개", self.toc.len())).size(crate::theme::ui(0.0)).color(t.dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let cur = self.cfg.toc_depth.clamp(1, 6);
                egui::ComboBox::from_id_salt("toc_depth").selected_text(format!("{cur}단계까지")).width(96.0).show_ui(ui, |ui| {
                    for d in 1..=6u8 {
                        if ui.selectable_label(cur == d, format!("{d}단계까지")).clicked() {
                            self.cfg.toc_depth = d;
                            self.graph_cfg_dirty = true; // 설정 저장
                        }
                    }
                });
            });
        });
        ui.separator();

        if self.cur.is_none() {
            ui.label(egui::RichText::new("문서를 선택하세요").color(t.dim));
            return;
        }
        let depth = self.cfg.toc_depth.clamp(1, 6) as usize;
        let shown: Vec<usize> = (0..self.toc.len()).filter(|&i| self.toc[i].level <= depth).collect();
        if shown.is_empty() {
            ui.label(egui::RichText::new("제목이 없습니다").color(t.dim));
            ui.label(egui::RichText::new("줄 맨 앞에 `# 제목`, `## 소제목`처럼 쓰면 여기에 나타납니다.").size(crate::theme::ui(0.0)).color(t.dim));
            return;
        }
        // 문서가 ## 부터 시작해도 왼쪽에 붙도록 가장 낮은 단계를 기준으로 들여쓴다.
        let base = shown.iter().map(|&i| self.toc[i].level).min().unwrap_or(1);
        // 지금 커서가 있는 구간의 제목(커서 줄 이전의 마지막 제목)
        let cur_line = self.cursor_line.unwrap_or(0);
        let current = shown.iter().rev().find(|&&i| self.toc[i].line <= cur_line).copied();
        let mut goto: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("toc_scroll").auto_shrink([false, false]).show(ui, |ui| {
            for &i in &shown {
                let hd = &self.toc[i];
                let (resp, rect) = Self::row_widget(ui, 24.0, Some(i) == current, &t);
                let indent = 8.0 + (hd.level - base) as f32 * 14.0;
                let (size, color) = if hd.level <= 2 { (14.0, t.text) } else { (13.0, t.quote) };
                // 단계를 알려 주는 작은 직각 마커
                let m = Rect::from_center_size(Pos2::new(rect.left() + indent - 3.0, rect.center().y), Vec2::splat(if hd.level == 1 { 5.0 } else { 3.0 }));
                ui.painter().rect_filled(m, CornerRadius::ZERO, if Some(i) == current { t.accent } else { t.dim });
                Self::label_in(ui, rect, indent + 6.0, &hd.text, size, color);
                if resp.on_hover_text(format!("{}  (줄 {})", hd.text, hd.line + 1)).clicked() {
                    goto = Some(hd.line);
                }
            }
        });
        if goto.is_some() {
            self.goto_line = goto;
        }
    }
}
