//! Generates the test set under `files/` and checks every file against its label.
//!
//! Run with `cargo run --release`. The run fails if the reference classifier disagrees with
//! any label, so `labels.json` is always derivable from the manifests themselves.

mod classify;
mod sources;

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use c2pa::{Builder, Context, Reader, Settings};
use classify::Verdict;
use serde_json::{json, Value};

const GENERATOR: &str = "c2pa-ai-examples";
const REMOTE_URL: &str = "https://northwindtraders.com/c2pa/manifests/u06-not-published.c2pa";

fn dst(kind: &str) -> String {
    format!("http://cv.iptc.org/newscodes/digitalsourcetype/{kind}")
}

fn agent(name: &str, version: &str) -> Value {
    json!({ "name": name, "version": version })
}

fn created(kind: &str, software: Value, description: &str) -> Value {
    json!({
        "action": "c2pa.created",
        "digitalSourceType": dst(kind),
        "softwareAgent": software,
        "description": description,
    })
}

fn opened(parent: &str) -> Value {
    json!({ "action": "c2pa.opened", "parameters": { "ingredientIds": [parent] } })
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

/// An ingredient for [`Gen::sign`]: relationship, label used by `ingredientIds`, source file.
struct Ing<'a>(&'a str, &'a str, &'a Path);

struct Gen {
    ctx: Arc<Context>,
    root: PathBuf,
    build: PathBuf,
    labels: Vec<(PathBuf, Verdict, &'static str)>,
}

impl Gen {
    fn out(&self, category: Verdict, name: &str) -> PathBuf {
        self.root.join("files").join(category.dir()).join(name)
    }

    fn src(&self, name: &str) -> PathBuf {
        self.build.join(name)
    }

    fn label(&mut self, path: &Path, expected: Verdict, story: &'static str) {
        self.labels.push((path.to_owned(), expected, story));
    }

    /// Signs `src` into `files/<category>/<name>` with the given actions and ingredients.
    fn sign(
        &mut self,
        category: Verdict,
        name: &str,
        src: &Path,
        actions: Vec<Value>,
        ingredients: &[Ing],
        story: &'static str,
    ) -> Result<PathBuf> {
        let out = self.out(category, name);
        let mut builder = self.builder(&out)?;
        for Ing(relationship, label, path) in ingredients {
            let title = path.file_name().unwrap().to_string_lossy();
            builder.add_ingredient_from_stream(
                json!({ "title": title, "relationship": relationship, "label": label }).to_string(),
                mime(path),
                &mut File::open(path)?,
            )?;
        }
        for action in actions {
            builder.add_action(action)?;
        }
        self.save(&mut builder, src, &out)?;
        self.label(&out, category, story);
        Ok(out)
    }

    fn builder(&self, out: &Path) -> Result<Builder> {
        let title = out.file_name().unwrap().to_string_lossy();
        Ok(Builder::from_shared_context(&self.ctx).with_definition(
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

fn settings(root: &Path) -> Result<Settings> {
    let path = root.join("settings/c2pa-test-settings.json");
    let mut s: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    // Every action in this set is written explicitly; nothing may be inferred.
    for auto in ["auto_created_action", "auto_opened_action", "auto_placed_action"] {
        s["builder"]["actions"][auto]["enabled"] = json!(false);
    }
    s["builder"]["claim_generator_info"] = json!({ "name": GENERATOR, "version": env!("CARGO_PKG_VERSION") });
    // Keep the check offline and deterministic: u06's remote manifest is never published.
    s["verify"]["remote_manifest_fetch"] = json!(false);
    Ok(Settings::new().with_json(&s.to_string())?)
}

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let build = root.join("target/sources");
    fs::create_dir_all(&build)?;
    let _ = fs::remove_dir_all(root.join("files"));
    let ctx = Context::new().with_settings(settings(&root)?)?.into_shared();
    let mut g = Gen { ctx, root: root.clone(), build, labels: Vec::new() };

    // ---- unsigned sources ------------------------------------------------------------------
    let dream = sources::dream(0.0);
    dream.save(g.src("dream.jpg"))?;
    dream.save(g.src("dream.png"))?;
    sources::photo(1).save(g.src("photo.jpg"))?;
    sources::photo(2).save(g.src("photo-hdr.jpg"))?;
    sources::photo(3).save(g.src("photo-unsigned.jpg"))?;
    sources::illustration().save(g.src("illustration.png"))?;
    sources::fractal().save(g.src("fractal.png"))?;
    let mut filled = sources::photo(1);
    sources::paste(&mut filled, &sources::dream(1.5), 380, 210, 200, 180);
    filled.save(g.src("photo-filled.jpg"))?;
    let mut placed = sources::photo(1);
    sources::paste(&mut placed, &dream, 40, 40, 220, 160);
    placed.save(g.src("photo-placed.jpg"))?;
    let mut collage = sources::illustration();
    sources::paste(&mut collage, &sources::dream(3.0), 300, 40, 300, 160);
    collage.save(g.src("collage.jpg"))?;
    sources::write_wav(&g.src("speech.wav"), true)?;
    sources::write_wav(&g.src("field.wav"), false)?;
    let have_mp4 = sources::write_mp4(&g.src("clip.mp4")).is_ok();
    if !have_mp4 {
        eprintln!("warning: ffmpeg unavailable, skipping a04_text_to_video.mp4");
    }

    let model = || agent("Fabrikam Diffusion", "4.2");
    let camera = || agent("Northwind Camera Firmware", "2.0.1");
    let editor = || agent("Northwind Photo Editor", "12.3");

    // ---- ai: the asset as a whole was produced by a trained model -------------------------
    use Verdict::*;
    let a01 = g.sign(Ai, "a01_text_to_image.jpg", &g.src("dream.jpg"),
        vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], &[],
        "Text-to-image output; c2pa.created with trainedAlgorithmicMedia.")?;
    g.sign(Ai, "a02_text_to_image.png", &g.src("dream.png"),
        vec![created("trainedAlgorithmicMedia", model(), "Generated from a text prompt")], &[],
        "Same as a01 in PNG.")?;
    g.sign(Ai, "a03_ai_then_cropped.jpg", &g.src("dream.jpg"),
        vec![opened("parent"), json!({ "action": "c2pa.cropped", "softwareAgent": editor() })],
        &[Ing("parentOf", "parent", &a01)],
        "Human crop of a01. The active manifest has no AI source type; the AI origin is only in the parent ingredient's manifest.")?;
    if have_mp4 {
        g.sign(Ai, "a04_text_to_video.mp4", &g.src("clip.mp4"),
            vec![created("trainedAlgorithmicMedia", agent("Fabrikam Video Model", "1.0"), "Generated from a text prompt")], &[],
            "Text-to-video output in MP4.")?;
    }
    g.sign(Ai, "a05_text_to_speech.wav", &g.src("speech.wav"),
        vec![created("trainedAlgorithmicMedia", agent("Fabrikam Voice", "3.0"), "Synthesised speech")], &[],
        "Synthetic speech in WAV.")?;

    // ---- no-ai: captured or made by people, with non-generative edits ---------------------
    let n01 = g.sign(NoAi, "n01_camera_capture.jpg", &g.src("photo.jpg"),
        vec![created("digitalCapture", camera(), "Captured by camera")], &[],
        "Camera capture; c2pa.created with digitalCapture.")?;
    g.sign(NoAi, "n02_capture_cropped_and_adjusted.jpg", &g.src("photo.jpg"),
        vec![
            opened("parent"),
            json!({ "action": "c2pa.cropped", "softwareAgent": editor() }),
            json!({ "action": "c2pa.color_adjustments", "softwareAgent": editor() }),
        ],
        &[Ing("parentOf", "parent", &n01)],
        "n01 cropped and colour-adjusted; only non-generative edits.")?;
    g.sign(NoAi, "n03_digital_illustration.png", &g.src("illustration.png"),
        vec![created("digitalCreation", agent("Northwind Paint", "8.0"), "Drawn by hand")], &[],
        "Hand-made illustration; digitalCreation.")?;
    g.sign(NoAi, "n04_hdr_composite_capture.jpg", &g.src("photo-hdr.jpg"),
        vec![created("compositeCapture", camera(), "HDR merge of bracketed exposures")], &[],
        "Several captures merged (HDR); compositeCapture.")?;
    g.sign(NoAi, "n05_field_recording.wav", &g.src("field.wav"),
        vec![created("digitalCapture", agent("Northwind Recorder", "1.4"), "Recorded on location")], &[],
        "Audio capture; digitalCapture.")?;
    g.sign(NoAi, "n06_procedural_fractal.png", &g.src("fractal.png"),
        vec![created("algorithmicMedia", agent("Northwind Fractal Renderer", "0.9"), "Rendered Mandelbrot set")], &[],
        "Edge case: algorithmicMedia is algorithmic but, per IPTC, not based on sampled training data, so it is not AI.")?;

    // ---- includes-ai: some, but not all, of the content came from a trained model ---------
    let i01 = g.sign(IncludesAi, "i01_generative_fill.jpg", &g.src("photo-filled.jpg"),
        vec![
            opened("parent"),
            json!({
                "action": "c2pa.edited",
                "digitalSourceType": dst("compositeWithTrainedAlgorithmicMedia"),
                "softwareAgent": agent("Northwind Photo Editor Generative Fill", "12.3"),
                "description": "Generative fill of a region",
            }),
        ],
        &[Ing("parentOf", "parent", &n01)],
        "Camera photo (n01) with a region replaced by generative fill; c2pa.edited with compositeWithTrainedAlgorithmicMedia.")?;
    g.sign(IncludesAi, "i02_ai_element_placed.jpg", &g.src("photo-placed.jpg"),
        vec![
            opened("parent"),
            json!({ "action": "c2pa.placed", "softwareAgent": editor(), "parameters": { "ingredientIds": ["ai-element"] } }),
        ],
        &[Ing("parentOf", "parent", &n01), Ing("componentOf", "ai-element", &a01)],
        "Camera photo with an AI image (a01) placed into it. No AI source type on any action; the AI is only in the componentOf ingredient's manifest.")?;
    g.sign(IncludesAi, "i03_created_with_ai_elements.jpg", &g.src("collage.jpg"),
        vec![created("compositeWithTrainedAlgorithmicMedia", agent("Northwind Layout", "5.1"), "Layout with AI-generated artwork")], &[],
        "Created directly as compositeWithTrainedAlgorithmicMedia.")?;
    g.sign(IncludesAi, "i04_generative_fill_then_cropped.jpg", &g.src("photo-filled.jpg"),
        vec![opened("parent"), json!({ "action": "c2pa.cropped", "softwareAgent": editor() })],
        &[Ing("parentOf", "parent", &i01)],
        "i01 cropped. The AI edit is one manifest back in the history.")?;

    // ---- unknown: the provenance cannot settle the question -------------------------------
    let u01 = g.out(Unknown, "u01_no_manifest.jpg");
    fs::create_dir_all(u01.parent().unwrap())?;
    fs::copy(g.src("photo-unsigned.jpg"), &u01)?;
    g.label(&u01, Unknown, "No C2PA manifest at all.");

    let u02 = g.out(Unknown, "u02_ai_with_manifest_stripped.jpg");
    image::open(&a01)?.save(&u02)?;
    g.label(&u02, Unknown, "a01 re-encoded, which drops the manifest. Ground truth is AI, but nothing in the file can show it.");

    // A v2 claim must give c2pa.created a digitalSourceType, so this one is a legacy v1 claim.
    let out = g.out(Unknown, "u03_v1_created_without_source_type.jpg");
    let mut builder = g.builder(&out)?;
    builder.definition.claim_version = Some(1);
    builder.add_action(json!({ "action": "c2pa.created", "softwareAgent": "Northwind Photo Editor 12.3" }))?;
    g.save(&mut builder, &g.src("photo-unsigned.jpg"), &out)?;
    g.label(&out, Unknown, "Valid legacy (v1) claim whose c2pa.created has no digitalSourceType.");

    let unsigned = g.src("photo-unsigned.jpg");
    g.sign(Unknown, "u04_edited_from_unsigned_parent.jpg", &unsigned,
        vec![opened("parent"), json!({ "action": "c2pa.color_adjustments", "softwareAgent": editor() })],
        &[Ing("parentOf", "parent", &unsigned)],
        "Signed edit of a parent that has no manifest, so the origin is unknown.")?;

    let u05 = g.out(Unknown, "u05_tampered_capture.jpg");
    let mut bytes = fs::read(&n01)?;
    let at = bytes.len() - 300; // inside the entropy-coded scan, after the manifest
    bytes[at] ^= 0xFF;
    fs::write(&u05, bytes)?;
    g.label(&u05, Unknown, "n01 with image bytes altered after signing; the manifest claims digitalCapture but the hard binding fails.");

    let out = g.out(Unknown, "u06_remote_manifest_unavailable.jpg");
    let mut builder = g.builder(&out)?;
    builder.add_action(created("digitalCapture", camera(), "Captured by camera"))?;
    builder.set_remote_url(REMOTE_URL).set_no_embed(true);
    let manifest = g.save(&mut builder, &g.src("photo.jpg"), &out)?;
    fs::write(g.build.join("u06-not-published.c2pa"), manifest)?;
    g.label(&out, Unknown, "Points to a remote manifest (XMP dcterms:provenance) that was never published, and embeds none.");

    g.sign(Unknown, "u07_composite_synthetic.jpg", &g.src("collage.jpg"),
        vec![created("compositeSynthetic", agent("Northwind Layout", "5.1"), "Layout with synthetic elements")], &[],
        "compositeSynthetic says synthetic elements are present, not whether a trained model made them.")?;
    g.sign(Unknown, "u08_algorithmically_enhanced.jpg", &g.src("photo.jpg"),
        vec![
            opened("parent"),
            json!({
                "action": "c2pa.edited",
                "digitalSourceType": dst("algorithmicallyEnhanced"),
                "softwareAgent": agent("Northwind Enhance", "2.2"),
                "description": "Automatic enhancement",
            }),
        ],
        &[Ing("parentOf", "parent", &n01)],
        "Camera photo with algorithmicallyEnhanced edits; the type does not say whether the algorithm was a trained model.")?;
    g.sign(Unknown, "u09_edited_from_tampered_parent.jpg", &g.src("photo.jpg"),
        vec![opened("parent"), json!({ "action": "c2pa.cropped", "softwareAgent": editor() })],
        &[Ing("parentOf", "parent", &u05)],
        "Valid signed crop of u05; the parent's claim of digitalCapture is untrustworthy because its own validation failed.")?;

    check(&g, &root)
}

/// Reads every generated file, classifies it, writes `labels.json`, and fails on any mismatch.
fn check(g: &Gen, root: &Path) -> Result<()> {
    let mut rows = Vec::new();
    let mut mismatches = 0;
    for (path, expected, story) in &g.labels {
        let reader = Reader::from_shared_context(&g.ctx).with_file(path);
        let store = reader.as_ref().ok().and_then(|r| serde_json::from_str::<Value>(&r.json()).ok());
        let state = match &reader {
            Ok(r) => format!("{:?}", r.validation_state()),
            Err(e) => format!("unreadable: {e}"),
        };
        let (got, why) = classify::classify(store.as_ref());
        let rel = path.strip_prefix(root)?.to_string_lossy().into_owned();
        let ok = got == *expected;
        mismatches += usize::from(!ok);
        println!("{} {:<13} {:<52} {}", if ok { "ok  " } else { "FAIL" }, got.dir(), rel, why);
        rows.push(json!({
            "file": rel,
            "expected": expected.dir(),
            "description": story,
            "validationState": state,
            "classifierReason": why,
        }));
    }
    fs::write(root.join("labels.json"), serde_json::to_string_pretty(&rows)? + "\n")?;
    anyhow::ensure!(mismatches == 0, "{mismatches} file(s) disagree with their label");
    println!("\n{} files, all labels confirmed", rows.len());
    Ok(())
}
