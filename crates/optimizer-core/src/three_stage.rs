//! Stage 8: bounded single-board three-stage guillotine pattern generator.
//!
//! Each candidate is first-stage strips, second-stage rows, and third-stage
//! partitions within rows. Beam search maintains several competing partial
//! strip layouts instead of keeping only the first greedy arrangement.
//! All reported solutions are re-evaluated by the independent Stage 5 tree
//! evaluator. This is NOT a multi-board or machine-stack optimizer.

use crate::guillotine::{evaluate_pattern, EvaluatedPattern, TreeError};
use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, CuttingPattern,
    ModelIssue, PatternNode, StagePattern, Stock, TrimMargins, Unit,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::time::{Duration, Instant};

const MAX_PART_TYPES: usize = 256;
const MAX_PIECES: usize = 64;
const MAX_STRIPS: usize = 16;
const MAX_ROWS: usize = 16;
const MAX_ROW_PIECES: usize = 20;
const MAX_WIDTH_CHOICES: usize = 32;
const MAX_BEAM_WIDTH: usize = 64;
const MAX_EXPANSIONS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreeStageHeuristic {
    LongestFirst,
    LargestAreaFirst,
    BestFit,
    PriorityFirst,
}
impl ThreeStageHeuristic {
    const ALL: [Self; 4] = [Self::LongestFirst, Self::LargestAreaFirst, Self::BestFit, Self::PriorityFirst];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreeStageError {
    InvalidModel(Vec<ModelIssue>),
    UnknownStock(String),
    InvalidSearchSettings,
    ResourceLimit(&'static str),
    InvalidGeneratedPattern(TreeError),
}
impl fmt::Display for ThreeStageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid model ({} issue(s))", issues.len()),
            Self::UnknownStock(id) => write!(f, "unknown stock '{id}'"),
            Self::InvalidSearchSettings => write!(f, "beam_width must be 1..=64 and max_expansions must be 1..=20000"),
            Self::ResourceLimit(text) => write!(f, "three-stage search limit: {text}"),
            Self::InvalidGeneratedPattern(err) => write!(f, "invalid generated pattern: {err}"),
        }
    }
}
impl std::error::Error for ThreeStageError {}

pub fn default_beam_width() -> usize { 12 }
pub fn default_max_expansions() -> usize { 1500 }
pub fn valid_search_settings(beam_width: usize, max_expansions: usize) -> bool {
    (1..=MAX_BEAM_WIDTH).contains(&beam_width) && (1..=MAX_EXPANSIONS).contains(&max_expansions)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreeStageBoard {
    pub stock_id: String,
    pub stock_unit_index: u32,
    pub board_rotated_90: bool,
    pub mode: StagePattern,
    pub heuristic: ThreeStageHeuristic,
    pub utilization_basis_points: u32,
    pub pattern: CuttingPattern,
    pub evaluated: EvaluatedPattern,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreeStageResult {
    pub found: bool,
    pub complete_order: bool,
    pub candidates_evaluated: usize,
    pub expanded_states: usize,
    pub search_exhausted: bool,
    pub beam_width: usize,
    pub board: Option<ThreeStageBoard>,
    pub remaining_part_counts: BTreeMap<String, u32>,
    pub note: String,
}

#[derive(Clone, Debug)]
struct Choice {
    index: usize,
    rotated: bool,
    breadth: Unit, // first/third-stage dimension
    length: Unit,  // second-stage dimension
    priority: u8,
}
#[derive(Clone, Debug)]
struct Row {
    length: Unit,
    pieces: Vec<Choice>,
    consumed_breadth: Unit, // includes kerf before residual, if any
    area: u128,
}
#[derive(Clone, Debug)]
struct Strip {
    breadth: Unit,
    rows: Vec<Row>,
    consumed_length: Unit,
    area: u128,
    count: usize,
}
#[derive(Clone, Debug)]
struct SearchState {
    strips: Vec<Strip>,
    counts: Vec<u32>,
    remaining_breadth: Unit,
    area: u128,
    count: usize,
}
#[derive(Clone, Debug)]
struct Candidate {
    stock_index: usize,
    rotated_board: bool,
    mode: StagePattern,
    heuristic: ThreeStageHeuristic,
    usable_area: u128,
    usable_length: Unit,
    state: SearchState,
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
fn axes(mode: StagePattern) -> (CutDirection, CutDirection) {
    match mode {
        StagePattern::VHV => (CutDirection::Vertical, CutDirection::Horizontal),
        StagePattern::HVH => (CutDirection::Horizontal, CutDirection::Vertical),
        _ => unreachable!("only three-stage modes"),
    }
}
fn dimensions(w: Unit, h: Unit, first: CutDirection) -> (Unit, Unit) {
    match first {
        CutDirection::Vertical => (w, h),
        CutDirection::Horizontal => (h, w),
    }
}
/// Avoid slivers smaller than the saw channel. The zero-scrap case is exact.
fn can_separate(available: Unit, taken: Unit, kerf: Unit) -> bool {
    available == taken || available.checked_sub(taken).is_some_and(|d| d > kerf)
}
fn get_choices(model: &CuttingJobModel, stock: &Stock, rotated_board: bool,
               outer: CutDirection, breadth: Unit, length: Unit) -> Vec<Choice> {
    let mut result = Vec::new();
    let board_grain = rotate_axis(stock.grain_axis, rotated_board);
    for (i, part) in model.parts.iter().enumerate() {
        if part.material_id != stock.material_id { continue; }
        for rotated in [false, true] {
            if rotated && !part.allow_rotate_90 { continue; }
            if part.grain_axis.is_some() && rotate_axis(part.grain_axis, rotated) != board_grain { continue; }
            let (w, h) = if rotated { (part.height, part.width) } else { (part.width, part.height) };
            if w < model.machine.min_piece_width || h < model.machine.min_piece_height { continue; }
            let (b, l) = dimensions(w, h, outer);
            if b > breadth || l > length || b < model.machine.min_strip_width { continue; }
            if rotated && w == h && part.grain_axis.is_none() { continue; }
            result.push(Choice { index: i, rotated, breadth: b, length: l, priority: part.priority });
        }
    }
    result
}

fn widths(choices: &[Choice], breadth: Unit, kerf: Unit) -> Vec<Unit> {
    let uniq: Vec<Unit> = choices.iter().map(|c| c.breadth).collect::<BTreeSet<_>>().into_iter().collect();
    let mut all: BTreeSet<Unit> = uniq.iter().copied().collect();
    // Include both a strip exactly containing several pieces and the full
    // available breadth. Candidate validation still checks actual row packing.
    all.insert(breadth);
    let best: Vec<Unit> = uniq.iter().rev().take(18).copied().collect();
    for &a in &best {
        for &b in &best {
            if let Some(v) = a.checked_add(kerf).and_then(|v| v.checked_add(b)) {
                if v <= breadth { all.insert(v); }
            }
        }
    }
    // Preserve ALL single-piece widths where possible, then include wide
    // combos. This is a bounded search, not an exhaustive subset solver.
    let mut ranked: Vec<Unit> = all.into_iter().collect();
    ranked.sort_by_key(|&v| (std::cmp::Reverse(v), v));
    ranked.truncate(MAX_WIDTH_CHOICES);
    ranked
}

fn choice_rank(c: &Choice, free: Unit, method: ThreeStageHeuristic, seed: u64) -> (u64, u64, u64) {
    let tie = (c.index as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ seed.rotate_left(11) ^ (if c.rotated { 1 } else { 0 });
    let b = u64::from(c.breadth);
    let l = u64::from(c.length);
    match method {
        ThreeStageHeuristic::LongestFirst => (b, l, tie),
        ThreeStageHeuristic::LargestAreaFirst => (b * l, b, tie),
        ThreeStageHeuristic::BestFit => (u64::MAX - u64::from(free - c.breadth), b, tie),
        ThreeStageHeuristic::PriorityFirst => (u64::from(c.priority), b, tie),
    }
}

fn fill_row(choices: &[Choice], row_length: Unit, breadth: Unit,
            quantities: &[u32], kerf: Unit, method: ThreeStageHeuristic,
            seed: u64, max_cut_length: Unit) -> Option<Row> {
    let mut counts = quantities.to_vec();
    let mut free = breadth;
    let mut pieces = Vec::new();
    let mut area = 0_u128;
    while pieces.len() < MAX_ROW_PIECES {
        let choice = choices.iter()
            .filter(|c| c.length == row_length && counts[c.index] > 0)
            .filter(|c| c.breadth <= free && can_separate(free, c.breadth, kerf))
            .filter(|c| c.breadth == free || row_length <= max_cut_length)
            .max_by_key(|c| choice_rank(c, free, method, seed))
            .cloned();
        let Some(choice) = choice else { break; };
        counts[choice.index] -= 1;
        free -= choice.breadth;
        if free != 0 { free -= kerf; }
        area += u128::from(choice.length) * u128::from(choice.breadth);
        pieces.push(choice);
        if free == 0 { break; }
    }
    if pieces.is_empty() { return None; }
    Some(Row { length: row_length, pieces, consumed_breadth: breadth - free, area })
}

fn fill_strip(choices: &[Choice], strip_breadth: Unit, available_length: Unit,
              initial_counts: &[u32], kerf: Unit, method: ThreeStageHeuristic,
              seed: u64, max_cut_length: Unit, max_pieces: usize) -> Option<Strip> {
    let row_lengths: Vec<Unit> = choices.iter().map(|c| c.length).collect::<BTreeSet<_>>().into_iter().collect();
    let mut counts = initial_counts.to_vec();
    let mut remaining = available_length;
    let mut rows = Vec::new();
    let mut area = 0_u128;
    let mut count = 0_usize;
    while rows.len() < MAX_ROWS && count < max_pieces {
        let mut best: Option<Row> = None;
        for &l in &row_lengths {
            if l > remaining || !can_separate(remaining, l, kerf) { continue; }
            if l != remaining && strip_breadth > max_cut_length { continue; }
            let Some(row) = fill_row(choices, l, strip_breadth, &counts, kerf,
                                     method, seed ^ (rows.len() as u64), max_cut_length) else { continue; };
            if count + row.pieces.len() > max_pieces { continue; }
            let better = best.as_ref().is_none_or(|old| {
                // Area per unit of row length: helps use the strip breadth.
                let a = row.area * u128::from(old.length);
                let b = old.area * u128::from(row.length);
                a > b || (a == b && (row.area, row.pieces.len()) > (old.area, old.pieces.len()))
            });
            if better { best = Some(row); }
        }
        let Some(row) = best else { break; };
        for part in &row.pieces { counts[part.index] -= 1; }
        area += row.area;
        count += row.pieces.len();
        remaining -= row.length;
        if remaining != 0 { remaining -= kerf; }
        rows.push(row);
        if remaining == 0 { break; }
    }
    if rows.is_empty() { return None; }
    Some(Strip { breadth: strip_breadth, rows, consumed_length: available_length - remaining, area, count })
}

fn score(state: &SearchState) -> (u128, usize, usize) {
    (state.area, state.count, usize::MAX - state.strips.len())
}
fn better(a: &Candidate, b: &Candidate) -> bool {
    (a.state.area, a.state.area.saturating_mul(b.usable_area), a.state.count) >
    (b.state.area, b.state.area.saturating_mul(a.usable_area), b.state.count)
}

struct Id(u32);
impl Id {
    fn next(&mut self) -> String {
        let s = format!("s8n{}", self.0); self.0 += 1; s
    }
    fn residual(&mut self) -> PatternNode {
        PatternNode::Residual { node_id: self.next(), reusable: false }
    }
    fn piece(&mut self, part: &Choice, model: &CuttingJobModel) -> PatternNode {
        PatternNode::Piece { node_id: self.next(), part_id: model.parts[part.index].id.clone(), rotated: part.rotated }
    }
    fn cut(&mut self, direction: CutDirection, at: Unit, left: PatternNode, right: PatternNode) -> PatternNode {
        PatternNode::Split { node_id: self.next(), direction, at, first: Box::new(left), second: Box::new(right) }
    }
}
fn row_tree(row: &Row, breadth: Unit, outer: CutDirection,
            model: &CuttingJobModel, ids: &mut Id) -> PatternNode {
    let mut tail = (row.consumed_breadth < breadth).then(|| ids.residual());
    for piece in row.pieces.iter().rev() {
        let node = ids.piece(piece, model);
        tail = Some(match tail {
            None => node,
            Some(right) => ids.cut(outer, piece.breadth, node, right),
        });
    }
    tail.expect("row is nonempty")
}
fn strip_tree(strip: &Strip, length: Unit, outer: CutDirection,
              inner: CutDirection, model: &CuttingJobModel, ids: &mut Id) -> PatternNode {
    let mut tail = (strip.consumed_length < length).then(|| ids.residual());
    for row in strip.rows.iter().rev() {
        let node = row_tree(row, strip.breadth, outer, model, ids);
        tail = Some(match tail {
            None => node,
            Some(other) => ids.cut(inner, row.length, node, other),
        });
    }
    tail.expect("strip is nonempty")
}
fn build_tree(candidate: &Candidate, model: &CuttingJobModel) -> PatternNode {
    let (outer, inner) = axes(candidate.mode);
    let mut ids = Id(0);
    let mut tail = (candidate.state.remaining_breadth > 0).then(|| ids.residual());
    for strip in candidate.state.strips.iter().rev() {
        let node = strip_tree(strip, candidate.usable_length, outer, inner, model, &mut ids);
        tail = Some(match tail {
            None => node,
            Some(other) => ids.cut(outer, strip.breadth, node, other),
        });
    }
    tail.expect("candidate has strips")
}

/// A valid three-stage solution uses no more than V-H-V or H-V-H along any
/// root-to-leaf path. Some branches may finish after fewer stages.
/// The returned tree is evaluated by Stage 5 before being reported.
pub fn generate_three_stage(
    model: &CuttingJobModel, stock_id: Option<&str>, seed: u64,
    max_runtime_ms: u64, beam_width: usize, max_expansions: usize,
) -> Result<ThreeStageResult, ThreeStageError> {
    validate_model(model).map_err(ThreeStageError::InvalidModel)?;
    if !valid_search_settings(beam_width, max_expansions) { return Err(ThreeStageError::InvalidSearchSettings); }
    if model.parts.len() > MAX_PART_TYPES { return Err(ThreeStageError::ResourceLimit("too many part types")); }
    if let Some(id) = stock_id {
        if !model.stocks.iter().any(|s| s.id == id) { return Err(ThreeStageError::UnknownStock(id.into())); }
    }
    let start = Instant::now();
    let deadline = Duration::from_millis(max_runtime_ms);
    let mut candidates_evaluated = 0_usize;
    let mut expanded_states = 0_usize;
    let mut exhausted = false;
    let mut best: Option<Candidate> = None;
    let mut probe_id = "s8_probe".to_string();
    let mut probe_counter = 1_u32;
    while model.patterns.iter().any(|p| p.id == probe_id) {
        probe_id = format!("s8_probe_{probe_counter}");
        probe_counter += 1;
    }
    let quantities: Vec<u32> = model.parts.iter().map(|p| p.quantity).collect();
    let initial_remaining: BTreeMap<String, u32> = model.parts.iter().map(|p| (p.id.clone(), p.quantity)).collect();

    'search: for (stock_index, stock) in model.stocks.iter().enumerate() {
        if stock_id.is_some_and(|id| id != stock.id.as_str()) { continue; }
        for rotated_board in [false, true] {
            if rotated_board && !model.machine.allow_board_rotation { continue; }
            let (w, h) = usable(stock, rotated_board);
            for mode in [StagePattern::VHV, StagePattern::HVH] {
                if !model.machine.allowed_patterns.contains(&mode) { continue; }
                let (outer, _) = axes(mode);
                let (breadth, length) = dimensions(w, h, outer);
                let options = get_choices(model, stock, rotated_board, outer, breadth, length);
                if options.is_empty() { continue; }
                let widths = widths(&options, breadth, model.machine.kerf);
                let mut beam = vec![SearchState {
                    strips: Vec::new(), counts: quantities.clone(), remaining_breadth: breadth,
                    area: 0, count: 0,
                }];
                for _ in 0..MAX_STRIPS {
                    if start.elapsed() >= deadline || expanded_states >= max_expansions {
                        exhausted = true; break 'search;
                    }
                    let mut next = Vec::new();
                    for state in &beam {
                        if state.count >= MAX_PIECES { continue; }
                        for &strip_width in &widths {
                            if strip_width > state.remaining_breadth ||
                               !can_separate(state.remaining_breadth, strip_width, model.machine.kerf) { continue; }
                            if strip_width != state.remaining_breadth && length > model.machine.max_cut_length { continue; }
                            if strip_width < model.machine.min_strip_width { continue; }
                            for heuristic in ThreeStageHeuristic::ALL {
                                if start.elapsed() >= deadline || expanded_states >= max_expansions {
                                    exhausted = true; break 'search;
                                }
                                expanded_states += 1;
                                let Some(strip) = fill_strip(&options, strip_width, length, &state.counts,
                                    model.machine.kerf, heuristic, seed, model.machine.max_cut_length,
                                    MAX_PIECES - state.count) else { continue; };
                                let mut child = state.clone();
                                for row in &strip.rows {
                                    for piece in &row.pieces { child.counts[piece.index] -= 1; }
                                }
                                child.count += strip.count;
                                child.area += strip.area;
                                child.remaining_breadth -= strip.breadth;
                                if child.remaining_breadth != 0 { child.remaining_breadth -= model.machine.kerf; }
                                child.strips.push(strip);
                                candidates_evaluated += 1;
                                let candidate = Candidate {
                                    stock_index, rotated_board, mode, heuristic,
                                    usable_area: u128::from(w) * u128::from(h),
                                    usable_length: length, state: child.clone(),
                                };
                                if best.as_ref().is_none_or(|prior| better(&candidate, prior)) {
                                    let mut trial = model.clone();
                                    let pattern = CuttingPattern {
                                        id: probe_id.clone(), stock_id: stock.id.clone(), mode,
                                        root: build_tree(&candidate, model),
                                    };
                                    if !trial.patterns.iter().any(|p| p.id == pattern.id) {
                                        trial.patterns.push(pattern);
                                        if evaluate_pattern(&trial, &probe_id, rotated_board).is_ok() {
                                            best = Some(candidate);
                                        }
                                    }
                                }
                                if child.remaining_breadth != 0 && child.count < MAX_PIECES {
                                    next.push(child);
                                }
                            }
                        }
                    }
                    if next.is_empty() { break; }
                    // Deduplicate equivalent quantity/width states. The retained
                    // tree is the one with the strongest partial score.
                    next.sort_by(|a,b| score(b).cmp(&score(a)));
                    let mut seen = HashSet::new();
                    next.retain(|s| seen.insert((s.remaining_breadth, s.counts.clone())));
                    next.truncate(beam_width);
                    beam = next;
                }
            }
        }
    }
    let Some(candidate) = best else {
        return Ok(ThreeStageResult {
            found: false, complete_order: false, candidates_evaluated, expanded_states,
            search_exhausted: exhausted, beam_width, board: None,
            remaining_part_counts: initial_remaining,
            note: "No valid single-board three-stage pattern found; this does not prove global infeasibility".into(),
        });
    };
    let stock = &model.stocks[candidate.stock_index];
    let mut pattern_id = "stage8_generated".to_string();
    let mut counter = 1_u32;
    while model.patterns.iter().any(|p| p.id == pattern_id) {
        pattern_id = format!("stage8_generated_{counter}"); counter += 1;
    }
    let pattern = CuttingPattern {
        id: pattern_id.clone(), stock_id: stock.id.clone(), mode: candidate.mode,
        root: build_tree(&candidate, model),
    };
    let mut trial = model.clone();
    trial.patterns.push(pattern.clone());
    let evaluated = evaluate_pattern(&trial, &pattern_id, candidate.rotated_board)
        .map_err(ThreeStageError::InvalidGeneratedPattern)?;
    let mut remaining = initial_remaining;
    for (id, count) in &evaluated.summary.per_part_counts {
        if let Some(r) = remaining.get_mut(id) { *r -= count; }
    }
    let complete = remaining.values().all(|&q| q == 0);
    let utilization_basis_points =
        (candidate.state.area * 10_000 / candidate.usable_area) as u32;
    Ok(ThreeStageResult {
        found: true, complete_order: complete, candidates_evaluated, expanded_states,
        search_exhausted: exhausted, beam_width,
        board: Some(ThreeStageBoard {
            stock_id: stock.id.clone(), stock_unit_index: 0,
            board_rotated_90: candidate.rotated_board, mode: candidate.mode,
            heuristic: candidate.heuristic, utilization_basis_points, pattern, evaluated,
        }),
        remaining_part_counts: remaining,
        note: "Single board only; no stock reservation, stacking, multi-board allocation or physical execution scheduling".into(),
    })
}
