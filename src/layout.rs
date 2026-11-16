//! 연결 그래프 배치 알고리즘(힘 기반 외의 정해진 배치). 모두 노드마다 목표 좌표를 돌려준다.
//!
//! 참고한 문헌(고전적인 그래프 그리기 연구):
//! - 원형: J. Six, I. Tollis, "Circular drawings of biconnected graphs" (1999) — 연결된 노드를 원 위에서 가깝게 두어 교차를 줄이는 발상.
//!   여기서는 연결 성분별로 너비 우선 순서로 늘어놓는 간단한 방식을 쓴다.
//! - 방사형: P. Eades, "Drawing free trees" (1992) — 중심에서 단계별 동심원, 하위 노드 수에 비례해 각도를 나눈다.
//! - 계층형: K. Sugiyama, S. Tagawa, M. Toda (1981) — 계층 배정 후 인접 계층 평균 위치(barycenter)로 교차를 줄인다.
//! - 별자리: 연결 성분을 하나의 '별자리'(허브를 중심으로 한 방사 배치)로 보고, 큰 것부터 동심 고리에 겹치지 않게 놓는다.
//! - 해바라기: H. Vogel, "A better way to construct the sunflower head" (1979)의 황금각(137.5°) 나선 — 연결이 많은 노드가 안쪽.
//! 특허 문헌은 검토하지 않았다.
use eframe::egui::Vec2;
use std::collections::VecDeque;

const TAU: f32 = std::f32::consts::TAU;

pub const NAMES: &[&str] = &["자유(물리)", "원형", "방사형", "계층형", "별자리", "해바라기"];

/// 한 연결 성분 안에서 root에서 시작하는 너비 우선 탐색(방문 순서, 깊이, 부모).
fn bfs(adj: &[Vec<usize>], root: usize, seen: &mut [bool]) -> (Vec<usize>, Vec<usize>, Vec<Option<usize>>) {
    let n = adj.len();
    let mut depth = vec![0usize; n];
    let mut parent = vec![None; n];
    let mut order = vec![];
    let mut q = VecDeque::new();
    seen[root] = true;
    q.push_back(root);
    while let Some(u) = q.pop_front() {
        order.push(u);
        let mut nb = adj[u].clone();
        nb.sort_by_key(|v| std::cmp::Reverse(adj[*v].len()));
        for v in nb {
            if !seen[v] {
                seen[v] = true;
                depth[v] = depth[u] + 1;
                parent[v] = Some(u);
                q.push_back(v);
            }
        }
    }
    (order, depth, parent)
}

/// 연결 성분(노드 번호 목록). 각 성분의 첫 노드는 연결이 가장 많은 노드(허브), `first`가 있으면 그 성분이 맨 앞.
fn components(adj: &[Vec<usize>], first: Option<usize>) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut seen = vec![false; n];
    let mut starts: Vec<usize> = (0..n).collect();
    starts.sort_by_key(|i| (Some(*i) != first, std::cmp::Reverse(adj[*i].len())));
    let mut out = vec![];
    for s in starts {
        if seen[s] {
            continue;
        }
        let (order, _, _) = bfs(adj, s, &mut seen);
        out.push(order); // order[0] = s = 성분의 허브(또는 지정한 중심)
    }
    out
}

/// 한 성분을 root 중심의 방사형으로: (노드, 좌표) 목록과 반지름.
fn radial(adj: &[Vec<usize>], comp: &[usize], gap: f32) -> (Vec<(usize, Vec2)>, f32) {
    let n = adj.len();
    let mut seen = vec![false; n];
    let (order, depth, parent) = bfs(adj, comp[0], &mut seen);
    let mut children = vec![vec![]; n];
    for &v in &order {
        if let Some(p) = parent[v] {
            children[p].push(v);
        }
    }
    let mut leaves = vec![1.0f32; n];
    for &v in order.iter().rev() {
        if !children[v].is_empty() {
            leaves[v] = children[v].iter().map(|c| leaves[*c]).sum();
        }
    }
    let mut ang = vec![(0.0f32, TAU); n]; // 각 노드가 쓰는 각도 구간
    let mut res = vec![];
    let mut maxd = 0;
    for &v in &order {
        let (a0, a1) = ang[v];
        let mid = (a0 + a1) * 0.5;
        let r = depth[v] as f32 * gap;
        maxd = maxd.max(depth[v]);
        res.push((v, if depth[v] == 0 { Vec2::ZERO } else { Vec2::new(r * mid.cos(), r * mid.sin()) }));
        let total = leaves[v].max(1.0);
        let mut a = a0;
        for &c in &children[v] {
            let w = (a1 - a0) * leaves[c] / total;
            ang[c] = (a, a + w);
            a += w;
        }
    }
    (res, maxd as f32 * gap)
}

/// 성분들을 겹치지 않게 동심 고리에 놓는다(첫 성분은 가운데).
fn pack(parts: Vec<(Vec<(usize, Vec2)>, f32)>, n: usize) -> Vec<Vec2> {
    let mut pos = vec![Vec2::ZERO; n];
    let margin = 40.0;
    let mut ring_r = 0.0f32;
    let mut placed_first = false;
    let mut angle = 0.0f32;
    let mut ring_max = 0.0f32;
    let mut prev_outer = 0.0f32;
    for (pts, r) in parts {
        let r = r.max(14.0);
        let center = if !placed_first {
            placed_first = true;
            prev_outer = r + margin;
            ring_r = 0.0;
            Vec2::ZERO
        } else {
            if ring_r == 0.0 {
                ring_r = prev_outer + r;
                angle = 0.0;
                ring_max = 0.0;
            }
            let need = 2.0 * ((r + margin * 0.5) / ring_r).min(1.0).asin();
            if angle + need > TAU + 1e-3 {
                prev_outer = ring_r + ring_max + margin;
                ring_r = prev_outer + r;
                angle = 0.0;
                ring_max = 0.0;
            }
            let need = 2.0 * ((r + margin * 0.5) / ring_r).min(1.0).asin();
            let a = angle + need * 0.5;
            angle += need;
            ring_max = ring_max.max(r);
            Vec2::new(ring_r * a.cos(), ring_r * a.sin())
        };
        for (i, p) in pts {
            pos[i] = center + p;
        }
    }
    pos
}

/// 배치 번호(1~5)에 따른 목표 좌표. 0(자유)은 쓰지 않는다.
pub fn compute(kind: u8, adj: &[Vec<usize>], center: Option<usize>) -> Vec<Vec2> {
    let n = adj.len();
    if n == 0 {
        return vec![];
    }
    match kind {
        1 => circular(adj, center),
        2 => {
            // 중심 문서 성분만 방사형, 나머지 노드는 바깥 고리에 개별로 둔다.
            let comps = components(adj, center);
            let mut parts = vec![];
            let mut rest: Vec<usize> = vec![];
            for (i, c) in comps.into_iter().enumerate() {
                if i == 0 {
                    parts.push(radial(adj, &c, 90.0));
                } else {
                    rest.extend(c);
                }
            }
            let mut pos = pack(parts, n);
            let base = 90.0 * 1.0 + pos.iter().map(|p| p.length()).fold(0.0, f32::max) + 70.0;
            let m = rest.len().max(1) as f32;
            let r = base.max(m * 30.0 / TAU);
            for (k, v) in rest.iter().enumerate() {
                let a = k as f32 / m * TAU;
                pos[*v] = Vec2::new(r * a.cos(), r * a.sin());
            }
            pos
        }
        3 => layered(adj, center),
        4 => {
            let mut comps = components(adj, center);
            comps.sort_by_key(|c| std::cmp::Reverse(c.len())); // 큰 별자리부터
            let parts = comps.into_iter().map(|c| radial(adj, &c, 46.0)).collect();
            pack(parts, n)
        }
        5 => {
            let mut idx: Vec<usize> = (0..n).collect();
            idx.sort_by_key(|i| (Some(*i) != center, std::cmp::Reverse(adj[*i].len())));
            let mut pos = vec![Vec2::ZERO; n];
            for (k, i) in idx.into_iter().enumerate() {
                let r = 34.0 * (k as f32).sqrt();
                let a = k as f32 * 2.399_963;
                pos[i] = Vec2::new(r * a.cos(), r * a.sin());
            }
            pos
        }
        _ => vec![Vec2::ZERO; n],
    }
}

fn circular(adj: &[Vec<usize>], center: Option<usize>) -> Vec<Vec2> {
    let n = adj.len();
    let order: Vec<usize> = components(adj, center).into_iter().flatten().collect();
    let r = (n as f32 * 34.0 / TAU).max(90.0);
    let mut pos = vec![Vec2::ZERO; n];
    for (k, v) in order.into_iter().enumerate() {
        let a = k as f32 / n as f32 * TAU - std::f32::consts::FRAC_PI_2;
        pos[v] = Vec2::new(r * a.cos(), r * a.sin());
    }
    pos
}

fn layered(adj: &[Vec<usize>], center: Option<usize>) -> Vec<Vec2> {
    let n = adj.len();
    // 성분마다 허브(또는 중심)에서 깊이를 매기고, 성분들은 같은 계층 번호를 공유한다.
    let mut layer = vec![0usize; n];
    let mut seen = vec![false; n];
    let mut starts: Vec<usize> = (0..n).collect();
    starts.sort_by_key(|i| (Some(*i) != center, std::cmp::Reverse(adj[*i].len())));
    let mut base_order: Vec<usize> = vec![];
    for s in starts {
        if seen[s] {
            continue;
        }
        let (order, depth, _) = bfs(adj, s, &mut seen);
        for &v in &order {
            layer[v] = depth[v];
        }
        base_order.extend(order);
    }
    let maxl = layer.iter().copied().max().unwrap_or(0);
    let mut rows: Vec<Vec<usize>> = vec![vec![]; maxl + 1];
    for &v in &base_order {
        rows[layer[v]].push(v);
    }
    // 교차 줄이기: 인접 계층 평균 위치로 위·아래를 번갈아 정렬
    let mut x = vec![0.0f32; n];
    let place = |rows: &Vec<Vec<usize>>, x: &mut Vec<f32>| {
        for r in rows {
            for (k, v) in r.iter().enumerate() {
                x[*v] = k as f32 - (r.len() as f32 - 1.0) / 2.0;
            }
        }
    };
    place(&rows, &mut x);
    for sweep in 0..4 {
        let order: Vec<usize> = if sweep % 2 == 0 { (1..=maxl).collect() } else { (0..maxl).rev().collect() };
        for l in order {
            let nl = if sweep % 2 == 0 { l - 1 } else { l + 1 };
            let mut keyed: Vec<(f32, usize)> = rows[l]
                .iter()
                .map(|v| {
                    let nb: Vec<f32> = adj[*v].iter().filter(|u| layer[**u] == nl).map(|u| x[*u]).collect();
                    (if nb.is_empty() { x[*v] } else { nb.iter().sum::<f32>() / nb.len() as f32 }, *v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[l] = keyed.into_iter().map(|(_, v)| v).collect();
            place(&rows, &mut x);
        }
    }
    let mut pos = vec![Vec2::ZERO; n];
    for (l, r) in rows.iter().enumerate() {
        for v in r {
            pos[*v] = Vec2::new(x[*v] * 78.0, l as f32 * 90.0 - maxl as f32 * 45.0);
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adj(n: usize, e: &[(usize, usize)]) -> Vec<Vec<usize>> {
        let mut a = vec![vec![]; n];
        for (x, y) in e {
            a[*x].push(*y);
            a[*y].push(*x);
        }
        a
    }

    #[test]
    fn all_layouts_place_every_node_without_overlap() {
        // 허브 0에 1..5, 5-6, 별도 성분 7-8, 고립 9
        let a = adj(10, &[(0, 1), (0, 2), (0, 3), (0, 4), (0, 5), (5, 6), (7, 8)]);
        for k in 1..=5u8 {
            let p = compute(k, &a, Some(0));
            assert_eq!(p.len(), 10);
            for i in 0..10 {
                assert!(p[i].x.is_finite() && p[i].y.is_finite());
                for j in (i + 1)..10 {
                    assert!((p[i] - p[j]).length() > 8.0, "배치 {k}: 노드 {i}와 {j}가 겹침");
                }
            }
        }
    }

    #[test]
    fn radial_puts_center_at_origin_and_children_farther() {
        let a = adj(4, &[(0, 1), (1, 2), (1, 3)]);
        let p = compute(2, &a, Some(0));
        assert!(p[0].length() < 1.0);
        assert!(p[1].length() < p[2].length());
    }

    #[test]
    fn layered_uses_distinct_rows_by_depth() {
        let a = adj(3, &[(0, 1), (1, 2)]);
        let p = compute(3, &a, Some(0));
        assert!(p[0].y < p[1].y && p[1].y < p[2].y);
    }
}
