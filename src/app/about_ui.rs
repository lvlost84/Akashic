//! 옵션의 "정보" 탭: 프로그램 정보와 단축키. 오픈소스 라이선스 고지는 빌드할 때 만들어지는 LICENSE.md에 있다.
use super::*;
impl App {
    /// 정보·라이선스 탭 내용.
    pub(super) fn about_tab(&mut self, ui: &mut egui::Ui) {
        let dim = self.theme.dim;
        ui.label(egui::RichText::new(format!("아카식 v{}", env!("CARGO_PKG_VERSION"))).size(crate::theme::ui(0.0)).strong());
        ui.label(egui::RichText::new("문서를 분류와 태그로 모아 두고 빠르게 편집하는 오프라인 마크다운 노트 (Rust)").color(dim));
        ui.label(egui::RichText::new(format!("저장소: {}", self.db.path.display())).color(dim).size(crate::theme::ui(0.0)));
        ui.add_space(6.0);

        if ui.button("도움말 열기 (F1) — 마크다운 문법·단축키를 새 창으로").clicked() {
            self.show_help = true;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("단축키").strong());
        ui.label("Ctrl+N 새 문서 · Ctrl+E 검색 · Ctrl+F 문서 안 찾기 · Ctrl+S 저장 · Ctrl+\\ 목록 접기 · Ctrl+, 옵션 · F1 도움말\nCtrl+B 굵게 · Ctrl+I 기울임 · Ctrl+Shift+X 취소선 · Ctrl+` 코드 · Ctrl+K 링크\nCtrl+Shift+D 즐겨찾기 · Alt+←/→ 뒤로/앞으로 · Ctrl+클릭 문서 링크 이동 · Ctrl+Alt+D 라이트/다크");
        ui.add_space(6.0);
        ui.separator();
        self.update_section(ui);
        ui.add_space(6.0);
        ui.separator();

        ui.label(egui::RichText::new("오픈소스 라이선스").strong());
        ui.label(egui::RichText::new("사용한 오픈소스의 목록과 라이선스 전문은 프로그램 옆의 LICENSE.md 파일에 있습니다 (빌드할 때 자동으로 만들어집니다).").color(dim));
    }
}