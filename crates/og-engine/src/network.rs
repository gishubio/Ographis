//! Shortest paths over a network of lines, such as the roads of NVDB, with Dijkstra's
//! algorithm (E. W. Dijkstra, 1959) and a binary heap.
//!
//! The lines are joined where their ends meet. Points are put on the network at the nearest
//! place on the nearest line, which need not be a junction. To find the nearest of many
//! destinations from each of many origins, one search runs backwards from all the
//! destinations at once: it gives every junction its nearest destination and the way there.

use geo::{Coord, LineString};
use rstar::{primitives::GeomWithData, PointDistance, RTree};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// A piece of the network between two junctions: one line, or one part of a line.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub line: LineString<f64>,
    pub length: f64,
    /// The cost of going along the line as it is drawn, and against it. None where it may
    /// not be travelled that way.
    pub along: Option<f64>,
    pub against: Option<f64>,
    /// The feature of the network it comes from.
    pub feature: usize,
}

/// A line of the network with what it costs to travel: (feature, line, cost per unit of
/// length along, and against).
pub struct NetLine {
    pub feature: usize,
    pub line: LineString<f64>,
    pub along: Option<f64>,
    pub against: Option<f64>,
}

/// A piece of a line in the index: the line, and which piece of which edge it is.
type Segment = GeomWithData<rstar::primitives::Line<[f64; 2]>, (usize, usize)>;

pub struct Network {
    pub edges: Vec<Edge>,
    pub nodes: usize,
    /// For each junction, the edges that leave it and those that come to it.
    out: Vec<Vec<usize>>,
    into: Vec<Vec<usize>>,
    segments: RTree<Segment>,
}

/// Where a point lies on the network: the edge, how far along it, and how far the point is
/// from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub edge: usize,
    pub at: f64,
    pub off: f64,
    pub point: Coord<f64>,
}

#[derive(Copy, Clone, PartialEq)]
struct Item(f64, usize);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// What a search found: for each junction the cost, the edge it was reached by, and which
/// of the starting places it came from.
pub struct Search {
    pub cost: Vec<f64>,
    pub via: Vec<Option<usize>>,
    pub source: Vec<usize>,
    backwards: bool,
}

fn length(l: &LineString<f64>) -> f64 {
    l.0.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum()
}

impl Network {
    /// Builds the network. Ends closer than `tolerance` are the same junction.
    pub fn build(lines: Vec<NetLine>, tolerance: f64) -> Network {
        let tol = tolerance.max(1e-9);
        let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
        let mut node = |c: Coord<f64>| {
            let key = ((c.x / tol).round() as i64, (c.y / tol).round() as i64);
            let n = ids.len();
            *ids.entry(key).or_insert(n)
        };
        let mut edges = Vec::with_capacity(lines.len());
        for l in lines {
            if l.line.0.len() < 2 || (l.along.is_none() && l.against.is_none()) {
                continue;
            }
            let (a, b) = (node(l.line.0[0]), node(*l.line.0.last().expect("two points")));
            let len = length(&l.line);
            edges.push(Edge {
                from: a,
                to: b,
                length: len,
                along: l.along.map(|k| k * len),
                against: l.against.map(|k| k * len),
                line: l.line,
                feature: l.feature,
            });
        }
        let nodes = ids.len();
        let (mut out, mut into) = (vec![Vec::new(); nodes], vec![Vec::new(); nodes]);
        for (i, e) in edges.iter().enumerate() {
            out[e.from].push(i);
            into[e.to].push(i);
        }
        let mut segs = Vec::new();
        for (i, e) in edges.iter().enumerate() {
            for (k, w) in e.line.0.windows(2).enumerate() {
                segs.push(GeomWithData::new(rstar::primitives::Line::new([w[0].x, w[0].y], [w[1].x, w[1].y]), (i, k)));
            }
        }
        Network { edges, nodes, out, into, segments: RTree::bulk_load(segs) }
    }

    /// The place on the network nearest a point, if one is within `most`.
    pub fn place(&self, p: Coord<f64>, most: f64) -> Option<Place> {
        let q = [p.x, p.y];
        let best = self.segments.nearest_neighbor(&q)?;
        let off = best.distance_2(&q).sqrt();
        if off > most {
            return None;
        }
        let (edge, k) = best.data;
        let line = &self.edges[edge].line;
        let (a, b) = (line.0[k], line.0[k + 1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let point = Coord { x: a.x + t * dx, y: a.y + t * dy };
        let before: f64 = line.0[..=k].windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum();
        Some(Place { edge, at: before + t * len2.sqrt(), off, point })
    }

    /// The cost per unit of length on an edge, along or against it.
    fn rate(&self, e: usize, along: bool) -> Option<f64> {
        let e = &self.edges[e];
        let c = if along { e.along } else { e.against }?;
        Some(if e.length > 0.0 { c / e.length } else { 0.0 })
    }

    /// The junctions a search starts from, for a place: (junction, cost, source). Forwards,
    /// leaving the place; backwards, the junctions from which the place is reached.
    fn seeds(&self, p: &Place, source: usize, backwards: bool) -> Vec<(usize, f64, usize)> {
        let e = &self.edges[p.edge];
        let mut s = Vec::new();
        // Forwards: to the end of the edge along it, to the start against it. Backwards:
        // from the start along it, from the end against it.
        match backwards {
            false => {
                if let Some(r) = self.rate(p.edge, true) {
                    s.push((e.to, (e.length - p.at) * r, source));
                }
                if let Some(r) = self.rate(p.edge, false) {
                    s.push((e.from, p.at * r, source));
                }
            }
            true => {
                if let Some(r) = self.rate(p.edge, true) {
                    s.push((e.from, p.at * r, source));
                }
                if let Some(r) = self.rate(p.edge, false) {
                    s.push((e.to, (e.length - p.at) * r, source));
                }
            }
        }
        s
    }

    /// Dijkstra's algorithm from the places, forwards or backwards. `stop` is asked now and
    /// then; when it answers false the search ends early.
    pub fn search(&self, starts: &[(Place, usize)], backwards: bool, stop: &dyn Fn() -> bool) -> Search {
        let n = self.nodes;
        let (mut cost, mut via, mut source) = (vec![f64::INFINITY; n], vec![None; n], vec![usize::MAX; n]);
        let mut heap = BinaryHeap::new();
        for (p, s) in starts {
            for (node, c, src) in self.seeds(p, *s, backwards) {
                if c < cost[node] {
                    cost[node] = c;
                    source[node] = src;
                    via[node] = None;
                    heap.push(Item(c, node));
                }
            }
        }
        let mut steps = 0usize;
        while let Some(Item(c, u)) = heap.pop() {
            if c > cost[u] {
                continue;
            }
            steps += 1;
            if steps.is_multiple_of(100_000) && !stop() {
                break;
            }
            // Forwards: edges leaving u along them, and edges coming to u against them.
            // Backwards the other way round.
            let (first, second) = if backwards { (&self.into[u], &self.out[u]) } else { (&self.out[u], &self.into[u]) };
            for &e in first {
                if let Some(w) = self.edges[e].along {
                    let v = if backwards { self.edges[e].from } else { self.edges[e].to };
                    if c + w < cost[v] {
                        cost[v] = c + w;
                        via[v] = Some(e);
                        source[v] = source[u];
                        heap.push(Item(c + w, v));
                    }
                }
            }
            for &e in second {
                if let Some(w) = self.edges[e].against {
                    let v = if backwards { self.edges[e].to } else { self.edges[e].from };
                    if c + w < cost[v] {
                        cost[v] = c + w;
                        via[v] = Some(e);
                        source[v] = source[u];
                        heap.push(Item(c + w, v));
                    }
                }
            }
        }
        Search { cost, via, source, backwards }
    }

    /// The way from a place to the nearest start of a backward search, or from the start of
    /// a forward search to a place: its cost, the start it ends or begins at, and its line.
    /// `places` are the places the search started from, by source.
    pub fn route(&self, s: &Search, p: &Place, places: &[(Place, usize)]) -> Option<(f64, usize, LineString<f64>)> {
        let e = &self.edges[p.edge];
        // The ways off the edge of the place: (junction, cost to it, going along).
        let mut best: Option<(f64, usize, Option<usize>, bool)> = None;
        let mut consider = |c: f64, src: usize, node: Option<usize>, along: bool| {
            if c.is_finite() && best.is_none_or(|b| c < b.0) {
                best = Some((c, src, node, along));
            }
        };
        if s.backwards {
            // From the place to a junction, then on towards the start.
            if let Some(r) = self.rate(p.edge, true) {
                consider((e.length - p.at) * r + s.cost[e.to], s.source[e.to], Some(e.to), true);
            }
            if let Some(r) = self.rate(p.edge, false) {
                consider(p.at * r + s.cost[e.from], s.source[e.from], Some(e.from), false);
            }
        } else {
            if let Some(r) = self.rate(p.edge, true) {
                consider(s.cost[e.from] + p.at * r, s.source[e.from], Some(e.from), true);
            }
            if let Some(r) = self.rate(p.edge, false) {
                consider(s.cost[e.to] + (e.length - p.at) * r, s.source[e.to], Some(e.to), false);
            }
        }
        // A start on the same edge can be reached without leaving it.
        for (q, src) in places.iter().filter(|x| x.0.edge == p.edge) {
            let (from, to) = if s.backwards { (p.at, q.at) } else { (q.at, p.at) };
            let along = to >= from;
            if let Some(r) = self.rate(p.edge, along) {
                consider((to - from).abs() * r, *src, None, along);
            }
        }
        let (cost, src, node, along) = best?;
        let start = places.iter().find(|q| q.1 == src)?.0;
        let mut coords: Vec<Coord<f64>> = Vec::new();
        let push = |coords: &mut Vec<Coord<f64>>, part: Vec<Coord<f64>>| {
            for c in part {
                if coords.last() != Some(&c) {
                    coords.push(c);
                }
            }
        };
        match node {
            None => push(&mut coords, cut(&e.line, if s.backwards { p.at } else { start.at }, if s.backwards { start.at } else { p.at })),
            Some(j) => {
                // The junctions from j to the start of the search, by the edges they were reached by.
                let mut chain: Vec<(usize, usize)> = Vec::new();
                let mut u = j;
                let mut guard = 0;
                while let Some(edge) = s.via[u] {
                    let ed = &self.edges[edge];
                    let next = if ed.from == u { ed.to } else { ed.from };
                    chain.push((edge, u));
                    u = next;
                    guard += 1;
                    if guard > self.edges.len() {
                        return None;
                    }
                }
                let last = u;
                let mut pieces: Vec<Vec<Coord<f64>>> = Vec::new();
                // The piece of the edge of the place, from the place to j.
                pieces.push(if along == s.backwards { cut(&e.line, p.at, e.length) } else { cut(&e.line, p.at, 0.0) });
                for (edge, at) in &chain {
                    let ed = &self.edges[*edge];
                    pieces.push(if ed.from == *at { ed.line.0.clone() } else { ed.line.0.iter().rev().copied().collect() });
                }
                // The piece of the edge of the start, from its junction to the start.
                let se = &self.edges[start.edge];
                pieces.push(if se.from == last { cut(&se.line, 0.0, start.at) } else { cut(&se.line, se.length, start.at) });
                if !s.backwards {
                    pieces.reverse();
                    for p in pieces.iter_mut() {
                        p.reverse();
                    }
                }
                for part in pieces {
                    push(&mut coords, part);
                }
            }
        }
        if coords.len() == 1 {
            coords.push(coords[0]);
        }
        Some((cost, src, LineString(coords)))
    }
}

/// The part of a line between two distances along it, in the order asked: from `a` to `b`,
/// backwards when `b` is less than `a`.
pub fn cut(l: &LineString<f64>, a: f64, b: f64) -> Vec<Coord<f64>> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out = Vec::new();
    let mut run = 0.0;
    let at = |p: Coord<f64>, q: Coord<f64>, t: f64| Coord { x: p.x + (q.x - p.x) * t, y: p.y + (q.y - p.y) * t };
    for w in l.0.windows(2) {
        let d = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
        let (s, e) = (run, run + d);
        if e >= lo && s <= hi && d > 0.0 {
            if out.is_empty() {
                out.push(at(w[0], w[1], ((lo - s) / d).clamp(0.0, 1.0)));
            }
            if e <= hi {
                out.push(w[1]);
            } else {
                out.push(at(w[0], w[1], ((hi - s) / d).clamp(0.0, 1.0)));
                break;
            }
        }
        run = e;
    }
    if out.is_empty() {
        let c = if lo <= 0.0 { l.0[0] } else { *l.0.last().expect("a line") };
        out.push(c);
    }
    if b < a {
        out.reverse();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(c: &[(f64, f64)]) -> LineString<f64> {
        LineString::from(c.to_vec())
    }

    /// A ladder: two long roads joined by rungs every 100 m, and one rung that is one-way.
    fn ladder() -> Network {
        let mut lines = Vec::new();
        let mut f = 0;
        let mut add = |l: LineString<f64>, along: Option<f64>, against: Option<f64>| {
            lines.push(NetLine { feature: f, line: l, along, against });
            f += 1;
        };
        for k in 0..10 {
            let x = k as f64 * 100.0;
            add(line(&[(x, 0.0), (x + 100.0, 0.0)]), Some(1.0), Some(1.0));
            add(line(&[(x, 100.0), (x + 50.0, 100.0), (x + 100.0, 100.0)]), Some(1.0), Some(1.0));
        }
        for k in [0, 5, 10] {
            let x = k as f64 * 100.0;
            add(line(&[(x, 0.0), (x, 100.0)]), Some(1.0), Some(1.0));
        }
        // From the upper road down at 300 only.
        add(line(&[(300.0, 100.0), (300.0, 0.0)]), Some(1.0), None);
        Network::build(lines, 0.01)
    }

    #[test]
    fn the_nearest_destination_is_found_along_the_roads() {
        let net = ladder();
        assert_eq!(net.nodes, 11 + 11);
        let h1 = (net.place(Coord { x: 950.0, y: 5.0 }, 50.0).unwrap(), 0);
        let h2 = (net.place(Coord { x: 50.0, y: 0.0 }, 50.0).unwrap(), 1);
        let hospitals = [h1, h2];
        let s = net.search(&hospitals, true, &|| true);
        // On the upper road at 320: down the one-way rung at 300 is 20 + 100, then 250 to h2:
        // 370. Up to 500 and down is 180 + 100 + 450 to h1. So h2.
        let p = net.place(Coord { x: 320.0, y: 103.0 }, 50.0).unwrap();
        let (cost, to, path) = net.route(&s, &p, &hospitals).unwrap();
        assert_eq!(to, 1);
        assert!((cost - 370.0).abs() < 1e-6, "{}", cost);
        assert_eq!(path.0.first().copied(), Some(Coord { x: 320.0, y: 100.0 }));
        assert_eq!(path.0.last().copied(), Some(Coord { x: 50.0, y: 0.0 }));
        assert!((length(&path) - 370.0).abs() < 1e-6, "{:?}", path);
        // From the lower road at 280 the one-way rung cannot be taken up: to h2 directly, 230.
        let q = net.place(Coord { x: 280.0, y: 0.0 }, 50.0).unwrap();
        let (cost, to, _) = net.route(&s, &q, &hospitals).unwrap();
        assert_eq!((to, cost), (1, 230.0));
        // Forwards from one origin to every destination gives the same.
        let start = [(p, 0)];
        let f = net.search(&start, false, &|| true);
        let (c1, _, back) = net.route(&f, &hospitals[1].0, &start).unwrap();
        assert!((c1 - 370.0).abs() < 1e-6);
        assert_eq!(back.0.first().copied(), Some(Coord { x: 320.0, y: 100.0 }));
        assert_eq!(back.0.last().copied(), Some(Coord { x: 50.0, y: 0.0 }));
        // On the same edge, without leaving it.
        let near = net.place(Coord { x: 60.0, y: 0.0 }, 50.0).unwrap();
        let (c, to, l) = net.route(&s, &near, &hospitals).unwrap();
        assert_eq!((to, c, l.0.len()), (1, 10.0, 2));
        assert!(net.place(Coord { x: 500.0, y: 500.0 }, 50.0).is_none(), "too far from the network");
    }

    #[test]
    fn parts_of_lines_are_cut_in_either_direction() {
        let l = line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        assert_eq!(cut(&l, 5.0, 15.0), vec![Coord { x: 5.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }, Coord { x: 10.0, y: 5.0 }]);
        assert_eq!(cut(&l, 15.0, 5.0), vec![Coord { x: 10.0, y: 5.0 }, Coord { x: 10.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 }]);
    }
}
