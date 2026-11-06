//! Generates the test set under `files/` and checks every file against its label.
//!
//! Run with `cargo run --release`. The run fails if the reference classifier disagrees with
//! any label, or if any manifest breaks a schema rule that `lint` checks, so `labels.json` is
//! always derivable from the manifests themselves.

mod classify;
mod lint;
mod sources;

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use c2pa::{Builder, BuilderIntent, Context, Reader, Settings};
use classify::{Verdict, IPTC};
use serde_json::{json, Value};

const GENERATOR: &str = "c2pa-ai-examples";
const REMOTE_URL: &str = "https://northwindtraders.com/c2pa/manifests/u06-not-published.c2pa";

fn dst(kind: &str) -> String {
    format!("{IPTC}{kind}")
}

fn agent(name: &str, version: &str) -> Value {
    json!({ "name": name, "version": version })
}

/// A v2 actions assertion. `complete` is `allActionsIncluded`.
fn actions(list: Vec<Value>, complete: bool) -> Value {
    json!({ "actions": list, "allActionsIncluded": complete })
}

fn created(kind: &str, software: Value, description: &str) -> Value {
    json!({
        "action": "c2pa.created",
        "digitalSourceType": dst(kind),
        "softwareAgent": software,
        "description": description,
    })
}

fn opened() -> Value {
    json!({ "action": "c2pa.opened", "parameters": { "ingredientIds": ["parent"] } })
}

fn edit(action: &str, software: Value) -> Value {
    json!({ "action": action, "softwareAgent": software })
}

fn placed(id: &str, software: Value) -> Value {
    json!({ "action": "c2pa.placed", "softwareAgent": software, "parameters": { "ingredientIds": [id] } })
}

fn disclosure(level: &str) -> Value {
    json!({
        "modelType": "c2pa.types.model",
        "modelName": "Fabrikam Diffusion",
        "modelIdentifier": "pkg:generic/fabrikam/diffusion@4.2",
        "contentProfile": { "humanOversightLevel": level },
    })
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg") => "image/jpeg",
        Some("png") => "image/png",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        other => panic!("no media type for {other:?}"),
    }
}

/// An ingredient for [`Gen::sign`].
struct Ing<'a> {
    relationship: &'a str,
    /// Referenced from actions through `ingredientIds`.
    id: &'a str,
    path: &'a Path,
    /// Declared on an ingredient that has no manifest of its own.
    source_type: Option<String>,
}

fn parent(path: &Path) -> Ing<'_> {
    Ing { relationship: "parentOf", id: "parent", path, source_type: None }
}

fn component<'a>(id: &'a str, path: &'a Path) -> Ing<'a> {
    Ing { relationship: "componentOf", id, path, source_type: None }
}

struct Label {
    path: PathBuf,
    expected: Verdict,
    story: &'static str,
    /// A status code the SDK reports against the specification; see the README.
    known_sdk_issue: Option<(&'static str, &'static str)>,
}

struct Gen {
    ctx: Arc<Context>,
    /// Same settings without verify-after-sign, for fixtures the SDK misjudges.
    lenient: Arc<Context>,
    root: PathBuf,
    build: PathBuf,
    labels: Vec<Label>,
}

impl Gen {
    fn out(&self, category: Verdict, name: &str) -> PathBuf {
        self.root.join("files").join(category.dir()).join(name)
    }

    fn src(&self, name: &str) -> PathBuf {
        self.build.join(name)
    }

    fn label(&mut self, path: &Path, expected: Verdict, story: &'static str) {
        self.labels.push(Label { path: path.to_owned(), expected, story, known_sdk_issue: None });
    }

    /// Signs `src` with a v2 actions assertion and the given ingredients.
    fn sign(
        &mut self,
        category: Verdict,
        name: &str,
        src: &Path,
        actions: Value,
        ingredients: &[Ing],
        story: &'static str,
    ) -> Result<PathBuf> {
        self.sign_with(category, name, src, ingredients, story, |b| {
            b.add_assertion("c2pa.actions.v2", &actions)?;
            Ok(())
        })
    }

    /// Signs `src` after `f` has added whatever assertions the case needs.
    fn sign_with(
        &mut self,
        category: Verdict,
        name: &str,
        src: &Path,
        ingredients: &[Ing],
        story: &'static str,
        f: impl FnOnce(&mut Builder) -> Result<()>,
    ) -> Result<PathBuf> {
        let out = self.out(category, name);
        let mut builder = self.builder(&out, &self.ctx)?;
        for ing in ingredients {
            let title = ing.path.file_name().unwrap().to_string_lossy();
            let mut def = json!({ "title": title, "relationship": ing.relationship, "label": ing.id });
            if let Some(uri) = &ing.source_type {
                def["digital_source_type"] = json!(uri);
            }
            builder.add_ingredient_from_stream(def.to_string(), mime(ing.path), &mut File::open(ing.path)?)?;
        }
        f(&mut builder)?;
        self.save(&mut builder, src, &out)?;
        self.label(&out, category, story);
        Ok(out)
    }

    fn builder(&self, out: &Path, ctx: &Arc<Context>) -> Result<Builder> {
        let title = out.file_name().unwrap().to_string_lossy();
        Ok(Builder::from_shared_context(ctx).with_definition(
            json!({
                "title": title,
                "format": mime(out),
                "claim_generator_info": [{ "name": GENERATOR, "version": env!("CARGO_PKG_VERSION") }],
            })
            .to_string(),
        )?)
    }

    fn save(&self, builder: &mut Builder, src: &Path, out: &Path) -> Result<Vec<u8>> {
        fs::create_dir_all(out.parent().unwrap())?;
        let mut dest = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(out)?;
        builder
            .save_to_stream(mime(src), &mut File::open(src)?, &mut dest)
            .with_context(|| format!("signing {}", out.display()))
    }
}

fn settings(root: &Path, verify_after_sign: bool) -> Result<Settings> {
    let path = root.join("settings/c2pa-test-settings.json");
    let mut s: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    // Every action in this set is written explicitly; nothing may be inferred.
    for auto in ["auto_created_action", "auto_opened_action", "auto_placed_action"] {
        s["builder"]["actions"][auto]["enabled"] = json!(false);
    }
    s["builder"]["claim_generator_info"] = json!({ "name": GENERATOR, "version": env!("CARGO_PKG_VERSION") });
    // Keep the check offline and deterministic: u06's remote manifest is never published.
    s["verify"]["remote_manifest_fetch"] = json!(false);
    s["verify"]["verify_after_sign"] = json!(verify_after_sign);
    Ok(Settings::new().with_json(&s.to_string())?)
}

/// The hashed URI of a `componentOf` ingredient assertion in the active manifest of `path`.
fn component_ref(ctx: &Arc<Context>, path: &Path) -> Result<Value> {
    let reader = Reader::from_shared_context(ctx).with_file(path)?;
    let manifest = reader.active_label().context("no active manifest")?.to_owned();
    let store: Value = serde_json::from_str(&reader.json())?;
    let ing = store["manifests"][&manifest]["ingredients"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["relationship"] == "componentOf")
        .and_then(|i| i["label"].as_str())
        .context("no componentOf ingredient")?
        .to_owned();
    fn find_hash(v: &Value, suffix: &str) -> Option<Value> {
        match v {
            Value::Object(o) if o.get("url").and_then(Value::as_str).is_some_and(|u| u.ends_with(suffix)) => {
                o.get("hash").cloned()
            }
            Value::Object(o) => o.values().find_map(|v| find_hash(v, suffix)),
            Value::Array(a) => a.iter().find_map(|v| find_hash(v, suffix)),
            _ => None,
        }
    }
    let detailed: Value = serde_json::from_str(&reader.detailed_json())?;
    let hash = find_hash(&detailed["manifests"][&manifest], &format!("c2pa.assertions/{ing}"))
        .context("ingredient hash not found")?;
    Ok(json!({ "url": format!("self#jumbf=/c2pa/{manifest}/c2pa.assertions/{ing}"), "hash": hash }))
}

/// `cargo run --release -- FILE...` lints and classifies existing files instead of generating.
fn inspect(ctx: &Arc<Context>, paths: &[String]) -> Result<()> {
    for path in paths {
        let reader = Reader::from_shared_context(ctx).with_file(path);
        let store = reader.as_ref().ok().and_then(|r| serde_json::from_str::<Value>(&r.json()).ok());
        let (verdict, why) = classify::classify(store.as_ref());
        println!("{:<12} {path}\n             {why}", verdict.dir());
        if let Ok(r) = &reader {
            for msg in lint::lint(&serde_json::from_str(&r.detailed_json())?) {
                println!("             LINT {msg}");
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return inspect(&Context::new().with_settings(settings(&root, true)?)?.into_shared(), &args);
    }
    let build = root.join("target/sources");
    fs::create_dir_all(&build)?;
    let _ = fs::remove_dir_all(root.join("files"));
    let ctx = Context::new().with_settings(settings(&root, true)?)?.into_shared();
    let lenient = Context::new().with_settings(settings(&root, false)?)?.into_shared();
    let mut g = Gen { ctx, lenient, root: root.clone(), build, labels: Vec::new() };

    // ---- unsigned sources ------------------------------------------------------------------
    // Procedural stand-ins. Each one has the pixels the manifest's history describes (a crop
    // is cropped, a fill is filled), but none is a real camera capture or model output.
    let dream = sources::dream(0.0);
    let photo = sources::photo(1);
    dream.save(g.src("dream.jpg"))?;
    dream.save(g.src("dream.png"))?;
    sources::crop(&dream).save(g.src("dream-cropped.jpg"))?;
    photo.save(g.src("photo.jpg"))?;
    sources::adjust(&sources::crop(&photo)).save(g.src("photo-cropped-adjusted.jpg"))?;
    sources::adjust(&photo).save(g.src("photo-enhanced.jpg"))?;
    sources::adjust(&photo).save(g.src("photo-adjusted.jpg"))?;
    sources::crop(&photo).save(g.src("photo-cropped.jpg"))?;
    sources::photo(2).save(g.src("photo-hdr.jpg"))?;
    sources::photo(3).save(g.src("photo-unsigned.jpg"))?;
    sources::adjust(&sources::photo(3)).save(g.src("photo-unsigned-adjusted.jpg"))?;
    sources::illustration().save(g.src("illustration.png"))?;
    sources::fractal().save(g.src("fractal.png"))?;
    sources::screen().save(g.src("screen.png"))?;
    let mut filled = photo.clone();
    sources::paste(&mut filled, &sources::dream(1.5), 380, 210, 200, 180);
    filled.save(g.src("photo-filled.jpg"))?;
    sources::crop(&filled).save(g.src("photo-filled-cropped.jpg"))?;
    let mut with_ai = photo.clone();
    sources::paste(&mut with_ai, &dream, 40, 40, 220, 160);
    with_ai.save(g.src("photo-placed.jpg"))?;
    let mut with_photo = dream.clone();
    sources::paste(&mut with_photo, &photo, 360, 260, 240, 180);
    with_photo.save(g.src("dream-with-photo.jpg"))?;
    let mut collage = sources::illustration();
    sources::paste(&mut collage, &sources::dream(3.0), 300, 40, 300, 160);
    collage.save(g.src("collage.jpg"))?;
    let mut virtual_set = sources::photo(4);
    sources::paste(&mut virtual_set, &sources::dream(5.0), 0, 0, 640, 250);
    virtual_set.save(g.src("virtual.jpg"))?;
    sources::write_wav(&g.src("speech.wav"), true)?;
    sources::write_wav(&g.src("field.wav"), false)?;
    let have_mp4 = sources::write_mp4(&g.src("clip.mp4")).is_ok();
    if !have_mp4 {
        eprintln!("warning: ffmpeg unavailable, skipping a04_text_to_video.mp4");
    }

    let model = || agent("Fabrikam Diffusion", "4.2");
    let camera = || agent("Northwind Camera Firmware", "2.0.1");
    let editor = || agent("Northwind Photo Editor", "12.3");

    use Verdict::*;

    // ---- ai: the provenance declares the whole current content model-generated ------------
    let a01 = g.sign(Ai, "a01_text_to_image.jpg", &g.src("dream.jpg"),
        actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true), &[],
        "Simulated text-to-image output: c2pa.created with trainedAlgorithmicMedia.")?;
    g.sign(Ai, "a02_text_to_image.png", &g.src("dream.png"),
        actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true), &[],
        "Same as a01 in PNG.")?;
    g.sign(Ai, "a03_ai_then_cropped.jpg", &g.src("dream-cropped.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor())], true), &[parent(&a01)],
        "a01 cropped. The active manifest has no AI source type; the AI origin is only in the parent ingredient's manifest.")?;
    if have_mp4 {
        g.sign(Ai, "a04_text_to_video.mp4", &g.src("clip.mp4"),
            actions(vec![created("trainedAlgorithmicMedia", agent("Fabrikam Video Model", "1.0"), "Generated from a text prompt")], true), &[],
            "Simulated text-to-video output in MP4.")?;
    }
    g.sign(Ai, "a05_text_to_speech.wav", &g.src("speech.wav"),
        actions(vec![created("trainedAlgorithmicMedia", agent("Fabrikam Voice", "3.0"), "Synthesised speech")], true), &[],
        "Simulated synthetic speech in WAV.")?;

    // c2pa-rs always writes c2pa.opened into an update manifest, and its own inserted actions
    // assertion has no allActionsIncluded, so this one records the opened action itself.
    g.sign_with(Ai, "a06_update_manifest_over_ai.jpg", &a01, &[parent(&a01)],
        "Update manifest over a01 adding a review assertion; its only action is c2pa.opened, so the AI origin is inherited from the parent.", |b| {
            b.set_intent(BuilderIntent::Update);
            b.add_assertion("c2pa.actions.v2", &actions(vec![opened()], true))?;
            b.add_assertion("com.northwindtraders.review", &json!({ "status": "approved" }))?;
            Ok(())
        })?;

    let out = g.out(Ai, "a07_source_type_from_template.jpg");
    let mut builder = g.builder(&out, &g.lenient)?;
    builder.add_assertion("c2pa.actions.v2", &json!({
        "actions": [{ "action": "c2pa.created", "description": "Generated from a text prompt" }],
        "templates": [{ "action": "c2pa.created", "digitalSourceType": dst("trainedAlgorithmicMedia"), "softwareAgent": model() }],
        "allActionsIncluded": true,
    }))?;
    g.save(&mut builder, &g.src("dream.jpg"), &out)?;
    g.labels.push(Label {
        path: out,
        expected: Ai,
        story: "c2pa.created takes its trainedAlgorithmicMedia source type from a template, which consumers must merge.",
        known_sdk_issue: Some(("assertion.action.malformed",
            "c2pa-rs checks c2pa.created for a digitalSourceType before applying templates")),
    });

    g.sign_with(Ai, "a08_ai_disclosure_fully_autonomous.jpg", &g.src("dream.jpg"), &[],
        "trainedAlgorithmicMedia plus c2pa.ai-disclosure with humanOversightLevel fully_autonomous.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated without review")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &disclosure("fully_autonomous"))?;
            Ok(())
        })?;
    g.sign_with(Ai, "a09_ai_disclosure_prompt_guided.jpg", &g.src("dream.jpg"), &[],
        "trainedAlgorithmicMedia plus c2pa.ai-disclosure with humanOversightLevel prompt_guided.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &disclosure("prompt_guided"))?;
            Ok(())
        })?;

    // ---- no-ai: the declared, complete history has no trained model in the current content -
    let n01 = g.sign(NoAi, "n01_camera_capture.jpg", &g.src("photo.jpg"),
        actions(vec![created("digitalCapture", camera(), "Captured by camera")], true), &[],
        "Simulated camera capture: digitalCapture.")?;
    g.sign(NoAi, "n02_capture_cropped_and_adjusted.jpg", &g.src("photo-cropped-adjusted.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor()), edit("c2pa.color_adjustments", editor())], true),
        &[parent(&n01)],
        "n01 cropped and colour-adjusted; only non-generative edits.")?;
    g.sign(NoAi, "n03_digital_illustration.png", &g.src("illustration.png"),
        actions(vec![created("digitalCreation", agent("Northwind Paint", "8.0"), "Drawn by hand")], true), &[],
        "Simulated hand-made illustration: digitalCreation.")?;
    g.sign(NoAi, "n04_hdr_composite_capture.jpg", &g.src("photo-hdr.jpg"),
        actions(vec![created("compositeCapture", camera(), "HDR merge of bracketed exposures")], true), &[],
        "Simulated HDR merge: compositeCapture.")?;
    g.sign(NoAi, "n05_field_recording.wav", &g.src("field.wav"),
        actions(vec![created("digitalCapture", agent("Northwind Recorder", "1.4"), "Recorded on location")], true), &[],
        "Simulated audio capture: digitalCapture.")?;
    g.sign(NoAi, "n06_procedural_fractal.png", &g.src("fractal.png"),
        actions(vec![created("algorithmicMedia", agent("Northwind Fractal Renderer", "0.9"), "Rendered Mandelbrot set")], true), &[],
        "Edge case: algorithmicMedia is algorithmic but, per IPTC, not based on sampled training data.")?;

    // ---- includes-ai: the provenance declares AI in part of the current content -----------
    let i01 = g.sign(IncludesAi, "i01_generative_fill.jpg", &g.src("photo-filled.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
            "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
            "description": "Generative fill of a region",
        })], true),
        &[parent(&n01)],
        "n01 with a region replaced by generative fill: c2pa.edited with compositeWithTrainedAlgorithmicMedia.")?;
    let i02 = g.sign(IncludesAi, "i02_ai_element_placed.jpg", &g.src("photo-placed.jpg"),
        actions(vec![opened(), placed("ai-element", editor())], true),
        &[parent(&n01), component("ai-element", &a01)],
        "n01 with a01 placed into it. No action has an AI source type; the AI is only in the componentOf ingredient's manifest.")?;
    g.sign(IncludesAi, "i03_created_with_ai_elements.jpg", &g.src("collage.jpg"),
        actions(vec![created("compositeWithTrainedAlgorithmicMedia", agent("Northwind Layout", "5.1"), "Layout with AI-generated artwork")], true), &[],
        "Created directly as compositeWithTrainedAlgorithmicMedia.")?;
    g.sign(IncludesAi, "i04_generative_fill_then_cropped.jpg", &g.src("photo-filled-cropped.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor())], true), &[parent(&i01)],
        "i01 cropped. The AI edit is one manifest back in the history.")?;
    let raw_ai = g.src("dream.jpg");
    g.sign(IncludesAi, "i05_unsigned_ai_ingredient_declared.jpg", &g.src("photo-placed.jpg"),
        actions(vec![opened(), placed("ai-element", editor())], true),
        &[parent(&n01), Ing { relationship: "componentOf", id: "ai-element", path: &raw_ai,
            source_type: Some(dst("trainedAlgorithmicMedia")) }],
        "n01 with a placed ingredient that has no manifest but declares trainedAlgorithmicMedia on the ingredient assertion.")?;
    g.sign(IncludesAi, "i06_ai_parent_with_photo_placed.jpg", &g.src("dream-with-photo.jpg"),
        actions(vec![opened(), placed("photo", editor())], true),
        &[parent(&a01), component("photo", &n01)],
        "a01 with the camera photo n01 placed into it: AI origin, but no longer entirely AI.")?;
    g.sign_with(IncludesAi, "i07_ai_disclosure_human_validated.jpg", &g.src("photo-filled.jpg"), &[parent(&n01)],
        "Generative fill (compositeWithTrainedAlgorithmicMedia) plus c2pa.ai-disclosure with humanOversightLevel human_validated.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![opened(), json!({
                "action": "c2pa.edited",
                "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
                "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
                "description": "Generative fill, reviewed by an editor",
            })], true))?;
            b.add_assertion("c2pa.ai-disclosure", &disclosure("human_validated"))?;
            Ok(())
        })?;

    let deleted = component_ref(&g.ctx, &i02)?;
    g.sign(NoAi, "n07_ai_component_deleted.jpg", &g.src("photo.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.deleted",
            "softwareAgent": editor(),
            "description": "Removed the placed AI element",
            "parameters": { "ingredients": [deleted] },
        })], true),
        &[parent(&i02)],
        "i02 with its AI component removed, recorded by c2pa.deleted. AI is in the history but not in the current content.")?;

    // ---- unknown: the provenance cannot settle the question -------------------------------
    let u01 = g.out(Unknown, "u01_no_manifest.jpg");
    fs::create_dir_all(u01.parent().unwrap())?;
    fs::copy(g.src("photo-unsigned.jpg"), &u01)?;
    g.label(&u01, Unknown, "No C2PA manifest at all.");

    let u02 = g.out(Unknown, "u02_ai_with_manifest_stripped.jpg");
    image::open(&a01)?.save(&u02)?;
    g.label(&u02, Unknown, "Represents an AI-origin asset whose manifest was lost: a01 re-encoded, which drops it.");

    // The Builder writes every actions assertion as c2pa.actions.v2, so the legacy case is a
    // genuine 1.x file from the c2pa-rs fixtures rather than one made here.
    let u03 = g.out(Unknown, "u03_legacy_v1_actions.jpg");
    fs::copy(root.join("../c2pa-rs/sdk/tests/fixtures/C.jpg"), &u03)?;
    g.label(&u03, Unknown, "c2pa-rs fixture C.jpg: a v1 claim whose v1 c2pa.actions declares algorithmicMedia. v1 has no allActionsIncluded, so unrecorded actions may have occurred.");

    let unsigned = g.src("photo-unsigned.jpg");
    g.sign(Unknown, "u04_edited_from_unsigned_parent.jpg", &g.src("photo-unsigned-adjusted.jpg"),
        actions(vec![opened(), edit("c2pa.color_adjustments", editor())], true), &[parent(&unsigned)],
        "Signed edit of a parent that has neither a manifest nor a declared source type.")?;

    let u05 = g.out(Unknown, "u05_tampered_capture.jpg");
    let mut bytes = fs::read(&n01)?;
    let at = bytes.len() - 300; // inside the entropy-coded scan, after the manifest
    bytes[at] ^= 0xFF;
    fs::write(&u05, bytes)?;
    g.label(&u05, Unknown, "n01 with image bytes altered after signing; the manifest declares digitalCapture but the hard binding fails.");

    let out = g.out(Unknown, "u06_remote_manifest_unavailable.jpg");
    let mut builder = g.builder(&out, &g.ctx)?;
    builder.add_assertion("c2pa.actions.v2", &actions(vec![created("digitalCapture", camera(), "Captured by camera")], true))?;
    builder.set_remote_url(REMOTE_URL).set_no_embed(true);
    let manifest = g.save(&mut builder, &g.src("photo.jpg"), &out)?;
    fs::write(g.build.join("u06-not-published.c2pa"), manifest)?;
    g.label(&out, Unknown, "Points to a remote manifest (XMP dcterms:provenance) that was never published, and embeds none.");

    g.sign(Unknown, "u07_composite_synthetic.jpg", &g.src("collage.jpg"),
        actions(vec![created("compositeSynthetic", agent("Northwind Layout", "5.1"), "Layout with synthetic elements")], true), &[],
        "compositeSynthetic says synthetic elements are present, not whether a trained model made them.")?;
    g.sign(Unknown, "u08_algorithmically_enhanced.jpg", &g.src("photo-enhanced.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("algorithmicallyEnhanced"),
            "softwareAgent": agent("Northwind Enhance", "2.2"),
            "description": "Automatic enhancement",
        })], true),
        &[parent(&n01)],
        "n01 with an algorithmicallyEnhanced edit; the type does not say whether the algorithm was a trained model.")?;
    g.sign(Unknown, "u09_edited_from_tampered_parent.jpg", &g.src("photo-cropped.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor())], true), &[parent(&u05)],
        "Valid crop of u05; the parent's digitalCapture declaration is untrustworthy because its validation failed.")?;
    g.sign(Unknown, "u10_incomplete_history_capture.jpg", &g.src("photo-adjusted.jpg"),
        actions(vec![created("digitalCapture", camera(), "Captured by camera")], false), &[],
        "digitalCapture with allActionsIncluded: false, so unrecorded (possibly generative) actions may have occurred.")?;
    g.sign(Unknown, "u11_screen_capture.png", &g.src("screen.png"),
        actions(vec![created("screenCapture", agent("Northwind Snip", "3.0"), "Screenshot")], true), &[],
        "screenCapture says how the screen was captured, not where the displayed content came from.")?;
    g.sign(Unknown, "u12_virtual_recording.jpg", &g.src("virtual.jpg"),
        actions(vec![created("virtualRecording", agent("Northwind Studio", "1.0"), "Recording of a virtual set")], true), &[],
        "virtualRecording allows synthetic, possibly generative, elements.")?;

    check(&g, &root)
}

/// Reads every generated file, lints and classifies it, writes `labels.json`, and fails on
/// any lint problem or label mismatch.
fn check(g: &Gen, root: &Path) -> Result<()> {
    let mut rows = Vec::new();
    let mut problems = 0;
    for l in &g.labels {
        let reader = Reader::from_shared_context(&g.ctx).with_file(&l.path);
        let store = reader.as_ref().ok().and_then(|r| serde_json::from_str::<Value>(&r.json()).ok());
        let state = match &reader {
            Ok(r) => format!("{:?}", r.validation_state()),
            Err(e) => format!("unreadable: {e}"),
        };
        let rel = l.path.strip_prefix(root)?.to_string_lossy().into_owned();
        let lint = match &reader {
            Ok(r) => lint::lint(&serde_json::from_str(&r.detailed_json())?),
            Err(_) => Vec::new(),
        };
        for msg in &lint {
            println!("LINT {rel}: {msg}");
        }

        let (sdk_verdict, sdk_why) = classify::classify(store.as_ref());
        // For a known SDK issue, the label must follow once that one code is set aside.
        let (got, why, status) = match (l.known_sdk_issue, &store) {
            (Some((code, _)), Some(s)) => {
                let mut s = s.clone();
                if let Some(f) = s["validation_results"]["activeManifest"]["failure"].as_array_mut() {
                    f.retain(|x| x["code"] != code);
                }
                let (v, why) = classify::classify(Some(&s));
                (v, why, if v == l.expected && sdk_verdict != l.expected { "KNOWN" } else { "ok" })
            }
            _ => (sdk_verdict, sdk_why.clone(), "ok"),
        };
        let ok = got == l.expected && lint.is_empty();
        problems += usize::from(!ok);
        println!("{:<5} {:<12} {:<55} {}", if ok { status } else { "FAIL" }, got.dir(), rel, why);

        let mut row = json!({
            "file": rel,
            "expected": l.expected.dir(),
            "description": l.story,
            "validationState": state,
            "classifierReason": why,
        });
        if let Some((code, note)) = l.known_sdk_issue {
            row["knownSdkIssue"] = json!({ "code": code, "note": note, "sdkVerdict": sdk_verdict.dir() });
        }
        rows.push(row);
    }
    fs::write(root.join("labels.json"), serde_json::to_string_pretty(&rows)? + "\n")?;
    anyhow::ensure!(problems == 0, "{problems} file(s) failed lint or disagree with their label");
    println!("\n{} files, all labels confirmed, no lint problems", rows.len());
    Ok(())
}
