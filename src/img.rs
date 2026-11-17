//! 이미지: 디코딩·축소·WebP 압축(구글 libwebp)과 화면용 텍스처 캐시.
use sha2::{Digest, Sha256};
use std::io::Cursor;

/// 입력 파일 크기 상한.
const MAX_INPUT_BYTES: usize = 100 * 1024 * 1024;
/// 디코딩해도 되는 픽셀 수 상한(1억).
const MAX_PIXELS: u64 = 100_000_000;

/// 압축을 마친 이미지.
pub struct Processed {
    /// 원본 픽셀·크기의 SHA-256 앞 16자리(소문자 16진수). 같은 그림이면 품질·크기 설정과 상관없이 같다.
    pub hash: String,
    pub webp: Vec<u8>,
    /// 저장된 WebP의 크기.
    pub w: u32,
    pub h: u32,
    /// 대체글(파일 이름에서 확장자를 뗀 것).
    pub alt: String,
}

fn check_pixels(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("빈 이미지입니다".into());
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!("이미지가 너무 큽니다({w}×{h}픽셀)"));
    }
    Ok(())
}

/// 파일 바이트(png/jpg/bmp/gif/webp)를 읽어 줄인다.
pub fn process(bytes: &[u8], name: &str, max_side: u32, quality: u32) -> Result<Processed, String> {
    if bytes.is_empty() {
        return Err("빈 파일입니다".into());
    }
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("파일이 너무 큽니다(100MB 초과)".into());
    }
    // 헤더만 읽어 크기를 먼저 검사한다(거대한 그림을 디코딩하다 메모리가 바닥나지 않도록).
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|x| x.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|x| format!("이미지를 읽을 수 없습니다: {x}"))?;
    check_pixels(w, h)?;
    let img = image::load_from_memory(bytes).map_err(|x| format!("이미지를 읽을 수 없습니다: {x}"))?;
    let rgba = img.to_rgba8();
    process_rgba(rgba.as_raw(), rgba.width(), rgba.height(), name, max_side, quality)
}

/// RGBA 픽셀(클립보드 등)을 줄여 WebP로 만든다.
pub fn process_rgba(rgba: &[u8], w: u32, h: u32, name: &str, max_side: u32, quality: u32) -> Result<Processed, String> {
    check_pixels(w, h)?;
    if rgba.len() as u64 != w as u64 * h as u64 * 4 {
        return Err("이미지 데이터의 길이가 맞지 않습니다".into());
    }
    let mut sha = Sha256::new();
    sha.update(w.to_le_bytes());
    sha.update(h.to_le_bytes());
    sha.update(rgba);
    let hash: String = sha.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();

    // 긴 변이 max_side보다 길면 비율대로 줄인다(키우지는 않는다).
    let m = max_side.max(1);
    let (nw, nh) = if w.max(h) > m {
        let s = m as f64 / w.max(h) as f64;
        (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
    } else {
        (w, h)
    };
    let shrunk;
    let data: &[u8] = if (nw, nh) != (w, h) {
        let src = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("이미지 데이터가 올바르지 않습니다")?;
        shrunk = image::imageops::resize(&src, nw, nh, image::imageops::FilterType::Lanczos3);
        shrunk.as_raw()
    } else {
        rgba
    };

    let enc = webp::Encoder::from_rgba(data, nw, nh);
    let mut best: Vec<u8> = enc.encode(quality.clamp(1, 100) as f32).to_vec();
    // 아주 작고 투명하지 않은 그림(UI 캡처 등)은 무손실이 더 작을 수 있어 둘 중 작은 쪽을 쓴다.
    let has_alpha = data.chunks_exact(4).any(|p| p[3] != 255);
    if !has_alpha && nw.max(nh) <= 256 {
        let ll = enc.encode_lossless();
        if ll.len() < best.len() {
            best = ll.to_vec();
        }
    }
    let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Processed { hash, webp: best, w: nw, h: nh, alt: crate::md::clean_alt(&stem) })
}

/// WebP를 RGBA 픽셀로 푼다. (가로, 세로, RGBA)
pub fn decode_webp(webp: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let img = webp::Decoder::new(webp).decode().ok_or("WebP를 풀 수 없습니다")?;
    let (w, h) = (img.width(), img.height());
    let px = w as usize * h as usize;
    let raw: &[u8] = &img;
    let rgba = if raw.len() == px * 4 {
        raw.to_vec()
    } else if raw.len() == px * 3 {
        raw.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    } else {
        return Err("WebP 데이터가 올바르지 않습니다".into());
    };
    Ok((w, h, rgba))
}

// ---------------------------------------------------------------- 텍스처 캐시
use crate::db::Db;
use std::collections::{HashMap, HashSet};

/// 화면에 그릴 이미지 한 장의 상태.
pub enum Tex<T> {
    Ready(T),
    /// 이번 프레임의 디코딩 예산을 다 써서 다음 프레임으로 미뤘다.
    Loading,
    /// DB에 없거나 깨졌다.
    Missing,
}

struct Entry<T> {
    tex: T,
    bytes: usize,
    used: u64,
}

/// 해시 → 텍스처 캐시(가장 오래 안 쓴 것부터 버림). 텍스처 타입은 제네릭이라 egui 없이도 시험할 수 있다.
pub struct ImgCache<T> {
    map: HashMap<String, Entry<T>>,
    missing: HashSet<String>,
    bytes: usize,
    max_items: usize,
    max_bytes: usize,
    tick: u64,
    budget: u32,
}

/// 한 프레임에 새로 디코딩하는 이미지 수(스크롤이 끊기지 않게).
const DECODES_PER_FRAME: u32 = 2;

impl<T: Clone> ImgCache<T> {
    /// 32개·약 64MB.
    pub fn new() -> Self {
        Self::with_limits(32, 64 * 1024 * 1024)
    }

    pub fn with_limits(max_items: usize, max_bytes: usize) -> Self {
        ImgCache { map: HashMap::new(), missing: HashSet::new(), bytes: 0, max_items, max_bytes, tick: 0, budget: DECODES_PER_FRAME }
    }

    /// 매 프레임 처음에 부른다(디코딩 예산을 되돌린다).
    pub fn begin_frame(&mut self) {
        self.budget = DECODES_PER_FRAME;
    }

    /// 모두 비운다(DB를 통째로 바꿔 넣은 뒤 등).
    pub fn clear(&mut self) {
        self.map.clear();
        self.missing.clear();
        self.bytes = 0;
    }

    /// `make(가로, 세로, RGBA)`로 텍스처를 만들어 캐시에 넣는다.
    pub fn get(&mut self, db: &Db, hash: &str, make: impl FnOnce(u32, u32, Vec<u8>) -> T) -> Tex<T> {
        self.tick += 1;
        if self.missing.contains(hash) {
            return Tex::Missing;
        }
        if let Some(e) = self.map.get_mut(hash) {
            e.used = self.tick;
            return Tex::Ready(e.tex.clone());
        }
        if self.budget == 0 {
            return Tex::Loading;
        }
        let blob = match db.get_image(hash) {
            Ok(Some((b, _, _))) => b,
            Ok(None) => {
                self.missing.insert(hash.to_string());
                return Tex::Missing;
            }
            Err(_) => return Tex::Missing, // 일시적인 읽기 오류는 기억하지 않는다
        };
        self.budget -= 1;
        match decode_webp(&blob) {
            Ok((w, h, rgba)) => {
                let bytes = rgba.len();
                let tex = make(w, h, rgba);
                self.map.insert(hash.to_string(), Entry { tex: tex.clone(), bytes, used: self.tick });
                self.bytes += bytes;
                while self.map.len() > 1 && (self.map.len() > self.max_items || self.bytes > self.max_bytes) {
                    let Some(old) = self.map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone()) else { break };
                    if let Some(e) = self.map.remove(&old) {
                        self.bytes -= e.bytes;
                    }
                }
                Tex::Ready(tex)
            }
            Err(_) => {
                self.missing.insert(hash.to_string());
                Tex::Missing
            }
        }
    }
}

impl ImgCache<eframe::egui::TextureHandle> {
    /// egui 텍스처로 돌려준다. 예산 때문에 미룬 그림이 있으면 다음 프레임을 요청한다.
    pub fn texture(&mut self, ctx: &eframe::egui::Context, db: &Db, hash: &str) -> Tex<eframe::egui::TextureHandle> {
        use eframe::egui::{ColorImage, TextureOptions};
        let r = self.get(db, hash, |w, h, rgba| {
            ctx.load_texture(format!("img-{hash}"), ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), TextureOptions::LINEAR)
        });
        if matches!(r, Tex::Loading) {
            ctx.request_repaint();
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba(f(x, y));
        }
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// 사진 비슷한 그림: 완만한 그라데이션에 잡음.
    fn photo_like(w: u32, h: u32) -> Vec<u8> {
        png(w, h, |x, y| {
            let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) % 23;
            [((x * 255 / w) as u8).saturating_add(n as u8), ((y * 255 / h) as u8).saturating_add(n as u8), 128, 255]
        })
    }

    #[test]
    fn process_shrinks_and_keeps_ratio() {
        let r = process(&png(3200, 1600, |_, _| [10, 20, 30, 255]), "a.png", 1600, 80).unwrap();
        assert_eq!((r.w, r.h), (1600, 800));
    }

    #[test]
    fn process_never_upscales() {
        let r = process(&png(100, 50, |_, _| [10, 20, 30, 255]), "a.png", 1600, 80).unwrap();
        assert_eq!((r.w, r.h), (100, 50));
    }

    #[test]
    fn same_pixels_same_hash_any_quality() {
        let src = photo_like(64, 48);
        let a = process(&src, "a.png", 1600, 60).unwrap();
        let b = process(&src, "b.png", 1600, 90).unwrap();
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.hash.len(), 16);
        assert!(a.hash.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')));
        let other = png(64, 48, |x, y| if (x, y) == (0, 0) { [1, 2, 3, 255] } else { [0, 0, 0, 255] });
        let base = png(64, 48, |_, _| [0, 0, 0, 255]);
        assert_ne!(process(&other, "o.png", 1600, 80).unwrap().hash, process(&base, "o.png", 1600, 80).unwrap().hash);
    }

    #[test]
    fn alpha_survives() {
        let r = process(&png(40, 40, |_, _| [200, 100, 50, 128]), "a.png", 1600, 80).unwrap();
        let (w, h, rgba) = decode_webp(&r.webp).unwrap();
        assert_eq!((w, h), (40, 40));
        assert_eq!(rgba.len(), 40 * 40 * 4);
        assert!(rgba.chunks_exact(4).any(|p| p[3] != 255));
    }

    #[test]
    fn lossy_is_smaller_than_png_for_photo_like() {
        let src = photo_like(800, 600);
        let r = process(&src, "p.png", 1600, 80).unwrap();
        assert!(r.webp.len() < src.len(), "webp {} png {}", r.webp.len(), src.len());
        let (w, h, _) = decode_webp(&r.webp).unwrap();
        assert_eq!((w, h), (800, 600));
    }

    #[test]
    fn bad_input_errors_not_panics() {
        assert!(process(&[], "a.png", 1600, 80).is_err());
        assert!(process(b"not an image", "a.png", 1600, 80).is_err());
        let ok = png(64, 64, |_, _| [1, 2, 3, 255]);
        assert!(process(&ok[..ok.len() / 2], "a.png", 1600, 80).is_err());
        // 픽셀 수 한도(1억)를 넘는 크기, 길이가 안 맞는 버퍼
        assert!(process_rgba(&[], 20_000, 20_000, "a", 1600, 80).is_err());
        assert!(process_rgba(&[0; 10], 4, 4, "a", 1600, 80).is_err());
        assert!(process_rgba(&[], 0, 4, "a", 1600, 80).is_err());
        assert!(decode_webp(b"junk").is_err());
    }

    fn tmp_db(name: &str) -> crate::db::Db {
        let d = std::env::temp_dir().join(format!("akashic-img-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::db::Db::open(&d.join("vault.db")).unwrap()
    }

    /// 8×8 그림을 DB에 넣고 해시를 돌려준다(shade마다 다른 그림).
    fn put(db: &crate::db::Db, shade: u8) -> String {
        let r = process_rgba(&[shade; 8 * 8 * 4], 8, 8, "t", 1600, 80).unwrap();
        db.put_image(&r.hash, &r.webp, r.w, r.h).unwrap();
        r.hash
    }

    #[test]
    fn cache_missing_hash_is_missing_not_panic() {
        let db = tmp_db("missing");
        let mut c: ImgCache<(u32, u32)> = ImgCache::new();
        c.begin_frame();
        assert!(matches!(c.get(&db, "0123456789abcdef", |_, _, _| panic!("만들면 안 됨")), Tex::Missing));
        assert!(matches!(c.get(&db, "0123456789abcdef", |_, _, _| panic!("만들면 안 됨")), Tex::Missing));
        // 깨진 바이트가 들어 있어도 Missing
        db.put_image("ffffffffffffffff", b"junk", 8, 8).unwrap();
        assert!(matches!(c.get(&db, "ffffffffffffffff", |_, _, _| panic!("만들면 안 됨")), Tex::Missing));
    }

    #[test]
    fn cache_respects_frame_budget() {
        let db = tmp_db("budget");
        let hs: Vec<String> = (1..=3).map(|i| put(&db, i * 40)).collect();
        let mut c: ImgCache<(u32, u32)> = ImgCache::new();
        c.begin_frame();
        assert!(matches!(c.get(&db, &hs[0], |w, h, _| (w, h)), Tex::Ready((8, 8))));
        assert!(matches!(c.get(&db, &hs[1], |w, h, _| (w, h)), Tex::Ready(_)));
        assert!(matches!(c.get(&db, &hs[2], |w, h, _| (w, h)), Tex::Loading)); // 한 프레임에 2개까지
        assert!(matches!(c.get(&db, &hs[0], |_, _, _| panic!("캐시에 있어야 함")), Tex::Ready(_))); // 적중은 예산 무관
        c.begin_frame();
        assert!(matches!(c.get(&db, &hs[2], |w, h, _| (w, h)), Tex::Ready(_)));
    }

    #[test]
    fn cache_evicts_least_recently_used() {
        let db = tmp_db("evict");
        let hs: Vec<String> = (1..=3).map(|i| put(&db, i * 40)).collect();
        let mut c: ImgCache<(u32, u32)> = ImgCache::with_limits(2, usize::MAX);
        let mut made = 0;
        for h in &hs {
            c.begin_frame();
            assert!(matches!(c.get(&db, h, |w, h, _| { made += 1; (w, h) }), Tex::Ready(_)));
        }
        assert_eq!(made, 3); // 셋째를 넣을 때 가장 오래된 첫째가 버려진다
        c.begin_frame();
        assert!(matches!(c.get(&db, &hs[1], |_, _, _| panic!("남아 있어야 함")), Tex::Ready(_)));
        assert!(matches!(c.get(&db, &hs[0], |w, h, _| { made += 1; (w, h) }), Tex::Ready(_)));
        assert_eq!(made, 4);
    }

    #[test]
    fn alt_comes_from_file_name() {
        let r = process(&png(8, 8, |_, _| [1, 2, 3, 255]), "a]b(c).png", 1600, 80).unwrap();
        assert!(!r.alt.contains(['[', ']', '(', ')']));
        assert!(!r.alt.contains('.'));
        assert_eq!(r.alt, "abc");
    }
}
