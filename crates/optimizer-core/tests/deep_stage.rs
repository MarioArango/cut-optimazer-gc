//! Stage 18: cutting patterns of up to six levels.
use optimizer_core::model::{
    CutDirection, CuttingJobModel, CuttingPattern, MachineProfile, Material, Part, PatternNode,
    StackingCapabilities, StagePattern, Stock, StockKind, TrimMargins, MAX_CUT_LEVELS,
};
use optimizer_core::{evaluate_pattern, generate_deep_stage};

fn model(parts: Vec<(&str, u32, u32, u32)>, patterns: Vec<StagePattern>) -> CuttingJobModel {
    model_with(parts, patterns, true)
}

fn model_with(parts: Vec<(&str, u32, u32, u32)>, patterns: Vec<StagePattern>, rotate: bool) -> CuttingJobModel {
    CuttingJobModel {
        materials: vec![Material { id: "m".into(), name: "Melamina".into(), thickness: 180, directional_grain: false }],
        parts: parts.into_iter().map(|(id, w, h, q)| Part {
            id: id.into(), material_id: "m".into(), width: w, height: h, quantity: q,
            allow_rotate_90: rotate, grain_axis: None, priority: 0,
        }).collect(),
        stocks: vec![Stock {
            id: "s".into(), material_id: "m".into(), kind: StockKind::FullBoard, width: 1000, height: 800,
            thickness: 180, quantity: 10, grain_axis: None, trim: TrimMargins::default(), unit_cost_minor: 1000,
        }],
        machine: MachineProfile {
            id: "saw".into(), kerf: 10, max_cut_length: 2000, min_piece_width: 50, min_piece_height: 50,
            min_strip_width: 50, min_clamp_width: 50, max_stack_height: 800, max_stack_members: 4,
            allow_board_rotation: false, allow_recuts: true, allowed_patterns: patterns,
            stacking: StackingCapabilities { full_boards: true, strips: true, restacking: true },
        },
        patterns: vec![], workpieces: vec![], stacks: vec![],
    }
}

/// A tree that needs exactly `cuts.len()` levels: every level splits off one
/// region that the next level keeps cutting; the deepest region is piece P.
fn nested(cuts: &[(CutDirection, u32)]) -> PatternNode {
    fn go(cuts: &[(CutDirection, u32)], n: &mut u32) -> PatternNode {
        *n += 1;
        let id = format!("n{n}");
        match cuts.split_first() {
            None => PatternNode::Piece { node_id: id, part_id: "P".into(), rotated: false },
            Some(((direction, at), rest)) => PatternNode::Split {
                node_id: id, direction: *direction, at: *at,
                first: Box::new(go(rest, n)),
                second: Box::new({ *n += 1; PatternNode::Residual { node_id: format!("r{n}"), reusable: false } }),
            },
        }
    }
    go(cuts, &mut 0)
}

use CutDirection::{Horizontal as H, Vertical as V};
/// 1000×800 → 600×800 → 600×500 → 300×500 → 300×300 → 100×300 → 100×100.
const SIX: [(CutDirection, u32); 6] = [(V, 600), (H, 500), (V, 300), (H, 300), (V, 100), (H, 100)];

fn evaluate(tree: PatternNode, mode: StagePattern) -> Result<u8, String> {
    let mut m = model(vec![("P", 100, 100, 1)], StagePattern::ALL.to_vec());
    m.patterns.push(CuttingPattern { id: "t".into(), stock_id: "s".into(), mode, root: tree });
    evaluate_pattern(&m, "t", false).map(|e| e.summary.stages_used).map_err(|e| e.code)
}

#[test]
fn every_pattern_alternates_and_reports_its_levels() {
    assert_eq!(MAX_CUT_LEVELS, 6);
    for mode in StagePattern::ALL {
        let dirs = mode.directions();
        assert_eq!(dirs.len(), usize::from(mode.levels()));
        assert_eq!(dirs[0], mode.first());
        assert!(dirs.windows(2).all(|w| w[0] != w[1]), "{mode:?} must alternate");
        assert_eq!(StagePattern::from_levels(mode.first(), mode.levels()), Some(mode));
        let json = serde_json::to_string(&mode).unwrap();
        let name: String = dirs.iter().map(|d| if *d == V { "V" } else { "H" }).collect::<Vec<_>>().join("-");
        assert_eq!(json, format!("\"{name}\""));
        assert_eq!(serde_json::from_str::<StagePattern>(&json).unwrap(), mode);
    }
}

#[test]
fn six_level_tree_needs_a_six_level_pattern() {
    assert_eq!(evaluate(nested(&SIX), StagePattern::VHVHVH), Ok(6));
    // One level short, or starting the other way, is rejected.
    assert_eq!(evaluate(nested(&SIX), StagePattern::VHVHV), Err("invalid_stage_transition".into()));
    assert_eq!(evaluate(nested(&SIX), StagePattern::VHV), Err("invalid_stage_transition".into()));
    assert_eq!(evaluate(nested(&SIX), StagePattern::HVHVHV), Err("invalid_stage_transition".into()));
}

/// Trees of 2..=6 levels, each ending on the 100×100 piece.
fn tree_of(levels: usize) -> PatternNode {
    let cuts: &[(CutDirection, u32)] = match levels {
        2 => &[(V, 100), (H, 100)],
        3 => &[(V, 300), (H, 100), (V, 100)],
        4 => &[(V, 600), (H, 300), (V, 100), (H, 100)],
        5 => &[(V, 600), (H, 500), (V, 300), (H, 100), (V, 100)],
        _ => &SIX,
    };
    nested(cuts)
}

#[test]
fn shallower_trees_fit_deeper_patterns() {
    for levels in 2..=6usize {
        for mode in StagePattern::ALL.into_iter().filter(|m| m.first() == V) {
            let result = evaluate(tree_of(levels), mode);
            if usize::from(mode.levels()) >= levels {
                assert_eq!(result, Ok(levels as u8), "{levels} levels under {mode:?}");
            } else {
                assert_eq!(result, Err("invalid_stage_transition".into()), "{levels} levels under {mode:?}");
            }
        }
    }
}

#[test]
fn deep_generator_respects_the_deepest_allowed_level() {
    let parts = vec![("A", 480, 390, 3), ("B", 230, 190, 6), ("C", 150, 120, 10), ("D", 300, 70, 8)];
    for levels in 2..=6u8 {
        let patterns: Vec<StagePattern> = StagePattern::ALL.into_iter().filter(|m| m.levels() <= levels).collect();
        let m = model(parts.clone(), patterns);
        let result = generate_deep_stage(&m, None, 7, 2_000, 8, 1_500).unwrap();
        let board = result.board.expect("a board");
        // Re-evaluated by the independent evaluator inside the generator.
        assert!(board.evaluated.summary.stages_used <= levels);
        assert!(board.mode.levels() <= levels);
        assert!(m.machine.allowed_patterns.contains(&board.mode));
        assert!(!board.evaluated.pieces.is_empty());
    }
}

#[test]
fn a_fourth_level_trims_a_piece_that_three_levels_cannot_place() {
    // 1000×800 board, kerf 10, no rotation, filled exactly:
    //   row 400: [cell 500: P1 480×195 over P2 500×195] + Q 490×400
    //   row 390: R 1000×390
    // P1 sits in a 500-wide cell: only a 4th (vertical) cut can trim it.
    let parts = vec![("P1", 480, 195, 1), ("P2", 500, 195, 1), ("Q", 490, 400, 1), ("R", 1000, 390, 1)];
    let upto = |n: u8| StagePattern::ALL.into_iter().filter(|m| m.levels() <= n).collect::<Vec<_>>();
    let placed = |n: u8| {
        let r = generate_deep_stage(&model_with(parts.clone(), upto(n), false), None, 1, 3_000, 16, 4_000).unwrap();
        r.board.map_or((0, 0), |b| (b.evaluated.pieces.len(), b.evaluated.summary.stages_used))
    };
    let (three, _) = placed(3);
    let (four, used) = placed(4);
    assert!(three <= 3, "three levels cannot place all four pieces");
    assert_eq!(four, 4, "four levels place every piece");
    assert_eq!(used, 4, "and need the fourth level to do it");
}
