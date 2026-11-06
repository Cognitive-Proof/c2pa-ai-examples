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
            if !d["modelType"].is_string() {
                err("c2pa.ai-disclosure has no modelType".into());
            }
            let level = &d["contentProfile"]["humanOversightLevel"];
            if !level.is_null()
                && !matches!(level.as_str(), Some("fully_autonomous" | "prompt_guided" | "human_validated"))
            {
                err(format!("c2pa.ai-disclosure: invalid humanOversightLevel {level}"));
            }
            // arXiv taxonomy, as the ai-disclosure.cddl regexp: "cs.AI", "physics.optics".
            let arxiv = |d: &str| {
                let parts: Vec<&str> = d.split('.').collect();
                parts.len() >= 2
                    && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            };
            match &d["scientificDomain"] {
                Value::Null => {}
                Value::Array(a) if !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(arxiv)) => {}
                other => err(format!("c2pa.ai-disclosure: invalid scientificDomain {other}")),
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
