//! 연결 그래프를 별도 창으로 분리해 크게 보고, 그래프·목록에서 문서를 찾는다.
use super::*;

impl App {
    /// 분리된 그래프 창. `graph_window`가 true인 동안 매 프레임 호출한다.
    pub(super) fn graph_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let theme = self.theme.clone();
        let mut close = false;
        let mut g = self.graph.take();
        let mut gs = self.cfg.graph.clone();
        let mut builder = egui::ViewportBuilder::default().with_title("아카식 연결 그래프").with_inner_size([980.0, 700.0]).with_min_inner_size([520.0, 360.0]);
        if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon-128.png")) {
            builder = builder.with_icon(icon);
        }
        let cur = self.cur;
        let mut changed = false;
        let mut rebuild = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("akashic_graph"), builder, |ui, _class| {
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                close = true;
            }
            let Some(gr) = g.as_mut() else {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("그래프를 준비하는 중입니다…");
                });
                return;
            };
            // 왼쪽: 문서 찾기 목록(검색어는 그래프 강조와 같은 값)
            egui::Panel::left("graph_find").default_size(230.0).size_range(170.0..=420.0).resizable(true).show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("문서 찾기").strong());
                ui.add(egui::TextEdit::singleline(&mut gs.filter).hint_text("제목으로 찾기 (그래프에도 강조)").desired_width(f32::INFINITY));
                ui.label(egui::RichText::new("목록이나 그래프의 점을 누르면 본 창에서 그 문서를 엽니다").size(crate::theme::ui(-1.0)).color(theme.dim));
                ui.separator();
                let f = gs.filter.trim().to_lowercase();
                let mut rows: Vec<(usize, &crate::graph::Node)> = gr.nodes.iter().enumerate().filter(|(_, n)| f.is_empty() || n.title.to_lowercase().contains(&f)).collect();
                // 연결이 많은 문서(허브)를 위로
                rows.sort_by_key(|(_, n)| std::cmp::Reverse(n.deg));
                ui.label(egui::RichText::new(format!("{}개", rows.len())).size(crate::theme::ui(-1.0)).color(theme.dim));
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (_, n) in rows {
                        let label = match n.id {
                            Some(_) => format!("{}   ·{}", n.title, n.deg),
                            None => format!("{} (없는 문서)   ·{}", n.title, n.deg),
                        };
                        let mine = n.id.is_some() && n.id == cur;
                        let text = if n.id.is_none() { egui::RichText::new(label).italics().color(theme.dim) } else { egui::RichText::new(label) };
                        if ui.selectable_label(mine, text).clicked() {
                            match n.id {
                                Some(id) => acts.push(Act::Open(id)),
                                None => acts.push(Act::Follow(n.title.clone())),
                            }
                        }
                    }
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                let size = ui.available_size();
                let out = gr.show(ui, size, &theme, &mut gs);
                match out.act {
                    Some(GraphAct::Open(id)) => acts.push(Act::Open(id)),
                    Some(GraphAct::Create(title)) => acts.push(Act::Follow(title)),
                    None => {}
                }
                if out.rebuild {
                    rebuild = true;
                }
                if out.changed {
                    changed = true;
                }
            });
        });
        self.graph = g;
        if gs != self.cfg.graph {
            self.cfg.graph = gs;
        }
        if rebuild {
            self.graph_stale = true;
        }
        if changed {
            self.graph_cfg_dirty = true;
        }
        if close {
            self.graph_window = false;
            if let Some(g) = self.graph.as_mut() {
                g.request_fit();
            }
        }
    }
}
