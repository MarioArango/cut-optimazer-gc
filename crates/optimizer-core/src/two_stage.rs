//! Stage 7: deterministic, bounded, single-board two-stage strip packing.
//!
//! Each candidate tree has outer parallel strip cuts followed by orthogonal
//! splits within each strip. A tree is evaluated by the Stage 5 geometry engine
//! before being returned. This is NOT multi-board optimization, and it does
//! not plan industrial stack operations.

use crate::guillotine::{evaluate_pattern, EvaluatedPattern, TreeError};
use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, CuttingPattern, ModelIssue,
    PatternNode, StagePattern, Stock, TrimMargins, Unit,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

const MAX_PART_TYPES: usize = 512;
const MAX_ORIENTATIONS: usize = 1_024;
const MAX_PIECES_PER_BOARD: usize = 96;
const MAX_CANDIDATE_WIDTHS: usize = 48;
const MAX_STRIPS: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TwoStageHeuristic {
    LongestFirst,
    ShortestFirst,
    LargestAreaFirst,
    BestFit,
    PriorityFirst,
}

impl TwoStageHeuristic {
    fn all() -> &'static [Self] {
        &[
            Self::LongestFirst, Self::ShortestFirst, Self::LargestAreaFirst,
            Self::BestFit, Self::PriorityFirst,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneratorError {
    InvalidModel(Vec<ModelIssue>),
    UnknownStock(String),
    ResourceLimit(&'static str),
    InvalidGeneratedPattern(TreeError),
}
impl fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid model ({} issue(s))", issues.len()),
            Self::UnknownStock(id) => write!(f, "stock '{id}' is not present in the model"),
            Self::ResourceLimit(msg) => write!(f, "stage 7 resource limit: {msg}"),
            Self::InvalidGeneratedPattern(e) => write!(f, "generated tree failed validation: {e}"),
        }
    }
}
impl std::error::Error for GeneratorError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwoStageResult {
    /// `true` means at least one piece was assigned to this board.
    pub found: bool,
    /// No claim is made that all requested pieces have been placed.
    pub complete_order: bool,
    pub candidates_evaluated: usize,
    pub search_exhausted: bool,
    pub board: Option<TwoStageBoard>,
    pub remaining_part_counts: BTreeMap<String, u32>,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwoStageBoard {
    pub stock_id: String,
    pub stock_unit_index: u32,
    pub board_rotated_90: bool,
    pub heuristic: TwoStageHeuristic,
    pub mode: StagePattern,
    /// Basis points: 10000 means 100% of usable stock area.
    pub utilization_basis_points: u32,
    pub pattern: CuttingPattern,
    pub evaluated: EvaluatedPattern,
}

#[derive(Clone, Debug)]
struct Choice {
    part_index: usize,
    rotated: bool,
    breadth: Unit,
    length: Unit,
    priority: u8,
}

#[derive(Clone, Debug)]
struct Strip {
    breadth: Unit,
    pieces: Vec<Choice>,
    used_length: Unit,
    piece_area: u128,
}

#[derive(Clone, Debug)]
struct Candidate {
    stock_index: usize,
    board_rotated_90: bool,
    mode: StagePattern,
    heuristic: TwoStageHeuristic,
    strips: Vec<Strip>,
    usable_area: u128,
    piece_area: u128,
    piece_count: usize,
    // Amount of cross-board length after allocated strips including kerfs.
    remaining_breadth: Unit,
    total_length: Unit,
}

fn rotate_axis(axis: Option<Axis>, rotated: bool) -> Option<Axis> {
    if !rotated { return axis; }
    axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
}
fn rotate_trim(t: TrimMargins) -> TrimMargins {
    TrimMargins { top: t.left, right: t.top, bottom: t.right, left: t.bottom }
}
fn usable_dimensions(stock: &Stock, rotated: bool) -> (Unit, Unit) {
    let (w, h, trim) = if rotated {
        (stock.height, stock.width, rotate_trim(stock.trim))
    } else {
        (stock.width, stock.height, stock.trim)
    };
    (w - trim.left - trim.right, h - trim.top - trim.bottom)
}
fn axis_dimensions(w: Unit, h: Unit, direction: CutDirection) -> (Unit, Unit) {
    match direction {
        CutDirection::Vertical => (w, h),
        CutDirection::Horizontal => (h, w),
    }
}
fn directions(mode: StagePattern) -> (CutDirection, CutDirection) {
    match mode {
        StagePattern::VH => (CutDirection::Vertical, CutDirection::Horizontal),
        StagePattern::HV => (CutDirection::Horizontal, CutDirection::Vertical),
        _ => unreachable!("only two-stage patterns are requested"),
    }
}
fn exact_or_splittable(available: Unit, taken: Unit, kerf: Unit) -> bool {
    available == taken ||
        available.checked_sub(taken).is_some_and(|gap| gap > kerf)
}

fn choices_for(
    model: &CuttingJobModel,
    stock: &Stock,
    rotated_board: bool,
    outer: CutDirection,
    length: Unit,
    breadth: Unit,
) -> Vec<Choice> {
    let mut result = Vec::new();
    let grain = rotate_axis(stock.grain_axis, rotated_board);
    for (index, part) in model.parts.iter().enumerate() {
        if part.material_id != stock.material_id { continue; }
        for rotated in [false, true] {
            if rotated && !part.allow_rotate_90 { continue; }
            // A part without grain_axis has no grain: any orientation is valid.
            if part.grain_axis.is_some() && rotate_axis(part.grain_axis, rotated) != grain { continue; }
            let (w, h) = if rotated { (part.height, part.width) } else { (part.width, part.height) };
            if w < model.machine.min_piece_width || h < model.machine.min_piece_height { continue; }
            let (b, l) = axis_dimensions(w, h, outer);
            if b > breadth || l > length || b < model.machine.min_strip_width { continue; }
            if rotated && w == h && part.grain_axis.is_none() { continue; }
            // One stock type can only be used by one material and grain orientation.
            result.push(Choice { part_index: index, rotated, breadth: b, length: l, priority: part.priority });
        }
    }
    result
}

fn heuristic_key(choice: &Choice, available: Unit, method: TwoStageHeuristic, seed: u64) -> (u64, u64, u64) {
    let length = choice.length as u64;
    // Stable seed-derived tie breaker; never changes geometry or admissibility.
    let tie = (choice.part_index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ seed.rotate_left(13) ^ (if choice.rotated { 1 } else { 0 });
    match method {
        TwoStageHeuristic::LongestFirst => (length, choice.priority as u64, tie),
        TwoStageHeuristic::ShortestFirst => (u64::MAX - length, choice.priority as u64, tie),
        TwoStageHeuristic::LargestAreaFirst => (length * choice.breadth as u64, length, tie),
        TwoStageHeuristic::BestFit => (u64::MAX - (available - choice.length) as u64, length, tie),
        TwoStageHeuristic::PriorityFirst => (choice.priority as u64, length, tie),
    }
}

fn fill_strip(
    all: &[Choice], breadth: Unit, available_length: Unit, quantities: &[u32],
    kerf: Unit, method: TwoStageHeuristic, seed: u64,
) -> Option<Strip> {
    let mut counts = quantities.to_vec();
    let mut remaining = available_length;
    let mut pieces = Vec::new();
    let mut piece_area = 0_u128;
    let mut used_length = 0_u32;
    while pieces.len() < MAX_PIECES_PER_BOARD {
        let best = all.iter()
            .filter(|c| c.breadth == breadth && counts[c.part_index] > 0)
            .filter(|c| c.length <= remaining && exact_or_splittable(remaining, c.length, kerf))
            .max_by_key(|c| heuristic_key(c, remaining, method, seed));
        let Some(choice) = best.cloned() else { break; };
        counts[choice.part_index] -= 1;
        piece_area += u128::from(choice.breadth) * u128::from(choice.length);
        used_length += choice.length;
        remaining -= choice.length;
        if remaining > 0 {
            // Every nonterminal split must reserve the full blade kerf.
            remaining -= kerf;
            used_length += kerf;
        }
        pieces.push(choice);
        if remaining == 0 { break; }
    }
    if pieces.is_empty() { return None; }
    Some(Strip { breadth, pieces, used_length, piece_area })
}

fn build_candidate(
    model: &CuttingJobModel, stock_index: usize, rotated_board: bool,
    mode: StagePattern, method: TwoStageHeuristic, seed: u64,
) -> Option<Candidate> {
    let stock = &model.stocks[stock_index];
    let (w, h) = usable_dimensions(stock, rotated_board);
    let (outer, _) = directions(mode);
    let (breadth, length) = axis_dimensions(w, h, outer);
    // The first-stage full-height/width cuts must be within saw reach.
    let outer_cut_possible = length <= model.machine.max_cut_length;
    let choices = choices_for(model, stock, rotated_board, outer, length, breadth);
    let unique_widths: BTreeSet<_> = choices.iter().map(|c| c.breadth).collect();
    let all_widths: Vec<Unit> = unique_widths.into_iter().collect();
    let widths: Vec<Unit> = if all_widths.len() > MAX_CANDIDATE_WIDTHS {
        all_widths.iter().take(MAX_CANDIDATE_WIDTHS / 2)
            .chain(all_widths.iter().rev().take(MAX_CANDIDATE_WIDTHS / 2))
            .copied().collect()
    } else { all_widths };
    if widths.is_empty() { return None; }
    let mut counts: Vec<u32> = model.parts.iter().map(|p| p.quantity).collect();
    let mut remaining_breadth = breadth;
    let mut strips = Vec::new();
    let mut pieces = 0_usize;
    let mut area = 0_u128;

    while strips.len() < MAX_STRIPS && pieces < MAX_PIECES_PER_BOARD {
        let mut best: Option<Strip> = None;
        for &strip_width in &widths {
            if strip_width > remaining_breadth ||
                !exact_or_splittable(remaining_breadth, strip_width, model.machine.kerf) ||
                (strip_width < remaining_breadth && !outer_cut_possible) ||
                strip_width > model.machine.max_cut_length ||
                pieces >= MAX_PIECES_PER_BOARD { continue; }
            let Some(strip) = fill_strip(
                &choices, strip_width, length, &counts, model.machine.kerf, method, seed,
            ) else { continue; };
            if pieces + strip.pieces.len() > MAX_PIECES_PER_BOARD { continue; }
            // Choose the largest useful piece area, then higher strip packing density.
            let better = best.as_ref().is_none_or(|prev| {
                let left = strip.piece_area * u128::from(prev.breadth);
                let right = prev.piece_area * u128::from(strip.breadth);
                left > right || (left == right && (strip.piece_area, strip.pieces.len(),
                    std::cmp::Reverse(strip.breadth)) >
                    (prev.piece_area, prev.pieces.len(), std::cmp::Reverse(prev.breadth)))
            });
            if better { best = Some(strip); }
        }
        let Some(strip) = best else { break; };
        for chosen in &strip.pieces { counts[chosen.part_index] -= 1; }
        area += strip.piece_area;
        pieces += strip.pieces.len();
        remaining_breadth -= strip.breadth;
        if remaining_breadth > 0 { remaining_breadth -= model.machine.kerf; }
        strips.push(strip);
        if remaining_breadth == 0 { break; }
    }
    if pieces == 0 { return None; }
    Some(Candidate {
        stock_index, board_rotated_90: rotated_board, mode,
        heuristic: method, strips, usable_area: u128::from(w) * u128::from(h),
        piece_area: area, piece_count: pieces, remaining_breadth,
        total_length: length,
    })
}

fn is_better(candidate: &Candidate, prior: &Candidate) -> bool {
    // Stage 7 optimizes ONE board. Prefer more placed area, then utilization,
    // then number of pieces; stage 9 handles economic multi-board selection.
    let lhs = (candidate.piece_area, candidate.piece_area * prior.usable_area,
        candidate.piece_count);
    let rhs = (prior.piece_area, prior.piece_area * candidate.usable_area,
        prior.piece_count);
    lhs > rhs
}

struct IdGenerator(u32);
impl IdGenerator {
    fn next(&mut self) -> String {
        let value = format!("s7n{}", self.0);
        self.0 += 1;
        value
    }
    fn piece(&mut self, choice: &Choice, model: &CuttingJobModel) -> PatternNode {
        PatternNode::Piece {
            node_id: self.next(), part_id: model.parts[choice.part_index].id.clone(),
            rotated: choice.rotated,
        }
    }
    fn residual(&mut self) -> PatternNode {
        PatternNode::Residual { node_id: self.next(), reusable: false }
    }
    fn split(&mut self, dir: CutDirection, at: Unit, first: PatternNode, second: PatternNode) -> PatternNode {
        PatternNode::Split {
            node_id: self.next(), direction: dir, at,
            first: Box::new(first), second: Box::new(second),
        }
    }
}

fn strip_tree(
    strip: &Strip, available_length: Unit, inner: CutDirection,
    _kerf: Unit, model: &CuttingJobModel, ids: &mut IdGenerator,
) -> PatternNode {
    let leftover = available_length - strip.used_length;
    let mut root = if leftover == 0 { None } else { Some(ids.residual()) };
    // Build backwards so the first item is closest to the registration edge.
    for (i, choice) in strip.pieces.iter().enumerate().rev() {
        let piece = ids.piece(choice, model);
        let is_last = i + 1 == strip.pieces.len();
        if is_last && root.is_none() {
            root = Some(piece);
        } else {
            // A physical saw path always separates this piece from the rest.
            let tail = root.take().expect("strip remainder must exist");
            root = Some(ids.split(inner, choice.length, piece, tail));
        }
    }
    // The backwards reconstruction should never involve zero-dimension regions.
    root.expect("nonempty strip")
}

/// A single strip spanning the full breadth needs no first-stage cut, so its
/// tree starts with the inner direction: it is really the transposed pattern.
fn effective_mode(candidate: &Candidate) -> StagePattern {
    let single_full_strip = candidate.strips.len() == 1 && candidate.remaining_breadth == 0;
    let strip = &candidate.strips[0];
    let inner_split = strip.pieces.len() > 1 || strip.used_length < candidate.total_length;
    match candidate.mode {
        StagePattern::VH if single_full_strip && inner_split => StagePattern::HV,
        StagePattern::HV if single_full_strip && inner_split => StagePattern::VH,
        mode => mode,
    }
}

fn make_tree(candidate: &Candidate, model: &CuttingJobModel) -> PatternNode {
    let kerf = model.machine.kerf;
    let (outer, inner) = directions(candidate.mode);
    let mut ids = IdGenerator(0);
    let mut tail = if candidate.remaining_breadth > 0 {
        Some(ids.residual())
    } else { None };
    for (i, strip) in candidate.strips.iter().enumerate().rev() {
        let part = strip_tree(strip, candidate.total_length, inner, kerf, model, &mut ids);
        if i + 1 == candidate.strips.len() && tail.is_none() {
            tail = Some(part);
        } else {
            let rest = tail.take().expect("strip remainder must exist");
            tail = Some(ids.split(outer, strip.breadth, part, rest));
        }
    }
    tail.expect("candidate must contain at least one strip")
}

/// Generate one guillotine board. `stock_id` filters the stock source; without
/// it, all compatible stock types are considered. No inventory is consumed.
/// Unlike the Stage 9 global allocator, any unplaced quantities are returned.
/// The bounded search may terminate early when `max_runtime_ms` expires.
pub fn generate_two_stage(
    model: &CuttingJobModel,
    stock_id: Option<&str>,
    seed: u64,
    max_runtime_ms: u64,
) -> Result<TwoStageResult, GeneratorError> {
    validate_model(model).map_err(GeneratorError::InvalidModel)?;
    if model.parts.len() > MAX_PART_TYPES { return Err(GeneratorError::ResourceLimit("too many part types")); }
    if model.parts.len().saturating_mul(2) > MAX_ORIENTATIONS {
        return Err(GeneratorError::ResourceLimit("too many part orientations"));
    }
    if let Some(id) = stock_id {
        if !model.stocks.iter().any(|stock| stock.id == id) {
            return Err(GeneratorError::UnknownStock(id.to_owned()));
        }
    }
    let started = Instant::now();
    let deadline = Duration::from_millis(max_runtime_ms);
    let mut evaluated = 0_usize;
    let mut best: Option<Candidate> = None;
    let mut exhausted = false;
    for (stock_idx, stock) in model.stocks.iter().enumerate() {
        if stock_id.is_some_and(|id| stock.id.as_str() != id) { continue; }
        for rotated in [false, true] {
            if rotated && !model.machine.allow_board_rotation { continue; }
            for mode in [StagePattern::VH, StagePattern::HV] {
                if !model.machine.allowed_patterns.contains(&mode) { continue; }
                for method in TwoStageHeuristic::all() {
                    if started.elapsed() >= deadline && evaluated > 0 { exhausted = true; break; }
                    evaluated += 1;
                    if let Some(candidate) = build_candidate(model, stock_idx, rotated, mode, *method, seed) {
                        if !model.machine.allowed_patterns.contains(&effective_mode(&candidate)) { continue; }
                        if best.as_ref().is_none_or(|old| is_better(&candidate, old)) {
                            best = Some(candidate);
                        }
                    }
                }
                if exhausted { break; }
            }
            if exhausted { break; }
        }
        if exhausted { break; }
    }
    let mut remaining: BTreeMap<String, u32> = model.parts.iter()
        .map(|part| (part.id.clone(), part.quantity)).collect();
    let Some(candidate) = best else {
        return Ok(TwoStageResult {
            found: false, complete_order: false, candidates_evaluated: evaluated,
            search_exhausted: exhausted, board: None, remaining_part_counts: remaining,
            note: "No eligible two-stage pattern found for a single board; this is not a global infeasibility proof".into(),
        });
    };
    let stock = &model.stocks[candidate.stock_index];
    let mut pattern_id = "stage7_generated".to_owned();
    let mut idx = 1_u32;
    while model.patterns.iter().any(|p| p.id == pattern_id) {
        pattern_id = format!("stage7_generated_{idx}"); idx += 1;
    }
    let mode = effective_mode(&candidate);
    let pattern = CuttingPattern {
        id: pattern_id.clone(), stock_id: stock.id.clone(), mode,
        root: make_tree(&candidate, model),
    };
    let mut augmented = model.clone();
    augmented.patterns.push(pattern.clone());
    let evaluated_pattern = evaluate_pattern(&augmented, &pattern_id, candidate.board_rotated_90)
        .map_err(GeneratorError::InvalidGeneratedPattern)?;
    for (id, count) in &evaluated_pattern.summary.per_part_counts {
        let Some(left) = remaining.get_mut(id) else { unreachable!("part validated by evaluate_pattern"); };
        *left -= count;
    }
    let complete = remaining.values().all(|&count| count == 0);
    let utilization = (candidate.piece_area * 10_000 / candidate.usable_area) as u32;
    Ok(TwoStageResult {
        found: true, complete_order: complete, candidates_evaluated: evaluated,
        search_exhausted: exhausted,
        board: Some(TwoStageBoard {
            stock_id: stock.id.clone(), stock_unit_index: 0,
            board_rotated_90: candidate.board_rotated_90,
            heuristic: candidate.heuristic, mode,
            utilization_basis_points: utilization, pattern, evaluated: evaluated_pattern,
        }),
        remaining_part_counts: remaining,
        note: "One guillotine board; stock is not reserved. Stage 9 will allocate the full multi-board order. Stacking and machine execution remain unverified".into(),
    })
}
