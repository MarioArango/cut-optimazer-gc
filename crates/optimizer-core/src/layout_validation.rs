//! Stage 6 — independent, read-only verification of supplied board layouts.
//!
//! The verifier traverses the original PatternNode tree; it does NOT call the
//! stage-5 evaluator, its split helper, or future packing/optimizing code.
//! It verifies exact job quantities across repeated pattern instances and
//! one-to-one ownership of concrete stock sheets. No machine execution,
//! clamping, stacking, repacking, or trim-cut *scheduling* is claimed here.

use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, CuttingPattern,
    Part, PatternNode, StagePattern, Stock, TrimMargins,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_ISSUES: usize = 100;
const MAX_BOARDS: usize = 100_000;
const MAX_DEPTH: usize = 128;

/// One physical sheet identified by an index inside a stock SKU, not a
/// repetition count. Same pattern ID may be listed on multiple sheets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardAssignment {
    pub board_id: String,
    pub stock_id: String,
    /// Zero-based concrete sheet index within this stock quantity.
    pub stock_unit_index: u32,
    pub pattern_id: String,
    #[serde(default)]
    pub board_rotated_90: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPlan {
    pub boards: Vec<BoardAssignment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutSummary {
    pub boards_submitted: usize,
    pub boards_geometry_verified: usize,
    pub requested_pieces: u64,
    /// Counted only from boards passing ALL tree checks.
    pub verified_placed_pieces: u64,
    pub cuts_verified: u64,
    pub per_part_requested: BTreeMap<String, u64>,
    pub per_part_verified: BTreeMap<String, u64>,
    pub missing_per_part: BTreeMap<String, u64>,
    pub extra_per_part: BTreeMap<String, u64>,
    pub stock_usage: BTreeMap<String, u64>,
    /// Squared 0.1-mm units, encoded as text for lossless JSON/JavaScript.
    pub verified_usable_area_sq_units: String,
    pub verified_piece_area_sq_units: String,
    pub verified_residual_area_sq_units: String,
    pub verified_kerf_area_sq_units: String,
    /// False if one or more board trees failed or lacked references.
    pub count_reconciliation_complete: bool,
    pub geometry_validated: bool,
    /// Sequence, clamps, stacks and physical production are not validated.
    pub production_validated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutValidationReport {
    pub valid: bool,
    pub issues: Vec<LayoutIssue>,
    pub summary: LayoutSummary,
}

struct Collector {
    issues: Vec<LayoutIssue>,
    total: usize,
}
impl Collector {
    fn add(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.total += 1;
        if self.issues.len() < MAX_ISSUES {
            self.issues.push(LayoutIssue {
                path: path.into(), code: code.into(), message: message.into(),
            });
        }
    }
}

#[derive(Clone, Copy)]
struct Region {
    x: u64,
    y: u64,
    w: u64,
    h: u64,
}
impl Region {
    fn area(self) -> u128 { u128::from(self.w) * u128::from(self.h) }
}

#[derive(Default)]
struct Audit {
    counts: BTreeMap<String, u64>,
    pieces: Vec<Region>,
    residuals: Vec<Region>,
    saws: Vec<Region>,
    cuts: u64,
    piece_area: u128,
    residual_area: u128,
    kerf_area: u128,
}

fn axis_rotated(axis: Option<Axis>, rotate: bool) -> Option<Axis> {
    if !rotate { return axis; }
    axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
}

fn rotate_margins(m: TrimMargins, rotate: bool) -> TrimMargins {
    if !rotate { return m; }
    TrimMargins { top: m.left, right: m.top, bottom: m.right, left: m.bottom }
}

fn directions(mode: StagePattern) -> &'static [CutDirection] {
    mode.directions()
}

struct TreeChecker<'a, 'b> {
    model: &'a CuttingJobModel,
    pattern: &'a CuttingPattern,
    parts: &'a HashMap<&'a str, &'a Part>,
    stock: &'a Stock,
    rotated_board: bool,
    issues: &'b mut Collector,
    board_path: &'b str,
    audit: Audit,
}

impl TreeChecker<'_, '_> {
    fn fail(&mut self, path: &str, code: &str, message: impl Into<String>) {
        self.issues.add(format!("{}.{}", self.board_path, path), code, message);
    }

    fn visit(&mut self, node: &PatternNode, region: Region, stage: Option<usize>, path: &str, depth: usize) {
        if depth > MAX_DEPTH {
            self.fail(path, "tree_depth_exceeded", "tree exceeds 128 recursive levels");
            return;
        }
        match node {
            PatternNode::Split { direction, at, first, second, .. } => {
                let sequence = directions(self.pattern.mode);
                let next = match stage {
                    None => 0,
                    Some(i) if sequence[i] == *direction => i,
                    Some(i) => i + 1,
                };
                if next >= sequence.len() || sequence[next] != *direction {
                    self.fail(path, "invalid_stage_transition", "cut direction violates the declared V/H stage sequence");
                    return;
                }
                let length = if *direction == CutDirection::Vertical { region.h } else { region.w };
                if length > u64::from(self.model.machine.max_cut_length) {
                    self.fail(path, "cut_too_long", format!("cut length {length} exceeds machine maximum"));
                    return;
                }
                let span = if *direction == CutDirection::Vertical { region.w } else { region.h };
                let offset = u64::from(*at);
                let kerf = u64::from(self.model.machine.kerf);
                if offset == 0 || kerf == 0 || offset + kerf >= span {
                    self.fail(path, "invalid_split", "cut and kerf must leave two positive regions");
                    return;
                }
                // Computed independently of Rect::split and EvaluatedPattern.
                let (left, saw, right) = if *direction == CutDirection::Vertical {
                    (
                        Region { w: offset, ..region },
                        Region { x: region.x + offset, w: kerf, ..region },
                        Region { x: region.x + offset + kerf, w: span - offset - kerf, ..region },
                    )
                } else {
                    (
                        Region { h: offset, ..region },
                        Region { y: region.y + offset, h: kerf, ..region },
                        Region { y: region.y + offset + kerf, h: span - offset - kerf, ..region },
                    )
                };
                if left.area() + saw.area() + right.area() != region.area() {
                    self.fail(path, "area_not_conserved", "split children and kerf do not conserve area");
                    return;
                }
                self.audit.kerf_area += saw.area();
                self.audit.saws.push(saw);
                self.audit.cuts += 1;
                self.visit(first, left, Some(next), &format!("{path}.first"), depth + 1);
                self.visit(second, right, Some(next), &format!("{path}.second"), depth + 1);
            }
            PatternNode::Piece { part_id, rotated, .. } => {
                let Some(&part) = self.parts.get(part_id.as_str()) else {
                    self.fail(path, "unknown_part", format!("unknown part '{part_id}'"));
                    return;
                };
                if part.material_id != self.stock.material_id {
                    self.fail(path, "material_mismatch", format!("part '{part_id}' material differs from stock"));
                }
                if *rotated && !part.allow_rotate_90 {
                    self.fail(path, "rotation_forbidden", format!("part '{part_id}' may not rotate"));
                }
                let (w, h) = if *rotated { (part.height, part.width) } else { (part.width, part.height) };
                if (region.w, region.h) != (u64::from(w), u64::from(h)) {
                    self.fail(path, "piece_dimension_mismatch", format!("part '{part_id}' requires {w}x{h}, region is {}x{}", region.w, region.h));
                }
                if part.grain_axis.is_some() && axis_rotated(part.grain_axis, *rotated) != axis_rotated(self.stock.grain_axis, self.rotated_board) {
                    self.fail(path, "grain_conflict", format!("part '{part_id}' grain differs from oriented stock"));
                }
                if w < self.model.machine.min_piece_width || h < self.model.machine.min_piece_height {
                    self.fail(path, "piece_below_machine_minimum", format!("part '{part_id}' below minimum piece size"));
                }
                *self.audit.counts.entry(part_id.to_owned()).or_default() += 1;
                self.audit.piece_area += region.area();
                self.audit.pieces.push(region);
            }
            PatternNode::Residual { .. } => {
                self.audit.residual_area += region.area();
                self.audit.residuals.push(region);
            }
        }
    }

    fn check_geometry(&mut self, usable: Region) {
        if self.audit.piece_area + self.audit.residual_area + self.audit.kerf_area != usable.area() {
            self.fail("root", "area_not_conserved", "piece, residual and saw areas must equal usable board area");
        }
        let all: Vec<Region> = self.audit.pieces.iter()
            .chain(&self.audit.residuals).chain(&self.audit.saws).copied().collect();
        // Sweep line in X with Y intervals in a BTreeMap. All boundaries are
        // positive and fit in u64. End events precede starts at touching edges.
        // Overlap detection costs O(n log n), not O(n²) for long strips.
        let mut events: Vec<(u64, bool, usize)> = Vec::with_capacity(all.len() * 2);
        for (i, r) in all.iter().enumerate() {
            if r.w == 0 || r.h == 0 || r.x < usable.x || r.y < usable.y
                || r.x + r.w > usable.x + usable.w
                || r.y + r.h > usable.y + usable.h {
                self.fail("root", "out_of_bounds", "a region is empty or exceeds the usable board");
            }
            events.push((r.x, true, i));
            events.push((r.x + r.w, false, i));
        }
        events.sort_unstable_by_key(|(x, begin, i)| (*x, *begin, *i));
        let mut active: BTreeMap<(u64, usize), u64> = BTreeMap::new();
        for (_, begin, i) in events {
            let r = all[i];
            if !begin {
                active.remove(&(r.y, i));
                continue;
            }
            let upper = r.y + r.h;
            let prev_overlaps = active.range(..=(r.y, usize::MAX))
                .next_back().is_some_and(|(_, end)| *end > r.y);
            let next_overlaps = active.range((r.y, usize::MAX)..)
                .next().is_some_and(|(&(y, _), _)| y < upper);
            if prev_overlaps || next_overlaps {
                self.fail("root", "overlap", format!("region {i} overlaps another region"));
            }
            active.insert((r.y, i), upper);
        }
    }
}

/// Validate a *complete* selection of cutting patterns on concrete sheets.
/// Returns a structured report, including invalid-plan issues. It does not
/// modify the model and does not generate, repair or schedule patterns.
pub fn validate_layout(model: &CuttingJobModel, layout: &LayoutPlan) -> LayoutValidationReport {
    let mut issues = Collector { issues: Vec::new(), total: 0 };
    let requested: BTreeMap<String, u64> = model.parts.iter().map(|p| (p.id.clone(), u64::from(p.quantity))).collect();
    let requested_pieces = model.parts.iter().map(|p| u64::from(p.quantity)).sum();
    let mut summary = LayoutSummary {
        boards_submitted: layout.boards.len(), boards_geometry_verified: 0,
        requested_pieces, verified_placed_pieces: 0, cuts_verified: 0,
        per_part_requested: requested.clone(), per_part_verified: BTreeMap::new(),
        missing_per_part: BTreeMap::new(), extra_per_part: BTreeMap::new(),
        stock_usage: BTreeMap::new(),
        verified_usable_area_sq_units: "0".into(), verified_piece_area_sq_units: "0".into(),
        verified_residual_area_sq_units: "0".into(), verified_kerf_area_sq_units: "0".into(),
        count_reconciliation_complete: false, geometry_validated: false, production_validated: false,
    };
    let model_errors = match validate_model(model) {
        Ok(_) => None,
        Err(errs) => Some(errs),
    };
    if let Some(errors) = model_errors {
        for error in errors {
            issues.add(format!("model.{}", error.path), &error.code, error.message);
        }
        return LayoutValidationReport { valid: false, issues: issues.issues, summary };
    }
    if layout.boards.is_empty() {
        issues.add("layout.boards", "empty_layout", "at least one board assignment is required");
    }
    if layout.boards.len() > MAX_BOARDS {
        issues.add("layout.boards", "too_many_boards", format!("maximum {MAX_BOARDS} boards"));
        return LayoutValidationReport { valid: false, issues: issues.issues, summary };
    }
    let stocks: HashMap<&str, &Stock> = model.stocks.iter().map(|s| (s.id.as_str(), s)).collect();
    let parts: HashMap<&str, &Part> = model.parts.iter().map(|p| (p.id.as_str(), p)).collect();
    let patterns: HashMap<&str, &CuttingPattern> = model.patterns.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut identifiers = HashSet::new();
    let mut stock_units = HashSet::new();
    let mut all_verified = true;
    let mut totals = (0u128, 0u128, 0u128, 0u128);

    for (i, board) in layout.boards.iter().enumerate() {
        let path = format!("layout.boards[{i}]");
        let issue_count = issues.total;
        if board.board_id.is_empty() || board.board_id.len() > 64 ||
            !board.board_id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            issues.add(format!("{path}.board_id"), "invalid_board_id", "board_id must be 1..64 ASCII letters, digits, dots, underscores or hyphens");
        }
        if !identifiers.insert(board.board_id.as_str()) {
            issues.add(format!("{path}.board_id"), "duplicate_board_id", "same board identifier used twice");
        }
        if !stock_units.insert((board.stock_id.as_str(), board.stock_unit_index)) {
            issues.add(format!("{path}.stock_unit_index"), "duplicate_stock_unit", "same physical stock sheet assigned twice");
        }
        let Some(stock) = stocks.get(board.stock_id.as_str()).copied() else {
            issues.add(format!("{path}.stock_id"), "unknown_stock", "referenced stock type does not exist");
            all_verified = false;
            continue;
        };
        if board.stock_unit_index >= stock.quantity {
            issues.add(format!("{path}.stock_unit_index"), "stock_capacity_exceeded", "stock sheet index is outside available inventory");
        }
        *summary.stock_usage.entry(board.stock_id.clone()).or_default() += 1;
        let Some(pattern) = patterns.get(board.pattern_id.as_str()).copied() else {
            issues.add(format!("{path}.pattern_id"), "unknown_pattern", "referenced cutting pattern does not exist");
            all_verified = false;
            continue;
        };
        if pattern.stock_id != board.stock_id {
            issues.add(format!("{path}.pattern_id"), "stock_pattern_mismatch", "pattern refers to another stock type");
        }
        if !model.machine.allowed_patterns.contains(&pattern.mode) {
            issues.add(format!("{path}.pattern_id"), "pattern_forbidden", "machine does not allow this cutting pattern");
        }
        if board.board_rotated_90 && !model.machine.allow_board_rotation {
            issues.add(format!("{path}.board_rotated_90"), "board_rotation_forbidden", "machine cannot rotate full boards");
        }
        // The physical usable region is calculated without Geometry::inset.
        let margins = rotate_margins(stock.trim, board.board_rotated_90);
        let (w, h) = if board.board_rotated_90 { (u64::from(stock.height), u64::from(stock.width)) }
            else { (u64::from(stock.width), u64::from(stock.height)) };
        let trim_x = u64::from(margins.left) + u64::from(margins.right);
        let trim_y = u64::from(margins.top) + u64::from(margins.bottom);
        if trim_x >= w || trim_y >= h {
            issues.add(format!("{path}.trim"), "invalid_trim", "trim removes all usable stock");
            all_verified = false;
            continue;
        }
        let usable = Region { x: u64::from(margins.left), y: u64::from(margins.top), w: w-trim_x, h: h-trim_y };
        let mut checker = TreeChecker {
            model, pattern, stock, parts: &parts, rotated_board: board.board_rotated_90,
            issues: &mut issues, board_path: &path, audit: Audit::default(),
        };
        checker.visit(&pattern.root, usable, None, "root", 0);
        checker.check_geometry(usable);
        let audit = checker.audit;
        if issues.total == issue_count {
            summary.boards_geometry_verified += 1;
            summary.cuts_verified += audit.cuts;
            totals.0 += usable.area();
            totals.1 += audit.piece_area;
            totals.2 += audit.residual_area;
            totals.3 += audit.kerf_area;
            for (part_id, count) in audit.counts {
                *summary.per_part_verified.entry(part_id).or_default() += count;
                summary.verified_placed_pieces += count;
            }
        } else {
            all_verified = false;
        }
    }

    for (stock_id, used) in &summary.stock_usage {
        if let Some(stock) = stocks.get(stock_id.as_str()) {
            if *used > u64::from(stock.quantity) {
                issues.add("layout.boards", "stock_capacity_exceeded", format!("stock '{stock_id}' uses {used} sheets, only {} available", stock.quantity));
            }
        }
    }
    for (part_id, expected) in &requested {
        let actual = summary.per_part_verified.get(part_id).copied().unwrap_or(0);
        if actual < *expected {
            summary.missing_per_part.insert(part_id.clone(), expected - actual);
            if all_verified {
                issues.add(format!("parts.{part_id}"), "missing_pieces", format!("{actual} placed, {expected} required"));
            }
        } else if actual > *expected {
            summary.extra_per_part.insert(part_id.clone(), actual - expected);
            issues.add(format!("parts.{part_id}"), "excess_pieces", format!("{actual} placed, {expected} required"));
        }
    }
    summary.verified_usable_area_sq_units = totals.0.to_string();
    summary.verified_piece_area_sq_units = totals.1.to_string();
    summary.verified_residual_area_sq_units = totals.2.to_string();
    summary.verified_kerf_area_sq_units = totals.3.to_string();
    summary.count_reconciliation_complete = all_verified;
    summary.geometry_validated = all_verified && issues.issues.is_empty() && summary.missing_per_part.is_empty() && summary.extra_per_part.is_empty();
    LayoutValidationReport { valid: summary.geometry_validated, issues: issues.issues, summary }
}

#[cfg(test)]
mod geometry_audit_tests {
    use super::*;
    use crate::contract::{JobOperation, JobRequest};

    fn sample_model() -> CuttingJobModel {
        let request: JobRequest = serde_json::from_str(
            include_str!("../../../examples/layout-valid.json")
        ).unwrap();
        let JobOperation::ValidateLayout { model, .. } = request.operation else {
            panic!("expected complete layout fixture")
        };
        model
    }

    #[test]
    fn independent_sweep_detects_overlapping_regions() {
        let model = sample_model();
        let stock = &model.stocks[0];
        let pattern = &model.patterns[0];
        let parts = model.parts.iter().map(|p| (p.id.as_str(), p)).collect();
        let mut issues = Collector { issues: vec![], total: 0 };
        let mut checker = TreeChecker {
            model: &model, stock, pattern, parts: &parts,
            rotated_board: false, issues: &mut issues,
            board_path: "board", audit: Audit::default(),
        };
        checker.audit.pieces.push(Region { x: 10, y: 10, w: 100, h: 100 });
        checker.audit.residuals.push(Region { x: 50, y: 50, w: 100, h: 100 });
        checker.check_geometry(Region { x: 10, y: 10, w: 980, h: 780 });
        assert!(issues.issues.iter().any(|e| e.code == "overlap"));
    }

    #[test]
    fn independent_sweep_detects_out_of_bounds_regions() {
        let model = sample_model();
        let stock = &model.stocks[0];
        let pattern = &model.patterns[0];
        let parts = model.parts.iter().map(|p| (p.id.as_str(), p)).collect();
        let mut issues = Collector { issues: vec![], total: 0 };
        let mut checker = TreeChecker {
            model: &model, stock, pattern, parts: &parts,
            rotated_board: false, issues: &mut issues,
            board_path: "board", audit: Audit::default(),
        };
        checker.audit.pieces.push(Region { x: 990, y: 10, w: 10, h: 100 });
        checker.check_geometry(Region { x: 10, y: 10, w: 980, h: 780 });
        assert!(issues.issues.iter().any(|e| e.code == "out_of_bounds"));
    }
}
