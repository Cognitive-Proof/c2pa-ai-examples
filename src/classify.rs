//! Reference classifier: reads a C2PA manifest store (as `Reader::json()` output) and answers
//! "is this AI?" with one of four verdicts. It exists to prove that every label in
//! `labels.json` follows from the manifest data alone, and to give implementers a baseline.
//!
//! Signing-credential trust is deliberately ignored here: every file in this set is signed
//! with the public C2PA test certificate, so trust is a property of the reader's trust list,
//! not of the file.

use std::fmt;

use serde_json::Value;

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

/// What a single digital source type says about AI involvement.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Ai,
    IncludesAi,
    NoAi,
    /// The type neither confirms nor rules out a trained model (e.g. `compositeSynthetic`).
    Unclear,
    /// `empty`: the content comes entirely from the ingredients.
    Empty,
}

fn source_of(uri: &str) -> Source {
    match uri.rsplit('/').next().unwrap_or_default() {
        "trainedAlgorithmicMedia" | "trainedAlgorithmicData" => Source::Ai,
        "compositeWithTrainedAlgorithmicMedia" => Source::IncludesAi,
        "digitalCapture" | "negativeFilm" | "positiveFilm" | "print" | "digitalCreation"
        | "compositeCapture" | "screenCapture" | "algorithmicMedia" | "humanEdits"
        | "minorHumanEdits" | "virtualRecording" => Source::NoAi,
        "empty" => Source::Empty,
        _ => Source::Unclear,
    }
}

/// Failure codes other than an untrusted signing credential (see module docs).
fn failures(results: &Value) -> Vec<String> {
    results["activeManifest"]["failure"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["code"].as_str())
        .filter(|c| *c != "signingCredential.untrusted")
        .map(str::to_owned)
        .collect()
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
        Some(label) => manifest(store, label),
        None => (Verdict::Unknown, "no active manifest".into()),
    }
}

fn ingredient(store: &Value, ing: &Value) -> (Verdict, String) {
    let title = ing["title"].as_str().unwrap_or("untitled ingredient");
    let bad = failures(&ing["validation_results"]);
    if !bad.is_empty() {
        return (Verdict::Unknown, format!("ingredient '{title}' failed validation: {}", bad.join(", ")));
    }
    match ing["active_manifest"].as_str() {
        Some(label) => {
            let (v, why) = manifest(store, label);
            (v, format!("ingredient '{title}' -> {why}"))
        }
        None => (Verdict::Unknown, format!("ingredient '{title}' has no C2PA manifest")),
    }
}

fn manifest(store: &Value, label: &str) -> (Verdict, String) {
    let m = &store["manifests"][label];
    let actions: Vec<&Value> = m["assertions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["label"].as_str().is_some_and(|l| l.starts_with("c2pa.actions")))
        .flat_map(|a| a["data"]["actions"].as_array().into_iter().flatten())
        .collect();
    let ingredients: Vec<&Value> = m["ingredients"].as_array().into_iter().flatten().collect();
    let components: Vec<(Verdict, String)> = ingredients
        .iter()
        .filter(|i| i["relationship"] == "componentOf")
        .map(|i| ingredient(store, i))
        .collect();

    let mut notes = Vec::new();
    let mut ai_signal = false;
    let mut unclear = false;
    let mut origin = None;

    for a in &actions {
        let name = a["action"].as_str().unwrap_or_default();
        let dst = a["digitalSourceType"].as_str();
        match (name, dst) {
            ("c2pa.created", Some(uri)) => {
                notes.push(format!("created as {}", uri.rsplit('/').next().unwrap_or(uri)));
                origin = Some(match source_of(uri) {
                    Source::Ai => Verdict::Ai,
                    Source::IncludesAi => Verdict::IncludesAi,
                    Source::NoAi => Verdict::NoAi,
                    Source::Empty if !components.is_empty() => Verdict::NoAi,
                    Source::Empty | Source::Unclear => Verdict::Unknown,
                });
            }
            ("c2pa.created", None) => {
                notes.push("created with no digitalSourceType".into());
                origin = Some(Verdict::Unknown);
            }
            ("c2pa.opened", _) => {
                let (v, why) = match ingredients.iter().find(|i| i["relationship"] == "parentOf") {
                    Some(parent) => ingredient(store, parent),
                    None => (Verdict::Unknown, "opened with no parent ingredient".into()),
                };
                notes.push(format!("opened parent: {why}"));
                origin = Some(v);
            }
            (_, Some(uri)) => {
                let kind = uri.rsplit('/').next().unwrap_or(uri);
                notes.push(format!("{name} as {kind}"));
                match source_of(uri) {
                    Source::Ai | Source::IncludesAi => ai_signal = true,
                    Source::Unclear => unclear = true,
                    Source::NoAi | Source::Empty => {}
                }
            }
            _ => {}
        }
    }
    for (v, why) in &components {
        notes.push(format!("component: {why}"));
        match v {
            Verdict::Ai | Verdict::IncludesAi => ai_signal = true,
            Verdict::Unknown => unclear = true,
            Verdict::NoAi => {}
        }
    }

    let origin = origin.unwrap_or_else(|| {
        notes.push("no created or opened action".into());
        Verdict::Unknown
    });
    let verdict = if origin == Verdict::Ai {
        Verdict::Ai
    } else if origin == Verdict::IncludesAi || ai_signal {
        Verdict::IncludesAi
    } else if origin == Verdict::Unknown || unclear {
        Verdict::Unknown
    } else {
        Verdict::NoAi
    };
    (verdict, notes.join("; "))
}
