# 이미지 삽입 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 문서에 이미지를 붙여넣기·끌어놓기·메뉴로 넣으면 WebP로 줄여 DB에 저장하고, 편집기에서 줄 높이를 키워 그림을 그려 보여 주며, 내보내기에서도 살아 있게 한다.

**Architecture:** 본문의 `![대체글](img:해시 가로x세로)` 한 줄을 `md.rs`가 `BlockKind::Image`로 인식해 줄 높이를 그림 높이로 키우고, `render.rs`가 투명해진 글자 위에 텍스처를 그린다(표·가로줄과 같은 방식). 이미지 바이트는 새 `images` 테이블(스키마 v3)에 WebP로 저장하고, 압축은 `src/img.rs`가 별도 스레드에서 한다.

**Tech Stack:** Rust 2024, eframe/egui 0.37, rusqlite(bundled), 새 의존성 `webp`(libwebp), `image`(png/jpeg/bmp/gif/webp만), `arboard`(클립보드 이미지, `image-data` 기능).

**Spec:** `docs/superpowers/specs/2026-11-17-image-insert-design.md`

## Global Constraints
- 기준 코드는 v2.0.6. 끝나면 `Cargo.toml` 버전을 `2.1.0`으로 올리고 DEVLOG·README를 갱신한다(워크플로가 태그=Cargo 버전을 검사).
- 스키마 `SCHEMA_VERSION` 2 → 3. 기존 문서·DB가 깨지면 안 된다. v2 DB를 열면 `migrate()`가 `images`를 만든다. 더 새 버전 DB는 기존처럼 거부한다.
- 본문 문법은 `![대체글](img:해시 가로x세로)`. 해시 = **디코딩한 원본 RGBA 픽셀과 크기**의 SHA-256 앞 **16자리**(소문자 16진수). 대체글은 `[ ] ( )`·줄바꿈을 뺀 뒤 **60자**로 자른다.
- 이미지 블록은 "그 줄에 이것만 있을 때"만. 줄 안에 다른 글이 섞이거나 코드 블록·표 안에 있으면 일반 글자.
- 압축 기본값: 긴 변 최대 **1600px**, 품질 **80**. 옵션 값: 최대 크기 800/1200/1600/2400, 품질 60~95. `Config.img_max`, `Config.img_quality`(없으면 기본값).
- 표시: 폭 = `min(원래 폭, 본문 폭)`, 높이는 비율대로 **최대 700px**. 텍스처 캐시 **32개·약 64MB**(LRU), 한 프레임에 새로 디코딩하는 수 **2개**.
- 정리(`prune_images`)는 영구 삭제·휴지통 비우기·옵션의 수동 버튼에서만 실행(종료·시작 때는 안 함). 휴지통 문서의 이미지는 보존.
- 내보내기는 WebP 그대로 `images/해시.webp`, 본문은 `![대체글](../…/images/해시.webp)`(폴더 깊이만큼 `../`). PNG 변환 없음.
- 변경 카운터 트리거(`meta.rev`)를 `images` insert/delete에 달아야 한다.
- 한글 UI 문구·주석은 기존 스타일을 따른다. 상태 알림은 `self.say(&str)`.
- **커밋은 사용자가 요청할 때만 한다**(각 작업의 마지막은 "테스트 통과 확인"이다).

## Review Focus
1. **깨진·거대한 이미지 파일**(0바이트, 확장자만 png, 디코딩 후 수억 픽셀): 알림만 하고 문서·DB를 건드리지 않는다. → Task 3 테스트.
2. **코드 블록·표 안의 `![…](img:…)`와 줄 중간에 섞인 것**: 이미지 블록이 아니라 글자로 남는다. → Task 1 테스트.
3. **DB에 없는 해시**(다른 문서에서 복사한 글, 동기화 전 상태): 패닉 없이 "이미지를 찾을 수 없음" 상자. → Task 4 테스트(`Tex::Missing`).
4. **같은 그림을 여러 문서가 쓸 때 한 문서만 영구 삭제**: 다른 문서가 쓰는 그림은 남는다. → Task 2 테스트.
5. **클립보드에 글과 그림이 함께 있을 때**(엑셀·웹 복사): 글 붙여넣기가 우선이고 이미지 삽입은 일어나지 않는다. 대체글에 `]`·`)`·줄바꿈이 있는 파일 이름도 줄을 깨지 않는다. → Task 1·5 테스트.

---

## File Structure
- `src/md.rs` (수정): 이미지 줄 파싱·서식·참조 수집, `BlockKind::Image`, 표시 크기 계산, 접힌 줄 높이, 블록 줄 삽입.
- `src/db.rs` (수정): 스키마 v3, `images` API, `prune_images`.
- `src/img.rs` (새): 디코딩·축소·WebP 인코딩·해시, 텍스처 캐시 `ImgCache`.
- `src/render.rs` (수정): 이미지 블록 그리기.
- `src/app/image_ui.rs` (새): 입력(붙여넣기·드롭·메뉴)·작업 스레드·삽입·옵션.
- `src/app.rs` (수정): Config 필드, 모듈 등록, 드롭 분기, 정리 호출, 옵션 창에 구역 추가.
- `src/export.rs`, `src/app/export_ui.rs` (수정): 이미지 파일 내보내기.
- `Cargo.toml`, `DEVLOG.md`, `README.md` (수정).

---

### Task 1: 본문 문법 — 파싱·참조·블록 인식 (`md.rs`)

**Files:**
- Modify: `src/md.rs` (`BlockKind`, `scan_blocks`, `is_external_target`, tests의 `kinds()` 매칭)
- Test: `src/md.rs`의 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub struct ImageRef { pub alt: String, pub hash: String, pub w: u32, pub h: u32 }`
  - `pub fn parse_image_line(line: &str) -> Option<ImageRef>` — 줄 전체가 `![alt](img:<16hex> <w>x<h>)`일 때만(앞뒤 공백 허용, w·h는 1 이상).
  - `pub fn clean_alt(alt: &str) -> String` — `[ ] ( )`·줄바꿈 제거 후 60자로 자름(Task 3이 파일 이름 정리에 재사용).
  - `pub fn format_image_line(alt: &str, hash: &str, w: u32, h: u32) -> String` — `clean_alt` 적용 후 문법대로 한 줄(개행 없음).
  - `pub fn image_refs(text: &str) -> Vec<String>` — 본문 어디든(코드 블록·줄 중간 포함) `img:` 바로 뒤 16자리 16진수를 모은 중복 없는 목록(보수적으로).
  - `pub fn insert_block_line(text: &str, after_line: usize, line: &str) -> String` — `after_line`(0부터, 범위를 넘으면 마지막 줄) 다음에 `line`을 넣고, 앞뒤 줄이 비어 있지 않으면 빈 줄을 하나씩 보정한다.
  - `BlockKind::Image { alt: String, hash: String, w: u32, h: u32 }` (한 줄 블록, `start == end`).

- [ ] **Step 1: 실패하는 테스트 작성** (`mod tests`에 추가)
  - `image_line_parses`: `parse_image_line("![스샷](img:0123456789abcdef 800x600)")` == `Some(ImageRef{alt:"스샷",hash:"0123456789abcdef",w:800,h:600})`; 앞뒤 공백 허용.
  - `image_line_rejects_mixed_or_bad`: `"글 ![a](img:0123456789abcdef 8x6)"`, `"![a](img:XYZ 8x6)"`, `"![a](img:0123456789abcdef 0x6)"`, `"![a](https://x/y.png)"` 모두 `None`.
  - `format_image_line_sanitizes_alt`: `format_image_line("a]b(c)\nd", "0123456789abcdef", 8, 6)`에서 `[ ] ( )`·개행이 대체글에 없고 결과가 `parse_image_line`으로 되읽힌다; 70자 대체글은 60자로 잘린다.
  - `scan_blocks_finds_image_only_on_own_line`: `"a\n![x](img:0123456789abcdef 8x6)\nb"` → 블록 하나 `Image`(start=end=1). 같은 줄이 ```` ``` ```` 안에 있으면 `Code` 블록만, 표 행 안(`| ![x](img:… 8x6) | b |` 표)이면 `Table`만 나온다.
  - `image_refs_collects_unique_everywhere`: 코드 블록 안·줄 중간·같은 해시 두 번이 섞인 본문에서 중복 없이 모두 나온다.
  - `img_scheme_is_not_external`: `is_external_target("img:0123456789abcdef 8x6")` == false.
  - `insert_block_line_pads_blank_lines`: `"a\nb"`에 `after_line=0`으로 넣으면 `"a\n\nX\n\nb"`; 이미 빈 줄이 있으면 중복 없음; `after_line`이 범위 밖이면 끝에 붙고 마지막 개행 유무를 유지.
  - 기존 테스트 `kinds()`(md.rs:826 부근)의 `match`에 `BlockKind::Image { .. } => "image"` 팔을 추가.

- [ ] **Step 2: 실패 확인** — Run: `cargo test md::tests::image -- --nocapture` / Expected: 컴파일 오류 또는 FAIL(함수 미정의).

- [ ] **Step 3: 구현** — 위 Interfaces 그대로 `src/md.rs`에. `scan_blocks`는 코드·표 검사 뒤, 가로줄 판정 자리에서 `parse_image_line`을 시도해 `Block { kind: Image{..}, start: i, end: i }`를 넣는다(코드/표가 먼저 소비하므로 그 안은 자동 제외). `is_external_target`은 `img:`로 시작하면 false.

- [ ] **Step 4: 통과 확인** — Run: `cargo test md::` / Expected: 전부 PASS(기존 md 테스트 포함).

---

### Task 2: DB — 스키마 v3와 이미지 API (`db.rs`)

**Files:**
- Modify: `src/db.rs` (`SCHEMA_VERSION`, `migrate`, `REV_SQL`, 새 메서드, tests)
- Test: `src/db.rs` tests

**Interfaces:**
- Consumes: `md::image_refs(&str) -> Vec<String>` (Task 1)
- Produces:
  - `images` 테이블: `hash TEXT PRIMARY KEY, webp BLOB NOT NULL, w INTEGER NOT NULL, h INTEGER NOT NULL, created INTEGER NOT NULL`
  - `Db::put_image(&self, hash: &str, webp: &[u8], w: u32, h: u32) -> Res<()>` (이미 있으면 무시)
  - `Db::get_image(&self, hash: &str) -> Res<Option<(Vec<u8>, u32, u32)>>`
  - `Db::image_stats(&self) -> Res<(usize, u64)>` (개수, 총 바이트)
  - `Db::prune_images(&self) -> Res<usize>` (지운 개수)
  - `Db::image_from_file(src: &Path, hash: &str) -> Option<Vec<u8>>` (읽기 전용 연결을 따로 열어 WebP 바이트 반환; 내보내기 스레드용)

- [ ] **Step 1: 실패하는 테스트 작성**
  - `image_roundtrip_and_dedup`: put 두 번(같은 해시) → `image_stats() == (1, len)`; `get_image`가 같은 바이트·크기를 돌려주고 없는 해시는 `None`.
  - `images_bump_rev`: put 전후 `db.rev()`가 증가, prune으로 삭제해도 증가.
  - `v2_db_migrates_to_v3`: 기존 v2 파일 시험 방식(파일 상단 `SCHEMA_V1` 사용 테스트를 따라)으로 v2 DB를 만든 뒤 `migrate()` → `images` 테이블 존재, 문서 보존, `meta.schema == '3'`.
  - `prune_keeps_referenced_and_trashed`: 이미지 A·B·C 저장; 문서1 본문은 A 참조, 문서2는 B를 참조하고 휴지통; C는 미참조 → `prune_images()` == 1, A·B 남고 C 삭제.
  - `prune_after_one_doc_purged_keeps_shared_image`: 문서1·2가 같은 해시 A 참조, 문서1을 `purge` → `prune_images()` == 0이고 A 남음; 문서2도 purge 후엔 1개 삭제.

- [ ] **Step 2: 실패 확인** — Run: `cargo test db::tests::image` / Expected: FAIL(미정의).

- [ ] **Step 3: 구현** — `SCHEMA_VERSION = 3`. `migrate()`에 `if ver < 3 { BEGIN; CREATE TABLE images(...); INSERT OR REPLACE INTO meta(key,value) VALUES('schema','3'); COMMIT; }`. `REV_SQL`에 `rev_img_i`/`rev_img_d` 트리거 두 줄 추가(기존 트리거와 같은 형식). `prune_images`: `SELECT body FROM docs`(휴지통 포함 전체)에서 `md::image_refs`로 해시 집합을 만들고, `SELECT hash FROM images` 중 집합에 없는 것만 `DELETE`(한 트랜잭션).

- [ ] **Step 4: 통과 확인** — Run: `cargo test db::` / Expected: 전부 PASS.

---

### Task 3: 압축 파이프라인 (`src/img.rs`, `Cargo.toml`)

**Files:**
- Create: `src/img.rs`
- Modify: `Cargo.toml`(의존성 3개), `src/main.rs`(`mod img;`)
- Test: `src/img.rs` tests

**Interfaces:**
- Consumes: 없음(독립).
- Produces:
  - `pub struct Processed { pub hash: String, pub webp: Vec<u8>, pub w: u32, pub h: u32, pub alt: String }` (`w`,`h`는 저장된 WebP의 크기)
  - `pub fn process(bytes: &[u8], name: &str, max_side: u32, quality: u32) -> Result<Processed, String>` — 파일 바이트 입력.
  - `pub fn process_rgba(rgba: &[u8], w: u32, h: u32, name: &str, max_side: u32, quality: u32) -> Result<Processed, String>` — 클립보드 입력. `process`는 디코딩 후 이것을 부른다.
  - `pub fn decode_webp(webp: &[u8]) -> Result<(u32, u32, Vec<u8>), String>` (RGBA)
  - 한도: 입력 바이트 100MB 초과 또는 디코딩 후 픽셀 수가 1억 초과면 `Err`.

- [ ] **Step 1: 의존성 추가** — `cargo add webp`, `cargo add image --no-default-features --features png,jpeg,bmp,gif,webp`, `cargo add arboard --no-default-features --features image-data`(최신 호환 버전). `cargo build`가 C 컴파일 포함해 성공하는지 확인.

- [ ] **Step 2: 실패하는 테스트 작성**
  - `process_shrinks_and_keeps_ratio`: 3200×1600 PNG(테스트에서 `image::RgbaImage`를 PNG로 인코딩해 생성) → `max_side=1600`일 때 결과 `w==1600, h==800`.
  - `process_never_upscales`: 100×50 → 100×50.
  - `same_pixels_same_hash_any_quality`: 같은 PNG를 품질 60과 90으로 처리해도 `hash`가 같고 16자리 16진수; 픽셀이 한 개 다른 그림은 해시가 다르다.
  - `alpha_survives`: 반투명 픽셀이 있는 PNG → `decode_webp` 결과의 알파 값이 255가 아닌 픽셀이 있다.
  - `lossy_is_smaller_than_png_for_photo_like`: 그라데이션+노이즈 800×600에서 WebP 바이트가 원본 PNG보다 작다.
  - `bad_input_errors_not_panics`: 빈 바이트, `b"not an image"`, 잘린 PNG → `Err`; 픽셀 수 한도 초과 크기 헤더(`process_rgba`에 `w*h > 1억`) → `Err`.
  - `alt_comes_from_file_name`: `name="a]b(c).png"` → `alt`에 `[ ] ( )`가 없고 확장자가 없다.

- [ ] **Step 3: 실패 확인** — Run: `cargo test img::` / Expected: FAIL.

- [ ] **Step 4: 구현** — 디코딩은 `image::load_from_memory`(→ `to_rgba8`). 해시는 `sha2::Sha256`을 (w,h 리틀엔디안 4바이트씩 + 원본 RGBA)에 걸어 앞 8바이트를 16진수 16자리로. 축소는 `image::imageops::resize(.., Lanczos3)`. 인코딩은 `webp::Encoder::from_rgba(..).encode(quality as f32)`(손실, 알파 유지); 알파가 없고 긴 변이 256 이하면 `encode_lossless()`도 만들어 더 작은 쪽을 쓴다. 대체글은 `name`에서 확장자를 떼고 `md::format_image_line`의 정리 규칙을 재사용(정리 함수를 `md`에서 `pub fn clean_alt(&str) -> String`로 노출해 둘이 공유).

- [ ] **Step 5: 통과 확인** — Run: `cargo test img::` / Expected: PASS.

---

### Task 4: 표시 — 줄 높이·그리기·텍스처 캐시 (`md.rs`, `render.rs`, `img.rs`)

**Files:**
- Modify: `src/md.rs`(`build`), `src/render.rs`(`paint`), `src/img.rs`(`ImgCache`), `src/app.rs`(`paint_blocks_behind` 호출부와 `App` 필드)
- Test: `src/md.rs`, `src/img.rs` tests

**Interfaces:**
- Consumes: `BlockKind::Image`, `md::ImageRef` (Task 1); `Db::get_image`, `img::decode_webp` (Task 2·3).
- Produces:
  - `pub const IMAGE_MAX_H: f32 = 700.0; pub const IMAGE_PAD: f32 = 8.0;` (`md.rs`)
  - `pub fn image_display_size(w: u32, h: u32, max_w: f32) -> (f32, f32)` (`md.rs`) — 확대하지 않고, 높이 700 초과면 비율대로 줄인다. `max_w <= 0`이면 `(0.0, 0.0)`이 아닌 최소 `(1.0, 1.0)` 이상을 돌려 패닉·0 높이를 막는다.
  - `pub enum Tex { Ready(egui::TextureHandle), Loading, Missing }` (`img.rs`)
  - `pub struct ImgCache` — `ImgCache::new()`, `begin_frame(&mut self)`(프레임당 디코딩 예산 2로 리셋), `texture(&mut self, ctx: &egui::Context, db: &Db, hash: &str) -> Tex`. 캐시는 32개·64MB를 넘으면 가장 오래 안 쓴 것부터 버린다. 예산이 없으면 `Loading`(그리고 `ctx.request_repaint()`), DB에 없거나 디코딩 실패면 `Missing`.
  - `render::paint`에 매개변수 `imgs: &mut dyn FnMut(&str) -> Tex` 추가.

- [ ] **Step 1: 실패하는 테스트 작성**
  - `md`: `image_display_size_cases` — `(800,600,400.0)==(400.0,300.0)`, `(100,100,400.0)==(100.0,100.0)`, `(500,2000,400.0)==(175.0,700.0)`, `(800,600,0.0)`의 두 값이 모두 `>= 1.0`.
  - `md`: `build_gives_collapsed_image_line_its_height` — 기존 `build` 테스트가 쓰는 `Theme` 생성 방식으로 `"![a](img:0123456789abcdef 800x600)\n"`, `cursor_line=None`, `hide_syntax=true`, `wrap=400.0`을 빌드해 첫 섹션 `format.line_height == Some(300.0 + IMAGE_PAD)`; `cursor_line=Some(0)`이거나 `hide_syntax=false`이면 `line_height`가 그보다 훨씬 작다(원문 표시).
  - `img`: `cache_missing_hash_is_missing_not_panic` — 빈 임시 DB와 없는 해시 → `Tex::Missing`(이미 한 번 `Missing`이어도 계속 `Missing`).
  - `img`: `cache_respects_frame_budget_and_evicts` — 이미지 3개를 DB에 넣고 `begin_frame()` 한 번에 `texture` 3번 → 앞 두 개 `Ready`, 세 번째 `Loading`; 다음 프레임에 `Ready`. 캐시 한도를 낮춰 만든 `ImgCache::with_limits(max_items: usize, max_bytes: usize)`로 2개 한도에서 세 번째를 넣으면 가장 오래된 것이 버려진다.

- [ ] **Step 2: 실패 확인** — Run: `cargo test md::tests::image_display md::tests::build_gives img::tests::cache` / Expected: FAIL.

- [ ] **Step 3: 구현**
  - `md::build`: `BlockKind::Image`를 가진 줄이 접혀 있으면(`hide_syntax`이고 `cursor_line != Some(n)`) 줄 글자를 `hidden_fmt(작은 크기)`로 붙이고 `format.line_height = Some(display_h + IMAGE_PAD)`; 개행의 `nl_fmt`도 같은 서식. 아닌 경우는 `push(.., St{dim:true,code:true,..})`로 원문.
  - `render::paint`: `BlockKind::Image`는 `spans`에서 그 줄의 세로 범위를 얻어 위쪽 `IMAGE_PAD/2`를 띄우고 본문 폭 가운데에 `image_display_size`만큼의 사각형을 계산한다. 화면 밖(`ui.clip_rect()`와 안 겹침)이면 `imgs`를 부르지 않는다. `Ready`면 텍스처를 그리고(둥근 모서리 약간, 테두리는 표 격자색), `Loading`이면 회색 상자, `Missing`이면 "이미지를 찾을 수 없음"을 담은 회색 상자.
  - `App`에 `img: ImgCache` 필드, 매 프레임 `begin_frame()`, `paint_blocks_behind` 호출부에서 `&mut |h| self.img.texture(ctx, &self.db, h)` 클로저 전달(연관 함수라 필요한 값은 인자로 넘긴다).

- [ ] **Step 4: 통과 확인** — Run: `cargo test` / Expected: 전부 PASS. 이어서 `cargo build --release`가 성공하는지 확인.

---

### Task 5: 입력·삽입·옵션 (`image_ui.rs`, `app.rs`)

**Files:**
- Create: `src/app/image_ui.rs`
- Modify: `src/app.rs`(`mod image_ui;`, `Config`, `App` 필드, 드롭 분기 `app.rs:2102` 부근, 영구 삭제·휴지통 비우기·`purge_old` 호출부, 옵션 창 `options_window`, 편집기 Ctrl+V 처리, 이미지 삽입 메뉴)
- Test: `src/app/image_ui.rs` tests (순수 함수 부분)

**Interfaces:**
- Consumes: `img::process`, `img::process_rgba`, `Processed` (Task 3); `Db::put_image/image_stats/prune_images` (Task 2); `md::format_image_line`, `md::insert_block_line` (Task 1).
- Produces:
  - `Config.img_max: u32`(기본 1600), `Config.img_quality: u32`(기본 80) — 기존 `Config`의 기본값 방식(serde default)을 따른다.
  - `pub(super) fn image_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>)` — 옵션 창 구역("이미지": 최대 크기 선택, 품질 슬라이더 60~95, "이미지 N개 · X MB", "사용하지 않는 이미지 정리" 버튼).
  - `pub(super) fn is_image_path(p: &Path) -> bool` — 확장자 png/jpg/jpeg/bmp/gif/webp(대소문자 무시).
  - `pub(super) fn start_image_job(&mut self, ctx: &egui::Context, src: ImageSrc)` — `enum ImageSrc { File(PathBuf), Rgba { w: u32, h: u32, data: Vec<u8> } }`. 시작 시점의 문서 id와 커서 줄을 기억하고, `std::thread::spawn`에서 `process`/`process_rgba`를 돌려 `mpsc`로 결과를 보낸다(상태바 "이미지 처리 중…", 끝나면 `ctx.request_repaint()`).
  - `pub(super) fn poll_image(&mut self)` — 매 프레임 호출. 결과가 오면: 문서가 바뀌었거나 닫혔으면 취소 알림, 아니면 `db.put_image` → `md::insert_block_line`으로 본문에 넣고 기존 편집 경로처럼 `last_edit`를 표시해 저장되게 한다. 실패는 알림만.
  - `pub(super) fn clipboard_image() -> Option<(u32, u32, Vec<u8>)>` — `arboard`로 읽되, 클립보드에 **글이 있으면 `None`**.

- [ ] **Step 1: 실패하는 테스트 작성** (순수 함수만)
  - `is_image_path_cases`: `a.PNG`, `b.jpeg`, `c.webp` true; `d.md`, `e.txt`, 확장자 없음 false.
  - `config_defaults_when_missing`: `img_max`·`img_quality`가 없는 JSON 설정을 `serde_json::from_str::<Config>`로 읽으면 1600·80.
  - `clipboard_text_wins`: 글이 있는 클립보드 상태를 흉내 내는 순수 함수 `fn pick_paste(has_text: bool, image: Option<(u32,u32,Vec<u8>)>) -> Option<(u32,u32,Vec<u8>)>` — `has_text==true`면 이미지가 있어도 `None`.

- [ ] **Step 2: 실패 확인** — Run: `cargo test app::image_ui` / Expected: FAIL.

- [ ] **Step 3: 구현**
  - 드롭: `app.rs:2102` 근처에서 `dropped`를 이미지 경로와 나머지로 나누어, 이미지는 `start_image_job(File)`(여러 개면 차례로, 각 작업이 끝날 때 다음 시작), 나머지는 기존 `import_paths`.
  - 붙여넣기: 본문 편집기가 포커스일 때 Ctrl+V 이벤트에서 `pick_paste`가 이미지를 돌려주면 `start_image_job(Rgba)`. 글 붙여넣기는 egui 기본 동작을 그대로 둔다.
  - 메뉴: 편집 도구 모음(또는 문서 메뉴)에 "이미지 삽입…" → `rfd::FileDialog`(이미지 필터) → `start_image_job(File)`.
  - 정리: `Act::Purge`, `Act::EmptyTrash`(실제 삭제 후), 휴지통 자동 비우기(`purge_old` 호출부)가 끝난 뒤 `self.db.prune_images()`를 부르고 오류는 무시한다. 옵션의 수동 버튼은 결과 개수를 `say`.
  - 휴지통 문서(`cur_trashed`)에는 이미지 삽입을 막는다(알림).

- [ ] **Step 4: 통과 확인** — Run: `cargo test` / Expected: PASS. 이어서 수동 확인 목록(스펙 6절)을 앱에서 확인: 붙여넣기·드롭·메뉴·커서 진입 시 원문·창 폭 변경·긴 문서 스크롤·다크/라이트·**좁은 창에서 접은 줄이 여러 행으로 감기지 않는지**.

---

### Task 6: 내보내기 (`export.rs`, `export_ui.rs`)

**Files:**
- Modify: `src/export.rs`(`rewrite_images`, `run_export` 시그니처, 호출하는 기존 테스트), `src/app/export_ui.rs`
- Test: `src/export.rs` tests

**Interfaces:**
- Consumes: `md::scan_blocks`, `BlockKind::Image` (Task 1); `Db::image_from_file` (Task 2).
- Produces:
  - `pub fn rewrite_images(body: &str, depth: usize) -> (String, Vec<(String, String)>)` — 이미지 블록 줄을 `![alt](<"../"×depth>images/<해시>.webp)`로 바꾸고 `(해시, alt)` 목록을 돌려준다. 코드·표 안은 건드리지 않는다. 줄바꿈(`\r\n`/`\n`)을 보존한다.
  - `pub fn run_export(parent: &Path, stamp: &str, docs: &[ExportDoc], cats: &[Category], load_image: &dyn Fn(&str) -> Option<Vec<u8>>) -> ExportResult` — 문서마다 `rel`의 폴더 깊이로 `rewrite_images`를 적용한 본문을 쓰고, 쓰인 해시마다 `<내보내기 폴더>/images/<해시>.webp`를 **한 번만** 쓴다. 이미지를 못 읽거나 못 쓰면 그 문서는 성공으로 세되 `first_error`에 기록한다.

- [ ] **Step 1: 실패하는 테스트 작성**
  - `rewrite_images_depth`: `"![x](img:0123456789abcdef 8x6)\n"`, depth 0 → `"![x](images/0123456789abcdef.webp)\n"`; depth 2 → `"![x](../../images/0123456789abcdef.webp)\n"`.
  - `rewrite_images_skips_code_and_inline`: 코드 블록 안·줄 중간의 `![x](img:…)`는 그대로.
  - `rewrite_images_keeps_crlf`: `\r\n` 본문에서 줄바꿈이 보존된다.
  - `run_export_writes_shared_image_once`: 같은 해시를 쓰는 두 문서(하나는 하위 분류 폴더) 내보내기 후 `images/<해시>.webp`가 한 개, 두 .md의 링크 경로가 각각 올바르다(`load_image`는 고정 바이트를 돌려주는 클로저).
  - `run_export_missing_image_is_reported`: `load_image`가 `None`이면 `ok`는 문서 수, `first_error`가 `Some`.
  - 기존 `run_export` 호출 테스트는 `&|_| None`을 넘기도록 고친다.

- [ ] **Step 2: 실패 확인** — Run: `cargo test export::` / Expected: FAIL.

- [ ] **Step 3: 구현** — `run_export`는 `plan_paths`의 `rel`에서 `rel.components().count() - 1`을 깊이로 쓴다. `export_ui.rs`의 작업 스레드는 `move |h| crate::db::Db::image_from_file(&src, h)` 클로저를 만들어 넘긴다(스레드 안에서만 DB 접근).

- [ ] **Step 4: 통과 확인** — Run: `cargo test export::` / Expected: PASS.

---

### Task 7: 동기화 측정·문서·버전

**Files:**
- Modify: `src/sync.rs`(측정 테스트 추가), `Cargo.toml`(version = "2.1.0"), `DEVLOG.md`, `README.md`

- [ ] **Step 1: 측정 테스트 추가** — `measure_chunk_churn`(ignored)을 참고해 `measure_image_churn`(ignored): 문서 20개짜리 DB에 800×600 이미지를 하나씩 추가하며 매번 `snapshot_file` 후 바뀐 조각 수를 출력한다. Run: `cargo test sync::tests::measure_image_churn -- --ignored --nocapture` → 출력 수치를 DEVLOG에 기록.
- [ ] **Step 2: 문서·버전** — `Cargo.toml` 버전 2.1.0, `DEVLOG.md`에 v2.1.0 항목(기능 요약·스키마 v3·의존성 3개·측정 수치·테스트 수), `README.md`에 이미지 기능과 사용법 한 단락. 설계 문서 `docs/superpowers/specs/…`의 상태를 "구현됨"으로 고친다.
- [ ] **Step 3: 전체 확인** — Run: `cargo test` / Expected: 전부 PASS. `cargo build --release` 성공 후 `target\release\akashic.exe`의 크기를 DEVLOG에 기록한다(증가량 예상 0.3~0.5MB). 배포용 `out\Akashic.exe`는 덮어쓰지 않는다.
- [ ] **Step 4: 마무리** — 푸시·태그는 사용자가 요청할 때만 한다.
