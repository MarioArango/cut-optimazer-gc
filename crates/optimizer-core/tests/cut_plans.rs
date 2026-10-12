//! Stage 18 cutting-plan export: structure and geometric invariants.
use optimizer_core::cut_plans::{default_cut_plan_search, generate_cut_plans, FIRST_REMNANT_ID};
use optimizer_core::model::CuttingJobModel;
use optimizer_core::{run_json, JobOutput, JobResponse};
use serde_json::Value;
use std::collections::BTreeSet;

const ROVERE: &str = include_str!("../../../examples/generate-cut-plans-rovere.json");
const EPS: f64 = 0.011;

fn rovere_model(scale_down: u32) -> CuttingJobModel {
    let request: Value = serde_json::from_str(ROVERE).unwrap();
    let mut model: CuttingJobModel = serde_json::from_value(request["operation"]["model"].clone()).unwrap();
    for part in &mut model.parts {
        part.quantity = (part.quantity / scale_down).max(1);
    }
    model
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("number expected, got {v}"))
}

/// Drawn rectangle of a piece or remnant: pieces swap `width`/`height` when rotated.
fn drawn(p: &Value) -> (f64, f64, f64, f64) {
    let (w, h) = (f(&p["width"]), f(&p["height"]));
    let (w, h) = if p["rotated"].as_bool() == Some(true) { (h, w) } else { (w, h) };
    (f(&p["x"]), f(&p["y"]), w, h)
}

const PART_KEYS: [&str; 10] = ["stock_no", "part", "x", "y", "width", "height", "rotated", "id_piece", "n_item", "veta"];

/// Every key is snake_case and every measure a JSON number.
fn check_keys(value: &Value) {
    match value {
        Value::Object(map) => for (key, v) in map {
            assert!(key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "key not snake_case: {key}");
            if ["x", "y", "x1", "y1", "x2", "y2", "width", "height", "dimension", "stock_no", "i_cut",
                "a_level", "id_piece", "n_item", "root_stock_no", "stock_cuts", "part"].contains(&key.as_str()) {
                assert!(v.is_number(), "{key} must be a number, got {v}");
            }
            check_keys(v);
        },
        Value::Array(items) => items.iter().for_each(check_keys),
        _ => {}
    }
}

fn walk<'a>(cuts: &'a [Value], level: u64, out: &mut Vec<&'a Value>) {
    for cut in cuts {
        assert_eq!(cut["a_level"].as_u64().unwrap(), level, "level mismatch in {cut}");
        out.push(cut);
        let next = format!("level{}", level + 1);
        if let Some(children) = cut.get(&next) {
            walk(children.as_array().unwrap(), level + 1, out);
        }
    }
}

fn check_plans(plans: &Value, model: &CuttingJobModel) {
    assert_eq!(plans["success"], true);
    let records = plans["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    check_keys(plans);
    let kerf = f(&record["saw_width"]);
    let mut piece_ids = BTreeSet::new();
    for board in record["boards"].as_array().unwrap() {
        let (bx1, by1, bx2, by2) = (f(&board["x1"]), f(&board["y1"]), f(&board["x2"]), f(&board["y2"]));
        let pieces = board["parts"].as_array().unwrap();
        let waste = board["waste_part"].as_array().unwrap();
        let mut rects = Vec::new();
        for part in pieces {
            let keys: Vec<&str> = part.as_object().unwrap().keys().map(String::as_str).collect();
            let mut expected = PART_KEYS.to_vec();
            expected.sort_unstable();
            assert_eq!(keys, expected, "parts carry exactly the agreed fields");
            assert!(piece_ids.insert(part["id_piece"].as_u64().unwrap()), "duplicate piece id");
        }
        for remnant in waste {
            assert!(remnant["id_piece"].as_u64().unwrap() >= FIRST_REMNANT_ID);
        }
        for region in pieces.iter().chain(waste) {
            let (x, y, w, h) = drawn(region);
            assert!(w > 0.0 && h > 0.0, "zero or negative region {region}");
            assert!(x >= bx1 - EPS && y >= by1 - EPS && x + w <= bx2 + EPS && y + h <= by2 + EPS,
                    "region outside the trimmed board: {region}");
            rects.push((x, y, w, h));
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let overlap = a.0 + a.2 > b.0 + EPS && b.0 + b.2 > a.0 + EPS
                    && a.1 + a.3 > b.1 + EPS && b.1 + b.3 > a.1 + EPS;
                assert!(!overlap, "regions overlap: {a:?} {b:?}");
            }
        }
        let mut cuts = Vec::new();
        walk(board["cuts"].as_array().unwrap(), 1, &mut cuts);
        let real = cuts.iter().filter(|c| c["type"] != "retal" && c.get("edge").is_none()).count();
        assert_eq!(board["stock_cuts"], real as u64);
        // Every piece and remnant is taken out by exactly one cut entry, and
        // a piece's cut runs half a kerf beyond its far edge.
        let produced: Vec<u64> = cuts.iter().map(|c| c["id_piece"].as_u64().unwrap()).filter(|&id| id != 0).collect();
        let listed: BTreeSet<u64> = pieces.iter().chain(waste).map(|p| p["id_piece"].as_u64().unwrap()).collect();
        assert_eq!(produced.len(), listed.len());
        assert_eq!(produced.iter().copied().collect::<BTreeSet<_>>(), listed);
        for cut in cuts.iter().filter(|c| c["type"] != "retal") {
            let id = cut["id_piece"].as_u64().unwrap();
            if id == 0 { continue; }
            let part = pieces.iter().find(|p| p["id_piece"].as_u64() == Some(id)).unwrap();
            let (x, y, w, h) = drawn(part);
            let vertical = cut["x1"] == cut["x2"];
            let (at, end, size) = if vertical { (f(&cut["x1"]), x + w, w) } else { (f(&cut["y1"]), y + h, h) };
            assert!((at - (end + kerf / 2.0)).abs() < EPS, "cut line not at piece edge: {cut}");
            assert!((f(&cut["dimension"]) - size).abs() < EPS);
        }
        // Every level list closes with the boundary entry.
        assert!(board["cuts"].as_array().unwrap().last().unwrap().get("i_cut").is_none());
    }
    let requested: u32 = model.parts.iter().map(|p| p.quantity).sum();
    assert_eq!(piece_ids.len() as u32, requested);
    assert_eq!(piece_ids, (1..=u64::from(requested)).collect());
    assert_eq!(record["summary"]["pieces"], u64::from(requested));
}

#[test]
fn rovere_order_exports_consistent_plans() {
    let model = rovere_model(5);
    let plans = generate_cut_plans(&model, 42, 20_000, default_cut_plan_search()).unwrap();
    assert_eq!(plans["complete_order"], true);
    check_plans(&plans, &model);
}

#[test]
fn grain_is_reported_and_respected() {
    let model = rovere_model(10);
    let plans = generate_cut_plans(&model, 42, 20_000, default_cut_plan_search()).unwrap();
    let pieces: Vec<&Value> = plans["records"][0]["boards"].as_array().unwrap().iter()
        .flat_map(|b| b["parts"].as_array().unwrap()).collect();
    // item 1 has grain along its width; item 3 has none and may rotate.
    let item1: Vec<&&Value> = pieces.iter().filter(|p| p["n_item"] == 1).collect();
    assert!(!item1.is_empty());
    assert!(item1.iter().all(|p| p["veta"] == "L" && p["rotated"] == false));
    assert!(pieces.iter().filter(|p| p["n_item"] == 3).all(|p| p["veta"].is_null()));
}

#[test]
fn exact_fit_strip_closes_with_edge_entry_not_negative_remnant() {
    // Two 1212.8 mm strips + one 4.4 mm kerf fill the 2430 mm usable width.
    let mut model = rovere_model(1);
    model.parts.truncate(1);
    model.parts[0].width = 12_128;
    model.parts[0].height = 21_300;
    model.parts[0].quantity = 2;
    let plans = generate_cut_plans(&model, 1, 5_000, default_cut_plan_search()).unwrap();
    check_plans(&plans, &model);
    let board = &plans["records"][0]["boards"][0];
    assert!(board["waste_part"].as_array().unwrap().is_empty(), "no remnant expected: {board}");
    let last = board["cuts"].as_array().unwrap().last().unwrap();
    assert_eq!(last["edge"], true);
    assert_eq!(board["stock_cuts"], 1);
}

#[test]
fn engine_operation_returns_cut_plans() {
    let mut request: Value = serde_json::from_str(ROVERE).unwrap();
    request["operation"]["model"] = serde_json::to_value(rovere_model(10)).unwrap();
    let response: JobResponse = serde_json::from_str(&run_json(&request.to_string()).unwrap()).unwrap();
    let JobOutput::CutPlans { result } = response.output else { panic!("cut_plans output expected") };
    check_plans(&result, &rovere_model(10));
}
