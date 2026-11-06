//! Reference classifier: reads a C2PA manifest store (as `Reader::json()` output) and answers
//! "does the signed provenance declare AI in the current content?" with one of four verdicts.
//!
//! The four categories are an application policy, not an algorithm the C2PA specification
//! prescribes. The policy is documented in the README; this file is its executable form.
//!
//! Signing-credential trust is deliberately ignored here: every file in this set is signed
//! with a public test certificate, so trust is a property of the reader's trust list, not of
//! the file.

use std::{collections::HashSet, fmt};

use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ai,
    IncludesAi,
    NoAi,
    Unknown,
}

impl Verdict {
    pub fn dir(self) -> &'static str {
        match self {
            Verdict::Ai => "ai",
            Verdict::IncludesAi => "includes-ai",
            Verdict::NoAi => "no-ai",
            Verdict::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dir())
    }
}

pub const IPTC: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/";
pub const C2PA: &str = "http://c2pa.org/digitalsourcetype/";

/// IPTC terms as of the 2025 vocabulary, plus the two C2PA-specific values.
pub fn is_known_source_type(uri: &str) -> bool {
    const IPTC_TERMS: &[&str] = &[
        "digitalCapture", "computationalCapture", "negativeFilm", "positiveFilm", "print",
        "humanEdits", "minorHumanEdits", "compositeCapture", "algorithmicallyEnhanced",
        "dataDrivenMedia", "digitalCreation", "trainedAlgorithmicMedia",
        "compositeWithTrainedAlgorithmicMedia", "algorithmicMedia", "screenCapture",
        "virtualRecording", "composite", "compositeSynthetic",
    ];
    if let Some(term) = uri.strip_prefix(IPTC) {
        IPTC_TERMS.contains(&term)
    } else {
        matches!(uri.strip_prefix(C2PA), Some("empty" | "trainedAlgorithmicData"))
    }
}

/// What a source type says about the content it describes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Ai,
    IncludesAi,
    NoAi,
    /// `empty`: the content comes entirely from the ingredients.
    Empty,
    /// Edits made by people with non-generative tools; says nothing about the origin.
    HumanEdits,
    /// Neither confirms nor rules out a trained model, or is not a recognised term.
    Unclear,
}

fn source_of(uri: &str) -> Source {
    if uri == format!("{C2PA}trainedAlgorithmicData") {
        return Source::Ai;
    }
    if uri == format!("{C2PA}empty") {
        return Source::Empty;
    }
    match uri.strip_prefix(IPTC) {
        Some("trainedAlgorithmicMedia") => Source::Ai,
        Some("compositeWithTrainedAlgorithmicMedia") => Source::IncludesAi,
        Some("digitalCapture" | "negativeFilm" | "positiveFilm" | "print" | "digitalCreation"
            | "compositeCapture" | "algorithmicMedia") => Source::NoAi,
        Some("humanEdits" | "minorHumanEdits") => Source::HumanEdits,
        // screenCapture says how a screen was captured, not where its content came from;
        // virtualRecording explicitly allows synthetic elements.
        _ => Source::Unclear,
    }
}

fn short(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or(uri)
}

/// Failure codes other than an untrusted signing credential (see module docs).
pub fn failures(results: &Value) -> Vec<String> {
    results["activeManifest"]["failure"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["code"].as_str())
        .filter(|c| *c != "signingCredential.untrusted")
        .map(str::to_owned)
        .collect()
}

/// The actions assertions of a manifest, as (label, data) pairs.
pub fn actions_assertions(m: &Value) -> Vec<(&str, &Value)> {
    m["assertions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["label"].as_str()?, &a["data"])))
        .filter(|(l, _)| *l == "c2pa.actions" || l.starts_with("c2pa.actions."))
        .collect()
}

/// Every action with its templates applied: the `*` template first, then same-named
/// templates in order, then the action's own fields; `parameters` merge key by key.
pub fn merged_actions(data: &Value) -> Vec<Value> {
    let templates: Vec<&Value> = data["templates"].as_array().into_iter().flatten().collect();
    data["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|action| {
            let name = &action["action"];
            let mut out = Map::new();
            let layers = templates
                .iter()
                .filter(|t| t["action"] == "*")
                .chain(templates.iter().filter(|t| &t["action"] == name))
                .copied()
                .chain(std::iter::once(action));
            for layer in layers {
                for (k, v) in layer.as_object().into_iter().flatten() {
                    match (k.as_str(), out.get_mut(k)) {
                        ("parameters", Some(Value::Object(p))) => {
                            p.extend(v.as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.clone())));
                        }
                        _ => {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            out.insert("action".into(), name.clone());
            Value::Object(out)
        })
        .collect()
}

/// (manifest label, ingredient assertion label) of a hashed JUMBF URI.
fn target(url: &str) -> Option<(String, String)> {
    let rest = url.split("/c2pa/").nth(1)?;
    let (manifest, assertion) = rest.split_once("/c2pa.assertions/")?;
    Some((manifest.to_owned(), assertion.to_owned()))
}

/// Classifies a manifest store. `None` means no manifest could be read.
pub fn classify(store: Option<&Value>) -> (Verdict, String) {
    let Some(store) = store else {
        return (Verdict::Unknown, "no readable C2PA manifest".into());
    };
    let bad = failures(&store["validation_results"]);
    if !bad.is_empty() {
        return (Verdict::Unknown, format!("active manifest failed validation: {}", bad.join(", ")));
    }
    match store["active_manifest"].as_str() {
        Some(label) => manifest(store, label, &HashSet::new()),
        None => (Verdict::Unknown, "no active manifest".into()),
    }
}

fn ingredient(store: &Value, ing: &Value, deleted: &HashSet<(String, String)>) -> (Verdict, String) {
    let title = ing["title"].as_str().unwrap_or("untitled ingredient");
    let bad = failures(&ing["validation_results"]);
    if !bad.is_empty() {
        return (Verdict::Unknown, format!("ingredient '{title}' failed validation: {}", bad.join(", ")));
    }
    if let Some(label) = ing["active_manifest"].as_str() {
        let (v, why) = manifest(store, label, deleted);
        return (v, format!("ingredient '{title}' -> {why}"));
    }
    // An ingredient without a manifest may still declare its own source type.
    match ing["digital_source_type"].as_str() {
        Some(uri) => {
            let v = match source_of(uri) {
                Source::Ai => Verdict::Ai,
                Source::IncludesAi => Verdict::IncludesAi,
                Source::NoAi => Verdict::NoAi,
                _ => Verdict::Unknown,
            };
            (v, format!("ingredient '{title}' declares {} (no manifest)", short(uri)))
        }
        None => (Verdict::Unknown, format!("ingredient '{title}' has no manifest or source type")),
    }
}

fn manifest(store: &Value, label: &str, deleted: &HashSet<(String, String)>) -> (Verdict, String) {
    let m = &store["manifests"][label];
    let assertions = actions_assertions(m);
    let actions: Vec<Value> = assertions.iter().flat_map(|(_, d)| merged_actions(d)).collect();
    // Only a v2 assertion saying allActionsIncluded: true rules out unrecorded actions.
    let complete = !assertions.is_empty()
        && assertions.iter().all(|(l, d)| *l == "c2pa.actions.v2" && d["allActionsIncluded"] == true);

    // Components removed by c2pa.deleted, here or in a later manifest, are not current content.
    let mut deleted = deleted.clone();
    for a in actions.iter().filter(|a| a["action"] == "c2pa.deleted") {
        for r in a["parameters"]["ingredients"].as_array().into_iter().flatten() {
            deleted.extend(r["url"].as_str().and_then(target));
        }
    }

    let ingredients: Vec<&Value> = m["ingredients"].as_array().into_iter().flatten().collect();
    let parent = ingredients.iter().find(|i| i["relationship"] == "parentOf");
    let components: Vec<(Verdict, String)> = ingredients
        .iter()
        .filter(|i| i["relationship"] == "componentOf")
        .filter(|i| {
            let key = (label.to_owned(), i["label"].as_str().unwrap_or_default().to_owned());
            !deleted.contains(&key)
        })
        .map(|i| ingredient(store, i, &deleted))
        .collect();

    let mut notes = Vec::new();
    let mut ai_signal = false;
    let mut unclear = false;
    let mut origin = None;
    let mut empty_origin = false;

    for a in &actions {
        let name = a["action"].as_str().unwrap_or_default();
        match (name, a["digitalSourceType"].as_str()) {
            ("c2pa.created", Some(uri)) => {
                notes.push(format!("created as {}", short(uri)));
                origin = Some(match source_of(uri) {
                    Source::Ai => Verdict::Ai,
                    Source::IncludesAi => Verdict::IncludesAi,
                    Source::NoAi => Verdict::NoAi,
                    Source::Empty => {
                        empty_origin = true;
                        Verdict::NoAi
                    }
                    Source::HumanEdits | Source::Unclear => Verdict::Unknown,
                });
            }
            ("c2pa.created", None) => {
                notes.push("created with no digitalSourceType".into());
                origin = Some(Verdict::Unknown);
            }
            ("c2pa.opened", _) => {
                let (v, why) = match parent {
                    Some(p) => ingredient(store, p, &deleted),
                    None => (Verdict::Unknown, "opened with no parent ingredient".into()),
                };
                notes.push(format!("opened parent: {why}"));
                origin = Some(v);
            }
            ("c2pa.deleted", _) => notes.push("c2pa.deleted removes a component".into()),
            (_, Some(uri)) => {
                notes.push(format!("{name} as {}", short(uri)));
                match source_of(uri) {
                    Source::Ai | Source::IncludesAi => ai_signal = true,
                    Source::Unclear => unclear = true,
                    Source::NoAi | Source::Empty | Source::HumanEdits => {}
                }
            }
            _ => {}
        }
    }

    let origin = match origin {
        Some(v) => v,
        // An update manifest need not record an opened action; it inherits its parent.
        None if parent.is_some() && actions.is_empty() => {
            let (v, why) = ingredient(store, parent.unwrap(), &deleted);
            return (v, format!("update manifest over {why}"));
        }
        None => {
            notes.push("no created or opened action".into());
            Verdict::Unknown
        }
    };

    if m["assertions"].as_array().into_iter().flatten().any(|a| a["label"] == "c2pa.ai-disclosure") {
        notes.push("c2pa.ai-disclosure present".into());
        ai_signal = true;
    }
    for (v, why) in &components {
        notes.push(format!("component: {why}"));
        match v {
            Verdict::Ai | Verdict::IncludesAi => ai_signal = true,
            Verdict::Unknown => unclear = true,
            Verdict::NoAi => {}
        }
    }
    if !complete {
        notes.push("history may be incomplete (allActionsIncluded not true)".into());
    }

    let all_components_ai = !components.is_empty() && components.iter().all(|(v, _)| *v == Verdict::Ai);
    let verdict = if (origin == Verdict::Ai && components.iter().all(|(v, _)| *v == Verdict::Ai))
        || (empty_origin && all_components_ai)
    {
        Verdict::Ai
    } else if origin == Verdict::Ai || origin == Verdict::IncludesAi || ai_signal {
        Verdict::IncludesAi
    } else if origin == Verdict::Unknown || unclear || !complete {
        Verdict::Unknown
    } else {
        Verdict::NoAi
    };
    (verdict, notes.join("; "))
}
