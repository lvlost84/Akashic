//! 템플릿·오늘의 노트·스니펫·저장된 검색.
use super::*;

const WEEK: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
/// "오늘의 노트"를 만들 때 쓰는 템플릿 이름
pub(super) const TODAY_TPL: &str = "오늘의 노트";

impl App {
    /// {날짜} {시간} {요일} {제목}을 채운다.
    pub(super) fn fill_vars(s: &str, title: &str) -> String {
        use chrono::Datelike;
        let now = chrono::Local::now();
        s.replace("{날짜}", &now.format("%Y-%m-%d").to_string())
            .replace("{시간}", &now.format("%H:%M").to_string())
            .replace("{요일}", WEEK[now.weekday().num_days_from_monday() as usize])
            .replace("{제목}", title)
    }

    /// 새 문서를 만들고 제목·본문을 채워 연다.
    fn create_filled(&mut self, title: &str, body: &str) {
        self.flush();
        let cat = match self.filter.view { View::Category(c) => Some(c), _ => None };
        match self.db.new_doc(cat).and_then(|id| self.db.save_doc(id, title, body).map(|_| id)) {
            Ok(id) => {
                if self.filter.view == View::Trash {
                    self.filter.view = View::All;
                }
                self.list_dirty = true;
                self.open_doc(id, true);
                self.refresh();
            }
            Err(x) => self.say(&format!("새 문서 실패: {x}")),
        }
    }

    /// 오늘 날짜(YYYY-MM-DD) 제목의 문서를 열고, 없으면 만든다. "오늘의 노트" 템플릿이 있으면 그 내용으로 시작한다.
    pub(super) fn today_note(&mut self) {
        self.flush();
        let title = chrono::Local::now().format("%Y-%m-%d").to_string();
        match self.db.find_by_title(&title) {
            Ok(Some(id)) => self.open_doc(id, true),
            Ok(None) => {
                let body = self.cfg.templates.iter().find(|t| t.0 == TODAY_TPL).map(|t| Self::fill_vars(&t.1, &title)).unwrap_or_default();
                self.create_filled(&title, &body);
            }
            Err(x) => self.say(&format!("오늘의 노트 실패: {x}")),
        }
    }

    pub(super) fn new_from_template(&mut self, i: usize) {
        let Some((name, body)) = self.cfg.templates.get(i).cloned() else { return };
        let title = if name == TODAY_TPL { chrono::Local::now().format("%Y-%m-%d").to_string() } else { String::new() };
        let body = Self::fill_vars(&body, &title);
        self.create_filled(&title, &body);
    }

    /// 지금 열린 문서를 템플릿으로 저장한다(이름은 문서 제목, 같은 이름이면 덮어씀).
    pub(super) fn save_as_template(&mut self) {
        let name = self.title.trim().to_string();
        let name = if name.is_empty() { "제목 없음".to_string() } else { name };
        let body = self.body.clone();
        match self.cfg.templates.iter_mut().find(|t| t.0 == name) {
            Some(t) => t.1 = body,
            None => self.cfg.templates.push((name.clone(), body)),
        }
        self.cfg.save();
        self.say(&format!("템플릿 \"{name}\"으로 저장했습니다"));
    }

    /// 스니펫 내용({날짜} 등을 채운 것)
    pub(super) fn snippet_text(&self, i: usize) -> Option<String> {
        self.cfg.snippets.get(i).map(|s| Self::fill_vars(&s.1, &self.title))
    }

    pub(super) fn filled_snippets(&self) -> Vec<(String, String)> {
        self.cfg.snippets.iter().map(|(n, t)| (n.clone(), Self::fill_vars(t, &self.title))).collect()
    }

    /// 스니펫 삽입 하위 메뉴(편집창 우클릭·"…" 메뉴 공용). 눌린 스니펫의 번호를 돌려준다.
    pub(super) fn snippet_menu(&self, ui: &mut egui::Ui) -> Option<usize> {
        let mut pick = None;
        ui.menu_button("스니펫 삽입", |ui| {
            if self.cfg.snippets.is_empty() {
                ui.label(egui::RichText::new("스니펫이 없습니다 (새 문서 ▾ > 서식·스니펫 관리)").color(self.theme.dim));
            }
            for (i, (n, t)) in self.cfg.snippets.iter().enumerate() {
                let first: String = t.lines().next().unwrap_or("").chars().take(24).collect();
                if ui.button(format!("{n}   {first}")).clicked() {
                    pick = Some(i);
                    ui.close();
                }
            }
        });
        pick
    }

    /// 새 문서 단추 옆 ▾ 메뉴
    pub(super) fn new_doc_menu(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        if ui.button("오늘의 노트  (Ctrl+Alt+T)").on_hover_text("오늘 날짜 제목의 문서를 열고, 없으면 만듭니다").clicked() {
            acts.push(Act::Today);
            ui.close();
        }
        ui.separator();
        if self.cfg.templates.is_empty() {
            ui.label(egui::RichText::new("템플릿이 없습니다").color(self.theme.dim));
        }
        for (i, (n, _)) in self.cfg.templates.iter().enumerate() {
            if ui.button(format!("템플릿: {n}")).clicked() {
                acts.push(Act::FromTemplate(i));
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(self.cur.is_some() && !self.cur_trashed, egui::Button::new("현재 문서를 템플릿으로 저장")).on_hover_text("이름은 문서 제목. 본문의 {날짜} {시간} {요일} {제목}은 새 문서를 만들 때 채워집니다. \"오늘의 노트\"라는 이름이면 오늘의 노트의 양식이 됩니다").clicked() {
            acts.push(Act::SaveTemplate);
            ui.close();
        }
        if ui.button("서식·스니펫 관리…").clicked() {
            acts.push(Act::ExtrasWindow);
            ui.close();
        }
    }

    /// 검색창 옆 ★ 메뉴: 저장된 검색
    pub(super) fn saved_search_menu(&mut self, ui: &mut egui::Ui) {
        let q = self.search.trim().to_string();
        let can = !q.is_empty() && !self.cfg.saved_searches.contains(&q);
        if ui.add_enabled(can, egui::Button::new("현재 검색어 저장")).clicked() {
            self.cfg.saved_searches.push(q);
            self.cfg.save();
        }
        ui.separator();
        if self.cfg.saved_searches.is_empty() {
            ui.label(egui::RichText::new("저장된 검색이 없습니다").color(self.theme.dim));
        }
        let mut del = None;
        let mut pick = None;
        for (i, s) in self.cfg.saved_searches.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(s.as_str()).clicked() {
                    pick = Some(s.clone());
                }
                if ui.small_button("×").on_hover_text("이 검색 지우기").clicked() {
                    del = Some(i);
                }
            });
        }
        if let Some(s) = pick {
            self.search = s;
            self.list_dirty = true;
            ui.close();
        }
        if let Some(i) = del {
            self.cfg.saved_searches.remove(i);
            self.cfg.save();
        }
    }

    /// 템플릿·스니펫 관리 창
    pub(super) fn extras_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut save = false;
        egui::Window::new("서식·스니펫 관리").open(&mut open).default_width(460.0).default_height(420.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.extras_tab, 0, "스니펫");
                ui.selectable_value(&mut self.extras_tab, 1, "템플릿");
            });
            ui.label(egui::RichText::new(if self.extras_tab == 0 {
                "이름을 입력하고 Ctrl+J를 누르면 내용으로 바뀝니다(이름 앞은 문서 처음이나 공백). 메뉴의 \"스니펫 삽입\"으로도 넣습니다."
            } else {
                "새 문서 ▾ 메뉴에서 고릅니다. 이름이 \"오늘의 노트\"면 오늘의 노트의 양식입니다."
            }).size(crate::theme::ui(-1.0)).color(self.theme.dim));
            ui.label(egui::RichText::new("본문에 {날짜} {시간} {요일} {제목}을 쓰면 넣을 때 채워집니다.").size(crate::theme::ui(-1.0)).color(self.theme.dim));
            ui.separator();
            let list = if self.extras_tab == 0 { &mut self.cfg.snippets } else { &mut self.cfg.templates };
            let mut del = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(300.0).show(ui, |ui| {
                for (i, (n, t)) in list.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(n).desired_width(140.0).hint_text("이름"));
                            if r.lost_focus() { save = true; }
                            if ui.small_button("삭제").clicked() {
                                del = Some(i);
                            }
                        });
                        let r = ui.add(egui::TextEdit::multiline(t).desired_rows(2).desired_width(f32::INFINITY).hint_text("내용"));
                        if r.lost_focus() { save = true; }
                        ui.add_space(4.0);
                    });
                }
            });
            if let Some(i) = del {
                list.remove(i);
                save = true;
            }
            if ui.button("+ 추가").clicked() {
                list.push((String::new(), String::new()));
            }
        });
        if !open {
            // 이름이 빈 항목은 버린다
            self.cfg.snippets.retain(|s| !s.0.trim().is_empty());
            self.cfg.templates.retain(|s| !s.0.trim().is_empty());
            self.extras_open = false;
            save = true;
        }
        if save {
            self.cfg.save();
        }
    }
}
