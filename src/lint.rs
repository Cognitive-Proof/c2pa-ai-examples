//! Structural checks the SDK's validator does not make, so a fixture can't silently drift from
//! the schemas in the C2PA specification (actions.cddl, ingredient.cddl, ai-disclosure.cddl).
//!
//! It reads `Reader::detailed_json()`, whose assertion store keeps the labels as written:
//! `Reader::json()` reports a v1 `c2pa.actions` assertion as `c2pa.actions.v2`.

use serde_json::Value;

use crate::classify::{is_known_source_type, merged_actions};

/// The hash that manifest `label`'s claim records for `assertion`.
fn claim_hash<'a>(detailed: &'a Value, label: &str, assertion: &str) -> Option<&'a Value> {
    let claim = &detailed["manifests"][label]["claim"];
    let url = format!("self#jumbf=c2pa.assertions/{assertion}");
    ["created_assertions", "gathered_assertions", "assertions"]
        .iter()
        .flat_map(|k| claim[*k].as_array().into_iter().flatten())
        .find(|a| a["url"] == url.as_str())
        .map(|a| &a["hash"])
}

/// `$action-reason` in actions.cddl: a defined value, or a dotted entity-specific one.
fn is_action_reason(r: &str) -> bool {
    let dotted = r.split('.').count() >= 2
        && r.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    matches!(
        r,
        "c2pa.PII.present" | "c2pa.invalid.data" | "c2pa.trade-secret.present" | "c2pa.government.confidential"
    ) || dotted
}

/// Structure of `ai-model-disclosure-map` in ai-disclosure.cddl. A present field must have its
/// type: optional means it may be absent, not null. The maps are closed.
fn disclosure_schema(d: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(map) = d.as_object() else {
        return vec![format!("not a map: {d}")];
    };
    for k in map.keys() {
        if !matches!(k.as_str(), "modelType" | "modelName" | "modelIdentifier" | "contentProfile" | "scientificDomain" | "metadata") {
            out.push(format!("unknown field {k}"));
        }
    }
    if !map.get("modelType").is_some_and(Value::is_string) {
        out.push(format!("modelType must be a text string, got {}", map.get("modelType").unwrap_or(&Value::Null)));
    }
    for k in ["modelName", "modelIdentifier"] {
        if let Some(v) = map.get(k).filter(|v| !v.is_string()) {
            out.push(format!("{k} must be a text string, got {v}"));
        }
    }
    if let Some(v) = map.get("metadata").filter(|v| !v.is_object()) {
        out.push(format!("metadata must be a map, got {v}"));
    }
    match map.get("contentProfile") {
        None => {}
        Some(Value::Object(p)) => {
            for (k, v) in p {
                match (k.as_str(), v.as_str()) {
                    ("humanOversightLevel", Some("fully_autonomous" | "prompt_guided" | "human_validated")) => {}
                    ("humanOversightLevel", _) => out.push(format!("invalid humanOversightLevel {v}")),
                    _ => out.push(format!("unknown contentProfile field {k}")),
                }
            }
        }
        Some(v) => out.push(format!("contentProfile must be a map, got {v}")),
    }
    // $scientific-domain-string: "^[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)+$"
    let pattern = |s: &str| {
        s.split('.').count() >= 2
            && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    match map.get("scientificDomain") {
        None => {}
        Some(Value::Array(a)) if !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(pattern)) => {}
        Some(v) => out.push(format!("invalid scientificDomain {v}")),
    }
    out
}

/// The vocabularies AIDisclosure.adoc requires beyond the CDDL: `modelType` from the
/// model-types table in AssetType.adoc, and `scientificDomain` from the arXiv taxonomy.
fn disclosure_vocabulary(d: &Value) -> Vec<String> {
    const MODEL_TYPES: &[&str] = &[
        "c2pa.types.model", "c2pa.types.model.caffe", "c2pa.types.model.caffe2",
        "c2pa.types.model.catboost", "c2pa.types.model.coreml", "c2pa.types.model.flax",
        "c2pa.types.model.huggingface.transformers", "c2pa.types.model.jax",
        "c2pa.types.model.keras", "c2pa.types.model.lightgbm", "c2pa.types.model.ml_net",
        "c2pa.types.model.mxnet", "c2pa.types.model.onnx", "c2pa.types.model.openvino",
        "c2pa.types.model.openvino.parameter", "c2pa.types.model.openvino.topology",
        "c2pa.types.model.paddle", "c2pa.types.model.pytorch", "c2pa.types.model.sklearn",
        "c2pa.types.model.tensorflow", "c2pa.types.model.tensorrt", "c2pa.types.model.tflite",
        "c2pa.types.model.torchscript", "c2pa.types.model.xgboost",
    ];
    // https://arxiv.org/category_taxonomy, retrieved 2026-11-06.
    const ARXIV: &str = "cs.AI cs.AR cs.CC cs.CE cs.CG cs.CL cs.CR cs.CV cs.CY cs.DB cs.DC cs.DL \
        cs.DM cs.DS cs.ET cs.FL cs.GL cs.GR cs.GT cs.HC cs.IR cs.IT cs.LG cs.LO cs.MA cs.MM cs.MS \
        cs.NA cs.NE cs.NI cs.OH cs.OS cs.PF cs.PL cs.RO cs.SC cs.SD cs.SE cs.SI cs.SY econ.EM \
        econ.GN econ.TH eess.AS eess.IV eess.SP eess.SY math.AC math.AG math.AP math.AT math.CA \
        math.CO math.CT math.CV math.DG math.DS math.FA math.GM math.GN math.GR math.GT math.HO \
        math.IT math.KT math.LO math.MG math.MP math.NA math.NT math.OA math.OC math.PR math.QA \
        math.RA math.RT math.SG math.SP math.ST astro-ph.CO astro-ph.EP astro-ph.GA astro-ph.HE \
        astro-ph.IM astro-ph.SR cond-mat.dis-nn cond-mat.mes-hall cond-mat.mtrl-sci cond-mat.other \
        cond-mat.quant-gas cond-mat.soft cond-mat.stat-mech cond-mat.str-el cond-mat.supr-con \
        gr-qc hep-ex hep-lat hep-ph hep-th math-ph nlin.AO nlin.CD nlin.CG nlin.PS nlin.SI nucl-ex \
        nucl-th physics.acc-ph physics.ao-ph physics.app-ph physics.atm-clus physics.atom-ph \
        physics.bio-ph physics.chem-ph physics.class-ph physics.comp-ph physics.data-an \
        physics.ed-ph physics.flu-dyn physics.gen-ph physics.geo-ph physics.hist-ph \
        physics.ins-det physics.med-ph physics.optics physics.plasm-ph physics.pop-ph \
        physics.soc-ph physics.space-ph quant-ph q-bio.BM q-bio.CB q-bio.GN q-bio.MN q-bio.NC \
        q-bio.OT q-bio.PE q-bio.QM q-bio.SC q-bio.TO q-fin.CP q-fin.EC q-fin.GN q-fin.MF \
        q-fin.PM q-fin.PR q-fin.RM q-fin.ST q-fin.TR stat.AP stat.CO stat.ME stat.ML stat.OT \
        stat.TH";
    let mut out = Vec::new();
    if let Some(t) = d["modelType"].as_str().filter(|t| !MODEL_TYPES.contains(t)) {
        out.push(format!("modelType {t} is not in the model-types table"));
    }
    for v in d["scientificDomain"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !ARXIV.split_whitespace().any(|c| c == v) {
            out.push(format!("scientificDomain {v} is not an arXiv category"));
        }
    }
    out
}

/// Returns one message per problem across every manifest in the detailed store.
pub fn lint(detailed: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for (label, m) in detailed["manifests"].as_object().into_iter().flatten() {
        let mut err = |msg: String| out.push(format!("{label}: {msg}"));
        let store: Vec<(&str, &Value)> = m["assertion_store"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.split("__").next().unwrap_or(k), v))
            .collect();
        for &(al, data) in store.iter().filter(|(l, _)| *l == "c2pa.actions" || *l == "c2pa.actions.v2") {
            let v2 = al == "c2pa.actions.v2";
            if v2 && !data["allActionsIncluded"].is_boolean() {
                err(format!("{al} has no boolean allActionsIncluded"));
            }
            let mut items: Vec<&Value> = data["templates"].as_array().into_iter().flatten().collect();
            let mut stack: Vec<&Value> = data["actions"].as_array().into_iter().flatten().collect();
            while let Some(a) = stack.pop() {
                stack.extend(a["related"].as_array().into_iter().flatten());
                items.push(a);
            }
            for a in items {
                let agent = &a["softwareAgent"];
                match (v2, agent) {
                    (_, Value::Null) => {}
                    (true, Value::Object(o)) if o.get("name").is_some_and(Value::is_string) => {}
                    (false, Value::String(_)) => {}
                    _ => err(format!("{al}: softwareAgent {agent} is not the {} form", if v2 { "generator-info-map" } else { "v1 string" })),
                }
                if let Some(uri) = a["digitalSourceType"].as_str() {
                    if !is_known_source_type(uri) {
                        err(format!("{al}: unrecognised digitalSourceType {uri}"));
                    }
                }
            }
            for a in merged_actions(data) {
                if a["action"] == "c2pa.created" && v2 && !a["digitalSourceType"].is_string() {
                    err(format!("{al}: c2pa.created has no digitalSourceType, even after templates"));
                }
                // Actions.adoc: a c2pa.redacted action's reason "shall contain the rationale".
                if a["action"] == "c2pa.redacted" && !a["reason"].as_str().is_some_and(is_action_reason) {
                    err(format!("{al}: c2pa.redacted has no valid reason ({})", a["reason"]));
                }
                // The SDK checks references within this manifest; one into another manifest
                // (as c2pa.deleted may make) must match that manifest's claim too.
                for r in a["parameters"]["ingredients"].as_array().into_iter().flatten() {
                    let Some(url) = r["url"].as_str() else { continue };
                    let Some((target, assertion)) = url
                        .split("/c2pa/")
                        .nth(1)
                        .and_then(|rest| rest.split_once("/c2pa.assertions/"))
                    else {
                        continue;
                    };
                    match claim_hash(detailed, target, assertion) {
                        Some(h) if *h == r["hash"] => {}
                        Some(h) => err(format!("{al}: hash of {url} is {}, the claim has {h}", r["hash"])),
                        None => err(format!("{al}: {url} is not in the manifest store")),
                    }
                }
            }
        }
        for &(_, d) in store.iter().filter(|(l, _)| *l == "c2pa.ai-disclosure") {
            for msg in disclosure_schema(d) {
                err(format!("c2pa.ai-disclosure schema: {msg}"));
            }
            for msg in disclosure_vocabulary(d) {
                err(format!("c2pa.ai-disclosure vocabulary: {msg}"));
            }
        }
        for &(_, i) in store.iter().filter(|(l, _)| l.starts_with("c2pa.ingredient")) {
            if !i["activeManifest"].is_null() && !i["digitalSourceType"].is_null() {
                err("ingredient has both activeManifest and digitalSourceType".into());
            }
            if let Some(uri) = i["digitalSourceType"].as_str() {
                if !is_known_source_type(uri) {
                    err(format!("ingredient: unrecognised digitalSourceType {uri}"));
                }
            }
        }
    }
    out
}
