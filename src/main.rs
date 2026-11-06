//! Generates the test set under `files/` and checks every file against its label.
//!
//! Run with `cargo run --release`. The run fails if the reference classifier disagrees with
//! any label, or if any manifest breaks a schema rule that `lint` checks, so `labels.json` is
//! always derivable from the manifests themselves.

mod classify;
mod lint;
mod sources;

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use c2pa::{
    assertions::{Action, ActionTemplate, Actions},
    Builder, BuilderIntent, ClaimGeneratorInfo, Context, HashedUri, Reader, Settings,
};
use classify::{Store, Verdict, C2PA, IPTC};
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

/// `c2pa.created` from a blank canvas: the content comes from the placed components.
fn empty_created() -> Value {
    json!({ "action": "c2pa.created", "digitalSourceType": format!("{C2PA}empty"), "softwareAgent": agent("Northwind Layout", "5.1") })
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
        "scientificDomain": ["cs.CV"],
        "contentProfile": { "humanOversightLevel": level },
    })
}

/// A disclosure that names a model but says nothing about what it produced.
fn model_disclosure(model_type: &str, name: &str) -> Value {
    json!({ "modelType": model_type, "modelName": name })
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
    /// For a deliberately invalid fixture: text every lint message must contain.
    expected_lint: Option<&'static str>,
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
        self.labels.push(Label { path: path.to_owned(), expected, story, known_sdk_issue: None, expected_lint: None });
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
    // Mark a manifest whose only action is c2pa.opened as allActionsIncluded: true.
    s["builder"]["actions"]["auto_all_actions_included"] = json!(true);
    s["builder"]["claim_generator_info"] = json!({ "name": GENERATOR, "version": env!("CARGO_PKG_VERSION") });
    // Keep the check offline and deterministic: u06's remote manifest is never published.
    s["verify"]["remote_manifest_fetch"] = json!(false);
    s["verify"]["verify_after_sign"] = json!(verify_after_sign);
    Ok(Settings::new().with_json(&s.to_string())?)
}

/// The hashed URI of a `componentOf` ingredient assertion in the active manifest of `path`.
///
/// `Reader` prints hashes as base64 text. Passing that text back through JSON would store its
/// bytes, not the hash, so this decodes it into a typed `HashedUri`.
fn component_ref(ctx: &Arc<Context>, path: &Path) -> Result<HashedUri> {
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
    let hash = BASE64.decode(hash.as_str().context("hash is not text")?)?;
    Ok(HashedUri::new(format!("self#jumbf=/c2pa/{manifest}/c2pa.assertions/{ing}"), None, &hash))
}

/// Opens the parent and deletes the component `target` from it, recording the reference either
/// in the action's `parameters` or through a template's deprecated `templateParameters`.
fn deleted_actions(target: HashedUri, via_template: bool) -> Result<Actions> {
    let editor: ClaimGeneratorInfo = serde_json::from_value(agent("Northwind Photo Editor", "12.3"))?;
    let mut deleted = Action::new("c2pa.deleted")
        .set_software_agent(editor)
        .set_description("Removed the placed AI element");
    let mut templates = None;
    if via_template {
        let mut template = ActionTemplate::new("c2pa.deleted");
        let ingredients = c2pa_cbor::value::to_value(vec![target])?;
        template.template_parameters = Some(HashMap::from([("ingredients".to_owned(), ingredients)]));
        templates = Some(vec![template]);
    } else {
        deleted = deleted.set_parameter("ingredients", vec![target])?;
    }
    let mut actions = Actions::new()
        .add_action(Action::new("c2pa.opened").set_parameter("ingredientIds", vec!["parent"])?)
        .add_action(deleted);
    actions.templates = templates;
    actions.all_actions_included = Some(true);
    Ok(actions)
}

/// The `Reader::json()` and `Reader::detailed_json()` values the classifier and lint read.
fn views(r: &Reader) -> Result<(Value, Value)> {
    Ok((serde_json::from_str(&r.json())?, serde_json::from_str(&r.detailed_json())?))
}

/// Removes failure status `code` everywhere in a store: active manifest and ingredients.
fn drop_status(v: &mut Value, code: &str) {
    match v {
        Value::Object(o) => {
            if let Some(Value::Array(f)) = o.get_mut("failure") {
                f.retain(|x| x["code"] != code);
            }
            if let Some(Value::Array(s)) = o.get_mut("validation_status") {
                s.retain(|x| x["code"] != code);
            }
            o.values_mut().for_each(|v| drop_status(v, code));
        }
        Value::Array(a) => a.iter_mut().for_each(|v| drop_status(v, code)),
        _ => {}
    }
}

/// The `w`×`h` region of `img` at (`x`, `y`).
fn photo_region(img: &image::RgbImage, x: u32, y: u32, w: u32, h: u32) -> image::RgbImage {
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

/// `cargo run --release -- FILE...` lints and classifies existing files instead of generating.
fn inspect(ctx: &Arc<Context>, paths: &[String]) -> Result<()> {
    for path in paths {
        let reader = Reader::from_shared_context(ctx).with_file(path);
        let views = reader.as_ref().ok().map(views).transpose()?;
        let (verdict, why) = classify::classify(views.as_ref().map(|(j, d)| Store { json: j, detailed: d }));
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
    sources::adjust(&dream).save(g.src("dream-adjusted.jpg"))?;
    image::imageops::resize(&photo, 1280, 960, image::imageops::FilterType::CatmullRom)
        .save(g.src("photo-upscaled.jpg"))?;
    let mut inpainted = dream.clone();
    sources::paste(&mut inpainted, &sources::dream(2.5), 380, 210, 200, 180);
    inpainted.save(g.src("dream-inpainted.jpg"))?;
    let mut annotated = dream.clone();
    sources::paste(&mut annotated, &sources::illustration(), 420, 300, 180, 140);
    annotated.save(g.src("dream-annotated.jpg"))?;
    let mut partly_removed = with_ai.clone();
    sources::paste(&mut partly_removed, &photo_region(&photo, 40, 40, 110, 160), 40, 40, 110, 160);
    partly_removed.save(g.src("photo-placed-partly-removed.jpg"))?;
    // i02's pixels cropped to the region away from its placed AI element.
    image::imageops::crop_imm(&with_ai, 280, 220, 360, 260).to_image().save(g.src("placed-cropped-no-ai.jpg"))?;
    let mut with_unsigned = dream.clone();
    sources::paste(&mut with_unsigned, &sources::photo(3), 360, 260, 240, 180);
    with_unsigned.save(g.src("dream-with-unsigned.jpg"))?;
    let mut photo_collage = photo.clone();
    sources::paste(&mut photo_collage, &sources::photo(2), 320, 0, 320, 480);
    photo_collage.save(g.src("photo-collage.jpg"))?;
    let mut collage = sources::illustration();
    sources::paste(&mut collage, &sources::dream(3.0), 300, 40, 300, 160);
    collage.save(g.src("collage.jpg"))?;
    let mut virtual_set = sources::photo(4);
    sources::paste(&mut virtual_set, &sources::dream(5.0), 0, 0, 640, 250);
    virtual_set.save(g.src("virtual.jpg"))?;
    sources::write_wav(&g.src("speech.wav"), true)?;
    sources::write_wav(&g.src("field.wav"), false)?;
    let have_mp4 = sources::write_mp4(&g.src("clip.mp4"), false).is_ok()
        && sources::write_mp4(&g.src("clip-audio.mp4"), true).is_ok();
    if !have_mp4 {
        eprintln!("warning: ffmpeg unavailable, skipping the MP4 cases");
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

    // c2pa-rs records the update manifest's c2pa.opened itself; auto_all_actions_included
    // (see settings) marks it allActionsIncluded: true, as the spec requires.
    g.sign_with(Ai, "a06_update_manifest_over_ai.jpg", &a01, &[],
        "Update manifest over a01 adding a review assertion; its only action is c2pa.opened, so the AI origin is inherited from the parent.", |b| {
            b.set_intent(BuilderIntent::Update);
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
        expected_lint: None,
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

    g.sign_with(Ai, "a10_ai_disclosure_human_validated.jpg", &g.src("dream.jpg"), &[],
        "trainedAlgorithmicMedia plus c2pa.ai-disclosure with human_validated: human approval doesn't make the content partly human-made.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated, then approved by an editor")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &disclosure("human_validated"))?;
            Ok(())
        })?;
    g.sign(Ai, "a11_empty_with_ai_component.jpg", &g.src("dream.jpg"),
        actions(vec![empty_created(), placed("ai-element", editor())], true),
        &[component("ai-element", &a01)],
        "Blank canvas (empty) whose only content is a01.")?;

    g.sign(Ai, "a12_ai_then_generative_inpaint.jpg", &g.src("dream-inpainted.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
            "softwareAgent": model(),
            "description": "Inpainted a region",
        })], true),
        &[parent(&a01)],
        "a01 with a region inpainted by generative AI. IPTC defines compositeWithTrainedAlgorithmicMedia as generative augmentation, so the content stays AI.")?;
    let reference = g.src("photo-unsigned.jpg");
    g.sign(Ai, "a13_text_to_image_with_reference_input.jpg", &g.src("dream.jpg"),
        actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a prompt and a reference image")], true),
        &[Ing { relationship: "inputTo", id: "reference", path: &reference, source_type: None }],
        "Text-to-image with an unsigned reference image as an inputTo ingredient. An input to the model is not placed content, so its unknown provenance doesn't matter.")?;

    g.sign_with(Ai, "a14_ai_minimal_disclosure.jpg", &g.src("dream.jpg"), &[],
        "trainedAlgorithmicMedia with the smallest valid c2pa.ai-disclosure: only modelType.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &json!({ "modelType": "c2pa.types.model" }))?;
            Ok(())
        })?;
    g.sign_with(Ai, "a15_ai_disclosure_empty_content_profile.jpg", &g.src("dream.jpg"), &[],
        "As a14 with an empty contentProfile map: humanOversightLevel is optional.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &json!({ "modelType": "c2pa.types.model", "contentProfile": {} }))?;
            Ok(())
        })?;
    g.sign_with(Ai, "a16_ai_two_model_disclosures.jpg", &g.src("dream.jpg"), &[],
        "Two models (a text encoder and a diffusion model), each with its own c2pa.ai-disclosure, linked from c2pa.created through relatedAssertions.", |b| {
            b.add_assertion("c2pa.ai-disclosure", &json!({
                "modelType": "c2pa.types.model.huggingface.transformers",
                "modelName": "Fabrikam Text Encoder",
                "scientificDomain": ["cs.CL"],
            }))?;
            b.add_assertion("c2pa.ai-disclosure", &disclosure("prompt_guided"))?;
            let mut action = created("trainedAlgorithmicMedia", model(), "Generated from a text prompt");
            action["parameters"] = json!({ "relatedAssertions": [{ "url": "c2pa.ai-disclosure" }, { "url": "c2pa.ai-disclosure__1" }] });
            b.add_assertion("c2pa.actions.v2", &actions(vec![action], true))?;
            Ok(())
        })?;

    // ---- no-ai: the declared, complete history has no trained model in the current content -
    let n01 = g.sign(NoAi, "n01_camera_capture.jpg", &g.src("photo.jpg"),
        actions(vec![created("digitalCapture", camera(), "Captured by camera")], true), &[],
        "Simulated camera capture: digitalCapture.")?;
    g.sign(NoAi, "n02_capture_cropped_and_adjusted.jpg", &g.src("photo-cropped-adjusted.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor()), json!({
            "action": "c2pa.adjustedColor",
            "digitalSourceType": dst("humanEdits"),
            "softwareAgent": editor(),
        })], true),
        &[parent(&n01)],
        "n01 cropped and colour-adjusted; only non-generative edits.")?;
    g.sign(NoAi, "n03_digital_illustration.png", &g.src("illustration.png"),
        actions(vec![created("digitalCreation", agent("Northwind Paint", "8.0"), "Drawn by hand")], true), &[],
        "Simulated hand-made illustration: digitalCreation.")?;
    g.sign(NoAi, "n04_hdr_computational_capture.jpg", &g.src("photo-hdr.jpg"),
        actions(vec![created("computationalCapture", camera(), "Automatic HDR merge of bracketed frames")], true), &[],
        "Simulated in-camera HDR: computationalCapture, which IPTC defines as frames merged by signal processing and/or non-generative AI.")?;
    g.sign(NoAi, "n08_composite_of_captures.jpg", &g.src("photo-collage.jpg"),
        actions(vec![created("compositeCapture", editor(), "Collage of two photographs")], true), &[],
        "Simulated collage of real-life captures: compositeCapture.")?;
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

    g.sign(IncludesAi, "i08_composite_synthetic.jpg", &g.src("collage.jpg"),
        actions(vec![created("compositeSynthetic", agent("Northwind Layout", "5.1"), "Layout with synthetic elements")], true), &[],
        "compositeSynthetic: IPTC defines it as a composite of several elements, at least one of which is generative AI.")?;
    g.sign(IncludesAi, "i09_generative_edit_in_related_action.jpg", &g.src("photo-filled.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "softwareAgent": editor(),
            "description": "Retouch",
            "related": [{
                "action": "c2pa.edited",
                "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
                "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
                "description": "Generative fill of a region",
            }],
        })], true),
        &[parent(&n01)],
        "n01 edited; the generative fill appears only in a related action, which consumers merge with its primary c2pa.edited.")?;
    g.sign(IncludesAi, "i10_ai_origin_incomplete_history.jpg", &g.src("dream-adjusted.jpg"),
        actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], false), &[],
        "trainedAlgorithmicMedia with allActionsIncluded: false. AI is present, but unrecorded changes may have added other content, so it can't be called entirely AI.")?;
    let unsigned_photo = g.src("photo-unsigned.jpg");
    g.sign(IncludesAi, "i11_ai_origin_unknown_component.jpg", &g.src("dream-with-unsigned.jpg"),
        actions(vec![opened(), placed("photo", editor())], true),
        &[parent(&a01), component("photo", &unsigned_photo)],
        "a01 with a component of unknown provenance placed into it: AI is present, the rest is unknown.")?;
    g.sign(IncludesAi, "i12_empty_with_mixed_components.jpg", &g.src("dream-with-photo.jpg"),
        actions(vec![empty_created(), placed("ai-element", editor()), placed("photo", editor())], true),
        &[component("ai-element", &a01), component("photo", &n01)],
        "Blank canvas (empty) with a01 and n01 placed on it.")?;

    g.sign(IncludesAi, "i13_ai_with_hand_drawn_annotation.jpg", &g.src("dream-annotated.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.drawing",
            "digitalSourceType": dst("humanEdits"),
            "softwareAgent": editor(),
            "description": "Hand-drawn annotation",
        })], true),
        &[parent(&a01)],
        "a01 with a hand-drawn annotation (c2pa.drawing, humanEdits): human content added without an ingredient.")?;
    g.sign(IncludesAi, "i14_ai_then_composite_synthetic_edit.jpg", &g.src("collage.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("compositeSynthetic"),
            "softwareAgent": agent("Northwind Layout", "5.1"),
            "description": "Combined with other elements",
        })], true),
        &[parent(&a01)],
        "a01 edited as compositeSynthetic, which declares a mix of elements, so it is no longer entirely AI.")?;
    g.sign(IncludesAi, "i15_mixed_cropped_away_from_ai.jpg", &g.src("placed-cropped-no-ai.jpg"),
        actions(vec![opened(), edit("c2pa.cropped", editor())], true),
        &[parent(&i02)],
        "i02 cropped so the placed AI element is outside the frame. A crop doesn't say what it removed, so the AI is still in the provenance and its removal isn't established.")?;

    let raw_ai_element = g.src("dream.jpg");
    let i16 = g.sign(IncludesAi, "i16_ai_declared_on_placed_action.jpg", &g.src("photo-placed.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.placed",
            "digitalSourceType": dst("trainedAlgorithmicMedia"),
            "softwareAgent": editor(),
            "parameters": { "ingredientIds": ["ai-element"] },
        })], true),
        &[parent(&n01), component("ai-element", &raw_ai_element)],
        "n01 with an unsigned element placed; only the c2pa.placed action declares it trainedAlgorithmicMedia.")?;
    g.sign(IncludesAi, "i17_ai_component_partly_deleted.jpg", &g.src("photo-placed-partly-removed.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.deleted",
            "softwareAgent": editor(),
            "description": "Removed part of the placed element",
        })], true),
        &[parent(&i02)],
        "i02 with part of its AI element deleted. The c2pa.deleted references no ingredient, so it doesn't establish the whole AI component was removed.")?;
    g.sign(IncludesAi, "i18_generative_fill_region.jpg", &g.src("photo-filled.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
            "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
            "description": "Generative fill of a region",
            "changes": [{ "region": [{ "type": "spatial", "shape": {
                "type": "rectangle", "unit": "pixel", "origin": { "x": 380, "y": 210 }, "width": 200, "height": 180,
            } }] }],
        })], true),
        &[parent(&n01)],
        "As i01, with the filled rectangle recorded in the action's changes. A region of AI is still AI in the content.")?;
    if have_mp4 {
        let clip = g.src("clip-audio.mp4");
        g.sign(IncludesAi, "i19_video_with_ai_voice_track.mp4", &clip,
            actions(vec![
                created("digitalCapture", camera(), "Recorded video"),
                json!({
                    "action": "c2pa.dubbed",
                    "digitalSourceType": dst("trainedAlgorithmicMedia"),
                    "softwareAgent": agent("Fabrikam Voice", "3.0"),
                    "description": "Replaced the audio track with synthetic speech",
                }),
            ], true),
            &[],
            "Captured video whose audio track was dubbed with synthetic speech: AI in one track of mixed media.")?;
    }
    g.sign_with(IncludesAi, "i20_ai_in_second_actions_assertion.jpg", &g.src("photo-filled.jpg"), &[parent(&n01)],
        "Two actions assertions; the generative fill is only in the second, so consumers must read all of them.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![opened()], true))?;
            b.add_assertion("c2pa.actions.v2", &actions(vec![json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
            "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
            "description": "Generative fill of a region",
        })], true))?;
            Ok(())
        })?;

    g.sign(IncludesAi, "i21_capture_generative_upscale.jpg", &g.src("photo-upscaled.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.resized",
            "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
            "softwareAgent": agent("Fabrikam Upscaler", "2.0"),
            "description": "Generative 2x upscale",
        })], true),
        &[parent(&n01)],
        "n01 upscaled by a generative model: resizing declared compositeWithTrainedAlgorithmicMedia.")?;

    let target = component_ref(&g.ctx, &i02)?;
    g.sign_with(NoAi, "n07_ai_component_deleted.jpg", &g.src("photo.jpg"), &[parent(&i02)],
        "i02 with its AI component removed, recorded by c2pa.deleted. AI is in the history but not in the current content.", |b| {
            b.add_assertion("c2pa.actions.v2", &deleted_actions(target.clone(), false)?)?;
            Ok(())
        })?;
    g.sign_with(NoAi, "n09_ai_component_deleted_via_template_parameters.jpg", &g.src("photo.jpg"), &[parent(&i02)],
        "As n07, but the deletion's ingredient reference comes from a template's deprecated templateParameters, which consumers must still merge.", |b| {
            b.add_assertion("c2pa.actions.v2", &deleted_actions(target, true)?)?;
            Ok(())
        })?;

    g.sign_with(NoAi, "n10_computational_capture_with_model_disclosure.jpg", &g.src("photo-hdr.jpg"), &[],
        "computationalCapture with a c2pa.ai-disclosure naming a scikit-learn model: non-generative AI that the source type already declares.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("computationalCapture", camera(), "Automatic HDR merge")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &model_disclosure("c2pa.types.model.sklearn", "Northwind Exposure Fusion"))?;
            Ok(())
        })?;

    g.sign(NoAi, "n11_capture_then_declared_human_edit.jpg", &g.src("photo-adjusted.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.edited",
            "digitalSourceType": dst("humanEdits"),
            "softwareAgent": editor(),
            "description": "Retouched by hand",
        })], true),
        &[parent(&n01)],
        "n01 edited with an explicit humanEdits source type: an affirmative non-generative declaration, unlike u16.")?;

    let i16_component = component_ref(&g.ctx, &i16)?;
    g.sign_with(NoAi, "n12_ai_placed_on_action_then_deleted.jpg", &g.src("photo.jpg"), &[parent(&i16)],
        "i16 with its placed element deleted. The AI was declared only on the c2pa.placed action, and that evidence goes with the component.", |b| {
            b.add_assertion("c2pa.actions.v2", &deleted_actions(i16_component, false)?)?;
            Ok(())
        })?;
    g.sign(NoAi, "n13_capture_enhanced_declared_non_generative.jpg", &g.src("photo-enhanced.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.enhanced",
            "digitalSourceType": dst("humanEdits"),
            "softwareAgent": editor(),
            "description": "Noise reduction and sharpening",
        })], true),
        &[parent(&n01)],
        "n01 enhanced, declared humanEdits (non-generative tools). Compare u18 and i21.")?;

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
        actions(vec![opened(), edit("c2pa.adjustedColor", editor())], true), &[parent(&unsigned)],
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

    g.sign_with(Unknown, "u14_capture_with_model_disclosure.jpg", &g.src("photo-adjusted.jpg"), &[],
        "digitalCapture with a c2pa.ai-disclosure, but no source type declares AI use: a model was used in some undeclared way.", |b| {
            b.add_assertion("c2pa.actions.v2", &actions(vec![created("digitalCapture", camera(), "Captured by camera")], true))?;
            b.add_assertion("c2pa.ai-disclosure", &model_disclosure("c2pa.types.model.pytorch", "Northwind Scene Model"))?;
            Ok(())
        })?;
    g.sign(Unknown, "u15_capture_then_custom_action.jpg", &g.src("photo-adjusted.jpg"),
        actions(vec![opened(), edit("com.northwindtraders.stylize", editor())], true), &[parent(&n01)],
        "n01 with an entity-specific action and no digitalSourceType. The history is complete, but what that action did is unknown.")?;
    g.sign(Unknown, "u16_capture_then_unspecified_edit.jpg", &g.src("photo-adjusted.jpg"),
        actions(vec![opened(), edit("c2pa.edited", editor())], true), &[parent(&n01)],
        "n01 then c2pa.edited with no source type. The spec doesn't require a generative edit to declare one, so a complete history still can't rule it out.")?;
    let redacted = component_ref(&g.ctx, &i02)?.url();
    let out = g.out(Unknown, "u17_ai_ingredient_redacted.jpg");
    let mut builder = g.builder(&out, &g.lenient)?;
    builder.add_ingredient_from_stream(
        json!({ "title": "i02_ai_element_placed.jpg", "relationship": "parentOf", "label": "parent" }).to_string(),
        "image/jpeg",
        &mut File::open(&i02)?,
    )?;
    builder.definition.redactions = Some(vec![redacted.clone()]);
    builder.add_assertion("c2pa.actions.v2", &actions(vec![opened(), json!({
        "action": "c2pa.redacted",
        "softwareAgent": editor(),
        "reason": "c2pa.trade-secret.present",
        "parameters": { "redacted": redacted },
    })], true))?;
    g.save(&mut builder, &g.src("photo-placed.jpg"), &out)?;
    g.labels.push(Label {
        path: out,
        expected: Unknown,
        story: "i02 opened and its AI ingredient assertion redacted. The AI content is still there; only its provenance is hidden, so redaction must not read as no AI.",
        known_sdk_issue: Some(("assertion.action.ingredientMismatch",
            "c2pa-rs rejects i02's c2pa.placed for referencing the redacted ingredient, which Validate_Assertions allows")),
        expected_lint: None,
    });
    g.sign(Unknown, "u18_capture_enhanced_undeclared.jpg", &g.src("photo-enhanced.jpg"),
        actions(vec![opened(), json!({
            "action": "c2pa.enhanced",
            "softwareAgent": editor(),
            "description": "Noise reduction and sharpening",
        })], true),
        &[parent(&n01)],
        "n01 enhanced with no source type. The action name doesn't say whether a generative model did it (as in AI upscaling), so a complete history can't establish no-ai.")?;
    g.sign_with(Unknown, "u19_update_adds_model_disclosure.jpg", &n01, &[],
        "Update manifest over n01 that adds a c2pa.ai-disclosure. The content is unchanged, but a model is now said to be involved in a way no source type declares.", |b| {
            b.set_intent(BuilderIntent::Update);
            b.add_assertion("c2pa.ai-disclosure", &model_disclosure("c2pa.types.model.pytorch", "Northwind Scene Model"))?;
            Ok(())
        })?;
    g.sign(Unknown, "u13_empty_with_unknown_component.jpg", &g.src("photo-unsigned.jpg"),
        actions(vec![empty_created(), placed("photo", editor())], true),
        &[component("photo", &unsigned_photo)],
        "Blank canvas (empty) whose only content has no manifest or declared source type.")?;

    // ---- invalid: deliberately malformed c2pa.ai-disclosure, for negative schema tests -----
    let bad = [
        ("x01_disclosure_missing_model_type.jpg", json!({ "modelName": "Fabrikam Diffusion" }), "modelType must be a text string"),
        ("x02_disclosure_invalid_oversight_level.jpg",
            json!({ "modelType": "c2pa.types.model", "contentProfile": { "humanOversightLevel": "mostly_autonomous" } }),
            "invalid humanOversightLevel"),
        ("x03_disclosure_scalar_scientific_domain.jpg",
            json!({ "modelType": "c2pa.types.model", "scientificDomain": "cs.CV" }), "invalid scientificDomain"),
        ("x04_disclosure_empty_scientific_domain.jpg",
            json!({ "modelType": "c2pa.types.model", "scientificDomain": [] }), "invalid scientificDomain"),
        ("x05_disclosure_content_profile_not_a_map.jpg",
            json!({ "modelType": "c2pa.types.model", "contentProfile": "bad" }), "contentProfile must be a map"),
        ("x06_disclosure_model_name_not_text.jpg",
            json!({ "modelType": "c2pa.types.model", "modelName": 42 }), "modelName must be a text string"),
        ("x07_disclosure_null_scientific_domain.jpg",
            json!({ "modelType": "c2pa.types.model", "scientificDomain": null }), "invalid scientificDomain null"),
        ("x08_disclosure_unknown_model_type.jpg",
            json!({ "modelType": "nonsense" }), "vocabulary: modelType nonsense"),
        ("x09_disclosure_unknown_arxiv_category.jpg",
            json!({ "modelType": "c2pa.types.model", "scientificDomain": ["not.ACategory"] }), "is not an arXiv category"),
    ];
    for (name, data, expect) in bad {
        let out = root.join("files/invalid").join(name);
        let mut builder = g.builder(&out, &g.ctx)?;
        builder.add_assertion("c2pa.actions.v2", &actions(vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], true))?;
        builder.add_assertion("c2pa.ai-disclosure", &data)?;
        g.save(&mut builder, &g.src("dream.jpg"), &out)?;
        g.labels.push(Label {
            path: out,
            expected: Ai,
            story: "Valid trainedAlgorithmicMedia file with a malformed c2pa.ai-disclosure (schema) or one outside the required vocabularies; for negative tests, not classification.",
            known_sdk_issue: None,
            expected_lint: Some(expect),
        });
    }

    check(&g, &root)
}

/// Reads every generated file, lints and classifies it, writes `labels.json`, and fails on
/// any lint problem or label mismatch.
fn check(g: &Gen, root: &Path) -> Result<()> {
    let mut rows = Vec::new();
    let mut problems = 0;
    for l in &g.labels {
        let reader = Reader::from_shared_context(&g.ctx).with_file(&l.path);
        let views = reader.as_ref().ok().map(views).transpose()?;
        let state = match &reader {
            Ok(r) => format!("{:?}", r.validation_state()),
            Err(e) => format!("unreadable: {e}"),
        };
        let rel = l.path.strip_prefix(root)?.to_string_lossy().into_owned();
        let lint = views.as_ref().map(|(_, d)| lint::lint(d)).unwrap_or_default();
        for msg in &lint {
            println!("LINT {rel}: {msg}");
        }

        let store = views.as_ref().map(|(j, d)| Store { json: j, detailed: d });
        let (sdk_verdict, sdk_why) = classify::classify(store);
        // For a known SDK issue, the label must follow once that one code is set aside.
        let (got, why, status) = match (l.known_sdk_issue, &views) {
            (Some((code, _)), Some((j, d))) => {
                let mut j = j.clone();
                drop_status(&mut j, code);
                let (v, why) = classify::classify(Some(Store { json: &j, detailed: d }));
                (v, why, if v == l.expected && sdk_verdict != l.expected { "KNOWN" } else { "ok" })
            }
            _ => (sdk_verdict, sdk_why.clone(), "ok"),
        };
        let lint_ok = match l.expected_lint {
            None => lint.is_empty(),
            Some(text) => !lint.is_empty() && lint.iter().all(|m| m.contains(text)),
        };
        let status = if l.expected_lint.is_some() && lint_ok { "INVAL" } else { status };
        let ok = got == l.expected && lint_ok;
        problems += usize::from(!ok);
        println!("{:<5} {:<12} {:<55} {}", if ok { status } else { "FAIL" }, got.dir(), rel, why);

        let mut row = json!({
            "file": rel,
            "expected": l.expected.dir(),
            "description": l.story,
            "validationState": state,
            "classifierReason": why,
        });
        if let Some(text) = l.expected_lint {
            row["expectedLint"] = json!(text);
        }
        if let Some((code, note)) = l.known_sdk_issue {
            row["knownSdkIssue"] = json!({ "code": code, "note": note, "sdkVerdict": sdk_verdict.dir() });
        }
        rows.push(row);
    }
    fs::write(root.join("labels.json"), serde_json::to_string_pretty(&rows)? + "\n")?;
    anyhow::ensure!(problems == 0, "{problems} file(s) failed lint or disagree with their label");
    println!("\n{} files, all labels confirmed, no unexpected lint problems", rows.len());
    Ok(())
}
