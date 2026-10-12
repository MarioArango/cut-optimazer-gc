//! Stage 3: deterministic, machine-aware preprocessing only.
//!
//! Performs early *individual-piece dimensional eligibility*, not cutting
//! feasibility, quantity allocation, or production-stack planning.
//! All lengths are integer tenths of a millimeter, as in `model`.

use crate::model::{
    validate_model, Axis, CuttingJobModel, ModelIssue, Part, Stock, StockKind,
    TrimMargins, Unit,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// Intentional resource budgets for JSON output and pairwise candidate checks.
/// These are *preprocessing* limits and not cutting-engine limitations.
pub const MAX_EXPANDED_PIECES: u64 = 100_000;
pub const MAX_FIT_CHECKS: u64 = 2_000_000;
pub const MAX_FIT_OPTIONS: u64 = 500_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreprocessError {
    InvalidModel(Vec<ModelIssue>),
    ResourceLimit { resource: &'static str, max: u64, actual: u64 },
}

impl fmt::Display for PreprocessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid cutting model ({} issue(s))", issues.len()),
            Self::ResourceLimit { resource, max, actual } => {
                write!(f, "preprocessing resource limit '{resource}': maximum {max}, received {actual}")
            }
        }
    }
}

impl std::error::Error for PreprocessError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eligibility {
    Eligible,
    Ineligible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IneligibilityCode {
    NoStockForMaterial,
    BelowMachineMinimum,
    GrainConflict,
    DoesNotFitAnyStock,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EligibilityIssue {
    pub code: IneligibilityCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedStockOrientation {
    /// False: source board layout; true: clockwise 90-degree rotation.
    pub rotated_90: bool,
    pub width: Unit,
    pub height: Unit,
    /// Origin of the usable rectangle in this board orientation.
    pub usable_x: Unit,
    pub usable_y: Unit,
    pub usable_width: Unit,
    pub usable_height: Unit,
    /// Material grain direction in the current board coordinates.
    pub grain_axis: Option<Axis>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedStock {
    pub stock_id: String,
    pub material_id: String,
    pub kind: StockKind,
    pub quantity: u32,
    pub unit_cost_minor: u64,
    pub orientations: Vec<PreparedStockOrientation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitOption {
    pub stock_id: String,
    pub stock_rotated_90: bool,
    pub part_rotated_90: bool,
    /// Placed piece dimensions in the selected board coordinates.
    pub width: Unit,
    pub height: Unit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedPart {
    pub part_id: String,
    pub material_id: String,
    pub requested_quantity: u32,
    pub eligibility: Eligibility,
    pub issue: Option<EligibilityIssue>,
    /// Individual-piece fit opportunities; does NOT allocate stock quantity.
    pub fit_options: Vec<FitOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartInstance {
    pub instance_id: String,
    pub part_id: String,
    pub material_id: String,
    pub ordinal: u32,
    pub width: Unit,
    pub height: Unit,
    pub priority: u8,
    pub eligible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGroup {
    pub material_id: String,
    pub part_ids: Vec<String>,
    pub stock_ids: Vec<String>,
    pub requested_pieces: u64,
    pub available_stock_units: u64,
    /// Exact decimal strings avoid loss of precision for area aggregates in JS.
    /// Unit is (0.1 mm)^2; 100 square units = 1 mm^2.
    pub requested_area_sq_units: String,
    pub available_usable_area_sq_units: String,
    pub aggregate_area_sufficient: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreprocessWarning {
    pub code: String,
    pub material_id: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreprocessSummary {
    pub material_groups: usize,
    pub part_types: usize,
    pub expanded_pieces: usize,
    pub eligible_part_types: usize,
    pub ineligible_part_types: usize,
    pub eligible_instances: usize,
    pub stock_types: usize,
    pub available_stock_units: u64,
    pub stock_orientations: usize,
    pub fit_options: usize,
    pub all_parts_individually_fit: bool,
    pub aggregate_area_sufficient: bool,
    /// Still false: no cutter, stages, kerf, allocation or stack validation.
    pub geometry_validated: bool,
    pub production_validated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreprocessedJob {
    pub summary: PreprocessSummary,
    pub groups: Vec<MaterialGroup>,
    pub stocks: Vec<PreparedStock>,
    pub parts: Vec<PreparedPart>,
    pub instances: Vec<PartInstance>,
    pub warnings: Vec<PreprocessWarning>,
}

fn rotate_axis(axis: Option<Axis>, rotated: bool) -> Option<Axis> {
    if rotated {
        axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
    } else {
        axis
    }
}

fn rotate_trim(t: TrimMargins) -> TrimMargins {
    // Clockwise 90 degrees: old left -> new top; old top -> new right;
    // old right -> new bottom; old bottom -> new left.
    TrimMargins {
        top: t.left,
        right: t.top,
        bottom: t.right,
        left: t.bottom,
    }
}

fn stock_orientation(stock: &Stock, rotated: bool) -> PreparedStockOrientation {
    let (width, height, trim) = if rotated {
        (stock.height, stock.width, rotate_trim(stock.trim))
    } else {
        (stock.width, stock.height, stock.trim)
    };
    // Stage 2 validation ensures positive remainder after margins.
    PreparedStockOrientation {
        rotated_90: rotated,
        width,
        height,
        usable_x: trim.left,
        usable_y: trim.top,
        usable_width: width - trim.left - trim.right,
        usable_height: height - trim.top - trim.bottom,
        grain_axis: rotate_axis(stock.grain_axis, rotated),
    }
}

fn rotated_dimensions(p: &Part, rotated: bool) -> (Unit, Unit) {
    if rotated { (p.height, p.width) } else { (p.width, p.height) }
}

fn is_part_machine_admissible(p: &Part, min_width: Unit, min_height: Unit) -> bool {
    let unrotated_ok = p.width >= min_width && p.height >= min_height;
    let rotated_ok = p.allow_rotate_90 && p.height >= min_width && p.width >= min_height;
    unrotated_ok || rotated_ok
}

/// Prepared data includes *both eligible and ineligible* requested pieces.
/// An ineligible part is not silently dropped; subsequent stages must refuse
/// to declare a completed job until all requested quantities are accounted for.
pub fn preprocess_model(model: &CuttingJobModel) -> Result<PreprocessedJob, PreprocessError> {
    validate_model(model).map_err(PreprocessError::InvalidModel)?;

    let total_instances: u64 = model.parts.iter().map(|p| u64::from(p.quantity)).sum();
    if total_instances > MAX_EXPANDED_PIECES {
        return Err(PreprocessError::ResourceLimit {
            resource: "expanded_pieces", max: MAX_EXPANDED_PIECES, actual: total_instances,
        });
    }

    let mut prepared_stock = Vec::with_capacity(model.stocks.len());
    let mut stock_by_material: HashMap<&str, Vec<usize>> = HashMap::new();
    for s in &model.stocks {
        let index = prepared_stock.len();
        let mut orientations = vec![stock_orientation(s, false)];
        if model.machine.allow_board_rotation {
            orientations.push(stock_orientation(s, true));
        }
        stock_by_material.entry(s.material_id.as_str()).or_default().push(index);
        prepared_stock.push(PreparedStock {
            stock_id: s.id.clone(), material_id: s.material_id.clone(),
            kind: s.kind, quantity: s.quantity, unit_cost_minor: s.unit_cost_minor,
            orientations,
        });
    }

    let mut material_groups: BTreeMap<String, MaterialGroupBuilder> = BTreeMap::new();
    for m in &model.materials {
        material_groups.insert(m.id.clone(), MaterialGroupBuilder::default());
    }
    for (s, prepared) in model.stocks.iter().zip(&prepared_stock) {
        let group = material_groups.get_mut(&s.material_id).expect("validated material reference");
        group.stock_ids.push(s.id.clone());
        group.available_stock_units += u64::from(s.quantity);
        let shape = &prepared.orientations[0];
        group.available_area += u128::from(shape.usable_width)
            * u128::from(shape.usable_height) * u128::from(s.quantity);
    }

    let mut result_parts = Vec::with_capacity(model.parts.len());
    let mut instances = Vec::with_capacity(total_instances as usize);
    let mut checks = 0u64;
    let mut option_count = 0u64;
    let mut eligible_instances = 0usize;
    let mut eligible_types = 0usize;

    for p in &model.parts {
        let group = material_groups.get_mut(&p.material_id).expect("validated material reference");
        group.part_ids.push(p.id.clone());
        group.requested_pieces += u64::from(p.quantity);
        group.requested_area += u128::from(p.width)
            * u128::from(p.height) * u128::from(p.quantity);

        let available = stock_by_material.get(p.material_id.as_str());
        let min_ok = is_part_machine_admissible(
            p, model.machine.min_piece_width, model.machine.min_piece_height,
        );
        let mut fits = Vec::new();
        let mut size_fits_some_stock = false;
        let mut grain_rejects_size_fit = false;

        // Check all allowed stock / part rotations; every match is a candidate
        // for subsequent layout stages, never a reservation of that inventory.
        if min_ok {
            if let Some(indices) = available {
                for &idx in indices {
                    let prepared = &prepared_stock[idx];
                    for s in &prepared.orientations {
                        for part_rotated in [false, true] {
                            if part_rotated && !p.allow_rotate_90 { continue; }
                            checks += 1;
                            if checks > MAX_FIT_CHECKS {
                                return Err(PreprocessError::ResourceLimit {
                                    resource: "fit_checks", max: MAX_FIT_CHECKS, actual: checks,
                                });
                            }
                            let (w, h) = rotated_dimensions(p, part_rotated);
                            if w < model.machine.min_piece_width || h < model.machine.min_piece_height {
                                continue;
                            }
                            if w <= s.usable_width && h <= s.usable_height {
                                size_fits_some_stock = true;
                                if p.grain_axis.is_none() || rotate_axis(p.grain_axis, part_rotated) == s.grain_axis {
                                    option_count += 1;
                                    if option_count > MAX_FIT_OPTIONS {
                                        return Err(PreprocessError::ResourceLimit {
                                            resource: "fit_options", max: MAX_FIT_OPTIONS, actual: option_count,
                                        });
                                    }
                                    fits.push(FitOption {
                                        stock_id: prepared.stock_id.clone(),
                                        stock_rotated_90: s.rotated_90,
                                        part_rotated_90: part_rotated,
                                        width: w, height: h,
                                    });
                                } else {
                                    grain_rejects_size_fit = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        let eligible = !fits.is_empty();
        if eligible {
            eligible_types += 1;
            eligible_instances += p.quantity as usize;
        }
        let issue = if eligible { None } else {
            let (code, message) = if available.is_none() {
                (IneligibilityCode::NoStockForMaterial, "No stock of the required material".to_owned())
            } else if !min_ok {
                (IneligibilityCode::BelowMachineMinimum, "Part dimensions cannot satisfy machine minimums with permitted part rotations".to_owned())
            } else if size_fits_some_stock && grain_rejects_size_fit {
                (IneligibilityCode::GrainConflict, "Only dimensionally fitting orientations violate the material grain direction".to_owned())
            } else {
                (IneligibilityCode::DoesNotFitAnyStock, "Part cannot fit within any available usable board rectangle".to_owned())
            };
            Some(EligibilityIssue { code, message })
        };
        result_parts.push(PreparedPart {
            part_id: p.id.clone(), material_id: p.material_id.clone(),
            requested_quantity: p.quantity,
            eligibility: if eligible { Eligibility::Eligible } else { Eligibility::Ineligible },
            issue,
            fit_options: fits,
        });
        for ordinal in 1..=p.quantity {
            instances.push(PartInstance {
                instance_id: format!("{}#{ordinal}", p.id),
                part_id: p.id.clone(), material_id: p.material_id.clone(),
                ordinal, width: p.width, height: p.height,
                priority: p.priority, eligible,
            });
        }
    }

    let mut warnings = Vec::new();
    let mut groups = Vec::with_capacity(material_groups.len());
    for (material_id, group) in material_groups {
        let enough = group.requested_area <= group.available_area;
        if !enough {
            warnings.push(PreprocessWarning {
                code: "insufficient_stock_area".to_owned(),
                material_id: material_id.clone(),
                message: "Total requested part area exceeds total usable stock area, even before saw kerf".to_owned(),
            });
        }
        groups.push(MaterialGroup {
            material_id, part_ids: group.part_ids,
            stock_ids: group.stock_ids, requested_pieces: group.requested_pieces,
            available_stock_units: group.available_stock_units,
            requested_area_sq_units: group.requested_area.to_string(),
            available_usable_area_sq_units: group.available_area.to_string(),
            aggregate_area_sufficient: enough,
        });
    }
    let overall_area_sufficient = groups.iter().all(|g| g.aggregate_area_sufficient);
    let stock_orientations = prepared_stock.iter().map(|stock| stock.orientations.len()).sum();
    let total_stock_units = model.stocks.iter().map(|s| u64::from(s.quantity)).sum();
    Ok(PreprocessedJob {
        summary: PreprocessSummary {
            material_groups: groups.len(), part_types: result_parts.len(),
            expanded_pieces: instances.len(), eligible_part_types: eligible_types,
            ineligible_part_types: result_parts.len() - eligible_types,
            eligible_instances, stock_types: prepared_stock.len(),
            available_stock_units: total_stock_units,
            stock_orientations, fit_options: option_count as usize,
            all_parts_individually_fit: eligible_types == result_parts.len(),
            aggregate_area_sufficient: overall_area_sufficient,
            geometry_validated: false, production_validated: false,
        },
        groups, stocks: prepared_stock, parts: result_parts,
        instances, warnings,
    })
}

#[derive(Default)]
struct MaterialGroupBuilder {
    part_ids: Vec<String>, stock_ids: Vec<String>,
    requested_pieces: u64, available_stock_units: u64,
    requested_area: u128, available_area: u128,
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    #[test]
    fn rotation_swaps_grain_axes() {
        assert_eq!(rotate_axis(Some(Axis::X), true), Some(Axis::Y));
        assert_eq!(rotate_axis(Some(Axis::Y), false), Some(Axis::Y));
        assert_eq!(rotate_axis(None, true), None);
    }
    #[test]
    fn clockwise_trim_rotation_preserves_each_margin() {
        let trim = TrimMargins { top: 1, right: 2, bottom: 3, left: 4 };
        assert_eq!(rotate_trim(trim), TrimMargins { top: 4, right: 1, bottom: 2, left: 3 });
    }
}
