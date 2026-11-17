# 그래프 입체 효과 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 그래프 노드를 연결 수에 비례해 크기가 달라지는 구슬 모양 원으로 그리고, 마우스에 따라 시차+기울임 원근으로 입체처럼 보이게 한다(위치 계산·클릭·끌기는 2D 그대로).

**Architecture:** `graph.rs`에 순수 함수(깊이·반지름·투영·시차)를 추가하고, `show()`가 한 프레임에 투영 결과를 한 번 계산해 그리기·강조·클릭 판정·글자 위치가 같은 값을 쓰게 한다. 모양(▲■◆) 코드는 삭제한다.

**Tech Stack:** Rust, egui/eframe 0.37 (`Painter`), serde(설정). 새 의존성 없음.

**Spec:** `docs/superpowers/specs/2026-11-17-graph-depth-design.md`

## Global Constraints
- 코드·문서 CRLF 파일이므로 Edit 도구로 고친다. 커밋 메시지 끝: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. 사용자가 요청할 때만 커밋·푸시한다.
- 상수(스펙 그대로): `Z_UNIT=120.0`, `PERSP_D=900.0`, `TILT_DEG=12.0`, `PARALLAX_PX=14.0`, `FOLLOW=0.12`, 반지름 `4.5·node_size·min(1+0.6·sqrt(deg), 5.0)`, 중심 ×1.3, 최소 2.0.
- `depth_fx` 기본 0.6, 범위 0..=1. 0이면 지금의 2D와 같은 화면(투영 항등, 시차 0, 하이라이트·흐림 없음).
- 설정 파일 호환: 모르는 필드는 무시(serde 기본). `shape_mode`·`hub_links`는 삭제.
- 마우스가 멈춘 뒤 `ptr`이 목표에 수렴하면 다시 그리기를 요청하지 않는다(기존 "잔잔한 움직임"은 그대로).

## Review Focus
- `depth_fx=0`이면 모든 노드의 투영이 원래 위치·배율 1(항등)이어야 한다 → Task 1 테스트.
- 최대 연결 수가 0(연결 없는 문서만)일 때 깊이 NaN/0 나누기 금지 → Task 1 테스트.
- 어떤 입력에서도 원근 분모(`PERSP_D − z′`)가 양수로 유지되어 배율이 유한해야 한다 → Task 1 테스트.
- 옛 설정 JSON(`shape_mode`·`hub_links` 포함)이 읽혀야 한다 → Task 1 테스트.
- 클릭·호버 판정이 화면에 보이는(투영된) 위치 기준이어야 한다 → Task 2 화면 확인.

---

### Task 1: 깊이·반지름·투영 순수 함수와 설정

**Files:**
- Modify: `src/graph.rs` (상수·함수·`GraphSettings`)
- Test: `src/graph.rs` 하단 `tests` 모듈

**Interfaces:**
- Produces:
  - `pub fn depth_of(deg: usize, max_deg: usize, is_center: bool) -> f32` — `[-1,1]`, 중심은 1.0, `max_deg==0`이면 0.0.
  - `pub fn node_radius(deg: usize, node_size: f32, size_by_links: bool, is_center: bool) -> f32`
  - `pub fn project(p: Vec2, z: f32, ptr: Vec2, fx: f32) -> (Vec2, f32)` — 월드 위치 `p`·깊이 `z`·마우스 `ptr∈[-1,1]²`·강도 `fx`로 (투영된 월드 위치, 크기 배율 k). `fx==0`이면 `(p, 1.0)`.
  - `pub fn parallax(z: f32, ptr: Vec2, fx: f32) -> Vec2` — 화면 픽셀 이동량(앞 점이 더 많이). `fx==0`이면 `Vec2::ZERO`.
  - `GraphSettings.depth_fx: f32`(기본 0.6, `serde(default)`). `shape_mode`, `hub_links` 필드 삭제.

- [ ] **Step 1: 실패하는 테스트 작성** (`tests` 모듈에 추가; 이름 그대로)
  - `depth_of_range_and_center`: `depth_of(0,0,false)==0.0`, `depth_of(0,10,false)==-1.0`, `depth_of(10,10,false)==1.0`, 중심은 항상 1.0, 모든 값이 유한하고 `[-1,1]`.
  - `node_radius_grows_with_links_and_caps`: deg 0→`4.5`, deg 4→약 `9.9`(±0.1), deg 25→`18.0`(상한 5배 이전이므로 `4.5·4.0`), deg 10000→`4.5·5.0`, `size_by_links=false`이면 deg와 무관, 중심은 ×1.3, 항상 ≥ 2.0.
  - `project_identity_when_fx_zero`: 임의 `p,z,ptr`에서 `project(p,z,ptr,0.0)==(p,1.0)`, `parallax(z,ptr,0.0)==Vec2::ZERO`.
  - `project_near_is_bigger_and_finite`: `ptr=(1,1)`, `fx=1.0`일 때 `z=1`의 배율 > `z=-1`의 배율, 모든 배율이 유한하고 > 0.
  - `settings_drop_old_shape_fields`: `{"shape_mode":2,"hub_links":7,"depth":2}`를 읽으면 오류 없이 `depth==2`, 기본 `depth_fx==0.6`.
  - 기존 `shape_tests` 모듈과 `shape_of` 참조 테스트는 삭제한다(Task 2에서 코드 삭제).
- [ ] **Step 2: 실행해 실패 확인** — `cargo test --release graph::` → 컴파일 오류(함수·필드 없음)로 FAIL.
- [ ] **Step 3: 구현** — `graph.rs`에 위 상수와 네 함수를 추가한다. `project`는 `(p.x, p.y, z·Z_UNIT)`를 x축으로 `ptr.y·TILT_DEG·fx`, y축으로 `−ptr.x·TILT_DEG·fx`(라디안 변환) 회전한 뒤 `k = PERSP_D/(PERSP_D − z′)`로 `(x′,y′)·k`를 돌려준다. `PERSP_D − z′`는 `max(·, PERSP_D·0.3)`로 하한을 둔다. `GraphSettings`에 `depth_fx`를 추가하고(`Default`에도), `shape_mode`·`hub_links`와 `forces`/`build_key`에서 쓰는 곳이 없으면 그대로 둔다.
- [ ] **Step 4: 통과 확인** — `cargo test --release graph::` PASS (이 시점에 `shape_of`·메뉴가 아직 `shape_mode`를 참조하면 컴파일이 안 되므로, 필드 삭제는 Task 2와 같은 커밋 안에서 처리하고 이 단계에서는 필드 삭제를 Task 2 시작 직전으로 미룬다 — 즉 Step 3에서는 `depth_fx`만 추가하고 삭제 테스트 `settings_drop_old_shape_fields`는 `#[ignore]`로 두었다가 Task 2에서 해제한다).
- [ ] **Step 5: Commit** — 사용자 요청 전에는 커밋하지 않는다(체크포인트만 기록).

### Task 2: show() 통합 — 원형 구슬 노드·입체 반응·모양 코드 삭제

**Files:**
- Modify: `src/graph.rs` (`Graph` 필드·`radius`·`show`·`menu`, `shape_of`/`shape_pts`/`dashed_poly` 정리)
- Test: 기존 그래프 테스트 유지, 화면 확인

**Interfaces:**
- Consumes: Task 1의 `depth_of`, `node_radius`, `project`, `parallax`, `GraphSettings.depth_fx`.
- Produces: `Graph.ptr: Vec2`(부드럽게 따라가는 마우스 위치), `show()`가 프레임마다 만드는 `proj: Vec<(Pos2, f32)>`(화면 위치, 크기 배율).

- [ ] **Step 1: 필드 삭제와 컴파일 정리** — `GraphSettings`에서 `shape_mode`·`hub_links` 삭제, `shape_of`·`shape_pts`·`shape_tests`·범례 블록·메뉴의 "노드 모양"/"허브 기준" 줄을 삭제한다. `dashed_poly`는 점선 원(16각형)에 재사용한다. Task 1에서 `#[ignore]`한 `settings_drop_old_shape_fields`를 해제한다.
- [ ] **Step 2: 투영 계산** — `Graph`에 `ptr: Vec2`를 추가한다(`build`·`new` 경로 초기값 `Vec2::ZERO`). `show()`에서: 마우스가 영역 위면 목표 `tgt=((pos−rect.center)/(rect.size/2)).clamp(-1,1)`, 아니면 `ZERO`. `ptr += (tgt−ptr)·FOLLOW`. `|tgt−ptr|>0.002`인 동안만 `request_repaint()`. `max_deg`를 구하고, 노드마다 `z=depth_of(...)`, `(pp,k)=project(dpos(i), z, ptr, fx)`, 화면 위치 `to_screen(pp)+parallax(z,ptr,fx)`를 `proj[i]`에 저장한다. 반지름은 `node_radius(...)·k`.
- [ ] **Step 3: 모든 그리기·판정에서 `proj`만 사용** — `hit`(호버·클릭·끌기 시작), 선(끝점·깊이 평균에 따른 흐림), 글자(실제 그리기 위치만 `proj`, 칸 계산은 기울임 없는 기본 위치 유지), 노드 그리기. 노드: 채운 원(`painter.circle_filled`) + 왼쪽 위 작은 밝은 원(하이라이트, `fx>0`일 때만) + 먼 점(z<0)은 `gamma_multiply(1−0.25·fx·(−z))`. 없는 문서는 점선 원, 고정 노드는 바깥 원 테두리(`circle_stroke`), 찾기 일치는 강조 원 테두리. 호버는 반지름 +1.5.
- [ ] **Step 4: 메뉴** — "모양" 구역에 `Slider::new(&mut s.depth_fx, 0.0..=1.0).text("입체 효과 (0=끔)")`를 추가한다. "연결이 많을수록 크게"·분류별 색·노드 크기 슬라이더는 유지.
- [ ] **Step 5: 빌드와 전체 테스트** — `cargo test --release` 전체 통과(기존 139건에서 `shape_tests` 제거분 감안).
- [ ] **Step 6: 화면 확인** — 릴리스 빌드 후 `--seed` 데이터로 그래프 창을 열어, 마우스를 좌/우/위/아래 네 위치에 둔 캡처 4장으로 시차·기울임이 보이는지, 노드 클릭·끌기가 보이는 위치에서 동작하는지, `depth_fx=0`에서 2D와 같은지, 다크 테마를 확인한다(캡처 자동화는 기존 `paste_test2.ps1`의 포그라운드 확인 방식을 따른다).

### Task 3: 문서·버전

**Files:**
- Modify: `src/app/help_ui.rs` (그래프 도움말 topic: "배치와 모양" → 노드 모양 문단을 "점 크기·입체 효과" 설명으로 교체), `README.md`, `DEVLOG.md`, `Cargo.toml`(2.5.0), `Cargo.lock`

- [ ] **Step 1:** 도움말의 `■ 노드 모양` 문단을 "연결이 많을수록 큰 점, 입체 효과 슬라이더(마우스에 따라 시차·기울임, 0=끔), 없는 문서는 점선 원"으로 교체하고, 마크다운 도움말 213행의 "점선 사각형"을 "점선 원"으로 고친다.
- [ ] **Step 2:** `Cargo.toml` 버전 2.5.0, `README.md` 그래프 설명 한 줄, `DEVLOG.md`에 이번 항목(설계·상수·확인한 것·미확인)을 추가한다.
- [ ] **Step 3:** `cargo test --release` 전체 통과를 확인한다. 커밋·태그는 사용자 요청 시에만.
