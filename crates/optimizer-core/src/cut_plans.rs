//! Stage 18: ready-to-use cutting plans ("planos de corte").
//!
//! Converts validated guillotine boards into a self-contained JSON document,
//! one record per material, in millimetres:
//!
//! All keys are snake_case and all measures are JSON numbers.
//!
//! - `cuts` is nested by level (`level2` … `level6`). Every cut "takes out"
//!   the region between the previous sibling cut and itself. Its `id_piece`
//!   is the piece it produces, `0` when the region is cut further, or a
//!   remnant id (`10000+`, `type: "retal"`).
//! - Every level list ends with its closing entry at the region boundary: a
//!   remnant, or — when the last strip reaches the boundary exactly — the
//!   strip itself flagged `edge: true` (no saw pass). Remnants are derived
//!   from the real geometry, so they never have zero or negative size.
//! - Cut lines are blade centre lines. `x1,y1 → x2,y2` extends half a kerf
//!   beyond the region on both ends, up to the centre of the bounding cuts.
//! - `parts` holds only pieces: top-left `x,y`, nominal `width`/`height`
//!   (swapped on the drawing when `rotated`), ids and grain (`veta`).
//!   Remnants are listed in `waste_part`.
//!
//! Every board also carries `metrics`, and each record a `summary`, so a
//! viewer does not need to recompute geometry.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::full_board_stacking::FullBoardStackPlan;
use crate::geometry::Rect;
use crate::global_optimizer::{optimize_global, GlobalError, GlobalSearchOptions};
use crate::guillotine::EvaluatedNode;
use crate::model::{Axis, CutDirection, CuttingJobModel, Part, StagePattern, Unit};
use crate::multi_board::{AllocatedBoard, MultiBoardResult};

/// Remnant ids start here on every board, as in the reference format.
pub const FIRST_REMNANT_ID: u64 = 10_000;
/// Deepest nesting level the plan format describes.
pub const MAX_PLAN_LEVEL: u8 = 6;

/// Hundredths of a millimetre: exact for every half-kerf position.
fn hundredths(units: Unit) -> i64 {
    i64::from(units) * 10
}

fn mm(hundredths: i64) -> Value {
    let value = hundredths as f64 / 100.0;
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn sq_m(tenths_sq: u128) -> Value {
    // 1 m² = 10^8 (tenths of mm)²; three decimals is enough for a report.
    let value = (tenths_sq as f64 / 1e8 * 1000.0).round() / 1000.0;
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn percent(numerator: u128, denominator: u128) -> Value {
    if denominator == 0 {
        return json!(0.0);
    }
    let value = (numerator as f64 * 10_000.0 / denominator as f64).round() / 100.0;
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn area(rect: &Rect) -> u128 {
    u128::from(rect.width) * u128::from(rect.height)
}

/// One region or one same-level cut, in left/top → right/bottom order.
enum Segment<'a> {
    Region(&'a EvaluatedNode),
    Cut { region: Rect, saw_path: Rect, direction: CutDirection },
}

fn node_rect(node: &EvaluatedNode) -> Rect {
    match node {
        EvaluatedNode::Split { rect, .. }
        | EvaluatedNode::Piece { rect, .. }
        | EvaluatedNode::Residual { rect, .. } => *rect,
    }
}

/// Flatten the binary tree into the cuts of one level: consecutive splits of
/// the same stage are siblings, whatever their nesting in the binary tree.
fn segments<'a>(node: &'a EvaluatedNode, level: u8, out: &mut Vec<Segment<'a>>) {
    match node {
        EvaluatedNode::Split { stage, rect, saw_path, direction, first, second, .. } if *stage == level => {
            segments(first, level, out);
            out.push(Segment::Cut { region: *rect, saw_path: *saw_path, direction: *direction });
            segments(second, level, out);
        }
        other => out.push(Segment::Region(other)),
    }
}

struct PieceOut {
    rect: Rect,
    part_index: usize,
    rotated: bool,
    id: u64,
}

struct RemnantOut {
    rect: Rect,
    id: u64,
    reusable: bool,
}

/// Per-board traversal state.
struct BoardBuilder<'a> {
    part_index: &'a BTreeMap<&'a str, usize>,
    next_piece_id: &'a mut u64,
    next_remnant_id: u64,
    next_cut: u64,
    stock_no: u64,
    half_kerf: i64,
    pieces: Vec<PieceOut>,
    remnants: Vec<RemnantOut>,
    real_cuts: u64,
    cut_length: u128,
    max_level: u8,
}

impl BoardBuilder<'_> {
    fn remnant(&mut self, rect: Rect, reusable: bool) -> u64 {
        let id = self.next_remnant_id;
        self.next_remnant_id += 1;
        self.remnants.push(RemnantOut { rect, id, reusable });
        id
    }

    fn piece(&mut self, rect: Rect, part_id: &str, rotated: bool) -> (u64, u64) {
        let id = *self.next_piece_id;
        *self.next_piece_id += 1;
        let part_index = self.part_index[part_id];
        self.pieces.push(PieceOut { rect, part_index, rotated, id });
        (id, part_index as u64 + 1)
    }

    /// Blade centre line of a cut separating `before` from what follows,
    /// spanning `parent` plus half a kerf on both ends.
    fn line(&self, direction: CutDirection, parent: Rect, at: i64) -> (i64, i64, i64, i64) {
        let x = hundredths(parent.x);
        let y = hundredths(parent.y);
        let w = hundredths(parent.width);
        let h = hundredths(parent.height);
        match direction {
            CutDirection::Vertical => (at, y - self.half_kerf, at, y + h + self.half_kerf),
            CutDirection::Horizontal => (x - self.half_kerf, at, x + w + self.half_kerf, at),
        }
    }

    /// Build the cut list of `container` at `level`.
    fn level(&mut self, container: &EvaluatedNode, level: u8) -> Vec<Value> {
        self.max_level = self.max_level.max(level);
        let parent = node_rect(container);
        let mut items = Vec::new();
        segments(container, level, &mut items);
        let direction = items.iter().find_map(|s| match s {
            Segment::Cut { direction, .. } => Some(*direction),
            Segment::Region(_) => None,
        });
        // A board that is one single piece or remnant has no level-1 cut; the
        // closing entry still needs an axis, so use the board's long side.
        let direction = direction.unwrap_or(if parent.width >= parent.height {
            CutDirection::Vertical
        } else {
            CutDirection::Horizontal
        });
        // As in the reference format, the closing remnant is numbered before
        // the remnants nested inside this level.
        let tail_remnant = match items.last() {
            Some(Segment::Region(EvaluatedNode::Residual { rect, reusable, .. })) => Some(self.remnant(*rect, *reusable)),
            _ => None,
        };
        let mut out = Vec::new();
        let mut pending: Option<&EvaluatedNode> = None;
        for item in &items {
            match item {
                Segment::Region(node) => pending = Some(node),
                Segment::Cut { region, saw_path, direction } => {
                    let node = pending.take().expect("a cut always follows a region");
                    let at = match direction {
                        CutDirection::Vertical => hundredths(saw_path.x) + hundredths(saw_path.width) / 2,
                        CutDirection::Horizontal => hundredths(saw_path.y) + hundredths(saw_path.height) / 2,
                    };
                    self.real_cuts += 1;
                    self.cut_length += u128::from(match direction {
                        CutDirection::Vertical => region.height,
                        CutDirection::Horizontal => region.width,
                    });
                    let line = self.line(*direction, *region, at);
                    out.push(self.entry(node, level, *direction, line, false));
                }
            }
        }
        let tail = pending.expect("a level always ends with a region");
        let rect = node_rect(tail);
        let at = match direction {
            CutDirection::Vertical => hundredths(rect.x) + hundredths(rect.width) + self.half_kerf,
            CutDirection::Horizontal => hundredths(rect.y) + hundredths(rect.height) + self.half_kerf,
        };
        let (x1, y1, x2, y2) = self.line(direction, parent, at);
        match tail_remnant {
            Some(id) => out.push(json!({
                "x1": mm(x1), "y1": mm(y1), "x2": mm(x2), "y2": mm(y2),
                "a_level": level, "n_item": id, "id_piece": id, "type": "retal",
            })),
            None => out.push(self.entry(tail, level, direction, (x1, y1, x2, y2), true)),
        }
        out
    }

    /// One cut entry taking out `node`. `edge` entries lie on the region
    /// boundary and need no saw pass.
    fn entry(&mut self, node: &EvaluatedNode, level: u8, direction: CutDirection,
             (x1, y1, x2, y2): (i64, i64, i64, i64), edge: bool) -> Value {
        let rect = node_rect(node);
        let dimension = match direction {
            CutDirection::Vertical => rect.width,
            CutDirection::Horizontal => rect.height,
        };
        let mut cut = Map::new();
        cut.insert("stock_no".into(), json!(self.stock_no));
        if !edge {
            cut.insert("i_cut".into(), json!(self.next_cut));
            self.next_cut += 1;
        }
        for (key, value) in [("x1", x1), ("y1", y1), ("x2", x2), ("y2", y2)] {
            cut.insert(key.into(), mm(value));
        }
        cut.insert("a_level".into(), json!(level));
        let (item, id, children) = match node {
            EvaluatedNode::Piece { part_id, rotated, rect, .. } => {
                let (id, item) = self.piece(*rect, part_id, *rotated);
                (item, id, None)
            }
            EvaluatedNode::Residual { rect, reusable, .. } => {
                let id = self.remnant(*rect, *reusable);
                cut.insert("type".into(), json!("retal"));
                (id, id, None)
            }
            EvaluatedNode::Split { .. } => (0, 0, Some(self.level(node, level + 1))),
        };
        cut.insert("n_item".into(), json!(item));
        cut.insert("id_piece".into(), json!(id));
        cut.insert("dimension".into(), mm(hundredths(dimension)));
        if edge {
            cut.insert("edge".into(), json!(true));
        }
        if let Some(children) = children {
            cut.insert(format!("level{}", level + 1), Value::Array(children));
        }
        Value::Object(cut)
    }
}

/// Grain of the piece along its own sides: `"L"` along `width` (largo),
/// `"A"` along `height` (ancho), `null` when the piece has no grain.
fn veta(part: &Part) -> Value {
    match part.grain_axis {
        Some(Axis::X) => json!("L"),
        Some(Axis::Y) => json!("A"),
        None => Value::Null,
    }
}

fn part_json(part: &Part, piece: &PieceOut, stock_no: u64, index: usize) -> Value {
    json!({
        "stock_no": stock_no,
        "part": index,
        "x": mm(hundredths(piece.rect.x)),
        "y": mm(hundredths(piece.rect.y)),
        "width": mm(hundredths(part.width)),
        "height": mm(hundredths(part.height)),
        "rotated": piece.rotated,
        "id_piece": piece.id,
        "n_item": piece.part_index + 1,
        "veta": veta(part),
    })
}

struct BoardOut {
    json: Value,
    gross_area: u128,
    usable_area: u128,
    piece_area: u128,
    remnant_area: u128,
    pieces: u64,
    cuts: u64,
    cut_length: u128,
}

fn export_board(
    model: &CuttingJobModel, part_index: &BTreeMap<&str, usize>, board: &AllocatedBoard,
    index: usize, root_stock_no: u64, next_piece_id: &mut u64,
) -> BoardOut {
    let stock = model.stocks.iter().find(|s| s.id == board.stock_id).expect("allocated stock exists");
    let (width, height) = if board.board_rotated_90 { (stock.height, stock.width) } else { (stock.width, stock.height) };
    let usable = board.evaluated.usable_rect;
    let kerf = hundredths(model.machine.kerf);
    let half_kerf = kerf / 2;
    let stock_no = index as u64 + 1;
    let mut builder = BoardBuilder {
        part_index, next_piece_id,
        next_remnant_id: FIRST_REMNANT_ID, next_cut: 0,
        stock_no, half_kerf,
        pieces: Vec::new(), remnants: Vec::new(),
        real_cuts: 0, cut_length: 0, max_level: 0,
    };
    let root_level = match &board.evaluated.root {
        EvaluatedNode::Split { stage, .. } => *stage,
        _ => 1,
    };
    let cuts = builder.level(&board.evaluated.root, root_level);
    let orientation = match &board.evaluated.root {
        EvaluatedNode::Split { direction: CutDirection::Horizontal, .. } => "h",
        _ => "v",
    };

    // Trim lines: the kerf of each trim cut ends exactly on the usable edge.
    let (left, top) = (usable.x, usable.y);
    let right = width - usable.x - usable.width;
    let bottom = height - usable.y - usable.height;
    let bx1 = if left > 0 { hundredths(left) - half_kerf } else { 0 };
    let by1 = if top > 0 { hundredths(top) - half_kerf } else { 0 };
    let bx2 = if right > 0 { hundredths(width - right) + half_kerf } else { hundredths(width) };
    let by2 = if bottom > 0 { hundredths(height - bottom) + half_kerf } else { hundredths(height) };
    let first_vertical = orientation == "v";
    let mut trims = Vec::new();
    let mut trim_cut = |vertical: bool, at: i64, from: i64, to: i64| {
        let level = if vertical == first_vertical { 0 } else { 1 };
        let (x1, y1, x2, y2) = if vertical { (at, from, at, to) } else { (from, at, to, at) };
        trims.push(json!({
            "stock_no": stock_no, "i_cut": trims.len(),
            "x1": mm(x1), "y1": mm(y1), "x2": mm(x2), "y2": mm(y2),
            "a_level": level,
        }));
    };
    // The first trim pair crosses the whole board; the second one runs
    // between the first pair's lines, as in the reference format.
    let (full_v, full_h) = (hundredths(height), hundredths(width));
    if first_vertical {
        if left > 0 { trim_cut(true, bx1, 0, full_v); }
        if right > 0 { trim_cut(true, bx2, 0, full_v); }
        if top > 0 { trim_cut(false, by1, bx1, bx2); }
        if bottom > 0 { trim_cut(false, by2, bx1, bx2); }
    } else {
        if top > 0 { trim_cut(false, by1, 0, full_h); }
        if bottom > 0 { trim_cut(false, by2, 0, full_h); }
        if left > 0 { trim_cut(true, bx1, by1, by2); }
        if right > 0 { trim_cut(true, bx2, by1, by2); }
    }

    let mut pieces = std::mem::take(&mut builder.pieces);
    pieces.sort_by_key(|p| p.id);
    let mut remnants = std::mem::take(&mut builder.remnants);
    remnants.sort_by_key(|r| r.id);
    let parts: Vec<Value> = pieces.iter().enumerate()
        .map(|(i, piece)| part_json(&model.parts[piece.part_index], piece, stock_no, i))
        .collect();
    let waste: Vec<Value> = remnants.iter().enumerate().map(|(i, remnant)| {
        let r = remnant.rect;
        json!({
            "stock_no": stock_no, "part": i,
            "x": mm(hundredths(r.x)), "y": mm(hundredths(r.y)),
            "width": mm(hundredths(r.width)), "height": mm(hundredths(r.height)),
            "id_piece": remnant.id, "reusable": remnant.reusable,
        })
    }).collect();

    let gross_area = u128::from(width) * u128::from(height);
    let usable_area = area(&usable);
    let piece_area: u128 = pieces.iter().map(|p| area(&p.rect)).sum();
    let remnant_area: u128 = remnants.iter().map(|r| area(&r.rect)).sum();
    let kerf_area = usable_area.saturating_sub(piece_area + remnant_area);
    let largest_remnant = remnants.iter().max_by_key(|r| area(&r.rect)).map(|r| json!({
        "id_piece": r.id, "width": mm(hundredths(r.rect.width)), "height": mm(hundredths(r.rect.height)),
    }));
    let metrics = json!({
        "pieces": pieces.len(),
        "remnants": remnants.len(),
        "cuts": builder.real_cuts,
        "levels": builder.max_level,
        "cut_length_m": (builder.cut_length as f64 / 10_000.0 * 100.0).round() / 100.0,
        "board_area_m2": sq_m(gross_area),
        "usable_area_m2": sq_m(usable_area),
        "pieces_area_m2": sq_m(piece_area),
        "remnant_area_m2": sq_m(remnant_area),
        "kerf_area_m2": sq_m(kerf_area),
        "trim_area_m2": sq_m(gross_area - usable_area),
        "utilization_pct": percent(piece_area, gross_area),
        "usable_utilization_pct": percent(piece_area, usable_area),
        "largest_remnant": largest_remnant,
        "stock_id": board.stock_id,
        "board_id": board.board_id,
        "rotated_90": board.board_rotated_90,
        "pattern": board.mode,
    });
    let json = json!({
        "index": stock_no,
        "orientation": orientation,
        "stock_no": stock_no,
        "root_stock_no": root_stock_no,
        "cuts_trims": trims,
        "trim": mm(hundredths(left)),
        "trim_margins": {
            "top": mm(hundredths(top)), "right": mm(hundredths(right)),
            "bottom": mm(hundredths(bottom)), "left": mm(hundredths(left)),
        },
        "saw_width": mm(kerf),
        "max_books": model.machine.max_stack_members,
        "stock_count": 1,
        "stock_cuts": builder.real_cuts,
        "cuts": cuts,
        "parts": parts,
        "waste_part": waste,
        "width": mm(hundredths(width)),
        "height": mm(hundredths(height)),
        "x1": mm(bx1), "x2": mm(bx2), "y1": mm(by1), "y2": mm(by2),
        "metrics": metrics,
    });
    BoardOut {
        json, gross_area, usable_area, piece_area, remnant_area,
        pieces: pieces.len() as u64, cuts: builder.real_cuts, cut_length: builder.cut_length,
    }
}

/// Export an allocation (and optionally its full-board stacks) as cutting
/// plans. `elapsed_ms` is reported as the optimisation time.
pub fn export_cut_plans(
    model: &CuttingJobModel, allocation: &MultiBoardResult,
    stacks: Option<&FullBoardStackPlan>, elapsed_ms: u128,
) -> Value {
    let part_index: BTreeMap<&str, usize> = model.parts.iter().enumerate()
        .map(|(i, p)| (p.id.as_str(), i)).collect();
    let mut next_piece_id = 1_u64;
    let mut records = Vec::new();
    for material in &model.materials {
        let boards: Vec<&AllocatedBoard> = allocation.boards.iter()
            .filter(|b| model.stocks.iter().any(|s| s.id == b.stock_id && s.material_id == material.id))
            .collect();
        if boards.is_empty() {
            continue;
        }
        // Board ids → position in this record, to map stacks to root_stock_no.
        let positions: BTreeMap<&str, u64> = boards.iter().enumerate()
            .map(|(i, b)| (b.board_id.as_str(), i as u64 + 1)).collect();
        let mut root_of: BTreeMap<&str, u64> = BTreeMap::new();
        if let Some(plan) = stacks {
            for stack in &plan.stacks {
                let members: Vec<u64> = stack.member_board_ids.iter()
                    .filter_map(|id| positions.get(id.as_str()).copied()).collect();
                if let Some(&root) = members.iter().min() {
                    for id in &stack.member_board_ids {
                        root_of.insert(id.as_str(), root);
                    }
                }
            }
        }
        let mut out_boards = Vec::new();
        let (mut gross, mut usable, mut piece_area, mut remnant_area) = (0_u128, 0_u128, 0_u128, 0_u128);
        let (mut pieces, mut cuts, mut cut_length) = (0_u64, 0_u64, 0_u128);
        for (i, board) in boards.iter().enumerate() {
            let root = root_of.get(board.board_id.as_str()).copied().unwrap_or(i as u64 + 1);
            let out = export_board(model, &part_index, board, i, root, &mut next_piece_id);
            gross += out.gross_area;
            usable += out.usable_area;
            piece_area += out.piece_area;
            remnant_area += out.remnant_area;
            pieces += out.pieces;
            cuts += out.cuts;
            cut_length += out.cut_length;
            out_boards.push(out.json);
        }
        let books: std::collections::BTreeSet<u64> = out_boards.iter()
            .filter_map(|b| b["root_stock_no"].as_u64()).collect();
        let stock = model.stocks.iter().find(|s| s.material_id == material.id);
        let trim = stock.map_or(0, |s| s.trim.left);
        records.push(json!({
            "index": records.len() + 1,
            "version": env!("CARGO_PKG_VERSION"),
            "elapsed_time": elapsed_ms as f64 / 1000.0,
            "material": material.name,
            "material_id": material.id,
            "saw_width": mm(hundredths(model.machine.kerf)),
            "trim": mm(hundredths(trim)),
            "max_books": model.machine.max_stack_members,
            "thickness": mm(hundredths(material.thickness)),
            "unit_measure": "mm",
            "boards": out_boards,
            "summary": {
                "boards": boards.len(),
                "books": books.len(),
                "pieces": pieces,
                "cuts": cuts,
                "cut_length_m": (cut_length as f64 / 10_000.0 * 100.0).round() / 100.0,
                "board_area_m2": sq_m(gross),
                "pieces_area_m2": sq_m(piece_area),
                "remnant_area_m2": sq_m(remnant_area),
                "kerf_area_m2": sq_m(usable.saturating_sub(piece_area + remnant_area)),
                "trim_area_m2": sq_m(gross - usable),
                "utilization_pct": percent(piece_area, gross),
                "usable_utilization_pct": percent(piece_area, usable),
            },
        }));
    }
    let complete = allocation.complete_order;
    json!({
        "success": true,
        "message": if complete {
            "Planos de corte generados correctamente".to_owned()
        } else {
            "Planos generados; hay piezas sin colocar (ver remaining_parts)".to_owned()
        },
        "complete_order": complete,
        "remaining_parts": allocation.remaining_part_counts.iter()
            .filter(|(_, &n)| n > 0).collect::<BTreeMap<_, _>>(),
        "records": records,
    })
}

/// Search settings for `generate_cut_plans` when the request omits them:
/// production orders need a wider search than the global optimiser's
/// conservative defaults. On a 345-piece, 12-item order this finds 34 boards
/// in about 1.5 s (release build) versus 38 with the defaults.
pub fn default_cut_plan_search() -> GlobalSearchOptions {
    GlobalSearchOptions {
        max_boards: 200, restarts: 8, beam_width: 16, max_expansions: 4_000,
        max_iterations: 64, neighborhood_boards: 8,
        ..GlobalSearchOptions::default()
    }
}

/// Run the global optimiser and export its best layout as cutting plans.
///
/// With patterns of 4+ levels allowed, the order is first solved exactly as a
/// 2-3 level job would be (same patterns, seed and time budget), then with
/// every allowed pattern in the remaining time (at least half the budget,
/// so the whole job takes at most 1.5× `max_runtime_ms`). The better layout
/// is kept: allowing more levels never gives a worse plan than 3 levels.
pub fn generate_cut_plans(
    model: &CuttingJobModel, seed: u64, max_runtime_ms: u64, options: GlobalSearchOptions,
) -> Result<Value, GlobalError> {
    let started = Instant::now();
    let shallow: Vec<StagePattern> = model.machine.allowed_patterns.iter().copied().filter(|m| m.levels() <= 3).collect();
    let deep = model.machine.allowed_patterns.iter().any(|m| m.levels() >= 4);
    if !deep || shallow.is_empty() || max_runtime_ms < 400 {
        let result = optimize_global(model, seed, max_runtime_ms, options)?;
        return Ok(export_cut_plans(model, &result.best, Some(&result.best_stacks), started.elapsed().as_millis()));
    }
    let mut shallow_model = model.clone();
    shallow_model.machine.allowed_patterns = shallow;
    let guard = optimize_global(&shallow_model, seed, max_runtime_ms, options)?;
    let used = started.elapsed().as_millis() as u64;
    let full = optimize_global(model, seed, max_runtime_ms.saturating_sub(used).max(max_runtime_ms / 2), options)?;
    let winner = if layout_key(&guard.best) < layout_key(&full.best) { guard } else { full };
    Ok(export_cut_plans(model, &winner.best, Some(&winner.best_stacks), started.elapsed().as_millis()))
}

/// Lower is better: unplaced pieces, then stock cost, then boards.
fn layout_key(layout: &MultiBoardResult) -> (u64, u128, usize) {
    let unplaced = layout.remaining_part_counts.values().map(|&n| u64::from(n)).sum();
    let cost = layout.stock_cost_minor.parse().unwrap_or(u128::MAX);
    (unplaced, cost, layout.boards.len())
}
