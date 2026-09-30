use serde_json::{json, Map, Value};
use worldline_core::presentation::{
    measurement_distance, validate_measurement, MapCanvas, MapMeasurement,
};
use worldline_core::presentation_commands::{
    apply, document_hash, undo, Command, CommandEnvelope, Revision,
};
use worldline_core::project::Project;

fn canvas(width: u32, height: u32) -> MapCanvas {
    MapCanvas {
        width,
        height,
        unit: "normalized".into(),
        extra: Map::new(),
    }
}
fn calibration() -> MapMeasurement {
    MapMeasurement {
        points: [[0.0, 0.0], [1.0, 0.0]],
        distance: 100.0,
        unit: "里".into(),
        extra: Map::new(),
    }
}
fn fixture(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!("measurement-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut p = Project::new(&root);
    p.create_authoring_document(
        &root.join(".world/project.json"),
        serde_json::to_vec(&json!({"schema_version":1,"maps":{"m":".world/maps/m.json"}})).unwrap(),
    )
    .unwrap();
    p.create_authoring_document(&root.join(".world/maps/m.json"), serde_json::to_vec(&json!({"schema_version":1,"id":"m","title":"地图","canvas":{"width":2000,"height":1000,"unit":"normalized"},"layer_order":[],"layers":{},"placements":{},"future":{"keep":true}})).unwrap()).unwrap();
    p
}
fn source(p: &Project) -> Value {
    serde_json::from_slice(bytes(p)).unwrap()
}
fn bytes(p: &Project) -> &[u8] {
    p.authoring_document(&p.root.join(".world/maps/m.json"))
        .unwrap()
        .bytes()
}
fn request(p: &Project, revision: Revision, measurement: MapMeasurement) -> CommandEnvelope {
    CommandEnvelope {
        expected_revision: revision,
        expected_documents: [(p.root.join(".world/maps/m.json"), document_hash(bytes(p)))].into(),
        command: Command::SetMapMeasurement {
            map_id: "m".into(),
            measurement,
        },
    }
}
#[test]
fn aspect_ratio_and_current_canvas_are_authoritative() {
    let c = calibration();
    assert_eq!(
        measurement_distance(&canvas(2000, 1000), &c, [[0., 0.], [0., 1.]]).unwrap(),
        50.
    );
    assert_eq!(
        measurement_distance(&canvas(1000, 2000), &c, [[0., 0.], [0., 1.]]).unwrap(),
        200.
    );
    assert_eq!(
        measurement_distance(&canvas(1000, 1000), &c, [[0., 0.], [0., 1.]]).unwrap(),
        100.
    );
    let diagonal = measurement_distance(&canvas(2000, 1000), &c, [[0., 0.], [1., 1.]]).unwrap();
    assert!((diagonal - 111.80339887498948).abs() < 1e-12);
    assert_eq!(
        diagonal,
        measurement_distance(&canvas(2000, 1000), &c, [[1., 1.], [0., 0.]]).unwrap()
    );
    assert_eq!(
        measurement_distance(&canvas(2000, 1000), &c, [[0.1, 0.1], [0.6, 0.1]]).unwrap(),
        50.
    );
}
#[test]
fn invalid_values_and_result_overflow_are_rejected() {
    let canvas = canvas(2000, 1000);
    for distance in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut c = calibration();
        c.distance = distance;
        assert!(validate_measurement(&canvas, &c).is_err());
    }
    for unit in [
        "".into(),
        "  ".into(),
        "x\ny".into(),
        "\t".into(),
        "字".repeat(25),
    ] {
        let mut c = calibration();
        c.unit = unit;
        assert!(validate_measurement(&canvas, &c).is_err());
    }
    for points in [
        [[0., 0.], [0., 0.]],
        [[-0.1, 0.], [1., 1.]],
        [[0., 0.], [1.1, 1.]],
        [[f64::NAN, 0.], [1., 1.]],
    ] {
        assert!(measurement_distance(&canvas, &calibration(), points).is_err());
        let mut c = calibration();
        c.points = points;
        assert!(validate_measurement(&canvas, &c).is_err());
    }
    let mut c = calibration();
    c.distance = f64::MAX;
    c.points = [[0., 0.], [0.01, 0.]];
    assert!(measurement_distance(&canvas, &c, [[0., 0.], [1., 1.]]).is_err());
}
#[test]
fn transaction_roundtrip_preserves_unknowns_and_undo() {
    let mut p = fixture("transaction");
    let original = bytes(&p).to_vec();
    let mut r = Revision::default();
    let cmd = request(&p, r, calibration());
    let result = apply(&mut p, &mut r, cmd).unwrap();
    assert!(p.map_index().diagnostics.is_empty());
    assert_eq!(
        source(&p)["required_features"],
        json!(["presentation.measurement.v1"])
    );
    assert_eq!(source(&p)["future"]["keep"], true);
    let mut modified = source(&p);
    modified["measurement"]["unknown"] = json!({"keep":42});
    p.set_authoring_document(
        &p.root.join(".world/maps/m.json"),
        serde_json::to_vec(&modified).unwrap(),
    )
    .unwrap();
    let mut c = calibration();
    c.distance = 120.;
    c.extra.insert("injected".into(), json!(true));
    let cmd = request(&p, r, c.clone());
    apply(&mut p, &mut r, cmd).unwrap();
    assert_eq!(source(&p)["measurement"]["unknown"]["keep"], 42);
    assert!(source(&p)["measurement"].get("injected").is_none());
    let before = bytes(&p).to_vec();
    let cmd = request(&p, r, c);
    assert!(apply(&mut p, &mut r, cmd).is_err());
    assert_eq!(bytes(&p), before);
    let mut p = fixture("undo");
    let mut r = Revision::default();
    let cmd = request(&p, r, calibration());
    let applied = apply(&mut p, &mut r, cmd).unwrap();
    undo(&mut p, &mut r, applied.new_revision, &applied.undo_record).unwrap();
    assert_eq!(bytes(&p), original);
    assert_ne!(result.new_revision, Revision::default());
}
#[test]
fn missing_capability_bad_calibration_and_stale_commands_fail() {
    let mut p = fixture("negative");
    let mut value = source(&p);
    value["measurement"] = serde_json::to_value(calibration()).unwrap();
    p.set_authoring_document(
        &p.root.join(".world/maps/m.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert!(p.map_index().maps.is_empty());
    assert!(p.map_index().diagnostics.iter().any(|d| d.code == "MAP002"));
    let mut p = fixture("stale");
    let mut r = Revision::default();
    let stale = request(&p, r, calibration());
    let mut changed = source(&p);
    changed["title"] = json!("新地图");
    p.set_authoring_document(
        &p.root.join(".world/maps/m.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let before = bytes(&p).to_vec();
    assert!(apply(&mut p, &mut r, stale).is_err());
    assert_eq!(bytes(&p), before);
    assert_eq!(r, Revision::default());
    for bad in [0., -1., f64::NAN] {
        let mut c = calibration();
        c.distance = bad;
        let cmd = request(&p, r, c);
        assert!(apply(&mut p, &mut r, cmd).is_err());
        assert_eq!(bytes(&p), before);
    }
}

#[test]
fn save_reopen_export_and_fingerprint_boundaries() {
    use worldline_core::reader_export::{ReaderExportSelection, ReaderMapSelection};
    let mut p = fixture("persistence");
    let fingerprint = p.compile().analysis.fingerprint;
    let mut r = Revision::default();
    let mut c = calibration();
    c.unit = "PRIVATE_UNIT_7391".into();
    let cmd = request(&p, r, c.clone());
    let applied = apply(&mut p, &mut r, cmd).unwrap();
    assert_eq!(p.compile().analysis.fingerprint, fingerprint);
    p.save().unwrap();
    let reopened = Project::open(&p.root).unwrap();
    assert_eq!(
        reopened.map_index().maps["m"].measurement.as_ref(),
        Some(&c)
    );
    let exported = reopened.export_files().unwrap();
    assert_eq!(
        exported[std::path::Path::new(".world/maps/m.json")],
        bytes(&p)
    );
    let selection = ReaderExportSelection {
        required_features: Vec::new(),
        fields: Vec::new(),
        schema_version: 1,
        site_title: "公开地图".into(),
        objects: Vec::new(),
        manuscripts: Vec::new(),
        attachments: Vec::new(),
        maps: vec![ReaderMapSelection {
            id: "m".into(),
            placements: Vec::new(),
            raster_layers: Vec::new(),
        }],
    };
    let plan = reopened.preview_reader_export(&selection).unwrap();
    let files = reopened
        .build_reader_export(&selection, &plan.plan_digest)
        .unwrap();
    for bytes in files.values() {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("PRIVATE_UNIT_7391"));
        assert!(!text.contains("presentation.measurement"));
        assert!(!text.contains("\"measurement\""));
    }
    undo(&mut p, &mut r, applied.new_revision, &applied.undo_record).unwrap();
    assert!(p.is_dirty());
    assert!(p.map_index().maps["m"].measurement.is_none());
    p.save().unwrap();
    assert!(Project::open(&p.root).unwrap().map_index().maps["m"]
        .measurement
        .is_none());
}

#[test]
fn invalid_persisted_measurement_reports_domain_error_and_preserves_bytes() {
    for (i, bad) in [
        Value::Null,
        json!({}),
        json!({"points":[[0,0],[0,0]],"distance":1,"unit":"m"}),
        json!({"points":[[0,0],[1,1]],"distance":-1,"unit":"m"}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut p = fixture(&format!("parse-invalid-{i}"));
        let mut value = source(&p);
        value["required_features"] = json!(["presentation.measurement.v1"]);
        value["measurement"] = bad;
        let raw = serde_json::to_vec(&value).unwrap();
        p.set_authoring_document(&p.root.join(".world/maps/m.json"), raw.clone())
            .unwrap();
        let index = p.map_index();
        assert!(index.maps.is_empty());
        assert!(index.diagnostics.iter().any(|d| d.code == "MAP013"));
        assert_eq!(bytes(&p), raw);
    }
}

#[test]
fn equivalent_integer_json_is_a_no_op_and_unknown_capabilities_are_read_only() {
    let mut p = fixture("integer-no-op");
    let mut value = source(&p);
    value["measurement"] = json!({"points":[[0,0],[1,0]],"distance":100,"unit":"里"});
    value["required_features"] = json!(["presentation.measurement.v1"]);
    p.set_authoring_document(
        &p.root.join(".world/maps/m.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let before = bytes(&p).to_vec();
    let mut revision = Revision::default();
    let cmd = request(&p, revision, calibration());
    assert!(apply(&mut p, &mut revision, cmd).is_err());
    assert_eq!(bytes(&p), before);
    assert_eq!(revision, Revision::default());
    p.save().unwrap();
    value["required_features"] = json!(["presentation.measurement.v2"]);
    std::fs::write(
        p.root.join(".world/maps/m.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let mut p = Project::open(&p.root).unwrap();
    let before = bytes(&p).to_vec();
    let mut c = calibration();
    c.distance = 120.;
    let cmd = request(&p, revision, c);
    assert!(apply(&mut p, &mut revision, cmd).is_err());
    assert_eq!(bytes(&p), before);
    assert_eq!(revision, Revision::default());
}

#[test]
fn representable_results_survive_intermediate_overflow_and_underflow() {
    let tiny = f64::from_bits(1);
    let mut c = calibration();
    c.points = [[0.0, 0.0], [tiny, 0.0]];
    c.distance = tiny;
    assert_eq!(
        measurement_distance(&canvas(1, 1), &c, [[0.0, 0.0], [1.0, 0.0]]).unwrap(),
        1.0
    );
    c.points = [[0.0, 0.0], [1.0, 0.0]];
    c.distance = f64::from(i32::MAX);
    assert_eq!(
        measurement_distance(&canvas(i32::MAX as u32, 1), &c, [[0.0, 0.0], [0.0, tiny]]).unwrap(),
        tiny
    );
}
