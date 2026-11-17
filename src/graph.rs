//! 노트 연결 그래프(옵시디언 스타일): `[[문서 링크]]`를 노드·선으로 보여 준다.
//! 스프링+반발력 물리, 직각 노드, 우클릭 메뉴로 조절하는 설정(저장됨).
use crate::md;
use crate::theme::Theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

pub fn norm(t: &str) -> String {
    t.trim().to_lowercase()
}

/// 그래프 설정. 설정 파일(config.json)에 저장되고, 우클릭 메뉴에서 바꾼다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GraphSettings {
    // ── 표시(바뀌면 그래프를 다시 만든다) ──
    /// 중심 문서에서 몇 단계까지 보일지(0 = 전체)
    pub depth: u8,
    /// 연결이 하나도 없는 문서도 보일지
    pub show_orphans: bool,
    /// 아직 없는 문서(점선)를 보일지
    pub show_missing: bool,
    // ── 모양(즉시 반영) ──
    pub arrows: bool,
    pub node_size: f32,
    pub size_by_links: bool,
    pub link_width: f32,
    pub label_size: f32,
    /// 이 확대 배율 이상에서 모든 글자를 보인다(낮을수록 항상 표시)
    pub label_zoom: f32,
    /// 마우스를 올린 노드의 이웃만 강조하고 나머지는 흐리게
    pub highlight: bool,
    pub color_by_category: bool,
    /// 입체 효과 세기(0 = 끔, 1 = 최대). 마우스에 따라 시차·기울임 원근으로 입체처럼 보이게 한다(그리기만 바뀜).
    pub depth_fx: f32,
    /// 배치: 0 = 자유(물리), 1 원형, 2 방사형, 3 계층형, 4 별자리, 5 해바라기
    pub layout: u8,
    /// 잔잔한 움직임 세기(0 = 정지). 노드가 천천히 떠다니며 영감을 준다.
    pub motion: f32,
    // ── 힘(바꾸면 다시 흔들어 자리 잡음) ──
    pub center_force: f32,
    pub repel_force: f32,
    pub link_force: f32,
    pub link_distance: f32,
    // ── 찾기 ──
    pub filter: String,
}

impl Default for GraphSettings {
    fn default() -> Self {
        GraphSettings {
            depth: 0,
            show_orphans: false,
            show_missing: true,
            arrows: false,
            node_size: 1.0,
            size_by_links: true,
            link_width: 1.0,
            label_size: 12.0,
            label_zoom: 0.6,
            highlight: true,
            color_by_category: false,
            depth_fx: 0.6,
            layout: 0,
            motion: 1.0,
            center_force: 1.0,
            repel_force: 1.0,
            link_force: 1.0,
            link_distance: 80.0,
            filter: String::new(),
        }
    }
}

impl GraphSettings {
    /// 이 값이 바뀌면 그래프를 다시 만들어야 한다.
    pub fn build_key(&self) -> (u8, bool, bool) {
        (self.depth, self.show_orphans, self.show_missing)
    }
    fn forces(&self) -> [f32; 4] {
        [self.center_force, self.repel_force, self.link_force, self.link_distance]
    }
}

pub struct Node {
    pub key: String,
    pub id: Option<i64>, // None = 아직 없는 문서
    pub title: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub fixed: bool,
    pub cat: Option<i64>,
    /// 전체 연결 수(깊이 필터와 무관)
    pub deg: usize,
    /// 지금 보는 문서를 위해 임시로 넣은 노드
    pub temp: bool,
}

pub enum GraphAct {
    Open(i64),
    Create(String),
}

#[derive(Default)]
pub struct GraphOut {
    pub act: Option<GraphAct>,
    /// 설정이 바뀌었다(저장 필요)
    pub changed: bool,
    /// 표시 조건이 바뀌어 그래프를 다시 만들어야 한다
    pub rebuild: bool,
}

pub struct Graph {
    pub nodes: Vec<Node>,
    /// 방향 있는 연결(원본 → 대상)
    pub edges: Vec<(usize, usize)>,
    /// 물리 계산용: 쌍방 링크도 한 번만
    pedges: Vec<(usize, usize)>,
    adj: Vec<Vec<usize>>,
    pub center: Option<usize>,
    pub zoom: f32,
    pub pan: Vec2,
    pub steps: u32,
    drag: Option<usize>,
    want_fit: bool,
    forces: [f32; 4],
    /// 정해진 배치의 목표 좌표와 그 계산 조건
    targets: Vec<Vec2>,
    /// 잔잔한 움직임으로 인한 화면 표시용 오프셋(노드별)
    wob: Vec<Vec2>,
    lay_key: (u8, usize, usize, Option<usize>),
    /// 글자 자리(칸) 안정 배치: 칸 번호, 지금 그려지는 칸 위치(부드럽게 이동), 투명도와 그 목표
    lbl_slot: Vec<u8>,
    lbl_off: Vec<f32>,
    lbl_alpha: Vec<f32>,
    lbl_ta: Vec<f32>,
    lbl_key: Option<(u32, u32, u32, usize, u32, bool, Option<usize>, u32)>,
    lbl_vis: Vec<bool>,
    lbl_next: f64,
    /// 부드럽게 따라가는 마우스 위치(각 축 -1..1, 입체 효과용)
    ptr: Vec2,
}

const MAX_NODES: usize = 500;
const MAX_STEPS: u32 = 420;

fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let h6 = (h.fract() * 6.0).max(0.0);
    let i = h6.floor();
    let f = h6 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// 분류마다 고정된 색(황금비 색상 분산). 미분류는 보조 글자색.
pub fn cat_color(cat: Option<i64>, t: &Theme) -> Color32 {
    match cat {
        None => t.dim,
        Some(c) => hsv((c as f32 * 0.618_034).fract(), 0.6, if t.dark { 0.92 } else { 0.72 }),
    }
}

/// 입체 효과 상수(그리기에만 쓰인다).
const Z_UNIT: f32 = 120.0;
const PERSP_D: f32 = 900.0;
const TILT_DEG: f32 = 12.0;
const PARALLAX_PX: f32 = 14.0;
const FOLLOW: f32 = 0.12;

/// 깊이 z ∈ [-1, 1]: 연결이 많을수록 앞(+1). 중심 노드는 항상 +1. 연결 있는 노드가 없으면 0.
pub fn depth_of(deg: usize, max_deg: usize, is_center: bool) -> f32 {
    if is_center {
        return 1.0;
    }
    if max_deg == 0 {
        return 0.0;
    }
    (2.0 * (deg as f32 / max_deg as f32).min(1.0).sqrt() - 1.0).clamp(-1.0, 1.0)
}

/// 노드 반지름(화면 픽셀, 투영 배율 적용 전). 연결 수에 따라 최대 5배까지 커진다.
pub fn node_radius(deg: usize, node_size: f32, size_by_links: bool, is_center: bool) -> f32 {
    let by = if size_by_links { (1.0 + 0.6 * (deg as f32).sqrt()).min(5.0) } else { 1.0 };
    let c = if is_center { 1.3 } else { 1.0 };
    (4.5 * node_size * by * c).max(2.0)
}

/// 월드 위치 `p`·깊이 `z`를 마우스 위치 `ptr`(각 축 -1..1)에 따라 기울여 원근 투영한다 → (투영된 위치, 크기 배율).
/// `fx`가 0이면 그대로 `(p, 1.0)`.
pub fn project(p: Vec2, z: f32, ptr: Vec2, fx: f32) -> (Vec2, f32) {
    if fx <= 0.0 {
        return (p, 1.0);
    }
    let zz = z * Z_UNIT;
    let (sx, cx) = (ptr.y * TILT_DEG.to_radians() * fx).sin_cos();
    let (y1, z1) = (p.y * cx - zz * sx, p.y * sx + zz * cx);
    let (sy, cy) = (-ptr.x * TILT_DEG.to_radians() * fx).sin_cos();
    let (x2, z2) = (p.x * cy + z1 * sy, -p.x * sy + z1 * cy);
    let k = PERSP_D / (PERSP_D - z2).max(PERSP_D * 0.3);
    (Vec2::new(x2 * k, y1 * k), k)
}

/// 시차: 마우스 반대 방향으로 화면 픽셀만큼 밀린다. 앞(z↑) 점이 더 많이 움직인다.
pub fn parallax(z: f32, ptr: Vec2, fx: f32) -> Vec2 {
    if fx <= 0.0 {
        return Vec2::ZERO;
    }
    -ptr * (PARALLAX_PX * fx * ((z + 1.6) / 2.6))
}

/// 원을 흉내 낸 16각형 꼭짓점(점선 원용).
fn circle_pts(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..16).map(|i| {
        let a = i as f32 * std::f32::consts::TAU / 16.0;
        Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

impl Graph {
    /// `cats`: 문서 id → 분류 id(분류별 색용). 위치·고정 상태는 `old` 와 같은 키의 노드에서 이어받는다.
    pub fn build(
        docs: &[(i64, String, String)],
        cats: &HashMap<i64, i64>,
        center_id: Option<i64>,
        old: Option<&Graph>,
        s: &GraphSettings,
    ) -> Graph {
        // 제목이 있는 문서만 링크 대상이 된다.
        let mut keys: Vec<(String, Option<i64>, String)> = Vec::new(); // (key,id,title)
        let mut index: HashMap<String, usize> = HashMap::new();
        for (id, title, _) in docs {
            let k = norm(title);
            if k.is_empty() || index.contains_key(&k) {
                continue;
            }
            index.insert(k.clone(), keys.len());
            keys.push((k, Some(*id), title.trim().to_string()));
        }
        let mut directed: HashSet<(usize, usize)> = HashSet::new();
        for (id, title, body) in docs {
            let Some(&src) = index.get(&norm(title)).filter(|&&i| keys[i].1 == Some(*id)) else { continue };
            for target in md::wiki_links(body) {
                let k = norm(&target);
                let dst = *index.entry(k.clone()).or_insert_with(|| {
                    keys.push((k.clone(), None, target.trim().to_string()));
                    keys.len() - 1
                });
                if dst != src {
                    directed.insert((src, dst));
                }
            }
        }
        let n_all = keys.len();
        let alive: Vec<bool> = (0..n_all).map(|i| keys[i].1.is_some() || s.show_missing).collect();
        directed.retain(|(a, b)| alive[*a] && alive[*b]);
        let mut adj_all: Vec<Vec<usize>> = vec![vec![]; n_all];
        for (a, b) in &directed {
            if !adj_all[*a].contains(b) {
                adj_all[*a].push(*b);
            }
            if !adj_all[*b].contains(a) {
                adj_all[*b].push(*a);
            }
        }
        let center_key = center_id.and_then(|c| keys.iter().position(|k| k.1 == Some(c)));

        // 보일 노드 고르기
        let mut keep: Vec<bool> = (0..n_all)
            .map(|i| alive[i] && (!adj_all[i].is_empty() || Some(i) == center_key || (s.show_orphans && keys[i].1.is_some())))
            .collect();
        if s.depth > 0 {
            if let Some(c) = center_key {
                let mut dist = vec![usize::MAX; n_all];
                dist[c] = 0;
                let mut q = VecDeque::from([c]);
                while let Some(n) = q.pop_front() {
                    if dist[n] >= s.depth as usize {
                        continue;
                    }
                    for &m in &adj_all[n] {
                        if dist[m] == usize::MAX {
                            dist[m] = dist[n] + 1;
                            q.push_back(m);
                        }
                    }
                }
                for i in 0..n_all {
                    keep[i] &= dist[i] != usize::MAX;
                }
            }
        }
        // 너무 크면 중심(없으면 연결이 많은 노드)에서 가까운 순으로 자른다.
        if keep.iter().filter(|k| **k).count() > MAX_NODES {
            let mut order: Vec<usize> = (0..n_all).filter(|i| keep[*i]).collect();
            order.sort_by_key(|i| (Some(*i) != center_key, std::cmp::Reverse(adj_all[*i].len())));
            let mut seen = vec![false; n_all];
            let mut picked: Vec<usize> = Vec::new();
            'outer: for start in order {
                if seen[start] {
                    continue;
                }
                seen[start] = true;
                let mut q = VecDeque::from([start]);
                while let Some(n) = q.pop_front() {
                    if picked.len() >= MAX_NODES {
                        break 'outer;
                    }
                    picked.push(n);
                    for &m in &adj_all[n] {
                        if keep[m] && !seen[m] {
                            seen[m] = true;
                            q.push_back(m);
                        }
                    }
                }
            }
            keep = vec![false; n_all];
            for i in picked {
                keep[i] = true;
            }
        }

        let mut remap = vec![usize::MAX; n_all];
        let mut nodes: Vec<Node> = Vec::new();
        let prev: HashMap<&str, &Node> = old.map(|g| g.nodes.iter().map(|n| (n.key.as_str(), n)).collect()).unwrap_or_default();
        for (i, (k, id, title)) in keys.iter().enumerate() {
            if !keep[i] {
                continue;
            }
            remap[i] = nodes.len();
            let n = nodes.len();
            let (pos, fixed) = match prev.get(k.as_str()) {
                Some(p) => (p.pos, p.fixed),
                None => {
                    let a = n as f32 * 2.399963; // 황금각: 겹치지 않는 초기 배치
                    let r = 40.0 + 14.0 * (n as f32).sqrt() * 3.0;
                    (Vec2::new(a.cos() * r, a.sin() * r), false)
                }
            };
            let cat = id.and_then(|d| cats.get(&d).copied());
            nodes.push(Node { key: k.clone(), id: *id, title: title.clone(), pos, vel: Vec2::ZERO, fixed, cat, deg: adj_all[i].len(), temp: false });
        }
        let edges: Vec<(usize, usize)> = {
            let mut v: Vec<(usize, usize)> = directed
                .iter()
                .filter(|(a, b)| remap[*a] != usize::MAX && remap[*b] != usize::MAX)
                .map(|(a, b)| (remap[*a], remap[*b]))
                .collect();
            v.sort_unstable();
            v
        };
        let center = center_key.map(|c| remap[c]).filter(|c| *c != usize::MAX);
        let (zoom, pan, steps) = match old {
            Some(g) => (g.zoom, g.pan, if nodes.len() == g.nodes.len() { MAX_STEPS - 60 } else { 80 }),
            None => (1.0, Vec2::ZERO, 0),
        };
        // 배치 종류를 이어받아, 재빌드 때 고정(끌어 놓은) 노드가 풀리지 않게 한다.
        let mut g = Graph { nodes, edges, pedges: vec![], adj: vec![], center, zoom, pan, steps, drag: None, want_fit: false, forces: s.forces(), targets: vec![], wob: vec![], lay_key: (old.map_or(0, |g| g.lay_key.0), 0, 0, None), lbl_slot: vec![], lbl_off: vec![], lbl_alpha: vec![], lbl_ta: vec![], lbl_key: None, lbl_vis: vec![], lbl_next: 0.0, ptr: Vec2::ZERO };
        g.rebuild_derived();
        g
    }

    fn rebuild_derived(&mut self) {
        let set: HashSet<(usize, usize)> = self.edges.iter().copied().collect();
        self.pedges = self.edges.iter().copied().filter(|(a, b)| !(a > b && set.contains(&(*b, *a)))).collect();
        self.adj = vec![vec![]; self.nodes.len()];
        for (a, b) in &self.pedges {
            self.adj[*a].push(*b);
            self.adj[*b].push(*a);
        }
    }

    pub fn set_center(&mut self, id: Option<i64>) -> bool {
        match id.and_then(|i| self.nodes.iter().position(|n| n.id == Some(i))) {
            Some(i) => {
                self.center = Some(i);
                true
            }
            None => false,
        }
    }

    /// 중심을 `id` 문서로 옮긴다. 그래프에 없으면(연결 없는 문서) 임시 노드로 보여 주고,
    /// 이전의 임시 노드는 치운다. 나머지 노드의 자리·확대·이동 상태는 그대로.
    pub fn focus(&mut self, id: Option<i64>, title: &str) {
        let Some(id) = id else {
            self.center = None;
            self.prune_temp();
            return;
        };
        match self.nodes.iter().position(|n| n.id == Some(id)) {
            Some(i) => self.center = Some(i),
            None => {
                let t = title.trim();
                // 원점에는 다른 노드가 몰려 있으므로, 기존 노드들 바깥쪽 빈 자리에 둔다.
                let far = self.nodes.iter().map(|n| n.pos.length()).fold(0.0f32, f32::max) + 70.0;
                let a = self.nodes.len() as f32 * 2.399_963;
                let pos = if self.nodes.is_empty() { Vec2::ZERO } else { Vec2::new(far * a.cos(), far * a.sin()) };
                self.nodes.push(Node {
                    key: format!("#{id}"),
                    id: Some(id),
                    title: if t.is_empty() { "(제목 없음)".into() } else { t.to_string() },
                    pos,
                    vel: Vec2::ZERO,
                    fixed: true,
                    cat: None,
                    deg: 0,
                    temp: true,
                });
                self.adj.push(vec![]);
                self.center = Some(self.nodes.len() - 1);
            }
        }
        self.prune_temp();
    }

    fn prune_temp(&mut self) {
        let keep: Vec<bool> = (0..self.nodes.len()).map(|i| !self.nodes[i].temp || Some(i) == self.center).collect();
        if keep.iter().all(|k| *k) {
            return;
        }
        let mut remap = vec![usize::MAX; self.nodes.len()];
        let mut next = 0;
        for i in 0..self.nodes.len() {
            if keep[i] {
                remap[i] = next;
                next += 1;
            }
        }
        let old = std::mem::take(&mut self.nodes);
        self.nodes = old.into_iter().enumerate().filter(|(i, _)| keep[*i]).map(|(_, n)| n).collect();
        // 임시 노드에는 연결이 없으므로 다른 노드의 번호만 당긴다.
        for e in &mut self.edges {
            *e = (remap[e.0], remap[e.1]);
        }
        self.center = self.center.map(|c| remap[c]);
        self.drag = None;
        self.rebuild_derived();
    }

    /// 다음 그리기에서 화면에 맞춘다.
    pub fn request_fit(&mut self) {
        self.want_fit = true;
    }

    /// 화면에 그릴 노드 위치(실제 위치 + 잔잔한 움직임).
    fn dpos(&self, i: usize) -> Vec2 {
        self.nodes[i].pos + self.wob.get(i).copied().unwrap_or(Vec2::ZERO)
    }

    pub fn has_doc(&self, id: i64) -> bool {
        self.nodes.iter().any(|n| n.id == Some(id))
    }

    /// 물리 한 걸음. 충분히 자리를 잡으면 멈춘다.
    pub fn step(&mut self, s: &GraphSettings) {
        if self.steps >= MAX_STEPS || self.nodes.is_empty() {
            return;
        }
        self.steps += 1;
        let n = self.nodes.len();
        let mut force = vec![Vec2::ZERO; n];
        let repel = 3200.0 * s.repel_force;
        for i in 0..n {
            for j in (i + 1)..n {
                let d = self.nodes[i].pos - self.nodes[j].pos;
                let dist2 = d.length_sq().max(25.0);
                let f = d / dist2.sqrt() * (repel / dist2);
                force[i] += f;
                force[j] -= f;
            }
        }
        for (a, b) in &self.pedges {
            let d = self.nodes[*b].pos - self.nodes[*a].pos;
            let dist = d.length().max(1.0);
            let f = d / dist * ((dist - s.link_distance) * 0.04 * s.link_force);
            force[*a] += f;
            force[*b] -= f;
        }
        for (i, nd) in self.nodes.iter_mut().enumerate() {
            let g = if Some(i) == self.center { 0.08 } else { 0.012 } * s.center_force;
            force[i] -= nd.pos * g;
            if nd.fixed {
                nd.vel = Vec2::ZERO;
                continue;
            }
            nd.vel = (nd.vel + force[i]) * 0.82;
            let sp = nd.vel.length();
            if sp > 20.0 {
                nd.vel *= 20.0 / sp;
            }
            nd.pos += nd.vel;
        }
    }

    fn radius(&self, i: usize, s: &GraphSettings) -> f32 {
        node_radius(self.nodes[i].deg, s.node_size, s.size_by_links, Some(i) == self.center)
    }

    /// 노드마다 (화면 위치, 크기 배율, 깊이). 그리기·강조·클릭 판정·글자 위치가 모두 이 값을 쓴다.
    fn projected(&self, center: Pos2, fx: f32) -> Vec<(Pos2, f32, f32)> {
        let maxd = self.nodes.iter().map(|n| n.deg).max().unwrap_or(0);
        let o = center + self.pan;
        (0..self.nodes.len())
            .map(|i| {
                let z = depth_of(self.nodes[i].deg, maxd, Some(i) == self.center);
                let (q, k) = project(self.dpos(i), z, self.ptr, fx);
                let sh = parallax(z, self.ptr, fx);
                (Pos2::new(o.x + q.x * self.zoom + sh.x, o.y + q.y * self.zoom + sh.y), k, z)
            })
            .collect()
    }

    fn fit(&mut self, size: Vec2) {
        if self.nodes.is_empty() {
            return;
        }
        let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let use_t = self.lay_key.0 != 0 && self.targets.len() == self.nodes.len();
        for (i, n) in self.nodes.iter().enumerate() {
            let p = if use_t { self.targets[i] } else { n.pos };
            min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
            max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
        }
        let (bw, bh) = ((max.x - min.x).max(60.0), (max.y - min.y).max(60.0));
        self.zoom = ((size.x - 140.0) / bw).min((size.y - 90.0) / bh).clamp(0.3, 2.5);
        let mid = (min + max) * 0.5;
        self.pan = -mid * self.zoom;
    }

    /// 우클릭 설정 메뉴. 값을 바꾸면 곧바로 반영되고 저장된다.
    fn menu(&mut self, ui: &mut egui::Ui, s: &mut GraphSettings) {
        ui.set_min_width(270.0);
        ui.spacing_mut().slider_width = 120.0;
        ui.label(egui::RichText::new("그래프 설정").strong());
        ui.label(egui::RichText::new("바꾸는 즉시 적용되고 자동 저장됩니다").size(crate::theme::ui(-1.0)));
        ui.separator();

        ui.label(egui::RichText::new("표시").strong());
        ui.add(egui::Slider::new(&mut s.depth, 0..=6).text("중심에서 단계 (0=전체)"));
        ui.checkbox(&mut s.show_orphans, "연결 없는 문서도 표시");
        ui.checkbox(&mut s.show_missing, "없는 문서(점선) 표시");
        ui.checkbox(&mut s.arrows, "방향 화살표");
        ui.separator();

        ui.add(egui::Slider::new(&mut s.motion, 0.0..=3.0).text("잔잔한 움직임 (0=정지)"));
        ui.label(egui::RichText::new("배치").strong());
        ui.horizontal_wrapped(|ui| {
            for (i, name) in crate::layout::NAMES.iter().enumerate() {
                ui.selectable_value(&mut s.layout, i as u8, *name);
            }
        });
        ui.label(
            egui::RichText::new(match s.layout {
                1 => "원형: 연결된 문서끼리 이웃하게 원 위에 늘어놓습니다",
                2 => "방사형: 지금 문서를 중심에 두고 단계별 동심원(Eades)",
                3 => "계층형: 지금 문서에서 몇 번 건너는지에 따라 위→아래 층(Sugiyama)",
                4 => "별자리: 서로 연결된 문서 묶음마다 허브를 중심으로 한 별자리",
                5 => "해바라기: 연결이 많은 문서일수록 가운데(황금각 나선)",
                _ => "자유: 힘으로 자연스럽게 자리 잡습니다(끌어서 고정 가능)",
            })
            .size(crate::theme::ui(-1.0)),
        );
        ui.separator();
        ui.label(egui::RichText::new("모양").strong());
        ui.add(egui::Slider::new(&mut s.node_size, 0.5..=3.0).text("노드 크기"));
        ui.checkbox(&mut s.size_by_links, "연결이 많을수록 크게");
        ui.checkbox(&mut s.color_by_category, "분류별로 색 구분");
        ui.add(egui::Slider::new(&mut s.depth_fx, 0.0..=1.0).text("입체 효과 (0=끔)"));
        ui.add(egui::Slider::new(&mut s.link_width, 0.5..=4.0).text("선 굵기"));
        ui.add(egui::Slider::new(&mut s.label_size, 9.0..=22.0).text("글자 크기"));
        ui.add(egui::Slider::new(&mut s.label_zoom, 0.3..=2.0).text("글자 표시 확대 기준"));
        ui.checkbox(&mut s.highlight, "마우스 올린 노드의 이웃 강조");
        ui.separator();

        ui.label(egui::RichText::new("힘").strong());
        ui.add(egui::Slider::new(&mut s.center_force, 0.0..=3.0).text("중심으로 당기는 힘"));
        ui.add(egui::Slider::new(&mut s.repel_force, 0.2..=4.0).text("서로 미는 힘"));
        ui.add(egui::Slider::new(&mut s.link_force, 0.0..=3.0).text("연결선 당기는 힘"));
        ui.add(egui::Slider::new(&mut s.link_distance, 30.0..=250.0).text("연결선 길이"));
        ui.separator();

        ui.label(egui::RichText::new("찾기").strong());
        ui.add(egui::TextEdit::singleline(&mut s.filter).hint_text("제목으로 강조 (나머지는 흐리게)").desired_width(250.0));
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            if ui.button("화면에 맞추기").clicked() {
                self.want_fit = true;
                ui.close();
            }
            if ui.button("다시 배치").on_hover_text("고정을 풀고 처음부터 다시 자리 잡습니다").clicked() {
                for n in &mut self.nodes {
                    n.fixed = false;
                    n.vel = Vec2::ZERO;
                }
                self.steps = 0;
                ui.close();
            }
            if ui.button("기본값으로").clicked() {
                let filter = std::mem::take(&mut s.filter);
                *s = GraphSettings::default();
                s.filter = filter;
            }
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui, size: Vec2, t: &Theme, s: &mut GraphSettings) -> GraphOut {
        let before = s.clone();
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, t.bg);
        painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, t.border), StrokeKind::Inside);
        let mut out = GraphOut::default();
        // 설정 메뉴가 열려 있던 프레임: 메뉴를 닫으려는 바깥 클릭·끌기가 노드 열기나 이동으로 이어지지 않게 한다.
        let menu_was_open = resp.context_menu_opened();

        if self.nodes.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "[[문서 링크]]로 연결된\n문서가 아직 없습니다", FontId::proportional(crate::theme::ui(0.0)), t.dim);
            egui::Popup::context_menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| self.menu(ui, s));
            out.changed = *s != before;
            out.rebuild = s.build_key() != before.build_key();
            return out;
        }
        // 힘 설정이 바뀌면 다시 흔들어 자리를 잡는다.
        if self.forces != s.forces() {
            self.forces = s.forces();
            self.steps = self.steps.min(MAX_STEPS - 150);
        }
        if s.layout != 0 {
            // 정해진 배치: 목표 좌표를 (다시) 계산하고 부드럽게 이동한다.
            let key = (s.layout, self.nodes.len(), self.edges.len(), self.center);
            if key != self.lay_key || self.targets.len() != self.nodes.len() {
                if key.0 != self.lay_key.0 {
                    for nd in &mut self.nodes {
                        if !nd.temp {
                            nd.fixed = false;
                        }
                    }
                }
                self.lay_key = key;
                self.targets = crate::layout::compute(s.layout, &self.adj, self.center);
                self.want_fit = true;
            }
            let mut moving = false;
            for (nd, t) in self.nodes.iter_mut().zip(&self.targets) {
                if nd.fixed && !nd.temp {
                    continue;
                }
                let d = *t - nd.pos;
                if d.length() > 0.4 {
                    nd.pos += d * 0.18;
                    moving = true;
                } else {
                    nd.pos = *t;
                }
                nd.vel = Vec2::ZERO;
            }
            if moving {
                ui.ctx().request_repaint();
            }
        } else {
            if self.lay_key.0 != 0 {
                // 자유 배치로 돌아오면 지금 자리에서 다시 자리 잡는다.
                self.lay_key = (0, 0, 0, None);
                self.targets.clear();
                self.steps = self.steps.min(MAX_STEPS - 150);
            }
            if self.steps < MAX_STEPS {
                for _ in 0..2 {
                    self.step(s);
                }
                ui.ctx().request_repaint();
            }
        }
        // 잔잔한 움직임: 실제 좌표는 건드리지 않고 표시 위치만 천천히 흔든다(계산량이 매우 적음).
        let active = s.motion > 0.01 && !ui.ctx().input(|i| i.viewport().minimized.unwrap_or(false)) && ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
        if s.motion > 0.01 {
            let tm = ui.ctx().input(|i| i.time) as f32;
            let amp = 5.0 * s.motion;
            self.wob.resize(self.nodes.len(), Vec2::ZERO);
            for (i, w) in self.wob.iter_mut().enumerate() {
                let ph = i as f32 * 2.399_963; // 노드마다 다른 위상(황금각)
                let sp = 0.35 + (i % 5) as f32 * 0.07;
                *w = Vec2::new(amp * (tm * sp + ph).sin() + amp * 0.4 * (tm * sp * 2.3 + ph * 1.7).cos(), amp * (tm * sp * 0.83 + ph * 1.3).cos() + amp * 0.4 * (tm * sp * 1.9 + ph).sin());
            }
            if active {
                // 약 16프레임/초. 창이 가려지거나 포커스를 잃으면 멈춘다.
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
            }
        } else if !self.wob.is_empty() {
            self.wob.clear();
        }
        if self.want_fit {
        self.want_fit = false;
            self.fit(size);
        }

        // 입체 효과: 마우스 위치를 부드럽게 따라가고, 수렴하면 다시 그리기를 멈춘다.
        let fx = s.depth_fx.clamp(0.0, 1.0);
        let tgt = if fx > 0.0 && !resp.context_menu_opened() {
            resp.hover_pos().map_or(Vec2::ZERO, |p| Vec2::new((p.x - rect.center().x) / (rect.width() / 2.0).max(1.0), (p.y - rect.center().y) / (rect.height() / 2.0).max(1.0)).clamp(Vec2::splat(-1.0), Vec2::splat(1.0)))
        } else {
            Vec2::ZERO
        };
        self.ptr += (tgt - self.ptr) * FOLLOW;
        if (tgt - self.ptr).length() < 0.002 {
            self.ptr = tgt;
        } else {
            ui.ctx().request_repaint();
        }

        let origin = |g: &Graph| rect.center() + g.pan;
        let to_screen = |g: &Graph, p: Vec2| {
            let o = origin(g);
            Pos2::new(o.x + p.x * g.zoom, o.y + p.y * g.zoom)
        };
        let hit = |g: &Graph, pr: &[(Pos2, f32, f32)], p: Pos2| -> Option<usize> {
            pr.iter()
                .enumerate()
                .filter(|(i, (q, k, _))| (*q - p).length() <= (g.radius(*i, s) * *k).max(6.0) + 4.0)
                .min_by(|a, b| (a.1.0 - p).length().total_cmp(&(b.1.0 - p).length()))
                .map(|(i, _)| i)
        };
        let proj = self.projected(rect.center(), fx);

        // 확대/축소
        if resp.hovered() {
            let sc = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if sc != 0.0 {
                self.zoom = (self.zoom * (1.0 + sc * 0.002)).clamp(0.3, 4.0);
            }
        }
        // 끌기: 노드는 자리 고정, 빈 곳은 이동
        if resp.drag_started_by(egui::PointerButton::Primary) && !menu_was_open {
            self.drag = resp.interact_pointer_pos().and_then(|p| hit(self, &proj, p));
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            match self.drag {
                Some(i) => {
                    let zz = self.zoom;
                    let n = &mut self.nodes[i];
                    n.pos += d / zz;
                    n.fixed = true;
                    n.vel = Vec2::ZERO;
                    self.steps = self.steps.min(MAX_STEPS - 120);
                }
                None => self.pan += d,
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }

        // 끌어서 노드가 움직였을 수 있으니 투영을 다시 계산한다(한 프레임 늦게 따라오지 않게).
        let proj = self.projected(rect.center(), fx);
        let zfade = |i: usize| 1.0 - 0.25 * fx * (-proj[i].2).max(0.0);
        let hover = if resp.context_menu_opened() { None } else { resp.hover_pos().and_then(|p| hit(self, &proj, p)) };
        let n = self.nodes.len();
        // 강조/흐림 대상 계산
        let mut near: Option<Vec<bool>> = None;
        if s.highlight {
            if let Some(h) = hover {
                let mut v = vec![false; n];
                v[h] = true;
                for &m in &self.adj[h] {
                    v[m] = true;
                }
                near = Some(v);
            }
        }
        let f = s.filter.trim().to_lowercase();
        let matched: Option<Vec<bool>> = if f.is_empty() { None } else { Some(self.nodes.iter().map(|nd| nd.title.to_lowercase().contains(&f)).collect()) };
        let faded = |i: usize| -> bool { near.as_ref().map_or(false, |v| !v[i]) || matched.as_ref().map_or(false, |v| !v[i]) };
        let dim_c = |c: Color32, fade: bool| if fade { c.gamma_multiply(0.28) } else { c };

        // 선
        let lw = s.link_width;
        for (a, b) in &self.edges {
            let (pa, pb) = (proj[*a].0, proj[*b].0);
            let touch_c = Some(*a) == self.center || Some(*b) == self.center;
            let touch_h = hover.map_or(false, |h| h == *a || h == *b);
            let fade = faded(*a) || faded(*b);
            let col = if touch_h || (touch_c && near.is_none()) { t.accent } else { dim_c(t.border, fade) };
            let col = col.gamma_multiply((zfade(*a) + zfade(*b)) / 2.0);
            let w = if touch_h || touch_c { lw * 1.4 } else { lw };
            painter.line_segment([pa, pb], Stroke::new(w, col));
            if s.arrows {
                let d = pb - pa;
                let len = d.length();
                if len > 1.0 {
                    let dir = d / len;
                    let tip = pb - dir * (self.radius(*b, s) * proj[*b].1 + 2.0);
                    let sz = 5.0 + lw * 1.5;
                    let perp = Vec2::new(-dir.y, dir.x);
                    let pts = vec![tip, tip - dir * sz + perp * sz * 0.5, tip - dir * sz - perp * sz * 0.5];
                    painter.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
                }
            }
        }

        // 글자: 겹치면 아래 칸으로 밀어 피한다. 칸은 잔잔한 움직임을 뺀 실제 좌표로 정하고(0.5초마다·화면이 바뀔 때만 다시 계산,
        // 겹침이 없으면 지금 칸을 그대로 둔다), 화면에서는 칸 사이를 부드럽게 미끄러지게 해서 튀지 않는다.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|i| (Some(*i) != self.center, !near.as_ref().map_or(false, |v| v[*i])));
        let font = FontId::proportional(s.label_size);
        let all_labels = self.zoom >= s.label_zoom;
        if self.lbl_slot.len() != n {
            self.lbl_slot = vec![0; n];
            self.lbl_off = vec![0.0; n];
            self.lbl_alpha = vec![1.0; n];
            self.lbl_ta = vec![1.0; n];
            self.lbl_key = None;
        }
        let ppp = ui.ctx().pixels_per_point();
        let snap = |p: Pos2| Pos2::new((p.x * ppp).round() / ppp, (p.y * ppp).round() / ppp);
        let mut vis_list: Vec<(usize, std::sync::Arc<egui::Galley>)> = Vec::new();
        let mut vis_flags = vec![false; n];
        for i in order {
            let p = proj[i].0;
            if !rect.expand(20.0).contains(p) {
                continue;
            }
            let is_c = Some(i) == self.center;
            let hot = near.as_ref().map_or(false, |v| v[i]) || matched.as_ref().map_or(false, |v| v[i]);
            if !all_labels && !is_c && !hot {
                continue;
            }
            // 투명도는 목표를 향해 부드럽게(사라질 때 서서히)
            let ta = self.lbl_ta[i];
            self.lbl_alpha[i] += (ta - self.lbl_alpha[i]) * 0.25;
            if (ta - self.lbl_alpha[i]).abs() < 0.02 {
                self.lbl_alpha[i] = ta;
            }
            // 아직 없는 문서(점선 점)는 이름을 흐리게 하고 "(없음)"을 붙여 만들어진 문서와 한눈에 구분한다.
            let missing = self.nodes[i].id.is_none();
            let base = if is_c { t.text } else if missing { t.dim } else { t.quote };
            let col = dim_c(base, faded(i)).gamma_multiply(self.lbl_alpha[i] * zfade(i));
            vis_flags[i] = true;
            let shown = if missing { format!("{} (없음)", self.nodes[i].title) } else { self.nodes[i].title.clone() };
            vis_list.push((i, painter.layout_no_wrap(shown, font.clone(), col)));
        }
        let now = ui.ctx().input(|i| i.time);
        let key = (self.zoom.to_bits(), self.pan.x.to_bits(), self.pan.y.to_bits(), n, s.label_size.to_bits(), all_labels, self.center, self.steps);
        if self.lbl_key != Some(key) || self.lbl_vis != vis_flags || now >= self.lbl_next {
            self.lbl_key = Some(key);
            self.lbl_vis = vis_flags;
            self.lbl_next = now + 0.5;
            let mut placed: Vec<Rect> = Vec::new();
            for (i, g) in &vis_list {
                let sp = to_screen(self, self.nodes[*i].pos);
                let sz = g.size();
                let bx = sp.x - sz.x / 2.0;
                let by = sp.y + self.radius(*i, s) + 3.0;
                let rect_at = |slot: u8| Rect::from_min_size(Pos2::new(bx, by + slot as f32 * sz.y), sz).expand(1.0);
                let free = |slot: u8, placed: &Vec<Rect>| {
                    let r = rect_at(slot);
                    !placed.iter().any(|q| q.intersects(r))
                };
                let cur = self.lbl_slot[*i];
                let slot = if free(cur, &placed) { Some(cur) } else { (0..8u8).find(|&sl| free(sl, &placed)) };
                match slot {
                    Some(sl) => {
                        self.lbl_slot[*i] = sl;
                        self.lbl_ta[*i] = 1.0;
                        placed.push(rect_at(sl));
                    }
                    None if Some(*i) == self.center => {
                        self.lbl_slot[*i] = 0;
                        self.lbl_ta[*i] = 1.0;
                        placed.push(rect_at(0));
                    }
                    None => self.lbl_ta[*i] = 0.0, // 자리가 없으면 서서히 숨긴다
                }
            }
        }
        let mut animating = false;
        for (i, g) in vis_list {
            let target = self.lbl_slot[i] as f32;
            let off = &mut self.lbl_off[i];
            *off += (target - *off) * 0.3;
            if (target - *off).abs() < 0.02 {
                *off = target;
            }
            animating |= *off != target || self.lbl_alpha[i] != self.lbl_ta[i];
            if self.lbl_alpha[i] <= 0.01 {
                continue;
            }
            let p = proj[i].0;
            let pos = snap(Pos2::new(p.x - g.size().x / 2.0, p.y + self.radius(i, s) * proj[i].1 + 3.0 + self.lbl_off[i] * g.size().y));
            painter.galley(pos, g, Color32::WHITE);
        }
        if animating {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        // 노드(구슬 모양 원). 입체 효과가 켜져 있으면 뒤(깊이 낮은) 노드부터 그려 앞 노드가 위에 겹친다.
        let mut ord: Vec<usize> = (0..n).collect();
        if fx > 0.0 {
            ord.sort_by(|a, b| proj[*a].2.total_cmp(&proj[*b].2));
        }
        for i in ord {
            let nd = &self.nodes[i];
            let p = proj[i].0;
            let is_c = Some(i) == self.center;
            let half = self.radius(i, s) * proj[i].1 + if hover == Some(i) { 1.5 } else { 0.0 };
            let fade = faded(i);
            let fill = if is_c {
                t.accent
            } else if s.color_by_category {
                cat_color(nd.cat, t)
            } else {
                t.dim
            };
            let h = half;
            let zf = zfade(i);
            if nd.id.is_none() {
                painter.circle_filled(p, h, t.bg);
                dashed_poly(&painter, &circle_pts(p, h), Stroke::new(1.5, dim_c(t.dim, fade).gamma_multiply(zf)));
            } else {
                painter.circle_filled(p, h, dim_c(fill, fade).gamma_multiply(zf));
                if fx > 0.0 {
                    // 구슬의 반짝이는 하이라이트(왼쪽 위)
                    let a = (90.0 * fx * zf * if fade { 0.28 } else { 1.0 }) as u8;
                    painter.circle_filled(p + Vec2::new(-0.32, -0.32) * h, h * 0.4, Color32::from_white_alpha(a));
                }
                if nd.fixed {
                    painter.circle_stroke(p, h + 2.0, Stroke::new(1.0, dim_c(t.accent, fade)));
                }
            }
            if matched.as_ref().map_or(false, |v| v[i]) {
                painter.circle_stroke(p, h + 3.0, Stroke::new(1.5, t.accent));
            }
        }

        // 요약(한눈에 보기): 노드·연결 수, 올린 노드의 연결 수
        let summary = match hover {
            Some(h) => format!("{} · 연결 {}", self.nodes[h].title, self.nodes[h].deg),
            None => format!("노드 {} · 연결 {}", n, self.pedges.len()),
        };
        painter.text(Pos2::new(rect.left() + 6.0, rect.bottom() - 4.0), Align2::LEFT_BOTTOM, summary, FontId::proportional(crate::theme::ui(-1.0)), t.dim);
        painter.text(Pos2::new(rect.right() - 6.0, rect.bottom() - 4.0), Align2::RIGHT_BOTTOM, "우클릭: 설정", FontId::proportional(crate::theme::ui(-1.0)), t.dim);

        if resp.clicked_by(egui::PointerButton::Primary) && !menu_was_open {
            if let Some(i) = resp.interact_pointer_pos().and_then(|p| hit(self, &proj, p)) {
                let nd = &self.nodes[i];
                out.act = Some(match nd.id {
                    Some(id) => GraphAct::Open(id),
                    None => GraphAct::Create(nd.title.clone()),
                });
            }
        }
        egui::Popup::context_menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| self.menu(ui, s));
        out.changed = *s != before;
        out.rebuild = s.build_key() != before.build_key();
        out
    }
}

fn dashed_poly(p: &egui::Painter, pts: &[Pos2], s: Stroke) {
    let mut v = pts.to_vec();
    v.push(pts[0]);
    let pts = v;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b - a).length();
        let dir = (b - a) / len.max(0.001);
        let mut d = 0.0;
        while d < len {
            let e = (d + 2.0).min(len);
            p.line_segment([a + dir * d, a + dir * e], s);
            d += 4.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<(i64, String, String)> {
        vec![
            (1, "A".into(), "[[B]] 그리고 [[없는 문서]]".into()),
            (2, "B".into(), "돌아가기 [[a]]".into()), // 대소문자 무시
            (3, "고립".into(), "링크 없음".into()),
            (4, "C".into(), "[[B|보임]]".into()),
        ]
    }
    fn build(center: Option<i64>, s: &GraphSettings) -> Graph {
        Graph::build(&docs(), &HashMap::new(), center, None, s)
    }

    #[test]
    fn builds_nodes_directed_edges_and_ghosts() {
        let g = build(Some(1), &GraphSettings::default());
        let titles: Vec<_> = g.nodes.iter().map(|n| n.title.as_str()).collect();
        assert!(!titles.contains(&"고립"), "연결 없는 문서는 기본적으로 숨김");
        assert_eq!(g.nodes.len(), 4); // A, B, C, 없는 문서
        assert_eq!(g.edges.len(), 4); // A→B, B→A, A→없는, C→B (방향 있음)
        assert_eq!(g.pedges.len(), 3); // 쌍방 링크는 물리 계산에서 한 번
        assert_eq!(g.nodes.iter().filter(|n| n.id.is_none()).count(), 1);
        assert_eq!(g.nodes[g.center.unwrap()].title, "A");
    }

    #[test]
    fn settings_filter_the_graph() {
        let mut s = GraphSettings::default();
        s.show_orphans = true;
        let g = build(Some(1), &s);
        assert!(g.nodes.iter().any(|n| n.title == "고립"), "고립 문서 표시");
        s.show_orphans = false;
        s.show_missing = false;
        let g = build(Some(1), &s);
        assert_eq!(g.nodes.len(), 3);
        assert!(g.nodes.iter().all(|n| n.id.is_some()), "없는 문서 숨김");
        assert!(g.edges.iter().all(|(a, b)| *a < 3 && *b < 3));
    }

    #[test]
    fn depth_limits_distance_from_center() {
        // A - B - C - D 사슬
        let d = vec![
            (1, "A".to_string(), "[[B]]".to_string()),
            (2, "B".to_string(), "[[C]]".to_string()),
            (3, "C".to_string(), "[[D]]".to_string()),
            (4, "D".to_string(), String::new()),
        ];
        let mut s = GraphSettings::default();
        s.depth = 1;
        let g = Graph::build(&d, &HashMap::new(), Some(1), None, &s);
        let mut t: Vec<_> = g.nodes.iter().map(|n| n.title.clone()).collect();
        t.sort();
        assert_eq!(t, vec!["A", "B"]);
        s.depth = 2;
        assert_eq!(Graph::build(&d, &HashMap::new(), Some(1), None, &s).nodes.len(), 3);
        s.depth = 0;
        assert_eq!(Graph::build(&d, &HashMap::new(), Some(1), None, &s).nodes.len(), 4);
    }

    #[test]
    fn degree_categories_and_build_key() {
        let mut cats = HashMap::new();
        cats.insert(2, 7);
        let g = Graph::build(&docs(), &cats, Some(1), None, &GraphSettings::default());
        let b = g.nodes.iter().find(|n| n.title == "B").unwrap();
        assert_eq!(b.deg, 2); // A, C
        assert_eq!(b.cat, Some(7));
        let mut s = GraphSettings::default();
        let k = s.build_key();
        s.node_size = 2.0;
        assert_eq!(s.build_key(), k, "모양만 바뀌면 다시 만들 필요 없음");
        s.depth = 3;
        assert_ne!(s.build_key(), k);
    }

    #[test]
    fn isolated_center_is_kept_and_layout_settles() {
        let g = build(Some(3), &GraphSettings::default());
        assert_eq!(g.nodes[g.center.unwrap()].title, "고립");
        let s = GraphSettings::default();
        let mut g = build(Some(1), &s);
        for _ in 0..MAX_STEPS {
            g.step(&s);
        }
        for i in 0..g.nodes.len() {
            assert!(g.nodes[i].pos.x.is_finite() && g.nodes[i].pos.length() < 1000.0);
            for j in (i + 1)..g.nodes.len() {
                assert!((g.nodes[i].pos - g.nodes[j].pos).length() > 12.0, "노드가 겹치지 않아야 함");
            }
        }
    }

    #[test]
    fn force_settings_change_the_layout() {
        let mut s = GraphSettings::default();
        let spread = |s: &GraphSettings| {
            let mut g = build(Some(1), s);
            for _ in 0..MAX_STEPS {
                g.step(s);
            }
            g.nodes.iter().map(|n| n.pos.length()).sum::<f32>()
        };
        let normal = spread(&s);
        s.link_distance = 200.0;
        s.repel_force = 3.0;
        assert!(spread(&s) > normal * 1.2, "반발·거리를 키우면 더 퍼져야 함");
    }

    #[test]
    fn rebuild_keeps_positions_and_fixed() {
        let mut g = build(Some(1), &GraphSettings::default());
        g.nodes[1].pos = Vec2::new(77.0, 55.0);
        g.nodes[1].fixed = true;
        let key = g.nodes[1].key.clone();
        let g2 = Graph::build(&docs(), &HashMap::new(), Some(2), Some(&g), &GraphSettings::default());
        let n = g2.nodes.iter().find(|n| n.key == key).unwrap();
        assert_eq!(n.pos, Vec2::new(77.0, 55.0));
        assert!(n.fixed);
    }

    #[test]
    fn focus_adds_and_prunes_temp_nodes() {
        let mut g = build(Some(1), &GraphSettings::default());
        let n0 = g.nodes.len();
        g.focus(Some(3), "고립");
        assert_eq!(g.nodes.len(), n0 + 1);
        assert_eq!(g.nodes[g.center.unwrap()].title, "고립");
        g.focus(Some(2), "B");
        assert_eq!(g.nodes.len(), n0, "이전 임시 노드는 치운다");
        assert_eq!(g.nodes[g.center.unwrap()].title, "B");
        assert_eq!(g.adj.len(), g.nodes.len());
    }

    #[test]
    fn large_vault_is_capped_even_without_center() {
        let mut d = vec![(0, "허브".to_string(), String::new())];
        let mut hub = String::new();
        for i in 1..800 {
            d.push((i, format!("n{i}"), "[[허브]]".into()));
            hub.push_str(&format!("[[n{i}]] "));
        }
        d[0].2 = hub;
        let g = Graph::build(&d, &HashMap::new(), Some(0), None, &GraphSettings::default());
        assert!(g.nodes.len() <= MAX_NODES);
        assert!(g.center.is_some());
        let g = Graph::build(&d, &HashMap::new(), None, None, &GraphSettings::default());
        assert!(g.nodes.len() <= MAX_NODES && !g.nodes.is_empty(), "중심이 없어도 일부는 보여야 함");
    }

    #[test]
    fn settings_serialize_with_defaults_for_missing_fields() {
        let s = GraphSettings::default();
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<GraphSettings>(&j).unwrap(), s);
        // 옛 설정 파일(필드 일부만 있음)도 읽혀야 한다.
        let part: GraphSettings = serde_json::from_str(r#"{"depth":2,"arrows":true}"#).unwrap();
        assert_eq!(part.depth, 2);
        assert!(part.arrows);
        assert_eq!(part.link_distance, 80.0);
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn depth_of_range_and_center() {
        assert_eq!(depth_of(0, 0, false), 0.0); // 연결 있는 노드가 없어도 NaN이 아니다
        assert_eq!(depth_of(0, 10, false), -1.0);
        assert_eq!(depth_of(10, 10, false), 1.0);
        assert_eq!(depth_of(0, 10, true), 1.0);
        for d in 0..=40 {
            let z = depth_of(d, 40, false);
            assert!(z.is_finite() && (-1.0..=1.0).contains(&z), "{d} {z}");
        }
    }

    #[test]
    fn node_radius_grows_with_links_and_caps() {
        assert!((node_radius(0, 1.0, true, false) - 4.5).abs() < 1e-4);
        assert!((node_radius(4, 1.0, true, false) - 9.9).abs() < 0.1);
        assert!((node_radius(25, 1.0, true, false) - 18.0).abs() < 1e-3);
        assert!((node_radius(10_000, 1.0, true, false) - 22.5).abs() < 1e-3); // 상한 5배
        assert_eq!(node_radius(50, 1.0, false, false), node_radius(0, 1.0, false, false));
        assert!((node_radius(0, 1.0, true, true) - 4.5 * 1.3).abs() < 1e-4);
        assert!(node_radius(0, 0.0, true, false) >= 2.0);
    }

    #[test]
    fn project_identity_when_fx_zero() {
        for (p, z, ptr) in [(Vec2::new(40.0, -25.0), 0.7, Vec2::new(0.9, -0.4)), (Vec2::ZERO, -1.0, Vec2::new(1.0, 1.0))] {
            assert_eq!(project(p, z, ptr, 0.0), (p, 1.0));
            assert_eq!(parallax(z, ptr, 0.0), Vec2::ZERO);
        }
    }

    #[test]
    fn project_near_is_bigger_and_finite() {
        let ptr = Vec2::new(1.0, 1.0);
        let (_, near) = project(Vec2::new(100.0, 80.0), 1.0, ptr, 1.0);
        let (_, far) = project(Vec2::new(100.0, 80.0), -1.0, ptr, 1.0);
        assert!(near > far, "{near} {far}");
        for z in [-1.0, 0.0, 1.0] {
            for p in [Vec2::new(5000.0, 5000.0), Vec2::new(-5000.0, 5000.0)] {
                let (q, k) = project(p, z, ptr, 1.0);
                assert!(q.x.is_finite() && q.y.is_finite() && k.is_finite() && k > 0.0, "{q:?} {k}");
            }
        }
        // 앞 점이 뒤 점보다 더 많이 밀린다
        assert!(parallax(1.0, ptr, 1.0).length() > parallax(-1.0, ptr, 1.0).length());
    }

    #[test]
    fn settings_drop_old_shape_fields() {
        let s: GraphSettings = serde_json::from_str(r#"{"shape_mode":2,"hub_links":7,"depth":2}"#).unwrap();
        assert_eq!(s.depth, 2);
        assert_eq!(s.depth_fx, 0.6);
    }
}
