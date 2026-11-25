//! 파일 첨부: 15MB까지 DB에 넣고 본문에 `[[file:해시|📎 이름]]` 링크를 둔다. Ctrl+클릭으로 연다.
use super::*;
use sha2::{Digest, Sha256};

/// 바로 열면 실행·설치될 수 있는 형식. 열지 않고 탐색기에서 위치만 보여 준다.
const RISKY: &[&str] = &[
    "exe", "com", "bat", "cmd", "scr", "msi", "msp", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "lnk", "reg", "dll", "jar", "cpl", "pif", "gadget", "inf", "sys", "application", "appx",
    "msix", "url", "scf", "mht", "py", "pyw", "rdp", "msc", "chm", "jnlp", "sct", "wsc", "ps1xml", "psc1", "xll", "vb", "mhtml",
];

pub fn is_risky(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.trim().to_lowercase()).unwrap_or_default();
    RISKY.contains(&ext.as_str())
}

pub fn file_hash(data: &[u8]) -> String {
    Sha256::digest(data).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

impl App {
    /// 파일을 골라 DB에 넣고 링크를 선택 자리에 넣을 준비를 한다.
    pub(super) fn attach_file(&mut self, sel: Option<(usize, usize)>) {
        if self.cur.is_none() || self.cur_trashed {
            return;
        }
        let Some(path) = rfd::FileDialog::new().set_title("첨부할 파일 선택").pick_file() else { return };
        let limit_mb = crate::db::ATTACH_MAX / 1024 / 1024;
        match std::fs::metadata(&path) {
            Ok(m) if m.len() as usize > crate::db::ATTACH_MAX => {
                self.say(&format!("{limit_mb}MB를 넘는 파일은 첨부할 수 없습니다 ({:.1}MB)", m.len() as f64 / 1024.0 / 1024.0));
                return;
            }
            Err(x) => {
                self.say(&format!("파일을 읽을 수 없습니다: {x}"));
                return;
            }
            _ => {}
        }
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(x) => {
                self.say(&format!("파일을 읽을 수 없습니다: {x}"));
                return;
            }
        };
        let hash = file_hash(&data);
        if let Err(x) = self.db.put_file(&hash, &data) {
            self.say(&format!("첨부 실패: {x}"));
            return;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "파일".into());
        self.attach_text = Some(crate::md::file_tag(&hash, &name));
        self.edit_sel = Some(sel.unwrap_or(self.last_sel));
        self.edit_req = Some(EditReq::AttachText);
        self.say(&format!("첨부했습니다: {name} ({:.1}MB)", data.len() as f64 / 1024.0 / 1024.0));
    }

    /// 첨부 파일을 임시 폴더에 풀어 연다(실행 파일류는 열지 않고 탐색기에서 위치만 보여 준다).
    pub(super) fn open_attachment(&mut self, hash: &str, name: &str) {
        let data = match self.db.get_file(hash) {
            Ok(Some(d)) => d,
            Ok(None) => {
                self.say("이 첨부 파일이 이 PC에 없습니다(동기화로 아직 받지 못했거나 지워졌습니다)");
                return;
            }
            Err(x) => {
                self.say(&format!("첨부 읽기 실패: {x}"));
                return;
            }
        };
        let clean = if name.trim().is_empty() { hash.to_string() } else { crate::export::sanitize_name(name) };
        let dir = std::env::temp_dir().join("Akashic-files").join(hash);
        let path = dir.join(&clean);
        if let Err(x) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &data)) {
            self.say(&format!("임시 파일을 만들 수 없습니다: {x}"));
            return;
        }
        use std::os::windows::process::CommandExt;
        if is_risky(&clean) {
            let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
            self.say("실행 파일 형식이라 열지 않고 위치만 보여 드립니다");
        } else {
            let _ = std::process::Command::new("explorer").arg(&path).spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risky_and_hash() {
        assert!(is_risky("setup.EXE") && is_risky("a.b.ps1") && is_risky("x.lnk"));
        assert!(!is_risky("보고서.pdf") && !is_risky("noext"));
        assert_eq!(file_hash(b"abc").len(), 16);
        assert_eq!(file_hash(b"abc"), file_hash(b"abc"));
        assert_ne!(file_hash(b"abc"), file_hash(b"abd"));
    }
}
