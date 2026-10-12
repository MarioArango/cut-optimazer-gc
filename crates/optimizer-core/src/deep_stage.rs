//! Stage 18: bounded single-board guillotine generator for 4..=6 cut levels.
//!
//! A region is partitioned by parallel cuts of the current level into cells.
//! Each cell is an exact piece, a remnant, or is filled recursively by the
//! next level (perpendicular cuts). With four or more levels a cell may hold
//! pieces shorter than itself — one more cut trims them — and nest smaller
//! pieces in the leftover, which a three-level pattern cannot express.
//!
//! First-level strips are chosen by beam search; inside a strip the filling
//! is greedy under several heuristics. Every reported pattern is re-evaluated
//! by the independent Stage 5 tree evaluator, which also enforces the level
//! sequence, kerf, exact piece sizes, grain and machine limits.

use crate::guillotine::{evaluate_pattern, EvaluatedPattern, TreeError};
use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, CuttingPattern, ModelIssue,
    PatternNode, StagePattern, Stock, TrimMargins, Unit,
};
use crate::three_stage::valid_search_settings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::time::{Duration, Instant};

const MAX_PART_TYPES: usize = 256;
/// Pieces on one board; also bounds the tree size handed to the evaluator.
const MAX_PIECES: usize = 400;
/// Cells (parallel cuts + 1) inside one region.
const MAX_CELLS: usize = 48;
const MAX_STRIPS: usize = 32;
const MAX_WIDTH_CHOICES: usize = 32;
/// Thickness values tried for one cell before giving up on it.
const MAX_CELL_TRIES: usize = 3;
/// Levels (counted from the first) where every tried cell is filled and the
/// densest one kept; deeper levels take the first cell that fits.
const EXPLORE_UNTIL_LEVEL: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepStageHeuristic {
    /// Prefer cells whose pieces fill the cross direction exactly (no trim).
    CrossFit,
    /// Thickest cell first.
    Thickest,
    /// Largest piece area first.
    LargestArea,
    PriorityFirst,
}
impl DeepStageHeuristic {
    const ALL: [Self; 4] = [Self::CrossFit, Self::Thickest, Self::LargestArea, Self::PriorityFirst];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeepStageError {
    InvalidModel(Vec<ModelIssue>),
    UnknownStock(String),
    InvalidSearchSettings,
    ResourceLimit(&'static str),
    InvalidGeneratedPattern(TreeError),
}
impl fmt::Display for DeepStageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid model ({} issue(s))", issues.len()),
            Self::UnknownStock(id) => write!(f, "unknown stock '{id}'"),
            Self::InvalidSearchSettings => write!(f, "beam_width must be 1..=64 and max_expansions must be 1..=20000"),
            Self::ResourceLimit(text) => write!(f, "multi-level search limit: {text}"),
            Self::InvalidGeneratedPattern(err) => write!(f, "invalid generated pattern: {err}"),
        }
    }
}
impl std::error::Error for DeepStageError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeepStageBoard {
    pub stock_id: String,
    pub stock_unit_index: u32,
    pub board_rotated_90: bool,
    pub mode: StagePattern,
    pub heuristic: DeepStageHeuristic,
    pub utilization_basis_points: u32,
    pub pattern: CuttingPattern,
    pub evaluated: EvaluatedPattern,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeepStageResult {
    pub found: bool,
    pub complete_order: bool,
    pub candidates_evaluated: usize,
    pub expanded_states: usize,
    pub search_exhausted: bool,
    pub board: Option<DeepStageBoard>,
    pub remaining_part_counts: BTreeMap<String, u32>,
    pub note: String,
}

/// One orientation of one part, in board coordinates.
#[derive(Clone, Debug)]
struct Choice {
    index: usize,
    rotated: bool,
    w: Unit,
    h: Unit,
    priority: u8,
}

fn rotate_axis(axis: Option<Axis>, rotated: bool) -> Option<Axis> {
    if !rotated { return axis; }
    axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
}
fn rotate_trim(t: TrimMargins) -> TrimMargins {
    TrimMargins { top: t.left, right: t.top, bottom: t.right, left: t.bottom }
}
fn usable(stock: &Stock, rotated: bool) -> (Unit, Unit) {
    let (w, h, t) = if rotated {
        (stock.height, stock.width, rotate_trim(stock.trim))
    } else { (stock.width, stock.height, stock.trim) };
    (w - t.left - t.right, h - t.top - t.bottom)
}
fn flip(direction: CutDirection) -> CutDirection {
    match direction { CutDirection::Vertical => CutDirection::Horizontal, CutDirection::Horizontal => CutDirection::Vertical }
}
/// (extent split by the cuts, cross extent spanned by each cut).
fn along(direction: CutDirection, w: Unit, h: Unit) -> (Unit, Unit) {
    match direction { CutDirection::Vertical => (w, h), CutDirection::Horizontal => (h, w) }
}
/// Width × height of a cell of `thickness` inside a region of `cross`.
fn cell(direction: CutDirection, thickness: Unit, cross: Unit) -> (Unit, Unit) {
    match direction { CutDirection::Vertical => (thickness, cross), CutDirection::Horizontal => (cross, thickness) }
}
/// Avoid slivers smaller than the saw channel. The zero-scrap case is exact.
fn can_separate(available: Unit, taken: Unit, kerf: Unit) -> bool {
    available == taken || available.checked_sub(taken).is_some_and(|d| d > kerf)
}

fn choices(model: &CuttingJobModel, stock: &Stock, rotated_board: bool) -> Vec<Choice> {
    let board_grain = rotate_axis(stock.grain_axis, rotated_board);
    let mut out = Vec::new();
    for (index, part) in model.parts.iter().enumerate() {
        if part.material_id != stock.material_id { continue; }
        for rotated in [false, true] {
            if rotated && !part.allow_rotate_90 { continue; }
            if part.grain_axis.is_some() && rotate_axis(part.grain_axis, rotated) != board_grain { continue; }
            let (w, h) = if rotated { (part.height, part.width) } else { (part.width, part.height) };
            if w < model.machine.min_piece_width || h < model.machine.min_piece_height { continue; }
            if rotated && w == h { continue; }
            out.push(Choice { index, rotated, w, h, priority: part.priority });
        }
    }
    out
}

/// A filled region: its subtree and what it holds. `value` is the area
/// weighted by each part's value (equal to `area` × 1000 when unweighted).
#[derive(Clone, Debug)]
struct Fill {
    node: PatternNode,
    area: u128,
    value: u128,
    count: usize,
}

/// Neutral weight: every part is worth its area.
pub const UNIT_VALUE: u64 = 1000;

struct Ctx<'a> {
    model: &'a CuttingJobModel,
    choices: &'a [Choice],
    kerf: Unit,
    max_cut: Unit,
    max_level: u8,
    method: DeepStageHeuristic,
    seed: u64,
    /// Value per unit of area, by part index.
    weights: &'a [u64],
    /// Strip number: keeps node ids unique across the strips of a board.
    prefix: usize,
    next_id: u32,
}

impl Ctx<'_> {
    fn id(&mut self) -> String {
        let id = format!("s18n{}x{}", self.prefix, self.next_id);
        self.next_id += 1;
        id
    }
    fn piece(&mut self, c: &Choice) -> PatternNode {
        PatternNode::Piece { node_id: self.id(), part_id: self.model.parts[c.index].id.clone(), rotated: c.rotated }
    }
    fn residual(&mut self) -> PatternNode {
        PatternNode::Residual { node_id: self.id(), reusable: false }
    }
    fn split(&mut self, direction: CutDirection, at: Unit, first: PatternNode, second: PatternNode) -> PatternNode {
        PatternNode::Split { node_id: self.id(), direction, at, first: Box::new(first), second: Box::new(second) }
    }

    /// Rank of a candidate cell thickness `t` (higher first). `fit` is the
    /// largest cross size among its pieces, `exact` whether one fills `cross`.
    fn rank(&self, t: Unit, fit: Unit, exact: bool, priority: u8) -> (u64, u64, u64) {
        let tie = u64::from(t).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ self.seed.rotate_left(17);
        let (t64, f64_) = (u64::from(t), u64::from(fit));
        match self.method {
            DeepStageHeuristic::CrossFit => (u64::from(exact), f64_ * t64, tie),
            DeepStageHeuristic::Thickest => (t64, f64_, tie),
            DeepStageHeuristic::LargestArea => (t64 * f64_, t64, tie),
            DeepStageHeuristic::PriorityFirst => (u64::from(priority), t64 * f64_, tie),
        }
    }

    /// Fill a `w × h` region whose cuts at `level` run in `direction`.
    /// `counts` is only changed when a fill is returned.
    fn fill(&mut self, w: Unit, h: Unit, direction: CutDirection, level: u8, counts: &mut [u32]) -> Option<Fill> {
        // An exact piece needs no cut at all.
        if let Some(c) = self.choices.iter()
            .filter(|c| c.w == w && c.h == h && counts[c.index] > 0)
            .max_by_key(|c| (c.priority, std::cmp::Reverse(c.index)))
            .cloned()
        {
            counts[c.index] -= 1;
            let node = self.piece(&c);
            let area = u128::from(w) * u128::from(h);
            return Some(Fill { node, area, value: area * u128::from(self.weights[c.index]), count: 1 });
        }
        if level > self.max_level { return None; }
        let (extent, cross) = along(direction, w, h);
        let mut local = counts.to_vec();
        let mut cells: Vec<(Unit, Fill)> = Vec::new();
        let mut remaining = extent;
        while cells.len() < MAX_CELLS && remaining > 0 {
            // Candidate thicknesses: each with its best cross fit.
            let mut options: BTreeMap<Unit, (Unit, bool, u8)> = BTreeMap::new();
            for c in self.choices {
                if local[c.index] == 0 { continue; }
                let (t, other) = along(direction, c.w, c.h);
                if t > remaining || other > cross || !can_separate(remaining, t, self.kerf) { continue; }
                // A cut spans `cross`; leaving the full extent needs no cut.
                if t != remaining && cross > self.max_cut { continue; }
                // On the last level a cell must be exactly one piece.
                if level == self.max_level && other != cross { continue; }
                let entry = options.entry(t).or_insert((0, false, 0));
                entry.0 = entry.0.max(other);
                entry.1 |= other == cross;
                entry.2 = entry.2.max(c.priority);
            }
            let mut ranked: Vec<(Unit, (u64, u64, u64))> = options.iter()
                .map(|(&t, &(fit, exact, priority))| (t, self.rank(t, fit, exact, priority))).collect();
            ranked.sort_by_key(|&(_, rank)| std::cmp::Reverse(rank));
            let explore = level <= EXPLORE_UNTIL_LEVEL;
            let mut chosen: Option<(Unit, Fill, Vec<u32>)> = None;
            for &(t, _) in ranked.iter().take(MAX_CELL_TRIES) {
                let (cw, ch) = cell(direction, t, cross);
                let mut trial = local.clone();
                let Some(f) = self.fill(cw, ch, flip(direction), level + 1, &mut trial) else { continue; };
                // Densest cell wins (value per thickness); ties keep the earlier rank.
                let denser = chosen.as_ref().is_none_or(|(ct, cf, _)| f.value * u128::from(*ct) > cf.value * u128::from(t));
                if denser { chosen = Some((t, f, trial)); }
                if !explore { break; }
            }
            let Some((t, f, trial)) = chosen else { break; };
            local = trial;
            remaining -= t;
            if remaining != 0 { remaining -= self.kerf; }
            cells.push((t, f));
        }
        if cells.is_empty() { return None; }
        let area = cells.iter().map(|(_, f)| f.area).sum();
        let value = cells.iter().map(|(_, f)| f.value).sum();
        let count = cells.iter().map(|(_, f)| f.count).sum();
        // Chain: first cell next to the registration edge, remnant last.
        let mut tail = (remaining > 0).then(|| self.residual());
        for (t, f) in cells.into_iter().rev() {
            tail = Some(match tail {
                None => f.node,
                Some(rest) => self.split(direction, t, f.node, rest),
            });
        }
        counts.copy_from_slice(&local);
        Some(Fill { node: tail.expect("at least one cell"), area, value, count })
    }
}

/// First cut direction and number of levels of a tree, with the same stage
/// rule as the evaluator (a cut parallel to its parent stays on its level).
fn tree_shape(node: &PatternNode) -> (Option<CutDirection>, u8) {
    fn walk(node: &PatternNode, prior: Option<(CutDirection, u8)>, deepest: &mut u8) {
        if let PatternNode::Split { direction, first, second, .. } = node {
            let level = match prior {
                None => 1,
                Some((d, l)) if d == *direction => l,
                Some((_, l)) => l + 1,
            };
            *deepest = (*deepest).max(level);
            walk(first, Some((*direction, level)), deepest);
            walk(second, Some((*direction, level)), deepest);
        }
    }
    let first = match node { PatternNode::Split { direction, .. } => Some(*direction), _ => None };
    let mut deepest = 0;
    walk(node, None, &mut deepest);
    (first, deepest)
}

/// Smallest allowed pattern able to hold a tree of this shape.
fn fitting_mode(model: &CuttingJobModel, first: Option<CutDirection>, levels: u8, fallback: CutDirection) -> Option<StagePattern> {
    let first = first.unwrap_or(fallback);
    model.machine.allowed_patterns.iter().copied()
        .filter(|m| m.first() == first && m.levels() >= levels.max(2))
        .min_by_key(|m| m.levels())
}

#[derive(Clone, Debug)]
struct State {
    strips: Vec<(Unit, Fill)>,
    counts: Vec<u32>,
    remaining: Unit,
    area: u128,
    value: u128,
    count: usize,
}

/// Board-level strip widths worth trying: single piece sizes, the full
/// breadth and pairs of sizes (a strip later holding two cells side by side).
fn strip_widths(choices: &[Choice], direction: CutDirection, breadth: Unit, length: Unit, kerf: Unit, min_strip: Unit) -> Vec<Unit> {
    let single: BTreeSet<Unit> = choices.iter()
        .map(|c| along(direction, c.w, c.h))
        .filter(|&(t, other)| t <= breadth && other <= length && t >= min_strip)
        .map(|(t, _)| t).collect();
    // Every single piece size is kept (largest first); pairs only fill the
    // remaining slots. Truncating one mixed list by size would drop the
    // single sizes, since pair sums are always larger.
    let mut ranked: Vec<Unit> = single.iter().rev().copied().collect();
    ranked.truncate(MAX_WIDTH_CHOICES);
    if !ranked.contains(&breadth) { ranked.push(breadth); }
    let top: Vec<Unit> = single.iter().rev().take(12).copied().collect();
    let mut pairs = BTreeSet::new();
    for &a in &top {
        for &b in &top {
            if let Some(v) = a.checked_add(kerf).and_then(|v| v.checked_add(b)) {
                if v <= breadth && !ranked.contains(&v) { pairs.insert(v); }
            }
        }
    }
    for v in pairs.into_iter().rev() {
        if ranked.len() >= MAX_WIDTH_CHOICES + 16 { break; }
        ranked.push(v);
    }
    ranked
}

/// Generate one board with up to the deepest allowed pattern (4..=6 levels;
/// also works with fewer). No inventory is consumed.
pub fn generate_deep_stage(
    model: &CuttingJobModel, stock_id: Option<&str>, seed: u64,
    max_runtime_ms: u64, beam_width: usize, max_expansions: usize,
) -> Result<DeepStageResult, DeepStageError> {
    generate_deep_stage_weighted(model, stock_id, seed, max_runtime_ms, beam_width, max_expansions, None)
}

/// As [`generate_deep_stage`], maximising the placed area weighted by
/// `weights` (value per unit area by part id, [`UNIT_VALUE`] = neutral)
/// instead of the raw area. Used by value-corrected multi-board passes.
pub fn generate_deep_stage_weighted(
    model: &CuttingJobModel, stock_id: Option<&str>, seed: u64,
    max_runtime_ms: u64, beam_width: usize, max_expansions: usize,
    weights: Option<&BTreeMap<String, u64>>,
) -> Result<DeepStageResult, DeepStageError> {
    validate_model(model).map_err(DeepStageError::InvalidModel)?;
    if !valid_search_settings(beam_width, max_expansions) { return Err(DeepStageError::InvalidSearchSettings); }
    if model.parts.len() > MAX_PART_TYPES { return Err(DeepStageError::ResourceLimit("too many part types")); }
    if let Some(id) = stock_id {
        if !model.stocks.iter().any(|s| s.id == id) { return Err(DeepStageError::UnknownStock(id.into())); }
    }
    let started = Instant::now();
    let deadline = Duration::from_millis(max_runtime_ms);
    let quantities: Vec<u32> = model.parts.iter().map(|p| p.quantity).collect();
    let part_weights: Vec<u64> = model.parts.iter()
        .map(|p| weights.and_then(|w| w.get(&p.id)).copied().unwrap_or(UNIT_VALUE).max(1)).collect();
    let initial_remaining: BTreeMap<String, u32> = model.parts.iter().map(|p| (p.id.clone(), p.quantity)).collect();
    let mut probe_id = "s18_probe".to_string();
    let mut probe_counter = 1_u32;
    while model.patterns.iter().any(|p| p.id == probe_id) {
        probe_id = format!("s18_probe_{probe_counter}");
        probe_counter += 1;
    }
    let mut expanded = 0_usize;
    let mut evaluated_count = 0_usize;
    let mut exhausted = false;
    let mut best: Option<Best> = None;

    'search: for (stock_index, stock) in model.stocks.iter().enumerate() {
        if stock_id.is_some_and(|id| id != stock.id.as_str()) { continue; }
        for rotated_board in [false, true] {
            if rotated_board && !model.machine.allow_board_rotation { continue; }
            let (w, h) = usable(stock, rotated_board);
            let options = choices(model, stock, rotated_board);
            if options.is_empty() { continue; }
            for first in [CutDirection::Vertical, CutDirection::Horizontal] {
                let Some(max_level) = model.machine.allowed_patterns.iter()
                    .filter(|m| m.first() == first).map(|m| m.levels()).max() else { continue; };
                let (breadth, length) = along(first, w, h);
                let widths = strip_widths(&options, first, breadth, length, model.machine.kerf, model.machine.min_strip_width);
                let mut beam = vec![State { strips: Vec::new(), counts: quantities.clone(), remaining: breadth, area: 0, value: 0, count: 0 }];
                for _ in 0..MAX_STRIPS {
                    let mut next: Vec<State> = Vec::new();
                    for state in &beam {
                        for &width in &widths {
                            if width > state.remaining || !can_separate(state.remaining, width, model.machine.kerf) { continue; }
                            if width != state.remaining && length > model.machine.max_cut_length { continue; }
                            for method in DeepStageHeuristic::ALL {
                                if started.elapsed() >= deadline || expanded >= max_expansions {
                                    exhausted = true;
                                    break 'search;
                                }
                                expanded += 1;
                                let mut ctx = Ctx {
                                    model, choices: &options, kerf: model.machine.kerf,
                                    max_cut: model.machine.max_cut_length, max_level, method,
                                    seed: seed ^ (state.strips.len() as u64).wrapping_mul(0x1000_0001),
                                    weights: &part_weights,
                                    prefix: state.strips.len(), next_id: 0,
                                };
                                let mut counts = state.counts.clone();
                                let (cw, ch) = cell(first, width, length);
                                let Some(strip) = ctx.fill(cw, ch, flip(first), 2, &mut counts) else { continue; };
                                if state.count + strip.count > MAX_PIECES { continue; }
                                let mut child = state.clone();
                                child.counts = counts;
                                child.area += strip.area;
                                child.value += strip.value;
                                child.count += strip.count;
                                child.remaining -= width;
                                if child.remaining != 0 { child.remaining -= model.machine.kerf; }
                                child.strips.push((width, strip));
                                let usable_area = u128::from(w) * u128::from(h);
                                let improves = best.as_ref().is_none_or(|b| (child.value, child.count) > (b.value, b.count));
                                if improves {
                                    evaluated_count += 1;
                                    if let Some(pattern) = build_and_evaluate(model, stock, rotated_board, first, &child, &probe_id) {
                                        best = Some(Best { value: child.value, count: child.count, stock_index, rotated_board,
                                                           heuristic: method, pattern, usable_area });
                                    }
                                }
                                if child.remaining != 0 && child.count < MAX_PIECES { next.push(child); }
                            }
                        }
                    }
                    if next.is_empty() { break; }
                    // Rank partial boards by density (area per breadth used), not
                    // by raw area: otherwise one near-full first strip wins and
                    // leaves no room for the strips that follow.
                    next.sort_by(|a, b| {
                        let (ua, ub) = (u128::from(breadth - a.remaining), u128::from(breadth - b.remaining));
                        (b.value * ua).cmp(&(a.value * ub)).then((b.value, b.count).cmp(&(a.value, a.count)))
                    });
                    let mut seen = HashSet::new();
                    next.retain(|s| seen.insert((s.remaining, s.counts.clone())));
                    next.truncate(beam_width);
                    beam = next;
                }
            }
        }
    }

    let Some(Best { stock_index, rotated_board, heuristic, mut pattern, usable_area, .. }) = best else {
        return Ok(DeepStageResult {
            found: false, complete_order: false, candidates_evaluated: evaluated_count, expanded_states: expanded,
            search_exhausted: exhausted, board: None, remaining_part_counts: initial_remaining,
            note: "No valid single-board multi-level pattern found; this does not prove global infeasibility".into(),
        });
    };
    let stock = &model.stocks[stock_index];
    let mut pattern_id = "stage18_generated".to_string();
    let mut counter = 1_u32;
    while model.patterns.iter().any(|p| p.id == pattern_id) {
        pattern_id = format!("stage18_generated_{counter}");
        counter += 1;
    }
    pattern.id = pattern_id.clone();
    let mut trial = model.clone();
    trial.patterns.push(pattern.clone());
    let evaluated = evaluate_pattern(&trial, &pattern_id, rotated_board).map_err(DeepStageError::InvalidGeneratedPattern)?;
    let mut remaining = initial_remaining;
    for (id, count) in &evaluated.summary.per_part_counts {
        if let Some(r) = remaining.get_mut(id) { *r -= count; }
    }
    let complete = remaining.values().all(|&q| q == 0);
    let piece_area: u128 = evaluated.summary.piece_area_sq_units.parse().unwrap_or(0);
    Ok(DeepStageResult {
        found: true, complete_order: complete, candidates_evaluated: evaluated_count, expanded_states: expanded,
        search_exhausted: exhausted,
        board: Some(DeepStageBoard {
            stock_id: stock.id.clone(), stock_unit_index: 0, board_rotated_90: rotated_board,
            mode: pattern.mode, heuristic,
            utilization_basis_points: (piece_area * 10_000 / usable_area.max(1)) as u32,
            pattern, evaluated,
        }),
        remaining_part_counts: remaining,
        note: "Single board only; no stock reservation, stacking, multi-board allocation or physical execution scheduling".into(),
    })
}

/// Best board found so far (its pattern already passed the evaluator).
struct Best {
    value: u128,
    count: usize,
    stock_index: usize,
    rotated_board: bool,
    heuristic: DeepStageHeuristic,
    pattern: CuttingPattern,
    usable_area: u128,
}

/// Assemble the board tree, label it with the smallest allowed pattern that
/// holds it and validate it with the independent evaluator.
fn build_and_evaluate(
    model: &CuttingJobModel, stock: &Stock, rotated_board: bool, first: CutDirection,
    state: &State, probe_id: &str,
) -> Option<CuttingPattern> {
    let mut next_id = 1_000_000_u32;
    let mut id = || { next_id += 1; format!("s18b{next_id}") };
    let mut tail = (state.remaining > 0).then(|| PatternNode::Residual { node_id: id(), reusable: false });
    for (width, strip) in state.strips.iter().rev() {
        tail = Some(match tail {
            None => strip.node.clone(),
            Some(rest) => PatternNode::Split {
                node_id: id(), direction: first, at: *width,
                first: Box::new(strip.node.clone()), second: Box::new(rest),
            },
        });
    }
    let root = tail?;
    let (root_direction, levels) = tree_shape(&root);
    let mode = fitting_mode(model, root_direction, levels, first)?;
    let pattern = CuttingPattern { id: probe_id.to_owned(), stock_id: stock.id.clone(), mode, root };
    let mut trial = model.clone();
    trial.patterns.retain(|p| p.id != probe_id);
    trial.patterns.push(pattern.clone());
    evaluate_pattern(&trial, probe_id, rotated_board).ok()?;
    Some(pattern)
}
