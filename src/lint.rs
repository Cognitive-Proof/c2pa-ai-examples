//! Structural checks the SDK's validator does not make, so a fixture can't silently drift from
//! the schemas in the C2PA specification (actions.cddl, ingredient.cddl, ai-disclosure.cddl).
//!
//! It reads `Reader::detailed_json()`, whose assertion store keeps the labels as written:
//! `Reader::json()` reports a v1 `c2pa.actions` assertion as `c2pa.actions.v2`.

use serde_json::Value;

use crate::classify::{is_known_source_type, merged_actions};

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
            let raw = data["actions"].as_array().into_iter().flatten();
            let templates = data["templates"].as_array().into_iter().flatten();
            for a in raw.chain(templates) {
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
            }
        }
        for &(_, d) in store.iter().filter(|(l, _)| *l == "c2pa.ai-disclosure") {
            if !d["modelType"].is_string() {
                err("c2pa.ai-disclosure has no modelType".into());
            }
            let level = &d["contentProfile"]["humanOversightLevel"];
            if !level.is_null()
                && !matches!(level.as_str(), Some("fully_autonomous" | "prompt_guided" | "human_validated"))
            {
                err(format!("c2pa.ai-disclosure: invalid humanOversightLevel {level}"));
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
