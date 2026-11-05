# C2PA AI disclosure test files

C2PA-signed test assets sorted by one question: **was AI involved in making this?**

| Folder | Meaning |
|---|---|
| [`files/ai/`](files/ai) | The asset as a whole came from a trained model. |
| [`files/includes-ai/`](files/includes-ai) | Part of the asset came from a trained model. |
| [`files/no-ai/`](files/no-ai) | Captured or made by people, with only non-generative edits. |
| [`files/unknown/`](files/unknown) | The file's provenance can't settle it either way. |

Every file's expected answer, a description, the SDK validation state and the reason the
reference classifier gives are in [`labels.json`](labels.json).

## The cases

| File | Why it has this label |
|---|---|
| **ai** | |
| `a01_text_to_image.jpg` | `c2pa.created` with `trainedAlgorithmicMedia`. |
| `a02_text_to_image.png` | Same, in PNG. |
| `a03_ai_then_cropped.jpg` | Human crop of a01. The active manifest has no AI source type; the AI origin is only in the parent ingredient's manifest. |
| `a04_text_to_video.mp4` | Text-to-video, MP4. |
| `a05_text_to_speech.wav` | Synthetic speech, WAV. |
| **includes-ai** | |
| `i01_generative_fill.jpg` | Camera photo (n01) with a region replaced; `c2pa.edited` with `compositeWithTrainedAlgorithmicMedia`. |
| `i02_ai_element_placed.jpg` | AI image (a01) `c2pa.placed` into a camera photo. No action has an AI source type; the AI is only in the `componentOf` ingredient. |
| `i03_created_with_ai_elements.jpg` | Created directly as `compositeWithTrainedAlgorithmicMedia`. |
| `i04_generative_fill_then_cropped.jpg` | i01 cropped. The AI edit is one manifest back in the history. |
| **no-ai** | |
| `n01_camera_capture.jpg` | `digitalCapture`. |
| `n02_capture_cropped_and_adjusted.jpg` | n01 cropped and colour-adjusted. |
| `n03_digital_illustration.png` | `digitalCreation`. |
| `n04_hdr_composite_capture.jpg` | `compositeCapture` (HDR merge). |
| `n05_field_recording.wav` | Audio `digitalCapture`. |
| `n06_procedural_fractal.png` | Edge case: `algorithmicMedia` is algorithmic but, by the IPTC definition, not based on training data. |
| **unknown** | |
| `u01_no_manifest.jpg` | No manifest. |
| `u02_ai_with_manifest_stripped.jpg` | a01 re-encoded, which drops the manifest. Ground truth is AI; the file can't show it. |
| `u03_v1_created_without_source_type.jpg` | Legacy v1 claim whose `c2pa.created` has no `digitalSourceType` (a v2 claim must have one). |
| `u04_edited_from_unsigned_parent.jpg` | Signed edit of a parent that has no manifest. |
| `u05_tampered_capture.jpg` | n01 with image bytes changed after signing: `assertion.dataHash.mismatch`. |
| `u06_remote_manifest_unavailable.jpg` | Only a remote manifest URL (on `northwindtraders.com`) that was never published. |
| `u07_composite_synthetic.jpg` | `compositeSynthetic`: synthetic elements, but not whether a trained model made them. |
| `u08_algorithmically_enhanced.jpg` | Camera photo with an `algorithmicallyEnhanced` edit; whether the algorithm was trained isn't stated. |
| `u09_edited_from_tampered_parent.jpg` | Valid crop of u05, so it inherits a parent whose validation failed. |

## How the labels are decided

The reference classifier ([`src/classify.rs`](src/classify.rs)) reads the manifest store and
walks the ingredient history:

1. No readable manifest, or a validation failure in the active manifest → **unknown**.
2. The origin comes from `c2pa.created`'s `digitalSourceType`, or from the parent ingredient
   for `c2pa.opened`. The parent is classified recursively. A parent with no manifest or a
   failed validation is unknown.
3. If the origin is AI, the result is **ai**, whatever human edits follow.
4. Otherwise, any `compositeWithTrainedAlgorithmicMedia`/`trainedAlgorithmicMedia` action, or
   any AI `componentOf` ingredient, makes it **includes-ai**.
5. Otherwise, an unknown origin, an unclear source type (`compositeSynthetic`,
   `algorithmicallyEnhanced`, `computationalCapture`, …) or an unknown component makes it
   **unknown**.
6. Anything else is **no-ai**.

The classifier ignores `signingCredential.untrusted`. See trust below.

## Trust

Every file is signed with the public **C2PA test certificate** from c2pa-rs
([`settings/c2pa-test-settings.json`](settings/c2pa-test-settings.json), which also holds its
test private key and trust anchors). With those anchors, the signed files validate as
`Trusted`. Standard tools such as `c2patool` and public viewers report
`signingCredential.untrusted`. Don't use these files to test trust decisions.

## Regenerating

The generator builds against the c2pa-rs checkout next to this repository (`../c2pa-rs/sdk`).
It needs Rust 1.96 (pinned in `rust-toolchain.toml`), and ffmpeg for the MP4 case, which is
skipped if ffmpeg is missing.

```bash
cargo run --release
```

The run rebuilds `files/` and `labels.json`, then reads every file back. It fails if any
file's verdict differs from its label. Signatures, instance IDs and timestamps change each
run; the source media are deterministic. The committed files were built with c2pa-rs
`f5ec249b`.
