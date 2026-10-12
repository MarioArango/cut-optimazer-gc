//! Stage 2 model checks: field bounds, uniqueness, cross-references, physical
//! stack descriptors. Does NOT prove geometric feasibility or machine execution.

use super::*;
use std::collections::{HashMap, HashSet};

const MAX_ENTRIES: usize = 100_000;
const MAX_PATTERNS: usize = 10_000;
const MAX_TREE_NODES: usize = 10_000;
const MAX_ISSUES: usize = 100;
const MAX_DIMENSION: Unit = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Default)]
struct Issues(Vec<ModelIssue>);

impl Issues {
    fn push(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        if self.0.len() < MAX_ISSUES {
            self.0.push(ModelIssue {
                path: path.into(),
                code: code.to_owned(),
                message: message.into(),
            });
        }
    }
    fn limit(&mut self, name: &str, count: usize, max: usize) {
        if count > max {
            self.push(name, "too_many", format!("maximum {max} entries (received {count})"));
        }
    }
    fn positive(&mut self, path: &str, value: Unit) {
        if value == 0 || value > MAX_DIMENSION {
            self.push(path, "dimension_out_of_range", format!("must be within 1..={MAX_DIMENSION} (0.1 mm units)"));
        }
    }
    fn id(&mut self, path: &str, value: &str) {
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        {
            self.push(path, "invalid_id", "must be 1..64 ASCII letters, digits, '.', '_' or '-'");
        }
    }
    fn unique<'a>(&mut self, path: &str, id: &'a str, seen: &mut HashSet<&'a str>) {
        if !seen.insert(id) {
            self.push(path, "duplicate_id", format!("duplicate identifier '{id}'"));
        }
    }
    fn grain(&mut self, path: &str, directional: bool, grain: Option<Axis>) {
        if directional != grain.is_some() {
            self.push(path, "grain_mismatch", "grain_axis must be present exactly for directional materials");
        }
    }
}

fn check_pattern_node<'a>(
    node: &'a PatternNode,
    path: &str,
    part_ids: &HashSet<&str>,
    seen: &mut HashSet<&'a str>,
    visited: &mut usize,
    issues: &mut Issues,
) {
    *visited += 1;
    if *visited > MAX_TREE_NODES {
        if *visited == MAX_TREE_NODES + 1 {
            issues.push(path, "tree_too_large", format!("pattern exceeds {MAX_TREE_NODES} nodes"));
        }
        return;
    }
    let id = match node {
        PatternNode::Split { node_id, .. }
        | PatternNode::Piece { node_id, .. }
        | PatternNode::Residual { node_id, .. } => node_id.as_str(),
    };
    issues.id(&format!("{path}.node_id"), id);
    issues.unique(&format!("{path}.node_id"), id, seen);
    match node {
        PatternNode::Split { at, first, second, .. } => {
            issues.positive(&format!("{path}.at"), *at);
            check_pattern_node(first, &format!("{path}.first"), part_ids, seen, visited, issues);
            check_pattern_node(second, &format!("{path}.second"), part_ids, seen, visited, issues);
        }
        PatternNode::Piece { part_id, .. } => {
            if !part_ids.contains(part_id.as_str()) {
                issues.push(format!("{path}.part_id"), "unknown_reference", format!("unknown part '{part_id}'"));
            }
        }
        PatternNode::Residual { .. } => {}
    }
}

/// Validate Stage 2 structural invariants; returns all detected issues (up to
/// 100), or a descriptive summary when valid. Geometrical layout and the
/// production sequence require the independent validators in later stages.
pub fn validate_model(model: &CuttingJobModel) -> Result<ModelSummary, Vec<ModelIssue>> {
    let mut errors = Issues::default();
    errors.limit("materials", model.materials.len(), MAX_ENTRIES);
    errors.limit("parts", model.parts.len(), MAX_ENTRIES);
    errors.limit("stocks", model.stocks.len(), MAX_ENTRIES);
    errors.limit("patterns", model.patterns.len(), MAX_PATTERNS);
    errors.limit("workpieces", model.workpieces.len(), MAX_ENTRIES);
    errors.limit("stacks", model.stacks.len(), MAX_ENTRIES);
    if model.materials.is_empty() {
        errors.push("materials", "required", "provide at least one material");
    }
    if model.parts.is_empty() {
        errors.push("parts", "required", "provide at least one requested part type");
    }
    if model.stocks.is_empty() {
        errors.push("stocks", "required", "provide at least one stock type");
    }

    let mut ids = HashSet::new();
    let mut materials = HashMap::new();
    for (i, mat) in model.materials.iter().enumerate() {
        let path = format!("materials[{i}]");
        errors.id(&format!("{path}.id"), &mat.id);
        errors.unique(&format!("{path}.id"), &mat.id, &mut ids);
        errors.positive(&format!("{path}.thickness"), mat.thickness);
        materials.entry(mat.id.as_str()).or_insert(mat);
        if mat.name.trim().is_empty() || mat.name.chars().count() > 250 {
            errors.push(format!("{path}.name"), "invalid_name", "name must contain 1..250 characters");
        }
    }

    ids.clear();
    let mut part_ids = HashSet::new();
    let mut parts_by_id = HashMap::new();
    let mut requested_pieces = 0u64;
    for (i, part) in model.parts.iter().enumerate() {
        let path = format!("parts[{i}]");
        errors.id(&format!("{path}.id"), &part.id);
        errors.unique(&format!("{path}.id"), &part.id, &mut ids);
        part_ids.insert(part.id.as_str());
        parts_by_id.entry(part.id.as_str()).or_insert(part);
        errors.positive(&format!("{path}.width"), part.width);
        errors.positive(&format!("{path}.height"), part.height);
        if part.quantity == 0 {
            errors.push(format!("{path}.quantity"), "invalid_quantity", "quantity must be positive");
        }
        if part.priority > 5 {
            errors.push(format!("{path}.priority"), "invalid_priority", "priority must be 0..5");
        }
        if let Some(material) = materials.get(part.material_id.as_str()) {
            // On directional materials a part may omit grain_axis: it has no
            // grain of its own and can be placed in either orientation.
            if !material.directional_grain && part.grain_axis.is_some() {
                errors.push(format!("{path}.grain_axis"), "grain_mismatch", "grain_axis requires a directional material");
            }
        } else {
            errors.push(format!("{path}.material_id"), "unknown_reference", format!("unknown material '{}'", part.material_id));
        }
        requested_pieces += u64::from(part.quantity);
    }

    ids.clear();
    let mut stocks = HashMap::new();
    let mut available_boards = 0u64;
    for (i, stock) in model.stocks.iter().enumerate() {
        let path = format!("stocks[{i}]");
        errors.id(&format!("{path}.id"), &stock.id);
        errors.unique(&format!("{path}.id"), &stock.id, &mut ids);
        stocks.entry(stock.id.as_str()).or_insert(stock);
        errors.positive(&format!("{path}.width"), stock.width);
        errors.positive(&format!("{path}.height"), stock.height);
        errors.positive(&format!("{path}.thickness"), stock.thickness);
        if stock.quantity == 0 {
            errors.push(format!("{path}.quantity"), "invalid_quantity", "quantity must be positive");
        }
        available_boards += u64::from(stock.quantity);
        if u64::from(stock.trim.left) + u64::from(stock.trim.right) >= u64::from(stock.width)
            || u64::from(stock.trim.top) + u64::from(stock.trim.bottom) >= u64::from(stock.height)
        {
            errors.push(format!("{path}.trim"), "invalid_trim", "trim must leave positive usable width and height");
        }
        if let Some(material) = materials.get(stock.material_id.as_str()) {
            if material.thickness != stock.thickness {
                errors.push(format!("{path}.thickness"), "thickness_mismatch", "stock thickness must match material thickness");
            }
            errors.grain(&format!("{path}.grain_axis"), material.directional_grain, stock.grain_axis);
        } else {
            errors.push(format!("{path}.material_id"), "unknown_reference", format!("unknown material '{}'", stock.material_id));
        }
    }

    let machine = &model.machine;
    errors.id("machine.id", &machine.id);
    for (field, value) in [
        ("machine.kerf", machine.kerf),
        ("machine.max_cut_length", machine.max_cut_length),
        ("machine.min_piece_width", machine.min_piece_width),
        ("machine.min_piece_height", machine.min_piece_height),
        ("machine.min_strip_width", machine.min_strip_width),
        ("machine.min_clamp_width", machine.min_clamp_width),
        ("machine.max_stack_height", machine.max_stack_height),
    ] {
        errors.positive(field, value);
    }
    if machine.max_stack_members == 0 {
        errors.push("machine.max_stack_members", "invalid_capacity", "must be at least one");
    }
    if machine.allowed_patterns.is_empty() {
        errors.push("machine.allowed_patterns", "required", "must contain at least one supported cutting pattern");
    }
    let mut patterns_set = HashSet::new();
    for (i, mode) in machine.allowed_patterns.iter().enumerate() {
        if !patterns_set.insert(*mode) {
            errors.push(format!("machine.allowed_patterns[{i}]"), "duplicate_mode", "cutting pattern listed more than once");
        }
    }
    if machine.stacking.restacking && !machine.stacking.strips {
        errors.push("machine.stacking.restacking", "invalid_capability", "restacking requires strip stacking support");
    }
    if (machine.stacking.full_boards || machine.stacking.strips) && machine.max_stack_members < 2 {
        errors.push("machine.max_stack_members", "invalid_capacity", "stacking requires at least two members");
    }

    ids.clear();
    let mut known_patterns = HashSet::new();
    let mut patterns_by_id = HashMap::new();
    let mut pattern_nodes: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (i, pattern) in model.patterns.iter().enumerate() {
        let path = format!("patterns[{i}]");
        errors.id(&format!("{path}.id"), &pattern.id);
        errors.unique(&format!("{path}.id"), &pattern.id, &mut ids);
        known_patterns.insert(pattern.id.as_str());
        patterns_by_id.entry(pattern.id.as_str()).or_insert(pattern);
        if !stocks.contains_key(pattern.stock_id.as_str()) {
            errors.push(format!("{path}.stock_id"), "unknown_reference", format!("unknown stock '{}'", pattern.stock_id));
        }
        if !patterns_set.contains(&pattern.mode) {
            errors.push(format!("{path}.mode"), "unsupported_mode", "pattern is not listed in machine.allowed_patterns");
        }
        let mut node_ids = HashSet::new();
        let mut visited = 0;
        check_pattern_node(&pattern.root, &format!("{path}.root"), &part_ids, &mut node_ids, &mut visited, &mut errors);
        // Referential check: a part in this pattern must belong to the same
        // material as the stock the pattern was built for.
        if let Some(stock) = stocks.get(pattern.stock_id.as_str()) {
            let mut pending = vec![&pattern.root];
            while let Some(node) = pending.pop() {
                match node {
                    PatternNode::Split { first, second, .. } => {
                        pending.push(first);
                        pending.push(second);
                    }
                    PatternNode::Piece { part_id, .. } => {
                        if let Some(part) = parts_by_id.get(part_id.as_str()) {
                            if part.material_id != stock.material_id {
                                errors.push(format!("{path}.root"), "material_mismatch", "pattern references part with material different from stock");
                            }
                        }
                    }
                    PatternNode::Residual { .. } => {}
                }
            }
        }
        pattern_nodes.insert(pattern.id.as_str(), node_ids);
    }

    ids.clear();
    let mut workpieces = HashMap::new();
    let mut physical_origins = HashSet::new();
    let mut origin_keys_by_stock: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (i, wp) in model.workpieces.iter().enumerate() {
        let path = format!("workpieces[{i}]");
        errors.id(&format!("{path}.id"), &wp.id);
        errors.unique(&format!("{path}.id"), &wp.id, &mut ids);
        workpieces.entry(wp.id.as_str()).or_insert(wp);
        errors.id(&format!("{path}.origin_board_key"), &wp.origin_board_key);
        origin_keys_by_stock.entry(wp.origin_stock_id.as_str())
            .or_default().insert(wp.origin_board_key.as_str());
        errors.positive(&format!("{path}.width"), wp.width);
        errors.positive(&format!("{path}.height"), wp.height);
        errors.positive(&format!("{path}.thickness"), wp.thickness);
        if let Some(stock) = stocks.get(wp.origin_stock_id.as_str()) {
            if wp.material_id != stock.material_id {
                errors.push(format!("{path}.material_id"), "material_mismatch", "must match origin stock material");
            }
            if wp.thickness != stock.thickness {
                errors.push(format!("{path}.thickness"), "thickness_mismatch", "must match origin stock thickness");
            }
            if wp.kind == WorkpieceKind::Board {
                let expected_grain = match (stock.grain_axis, wp.orientation) {
                    (Some(Axis::X), Orientation::Deg90) => Some(Axis::Y),
                    (Some(Axis::Y), Orientation::Deg90) => Some(Axis::X),
                    (axis, _) => axis,
                };
                if wp.grain_axis != expected_grain {
                    errors.push(format!("{path}.grain_axis"), "grain_mismatch", "board workpiece grain must follow stock rotation");
                }
                let expected = match wp.orientation {
                    Orientation::Deg0 => (stock.width, stock.height),
                    Orientation::Deg90 => (stock.height, stock.width),
                };
                if (wp.width, wp.height) != expected {
                    errors.push(format!("{path}.width"), "board_size_mismatch", "full-board workpiece dimensions must match oriented stock dimensions");
                }
                if !physical_origins.insert((wp.origin_stock_id.as_str(), wp.origin_board_key.as_str())) {
                    errors.push(format!("{path}.origin_board_key"), "duplicate_origin", "two full-board workpieces identify the same physical stock board");
                }
            }
        } else {
            errors.push(format!("{path}.origin_stock_id"), "unknown_reference", format!("unknown stock '{}'", wp.origin_stock_id));
        }
        if let Some(material) = materials.get(wp.material_id.as_str()) {
            errors.grain(&format!("{path}.grain_axis"), material.directional_grain, wp.grain_axis);
            if material.thickness != wp.thickness {
                errors.push(format!("{path}.thickness"), "thickness_mismatch", "workpiece thickness must match material thickness");
            }
        } else {
            errors.push(format!("{path}.material_id"), "unknown_reference", format!("unknown material '{}'", wp.material_id));
        }
        if wp.kind == WorkpieceKind::Board && wp.parent_workpiece_id.is_some() {
            errors.push(format!("{path}.parent_workpiece_id"), "invalid_parent", "full board may not have a parent");
        }
        if wp.kind != WorkpieceKind::Board && wp.parent_workpiece_id.is_none() {
            errors.push(format!("{path}.parent_workpiece_id"), "missing_parent", "strip/region must reference the source workpiece");
        }
        if let Some(pattern_id) = &wp.pattern_id {
            if !known_patterns.contains(pattern_id.as_str()) {
                errors.push(format!("{path}.pattern_id"), "unknown_reference", "unknown pattern template");
            } else {
                if let Some(pattern) = patterns_by_id.get(pattern_id.as_str()) {
                    if pattern.stock_id != wp.origin_stock_id {
                        errors.push(format!("{path}.pattern_id"), "origin_mismatch", "pattern and workpiece must reference the same original stock type");
                    }
                }
                if let Some(node_id) = &wp.pending_node_id {
                    if !pattern_nodes.get(pattern_id.as_str())
                        .is_some_and(|nodes| nodes.contains(node_id.as_str()))
                    {
                        errors.push(format!("{path}.pending_node_id"), "unknown_reference", "pending node ID is absent from pattern tree");
                    }
                }
            }
        } else if wp.pending_node_id.is_some() {
            errors.push(format!("{path}.pending_node_id"), "missing_pattern", "pending node requires pattern_id");
        }
    }

    for (stock_id, origin_keys) in &origin_keys_by_stock {
        if let Some(stock) = stocks.get(stock_id) {
            if origin_keys.len() as u64 > u64::from(stock.quantity) {
                errors.push("workpieces", "stock_capacity_exceeded", format!("stock '{stock_id}' has {} distinct physical board origins but quantity is {}", origin_keys.len(), stock.quantity));
            }
        }
    }

    // Parent traceability and cycle detection; no recursion or quadratic walks.
    let mut finished = HashSet::new();
    for (i, wp) in model.workpieces.iter().enumerate() {
        if let Some(parent_id) = wp.parent_workpiece_id.as_ref() {
            match workpieces.get(parent_id.as_str()) {
                None => errors.push(format!("workpieces[{i}].parent_workpiece_id"), "unknown_reference", format!("unknown parent workpiece '{parent_id}'")),
                Some(parent) => {
                    if wp.origin_stock_id != parent.origin_stock_id || wp.origin_board_key != parent.origin_board_key {
                        errors.push(format!("workpieces[{i}].parent_workpiece_id"), "origin_mismatch", "child and parent must trace to the same physical board");
                    }
                    if wp.id == parent.id {
                        errors.push(format!("workpieces[{i}].parent_workpiece_id"), "parent_cycle", "workpiece cannot be its own parent");
                    }
                }
            }
        }
        if finished.contains(wp.id.as_str()) {
            continue;
        }
        let mut seen_local = HashSet::new();
        let mut path = Vec::new();
        let mut current = Some(wp.id.as_str());
        while let Some(id) = current {
            if finished.contains(id) { break; }
            if !seen_local.insert(id) {
                errors.push(format!("workpieces[{i}].parent_workpiece_id"), "parent_cycle", "workpiece ancestry contains a cycle");
                break;
            }
            path.push(id);
            current = workpieces.get(id).and_then(|w| w.parent_workpiece_id.as_deref());
        }
        finished.extend(path);
    }

    ids.clear();
    let mut member_occurrence = HashSet::new();
    let mut stacked_workpieces = 0usize;
    for (i, stack) in model.stacks.iter().enumerate() {
        let path = format!("stacks[{i}]");
        errors.id(&format!("{path}.id"), &stack.id);
        errors.unique(&format!("{path}.id"), &stack.id, &mut ids);
        if stack.member_ids.len() < 2 {
            errors.push(format!("{path}.member_ids"), "too_few_members", "stack must have at least two distinct physical members");
        }
        if stack.member_ids.len() > machine.max_stack_members as usize {
            errors.push(format!("{path}.member_ids"), "stack_capacity_exceeded", "exceeds machine.max_stack_members");
        }
        if (stack.kind == StackKind::FullBoard && !machine.stacking.full_boards)
            || (stack.kind != StackKind::FullBoard && !machine.stacking.strips)
        {
            errors.push(format!("{path}.kind"), "stacking_disabled", "machine does not support this stacking category");
        }
        let mut first: Option<&Workpiece> = None;
        let mut total_height = 0u64;
        for (j, member_id) in stack.member_ids.iter().enumerate() {
            let member_path = format!("{path}.member_ids[{j}]");
            if !member_occurrence.insert(member_id.as_str()) {
                errors.push(member_path.clone(), "duplicate_stack_member", "a workpiece may belong to only one active stack");
            }
            if let Some(wp) = workpieces.get(member_id.as_str()).copied() {
                stacked_workpieces += 1;
                total_height += u64::from(wp.thickness);
                let correct_kind = match stack.kind {
                    StackKind::FullBoard => wp.kind == WorkpieceKind::Board,
                    StackKind::Strip => wp.kind == WorkpieceKind::Strip,
                    StackKind::MixedRegion => wp.kind != WorkpieceKind::Board,
                };
                if !correct_kind {
                    errors.push(member_path.clone(), "stack_kind_mismatch", "workpiece kind is incompatible with stack kind");
                }
                if wp.orientation != stack.orientation || wp.registration_edge != stack.registration_edge {
                    errors.push(member_path.clone(), "registration_mismatch", "stack and workpiece orientation/registration must match");
                }
                if let Some(a) = first {
                    if (wp.width, wp.height, wp.thickness, wp.material_id.as_str(), wp.grain_axis)
                        != (a.width, a.height, a.thickness, a.material_id.as_str(), a.grain_axis)
                    {
                        errors.push(member_path, "incompatible_stack_member", "stack members must have equal dimensions, thickness, material, and grain axis");
                    }
                } else {
                    first = Some(wp);
                }
            } else {
                errors.push(member_path, "unknown_reference", format!("unknown physical workpiece '{member_id}'"));
            }
        }
        if total_height > u64::from(machine.max_stack_height) {
            errors.push(format!("{path}.member_ids"), "stack_height_exceeded", format!("package height {total_height} exceeds machine maximum {}", machine.max_stack_height));
        }
    }

    if !errors.0.is_empty() {
        return Err(errors.0);
    }

    Ok(ModelSummary {
        material_types: model.materials.len(),
        part_types: model.parts.len(),
        requested_pieces,
        stock_types: model.stocks.len(),
        available_boards,
        pattern_templates: model.patterns.len(),
        tracked_workpieces: model.workpieces.len(),
        stacks: model.stacks.len(),
        stacked_workpieces,
        geometry_validated: false,
        production_validated: false,
    })
}
