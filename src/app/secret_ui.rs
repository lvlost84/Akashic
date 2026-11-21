//! 본문 일부 자동 암호화: 키 관리와 열기·저장 때의 변환, 키 가져오기 창.
use super::*;
use crate::textcrypt::{self, Key};

impl App {
    /// 풀 수 있는 키들: 저장소(WebDAV) 키에서 나온 키, 이 PC의 키, 가져온 키
    pub(super) fn text_ring(&self) -> Vec<Key> {
        let mut v = Vec::new();
        if !self.cfg.wd_key.trim().is_empty() {
            if let Ok(k) = crate::vault::VaultKey::from_recovery(&self.cfg.wd_key) {
                v.push(k.text_key());
            }
        }
        for s in std::iter::once(&self.cfg.text_key).chain(self.cfg.text_keys_extra.iter()) {
            if let Some(k) = textcrypt::parse_key(s) {
                v.push(k);
            }
        }
        v
    }

    /// 잠글 때 쓰는 키: 저장소 키가 있으면 그것(여러 PC가 자동으로 같은 키를 씀), 없으면 이 PC의 키(없으면 만든다).
    fn text_enc_key(&mut self) -> Result<Key, String> {
        if !self.cfg.wd_key.trim().is_empty() {
            if let Ok(k) = crate::vault::VaultKey::from_recovery(&self.cfg.wd_key) {
                return Ok(k.text_key());
            }
        }
        if let Some(k) = textcrypt::parse_key(&self.cfg.text_key) {
            return Ok(k);
        }
        let k = textcrypt::new_key()?;
        self.cfg.text_key = textcrypt::key_string(&k);
        self.cfg.save();
        self.say("본문 암호화 키를 만들었습니다. \"…\" 메뉴 > 암호화 키 복사로 따로 보관해 두세요(이 키를 잃으면 암호화한 글을 복구할 수 없습니다)");
        Ok(k)
    }

    /// DB 본문 → 편집창 본문
    pub(super) fn reveal_body(&mut self, stored: &str) -> String {
        if !textcrypt::has_markers(stored) {
            return stored.to_string();
        }
        let ring = self.text_ring();
        textcrypt::reveal(stored, &ring, &mut self.secret_cache)
    }

    /// 편집창 본문 → DB에 쓸 본문(암호화에 실패하면 Err: 평문을 저장하지 않는다)
    pub(super) fn conceal_body(&mut self) -> Result<String, String> {
        if !textcrypt::has_markers(&self.body) {
            return Ok(self.body.clone());
        }
        let key = self.text_enc_key()?;
        let body = std::mem::take(&mut self.body);
        let r = textcrypt::conceal(&body, &key, &mut self.secret_cache);
        self.body = body;
        r
    }

    /// 암호화 키 가져오기 창(다른 PC에서 복사한 키)
    pub(super) fn key_import_window(&mut self) {
        let Some(mut text) = self.key_import.take() else { return };
        let mut keep = true;
        let mut apply = false;
        let ctx = self.ctx_for_windows.clone();
        let Some(ctx) = ctx else {
            self.key_import = Some(text);
            return;
        };
        egui::Window::new("암호화 키 가져오기").collapsible(false).resizable(false).show(&ctx, |ui| {
            ui.label("다른 PC에서 \"암호화 키 복사\"로 복사한 키(64자리)를 붙여 넣으세요.");
            ui.add(egui::TextEdit::singleline(&mut text).desired_width(420.0).hint_text("키"));
            ui.horizontal(|ui| {
                if ui.button("가져오기").clicked() {
                    apply = true;
                }
                if ui.button("취소").clicked() {
                    keep = false;
                }
            });
        });
        if apply {
            match textcrypt::parse_key(&text) {
                Some(k) => {
                    let s = textcrypt::key_string(&k);
                    if s != self.cfg.text_key && !self.cfg.text_keys_extra.contains(&s) {
                        self.cfg.text_keys_extra.push(s);
                        self.cfg.save();
                    }
                    self.say("암호화 키를 가져왔습니다");
                    keep = false;
                    self.flush();
                    if let Some(id) = self.cur {
                        self.open_doc(id, false); // 이제 풀리는 글을 다시 보여 준다
                    }
                }
                None => self.say("키 형식이 올바르지 않습니다(64자리 16진수)"),
            }
        }
        if keep {
            self.key_import = Some(text);
        }
    }
}
