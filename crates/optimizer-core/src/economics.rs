//! Stage 16: auditable monetary estimates for an independently validated
//! guillotine layout. Costs are integer *minor currency units* (PEN cents,
//! USD cents, etc.) and area uses integer square tenths of millimetres.
//!
//! This module quotes two possible ways to process the SAME geometry:
//! separately or in identical full-board stacks. It does NOT certify machine
//! time, cut order, strip stacking or any vendor-specific machine program.

use crate::full_board_stacking::{plan_full_board_stacks, FullBoardStackPlan, StackCostModel};
use crate::guillotine::evaluate_pattern;
use crate::layout_validation::{validate_layout, LayoutPlan};
use crate::model::CuttingJobModel;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

fn yes() -> bool { true }
fn default_saw() -> u64 { 100 }
fn default_stack() -> u64 { 40 }
fn default_unstack() -> u64 { 30 }
fn default_board_handling() -> u64 { 15 }
fn default_rotate() -> u64 { 15 }
fn default_move() -> u64 { 20 }
fn default_credit_bps() -> u32 { 2_500 }
fn default_credit_cap_bps() -> u32 { 7_500 }
fn default_min_width() -> u32 { 300 }
fn default_min_height() -> u32 { 300 }
fn default_min_area() -> u64 { 90_000 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicObjective {
    #[default]
    MinimumTotalCost,
    MinimumMaterialCost,
    MinimumProductionCost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicRates {
    #[serde(default = "default_saw")]
    pub saw_pass_minor: u64,
    #[serde(default = "default_stack")]
    pub stack_setup_minor: u64,
    #[serde(default = "default_unstack")]
    pub stack_release_minor: u64,
    #[serde(default = "default_board_handling")]
    pub board_handling_minor: u64,
    #[serde(default = "default_rotate")]
    pub rotation_minor: u64,
    #[serde(default = "default_move")]
    pub movement_minor: u64,
    #[serde(default)]
    pub job_setup_minor: u64,
}
impl Default for EconomicRates {
    fn default() -> Self {
        Self { saw_pass_minor: default_saw(), stack_setup_minor: default_stack(),
            stack_release_minor: default_unstack(), board_handling_minor: default_board_handling(),
            rotation_minor: default_rotate(), movement_minor: default_move(), job_setup_minor: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    /// Recovery requires the guillotine tree residual to be explicitly reusable.
    #[serde(default = "default_min_width")]
    pub min_width: u32,
    #[serde(default = "default_min_height")]
    pub min_height: u32,
    /// In squared 0.1 mm units: 90_000 = 900 mm².
    #[serde(default = "default_min_area")]
    pub min_area_sq_units: u64,
    /// Fraction of the parent board unit cost allocated to eligible leftovers.
    #[serde(default = "default_credit_bps")]
    pub recovery_rate_bps: u32,
    /// Upper limit on the recovery credit from any single consumed board.
    #[serde(default = "default_credit_cap_bps")]
    pub max_board_credit_bps: u32,
}
impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self { min_width: default_min_width(), min_height: default_min_height(),
            min_area_sq_units: default_min_area(), recovery_rate_bps: default_credit_bps(),
            max_board_credit_bps: default_credit_cap_bps() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraHandling {
    /// Estimated or measured additional operations, not inferred from geometry.
    #[serde(default)]
    pub rotations: u64,
    #[serde(default)]
    pub movements: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicOptions {
    #[serde(default)]
    pub objective: EconomicObjective,
    #[serde(default)]
    pub rates: EconomicRates,
    #[serde(default)]
    pub recovery: RecoveryPolicy,
    #[serde(default)]
    pub additional_handling: ExtraHandling,
    #[serde(default = "yes")]
    pub compare_full_board_stacking: bool,
    /// A partial layout is an estimate for ONLY the boards already assigned;
    /// it must never be passed off as a quote for the full order.
    #[serde(default)]
    pub allow_partial_order: bool,
}
impl Default for EconomicOptions {
    fn default() -> Self {
        Self { objective: EconomicObjective::default(), rates: EconomicRates::default(),
            recovery: RecoveryPolicy::default(), additional_handling: ExtraHandling::default(),
            compare_full_board_stacking: true, allow_partial_order: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EconomicError {
    InvalidSettings(String),
    InvalidLayout(Vec<String>),
    Evaluation(String),
    Overflow(String),
}
impl fmt::Display for EconomicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(s) => write!(f, "invalid economic parameters: {s}"),
            Self::InvalidLayout(s) => write!(f, "cannot price invalid geometry: {}", s.join("; ")),
            Self::Evaluation(s) => write!(f, "cannot evaluate economics: {s}"),
            Self::Overflow(s) => write!(f, "economic arithmetic overflow: {s}"),
        }
    }
}
impl std::error::Error for EconomicError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SalvageLine {
    pub board_id: String,
    pub stock_id: String,
    pub node_id: String,
    pub width: u32,
    pub height: u32,
    pub eligible_area_sq_units: String,
    /// Floor rounding; board-level cap allocated in deterministic node order.
    pub credit_minor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicAreaSummary {
    pub gross_stock_area_sq_units: String,
    pub trimmed_area_sq_units: String,
    pub usable_area_sq_units: String,
    pub pieces_area_sq_units: String,
    pub kerf_area_sq_units: String,
    pub residual_area_sq_units: String,
    pub recoverable_residual_area_sq_units: String,
    pub other_residual_area_sq_units: String,
    /// In basis points (10000 = 100%), of usable sheet area.
    pub utilization_bps: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotedMode { SeparateBoards, IdenticalBoardStacks }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicQuote {
    pub mode: QuotedMode,
    pub board_count: usize,
    pub estimated_saw_passes: u64,
    pub stack_setups: usize,
    pub stack_releases: usize,
    pub additional_rotations: u64,
    pub additional_movements: u64,
    pub material_cost_minor: String,
    pub saw_cost_minor: String,
    pub stack_cost_minor: String,
    pub handling_cost_minor: String,
    pub setup_cost_minor: String,
    pub production_cost_minor: String,
    pub recoverable_credit_minor: String,
    pub net_estimated_cost_minor: String,
    pub physically_certified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicReport {
    pub objective: EconomicObjective,
    pub layout_geometry_validated: bool,
    pub order_complete: bool,
    pub remaining_part_counts: BTreeMap<String, u64>,
    pub area: EconomicAreaSummary,
    pub salvage: Vec<SalvageLine>,
    pub separate_boards: EconomicQuote,
    pub full_board_stacks: Option<EconomicQuote>,
    pub proposed_full_board_stacks: usize,
    pub selected_mode: QuotedMode,
    pub selected_net_cost_minor: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicComparison {
    pub objective: EconomicObjective,
    /// Index is zero-based, and preserves the original input order.
    pub selected_layout_index: usize,
    pub reports: Vec<EconomicReport>,
    pub note: String,
}

fn selected_quote(r: &EconomicReport) -> &EconomicQuote {
    if r.selected_mode == QuotedMode::IdenticalBoardStacks {
        r.full_board_stacks.as_ref().unwrap_or(&r.separate_boards)
    } else { &r.separate_boards }
}

/// Compare separately validated layouts for the SAME job and inventory.
/// Completing more pieces always outranks a cheaper partial layout.
pub fn compare_economic_layouts(
    model: &CuttingJobModel, layouts: &[LayoutPlan], options: EconomicOptions,
) -> Result<EconomicComparison, EconomicError> {
    if layouts.is_empty() || layouts.len() > 32 {
        return Err(EconomicError::InvalidSettings("number of candidate layouts must be 1..32".into()));
    }
    let mut reports = Vec::with_capacity(layouts.len());
    let mut best = 0usize;
    let mut best_key: Option<(u64, u128, u128, u128)> = None;
    for (index, layout) in layouts.iter().enumerate() {
        let report = evaluate_economics(model, layout, options)?;
        let missing: u64 = report.remaining_part_counts.values().copied().sum();
        let ranked = ranking(selected_quote(&report), options.objective)?;
        let key = (missing, ranked.0, ranked.1, ranked.2);
        if best_key.map_or(true, |current| key < current) {
            best_key = Some(key);
            best = index;
        }
        reports.push(report);
    }
    Ok(EconomicComparison {
        objective: options.objective, selected_layout_index: best, reports,
        note: "Each layout priced independently. Geometry and quantities verified; no machine-specific production sequence or actual time certification.".into(),
    })
}

pub fn validate_economic_options(o: &EconomicOptions) -> Result<(), EconomicError> {
    if o.recovery.recovery_rate_bps > 10_000 || o.recovery.max_board_credit_bps > 10_000 {
        return Err(EconomicError::InvalidSettings("recovery basis points must be 0..10000".into()));
    }
    if o.recovery.min_width == 0 || o.recovery.min_height == 0 || o.recovery.min_area_sq_units == 0 {
        return Err(EconomicError::InvalidSettings("minimum reusable dimensions and area must be positive".into()));
    }
    if o.additional_handling.rotations > 1_000_000 || o.additional_handling.movements > 1_000_000 {
        return Err(EconomicError::InvalidSettings("additional handling counts exceed 1000000".into()));
    }
    Ok(())
}

fn add(a: u128, b: u128, label: &str) -> Result<u128, EconomicError> {
    a.checked_add(b).ok_or_else(|| EconomicError::Overflow(label.into()))
}
fn mul(a: u128, b: u128, label: &str) -> Result<u128, EconomicError> {
    a.checked_mul(b).ok_or_else(|| EconomicError::Overflow(label.into()))
}
fn amount(s: &str) -> Result<u128, EconomicError> {
    s.parse::<u128>().map_err(|_| EconomicError::Evaluation(format!("invalid monetary or area number: {s}")))
}

fn quote(
    mode: QuotedMode, board_count: usize, saw_passes: u64, stacks: usize,
    material: u128, credit: u128, o: &EconomicOptions,
) -> Result<EconomicQuote, EconomicError> {
    let rates = o.rates;
    let saw = mul(u128::from(saw_passes), u128::from(rates.saw_pass_minor), "saw cost")?;
    let stack_unit = add(u128::from(rates.stack_setup_minor),
        u128::from(rates.stack_release_minor), "stack rate")?;
    let stacking = mul(stacks as u128, stack_unit, "stacking cost")?;
    let boards_handled = mul(board_count as u128, u128::from(rates.board_handling_minor), "board handling")?;
    let rotations = mul(u128::from(o.additional_handling.rotations), u128::from(rates.rotation_minor), "rotation")?;
    let movements = mul(u128::from(o.additional_handling.movements), u128::from(rates.movement_minor), "movement")?;
    let handling = add(add(boards_handled, rotations, "handling")?, movements, "handling")?;
    let operation = add(add(saw, stacking, "production")?, handling, "production")?;
    let operation = add(operation, u128::from(rates.job_setup_minor), "production")?;
    let net = add(material, operation, "gross estimate")?
        .checked_sub(credit).ok_or_else(|| EconomicError::Overflow("recovery exceeds material".into()))?;
    Ok(EconomicQuote {
        mode, board_count, estimated_saw_passes: saw_passes,
        stack_setups: stacks, stack_releases: stacks,
        additional_rotations: o.additional_handling.rotations,
        additional_movements: o.additional_handling.movements,
        material_cost_minor: material.to_string(), saw_cost_minor: saw.to_string(),
        stack_cost_minor: stacking.to_string(), handling_cost_minor: handling.to_string(),
        setup_cost_minor: rates.job_setup_minor.to_string(), production_cost_minor: operation.to_string(),
        recoverable_credit_minor: credit.to_string(), net_estimated_cost_minor: net.to_string(),
        physically_certified: false,
    })
}

fn ranking(quote: &EconomicQuote, objective: EconomicObjective) -> Result<(u128, u128, u128), EconomicError> {
    let net = amount(&quote.net_estimated_cost_minor)?;
    let material = amount(&quote.material_cost_minor)?.saturating_sub(amount(&quote.recoverable_credit_minor)?);
    let work = amount(&quote.production_cost_minor)?;
    Ok(match objective {
        EconomicObjective::MinimumTotalCost => (net, material, work),
        EconomicObjective::MinimumMaterialCost => (material, net, work),
        EconomicObjective::MinimumProductionCost => (work, net, material),
    })
}

/// Geometry, stock uniqueness, quantities and costs all come from the validated
/// input; no assignment is implied by existing stock *availability* alone.
/// Result is a quote for the layout, not a vendor-certified manufacturing plan.
pub fn evaluate_economics(
    model: &CuttingJobModel, layout: &LayoutPlan, options: EconomicOptions,
) -> Result<EconomicReport, EconomicError> {
    validate_economic_options(&options)?;
    let validation = validate_layout(model, layout);
    let other_issues: Vec<String> = validation.issues.iter()
        .filter(|x| !options.allow_partial_order || x.code != "missing_pieces")
        .map(|x| format!("{} [{}]", x.path, x.code)).collect();
    if !other_issues.is_empty() || layout.boards.is_empty()
        || validation.summary.boards_geometry_verified != layout.boards.len()
        || !validation.summary.count_reconciliation_complete {
        return Err(EconomicError::InvalidLayout(other_issues));
    }
    let mut gross_area = 0u128;
    let mut usable_area = 0u128;
    let mut placed_area = 0u128;
    let mut kerf_area = 0u128;
    let mut residual_area = 0u128;
    let mut salvage_area = 0u128;
    let mut price = 0u128;
    let mut credited = 0u128;
    let mut passes = 0u64;
    let mut salvage = Vec::new();
    for board in &layout.boards {
        let stock = model.stocks.iter().find(|s| s.id == board.stock_id)
            .ok_or_else(|| EconomicError::Evaluation(format!("missing stock {}", board.stock_id)))?;
        let evaluated = evaluate_pattern(model, &board.pattern_id, board.board_rotated_90)
            .map_err(|e| EconomicError::Evaluation(e.to_string()))?;
        let stock_area = u128::from(stock.width) * u128::from(stock.height);
        let unit_usable = amount(&evaluated.summary.usable_area_sq_units)?;
        gross_area = add(gross_area, stock_area, "stock area")?;
        usable_area = add(usable_area, unit_usable, "usable area")?;
        placed_area = add(placed_area, amount(&evaluated.summary.piece_area_sq_units)?, "piece area")?;
        kerf_area = add(kerf_area, amount(&evaluated.summary.kerf_area_sq_units)?, "kerf area")?;
        residual_area = add(residual_area, amount(&evaluated.summary.residual_area_sq_units)?, "residual area")?;
        price = add(price, u128::from(stock.unit_cost_minor), "material cost")?;
        passes = passes.checked_add(evaluated.summary.cuts as u64)
            .ok_or_else(|| EconomicError::Overflow("geometric saw passes".into()))?;
        let max_credit = mul(u128::from(stock.unit_cost_minor),
            u128::from(options.recovery.max_board_credit_bps), "board credit cap")? / 10_000;
        let mut board_credit = 0u128;
        for residual in &evaluated.residuals {
            let area = residual.rect.area_sq_units();
            if !residual.reusable || residual.rect.width < options.recovery.min_width
                || residual.rect.height < options.recovery.min_height
                || area < u128::from(options.recovery.min_area_sq_units) { continue; }
            salvage_area = add(salvage_area, area, "reusable area")?;
            // Divide only after multiplication to avoid premature rounding.
            let weighted = mul(mul(u128::from(stock.unit_cost_minor), area, "recovery")?,
                u128::from(options.recovery.recovery_rate_bps), "recovery")?;
            let nominal = weighted / mul(stock_area, 10_000, "recovery denominator")?;
            let remaining_credit = max_credit.saturating_sub(board_credit);
            let credit = nominal.min(remaining_credit);
            board_credit = add(board_credit, credit, "board recovery")?;
            salvage.push(SalvageLine {
                board_id: board.board_id.clone(), stock_id: board.stock_id.clone(),
                node_id: residual.node_id.clone(), width: residual.rect.width,
                height: residual.rect.height, eligible_area_sq_units: area.to_string(),
                credit_minor: credit.to_string(),
            });
        }
        credited = add(credited, board_credit, "recovery credit")?;
    }
    let area = EconomicAreaSummary {
        gross_stock_area_sq_units: gross_area.to_string(),
        trimmed_area_sq_units: gross_area.checked_sub(usable_area)
            .ok_or_else(|| EconomicError::Overflow("trim area".into()))?.to_string(),
        usable_area_sq_units: usable_area.to_string(), pieces_area_sq_units: placed_area.to_string(),
        kerf_area_sq_units: kerf_area.to_string(), residual_area_sq_units: residual_area.to_string(),
        recoverable_residual_area_sq_units: salvage_area.to_string(),
        other_residual_area_sq_units: residual_area.checked_sub(salvage_area)
            .ok_or_else(|| EconomicError::Overflow("residual area".into()))?.to_string(),
        utilization_bps: (mul(placed_area, 10_000, "utilization")? / usable_area)
            .try_into().map_err(|_| EconomicError::Overflow("utilization".into()))?,
    };
    let separate_boards = quote(QuotedMode::SeparateBoards, layout.boards.len(), passes,
        0, price, credited, &options)?;
    let (full_board_stacks, proposed_full_board_stacks) = if options.compare_full_board_stacking {
        let stack_plan: FullBoardStackPlan = plan_full_board_stacks(model, layout, StackCostModel {
            cut_cost_units: options.rates.saw_pass_minor,
            stack_setup_cost_units: options.rates.stack_setup_minor,
            stack_release_cost_units: options.rates.stack_release_minor,
        }).map_err(|e| EconomicError::Evaluation(e.to_string()))?;
        let quote = quote(QuotedMode::IdenticalBoardStacks, layout.boards.len(),
            stack_plan.estimated_saw_passes_with_stacking, stack_plan.stacks.len(),
            price, credited, &options)?;
        (Some(quote), stack_plan.stacks.len())
    } else { (None, 0) };
    let selected = match &full_board_stacks {
        Some(stacked) if ranking(stacked, options.objective)? < ranking(&separate_boards, options.objective)? => stacked,
        _ => &separate_boards,
    };
    let selected_mode = selected.mode.clone();
    let selected_net_cost_minor = selected.net_estimated_cost_minor.clone();
    let order_complete = validation.valid;
    Ok(EconomicReport {
        objective: options.objective,
        layout_geometry_validated: true,
        order_complete,
        remaining_part_counts: validation.summary.missing_per_part,
        area, salvage, separate_boards,
        full_board_stacks, proposed_full_board_stacks,
        selected_mode,
        selected_net_cost_minor,
        note: if order_complete {
            "Estimated minor currency units; identical full-board stacks only. Recoverable remnants are credits, not sales. No strip-stack scheduling, measured times, vendor machine certification or physical plan equivalence is claimed.".into()
        } else {
            "PARTIAL ORDER ONLY: costs and leftovers represent assigned boards, not a complete-order quote. No physical machine sequence is certified.".into()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{JobOperation, JobRequest};
    use crate::model::PatternNode;

    fn sample() -> (CuttingJobModel, LayoutPlan) {
        let req: JobRequest = serde_json::from_str(include_str!("../../../examples/layout-valid.json")).unwrap();
        match req.operation {
            JobOperation::ValidateLayout { model, layout } => (model, layout),
            _ => panic!("invalid test fixture"),
        }
    }

    fn integer(s: &str) -> u128 { s.parse::<u128>().unwrap() }

    #[test]
    fn quotes_real_inventory_cost_once() {
        let (model, layout) = sample();
        let r = evaluate_economics(&model, &layout, EconomicOptions::default()).unwrap();
        assert!(r.order_complete);
        assert!(r.layout_geometry_validated);
        assert_eq!(integer(&r.separate_boards.material_cost_minor), 10_000);
        assert_eq!(r.separate_boards.estimated_saw_passes, 4);
        assert_eq!(r.proposed_full_board_stacks, 1);
        let stack = r.full_board_stacks.as_ref().unwrap();
        assert_eq!(stack.estimated_saw_passes, 2);
        assert_eq!(stack.stack_setups, 1);
        assert_eq!(stack.stack_releases, 1);
        assert!(!stack.physically_certified);
        assert_eq!(r.selected_mode, QuotedMode::IdenticalBoardStacks);
    }

    #[test]
    fn recoverable_credit_is_capped_at_per_board_limit() {
        let (model, layout) = sample();
        let mut o = EconomicOptions::default();
        o.recovery.recovery_rate_bps = 10_000;
        o.recovery.max_board_credit_bps = 1_000;
        let r = evaluate_economics(&model, &layout, o).unwrap();
        assert!(integer(&r.separate_boards.recoverable_credit_minor) <= 1_000);
        assert_eq!(r.area.gross_stock_area_sq_units, "1600000");
        assert_eq!(integer(&r.area.usable_area_sq_units)
            + integer(&r.area.trimmed_area_sq_units),
            integer(&r.area.gross_stock_area_sq_units));
        assert_eq!(integer(&r.area.pieces_area_sq_units)
            + integer(&r.area.residual_area_sq_units)
            + integer(&r.area.kerf_area_sq_units),
            integer(&r.area.usable_area_sq_units));
    }

    #[test]
    fn net_equals_material_plus_production_minus_recovery() {
        let (model, layout) = sample();
        let r = evaluate_economics(&model, &layout, EconomicOptions::default()).unwrap();
        for q in [&r.separate_boards, r.full_board_stacks.as_ref().unwrap()] {
            assert_eq!(integer(&q.net_estimated_cost_minor),
                integer(&q.material_cost_minor)
                    + integer(&q.production_cost_minor)
                    - integer(&q.recoverable_credit_minor));
        }
    }

    #[test]
    fn ineligible_residuals_receive_no_credit() {
        let (mut model, layout) = sample();
        fn disable(n: &mut PatternNode) {
            match n {
                PatternNode::Residual { reusable, .. } => *reusable = false,
                PatternNode::Split { first, second, .. } => { disable(first); disable(second); }
                PatternNode::Piece { .. } => {}
            }
        }
        disable(&mut model.patterns[0].root);
        let r = evaluate_economics(&model, &layout, EconomicOptions::default()).unwrap();
        assert!(r.salvage.is_empty());
        assert_eq!(r.separate_boards.recoverable_credit_minor, "0");
        assert_eq!(r.area.recoverable_residual_area_sq_units, "0");
    }

    #[test]
    fn too_small_residual_does_not_receive_credit() {
        let (model, layout) = sample();
        let mut o = EconomicOptions::default();
        o.recovery.min_width = 999_999;
        let r = evaluate_economics(&model, &layout, o).unwrap();
        assert!(r.salvage.is_empty());
        assert_eq!(r.separate_boards.recoverable_credit_minor, "0");
    }

    #[test]
    fn rejects_duplicate_physical_sheet() {
        let (model, mut layout) = sample();
        layout.boards[1].stock_unit_index = 0;
        assert!(matches!(evaluate_economics(&model, &layout, EconomicOptions::default()),
            Err(EconomicError::InvalidLayout(_))));
    }

    #[test]
    fn partial_order_requires_explicit_opt_in() {
        let (model, mut layout) = sample();
        layout.boards.truncate(1);
        assert!(matches!(evaluate_economics(&model, &layout, EconomicOptions::default()),
            Err(EconomicError::InvalidLayout(_))));
        let mut o = EconomicOptions::default();
        o.allow_partial_order = true;
        let r = evaluate_economics(&model, &layout, o).unwrap();
        assert!(!r.order_complete);
        assert_eq!(r.remaining_part_counts.get("A"), Some(&1));
        assert_eq!(r.separate_boards.material_cost_minor, "5000");
    }

    #[test]
    fn handles_all_zero_costs_without_fake_discount() {
        let (mut model, layout) = sample();
        model.stocks[0].unit_cost_minor = 0;
        let mut o = EconomicOptions::default();
        o.rates = EconomicRates { saw_pass_minor: 0, stack_setup_minor: 0,
            stack_release_minor: 0, board_handling_minor: 0, rotation_minor: 0,
            movement_minor: 0, job_setup_minor: 0 };
        let r = evaluate_economics(&model, &layout, o).unwrap();
        assert_eq!(r.selected_net_cost_minor, "0");
        assert_eq!(r.separate_boards.recoverable_credit_minor, "0");
    }

    #[test]
    fn can_disable_stacking_comparison() {
        let (model, layout) = sample();
        let mut o = EconomicOptions::default();
        o.compare_full_board_stacking = false;
        let r = evaluate_economics(&model, &layout, o).unwrap();
        assert!(r.full_board_stacks.is_none());
        assert_eq!(r.selected_mode, QuotedMode::SeparateBoards);
    }

    #[test]
    fn rejects_invalid_recovery_basis_points() {
        let (model, layout) = sample();
        let mut o = EconomicOptions::default();
        o.recovery.recovery_rate_bps = 10_001;
        assert!(matches!(evaluate_economics(&model, &layout, o),
            Err(EconomicError::InvalidSettings(_))));
    }

    #[test]
    fn extra_handling_is_charged_once_per_quote() {
        let (model, layout) = sample();
        let standard = evaluate_economics(&model, &layout, EconomicOptions::default()).unwrap();
        let mut o = EconomicOptions::default();
        o.additional_handling = ExtraHandling { rotations: 2, movements: 3 };
        let adjusted = evaluate_economics(&model, &layout, o).unwrap();
        assert_eq!(integer(&adjusted.separate_boards.net_estimated_cost_minor)
            - integer(&standard.separate_boards.net_estimated_cost_minor), 2 * 15 + 3 * 20);
    }

    #[test]
    fn stack_calculation_obeys_machine_capability() {
        let (mut model, layout) = sample();
        model.machine.stacking.full_boards = false;
        let r = evaluate_economics(&model, &layout, EconomicOptions::default()).unwrap();
        assert_eq!(r.proposed_full_board_stacks, 0);
        assert_eq!(r.full_board_stacks.unwrap().stack_setups, 0);
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::*;
    use crate::contract::{JobOperation, JobRequest};

    #[test]
    fn comparison_prefers_lower_material_cost_for_same_pieces() {
        let request: JobRequest = serde_json::from_str(include_str!("../../../examples/compare-economic-layouts.json")).unwrap();
        let JobOperation::CompareEconomicLayouts { model, layouts, economic_options } = request.operation else { panic!("expected comparison fixture") };
        let result = compare_economic_layouts(&model, &layouts, economic_options).unwrap();
        assert_eq!(result.reports.len(), 2);
        assert_eq!(result.selected_layout_index, 1);
        assert_eq!(result.reports[1].separate_boards.material_cost_minor, "10000");
        assert_eq!(result.reports[0].separate_boards.material_cost_minor, "15000");
    }

    #[test]
    fn comparison_cannot_choose_partial_cheaper_layout_over_complete() {
        let request: JobRequest = serde_json::from_str(include_str!("../../../examples/compare-economic-layouts.json")).unwrap();
        let JobOperation::CompareEconomicLayouts { model, mut layouts, mut economic_options } = request.operation else { panic!("expected comparison fixture") };
        layouts[0].boards.truncate(1);
        economic_options.allow_partial_order = true;
        let result = compare_economic_layouts(&model, &layouts, economic_options).unwrap();
        assert_eq!(result.selected_layout_index, 1);
        assert!(result.reports[1].order_complete);
    }

    #[test]
    fn comparison_rejects_empty_list() {
        let request: JobRequest = serde_json::from_str(include_str!("../../../examples/compare-economic-layouts.json")).unwrap();
        let JobOperation::CompareEconomicLayouts { model, economic_options, .. } = request.operation else { panic!("expected comparison fixture") };
        assert!(matches!(compare_economic_layouts(&model, &[], economic_options),
            Err(EconomicError::InvalidSettings(_))));
    }
}
