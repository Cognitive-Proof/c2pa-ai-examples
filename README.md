# C2PA AI disclosure test files

C2PA-signed test assets sorted by one question: **what does the signed provenance declare about
generative AI in the current content?**

| Folder | The provenance declares… |
|---|---|
| [`files/ai/`](files/ai) | the whole current content was made by generative AI, with a complete history and no later change that may have added other content. |
| [`files/includes-ai/`](files/includes-ai) | generative AI is in the provenance of the current content and its removal hasn't been established. The rest may not be AI, or the history may be incomplete. |
| [`files/no-ai/`](files/no-ai) | a complete history with no generative AI in the current content. |
| [`files/unknown/`](files/unknown) | not enough to settle it either way. |
| [`files/invalid/`](files/invalid) | not a category: deliberately malformed `c2pa.ai-disclosure` assertions, for negative schema tests. |

These four categories are an application policy, not something the C2PA specification
prescribes. Provenance records what was done, not what is left: a crop or an unrecorded change
could remove AI content without saying so. So **includes-ai** means AI is in the relevant
provenance and nothing establishes its removal. Only an explicit `c2pa.deleted` of the AI
ingredient does (n07, n09). The labels describe what each file's manifests *declare*. All media is procedural:
each file has the pixels its history describes (a crop is cropped, a fill is filled), but none
is a real camera capture or model output. "AI" in a description means a simulated AI-origin
scenario.

The policy is about **generative** AI and follows the IPTC digital source type vocabulary, which
separates generative from non-generative tools. `computationalCapture` ("non-generative AI",
for example in-camera HDR), `digitalCreation` and `humanEdits` ("non-generative tools") count
as no generative AI. A consumer that wants to know about *any* trained-model involvement would
treat those differently.

Every file's label, a description, the SDK validation state and the reason the reference
classifier gives are in [`labels.json`](labels.json).

## The cases

| File | Why it has this label |
|---|---|
| **ai** | |
| `a01_text_to_image.jpg` | `c2pa.created` with `trainedAlgorithmicMedia`. |
| `a02_text_to_image.png` | Same, in PNG. |
| `a03_ai_then_cropped.jpg` | a01 cropped. The active manifest has no AI source type; the AI origin is only in the parent ingredient's manifest. |
| `a04_text_to_video.mp4` | Text-to-video, MP4. |
| `a05_text_to_speech.wav` | Synthetic speech, WAV. |
| `a06_update_manifest_over_ai.jpg` | Update manifest over a01. An update can't change the content, so it inherits a01's verdict. |
| `a07_source_type_from_template.jpg` | `c2pa.created` gets `trainedAlgorithmicMedia` from an action template, which consumers must merge. See [known SDK issues](#known-c2pa-rs-issues). |
| `a08_ai_disclosure_fully_autonomous.jpg` | `trainedAlgorithmicMedia` plus `c2pa.ai-disclosure`, `humanOversightLevel: fully_autonomous`, `scientificDomain: ["cs.CV"]`. |
| `a09_ai_disclosure_prompt_guided.jpg` | Same with `prompt_guided`. |
| `a10_ai_disclosure_human_validated.jpg` | Same with `human_validated`: human approval doesn't make the content partly human-made. |
| `a11_empty_with_ai_component.jpg` | Blank canvas (`empty`) whose only content is a01. |
| `a12_ai_then_generative_inpaint.jpg` | a01 with a region inpainted: `compositeWithTrainedAlgorithmicMedia` is generative augmentation, so the content stays AI. |
| `a13_text_to_image_with_reference_input.jpg` | Text-to-image with an unsigned reference image as an `inputTo` ingredient. An input to the model isn't placed content, so its unknown provenance doesn't count. |
| **includes-ai** | |
| `i01_generative_fill.jpg` | Camera photo (n01) with a region filled; `c2pa.edited` with `compositeWithTrainedAlgorithmicMedia`. |
| `i02_ai_element_placed.jpg` | a01 `c2pa.placed` into n01. No action has an AI source type; the AI is only in the `componentOf` ingredient. |
| `i03_created_with_ai_elements.jpg` | Created directly as `compositeWithTrainedAlgorithmicMedia`. |
| `i04_generative_fill_then_cropped.jpg` | i01 cropped. The AI edit is one manifest back in the history. |
| `i05_unsigned_ai_ingredient_declared.jpg` | A placed ingredient with no manifest whose ingredient assertion declares `trainedAlgorithmicMedia`. |
| `i06_ai_parent_with_photo_placed.jpg` | a01 with n01 placed into it: the origin is AI, but the content is mixed. |
| `i07_ai_disclosure_human_validated.jpg` | Generative fill plus `c2pa.ai-disclosure`, `humanOversightLevel: human_validated`. |
| `i08_composite_synthetic.jpg` | `compositeSynthetic`: IPTC defines it as a composite in which at least one element is generative AI. (Previously `u07` under unknown.) |
| `i09_generative_edit_in_related_action.jpg` | n01 retouched; the generative fill is only in a `related` action, which consumers merge with its primary `c2pa.edited`. |
| `i10_ai_origin_incomplete_history.jpg` | `trainedAlgorithmicMedia` with `allActionsIncluded: false`. AI is present, but unrecorded changes may have added other content. |
| `i11_ai_origin_unknown_component.jpg` | a01 with a component of unknown provenance placed into it. |
| `i12_empty_with_mixed_components.jpg` | Blank canvas with a01 and n01 placed on it. |
| `i13_ai_with_hand_drawn_annotation.jpg` | a01 with a `c2pa.drawing` (`humanEdits`): human content added without an ingredient. |
| `i14_ai_then_composite_synthetic_edit.jpg` | a01 edited as `compositeSynthetic`, which declares a mix with other elements. |
| `i15_mixed_cropped_away_from_ai.jpg` | i02 cropped so the placed AI element is out of frame. A crop doesn't say what it removed, so the label stays includes-ai. |
| `i16_ai_declared_on_placed_action.jpg` | n01 with an unsigned element placed; only the `c2pa.placed` action declares it `trainedAlgorithmicMedia`. |
| `i17_ai_component_partly_deleted.jpg` | i02 with part of its AI element deleted. The `c2pa.deleted` references no ingredient, so it doesn't establish the whole component is gone. |
| `i18_generative_fill_region.jpg` | As i01, with the filled rectangle recorded in the action's `changes` region of interest. |
| `i19_video_with_ai_voice_track.mp4` | Captured video whose audio track was dubbed (`c2pa.dubbed`) with synthetic speech: AI in one track of mixed media. |
| `i20_ai_in_second_actions_assertion.jpg` | Two actions assertions (created and gathered); the generative fill is only in the second. |
| **no-ai** | |
| `n01_camera_capture.jpg` | `digitalCapture`. |
| `n02_capture_cropped_and_adjusted.jpg` | n01 cropped and colour-adjusted (`c2pa.adjustedColor`; `c2pa.color_adjustments` is deprecated). |
| `n03_digital_illustration.png` | `digitalCreation`: made with non-generative tools. |
| `n04_hdr_computational_capture.jpg` | In-camera HDR: `computationalCapture`, where IPTC puts automatic multi-frame HDR (non-generative AI at most). |
| `n05_field_recording.wav` | Audio `digitalCapture`. |
| `n06_procedural_fractal.png` | Edge case: `algorithmicMedia` is algorithmic but, by the IPTC definition, not based on training data. |
| `n07_ai_component_deleted.jpg` | i02 with its AI component removed, recorded by `c2pa.deleted` referencing that `componentOf` ingredient. AI is in the history, not in the current content. |
| `n08_composite_of_captures.jpg` | A collage of two photographs: `compositeCapture`. |
| `n09_ai_component_deleted_via_template_parameters.jpg` | As n07, but the deletion's reference comes from a template's deprecated `templateParameters`, which consumers must still merge. |
| `n10_computational_capture_with_model_disclosure.jpg` | `computationalCapture` with a `c2pa.ai-disclosure` naming a scikit-learn model: non-generative AI that the source type already declares. |
| `n11_capture_then_declared_human_edit.jpg` | n01 then `c2pa.edited` declaring `humanEdits`: an explicit non-generative declaration, unlike u16. |
| **unknown** | |
| `u01_no_manifest.jpg` | No manifest. |
| `u02_ai_with_manifest_stripped.jpg` | Represents an AI-origin asset whose manifest was lost (a01 re-encoded). |
| `u03_legacy_v1_actions.jpg` | Genuine 1.x file, copied from c2pa-rs `sdk/tests/fixtures/C.jpg`: v1 claim, v1 `c2pa.actions` (`algorithmicMedia`). v1 has no `allActionsIncluded`. |
| `u04_edited_from_unsigned_parent.jpg` | Signed edit of a parent with neither a manifest nor a declared source type. |
| `u05_tampered_capture.jpg` | n01 with image bytes changed after signing: `assertion.dataHash.mismatch`. |
| `u06_remote_manifest_unavailable.jpg` | Only a remote manifest URL (on `northwindtraders.com`) that was never published. |
| `u08_algorithmically_enhanced.jpg` | n01 with an `algorithmicallyEnhanced` edit; IPTC doesn't say whether the algorithm is generative. |
| `u09_edited_from_tampered_parent.jpg` | Valid crop of u05, so its parent failed validation. |
| `u10_incomplete_history_capture.jpg` | `digitalCapture` with `allActionsIncluded: false`: unrecorded, possibly generative, actions may have occurred. |
| `u11_screen_capture.png` | `screenCapture` says how a screen was captured, not where the displayed content came from. |
| `u12_virtual_recording.jpg` | `virtualRecording` may be "based on Generative AI and/or captured elements". |
| `u13_empty_with_unknown_component.jpg` | Blank canvas whose only content has no manifest or declared source type. |
| `u14_capture_with_model_disclosure.jpg` | `digitalCapture` with a `c2pa.ai-disclosure`, but no source type declares AI use: a model was used in some undeclared way. |
| `u15_capture_then_custom_action.jpg` | n01 with an entity-specific action and no source type. The history is complete, but what that action did is unknown. |
| `u16_capture_then_unspecified_edit.jpg` | n01 then `c2pa.edited` with no source type. The spec doesn't require a generative edit to declare one, so even a complete history can't rule it out. |
| `u17_ai_ingredient_redacted.jpg` | i02 opened and its AI ingredient assertion redacted. The AI content is still there; only its provenance is hidden, so redaction must not read as no AI. See [known SDK issues](#known-c2pa-rs-issues). |
| **invalid** | |
| `x01_disclosure_missing_model_type.jpg` | `c2pa.ai-disclosure` without the required `modelType`. |
| `x02_disclosure_invalid_oversight_level.jpg` | `humanOversightLevel` outside the three allowed values. |
| `x03_disclosure_scalar_scientific_domain.jpg` | `scientificDomain` as a string, not an array. |
| `x04_disclosure_empty_scientific_domain.jpg` | `scientificDomain` as an empty array. |

## The policy

The reference classifier ([`src/classify.rs`](src/classify.rs)) reads the manifest store and
walks the ingredient history:

1. **Unreadable or failing.** No readable manifest, or a validation failure in the active
   manifest → **unknown**.
2. **Update manifests** (those with no `c2pa.hash.*` hard binding) can't change the content.
   Whatever they record (`c2pa.opened`, `c2pa.published`, `c2pa.edited.metadata`,
   `c2pa.redacted`), they get their parent's verdict.
3. **Merging actions.** Actions are merged as the specification requires:
   - templates first: the `*` template, then same-named templates in order, then the action's
     own fields;
   - a template's deprecated `templateParameters` counts as `parameters`;
   - each `related` action is merged onto its primary action, recursively.
4. **Origin.** The origin comes from `c2pa.created`'s `digitalSourceType`, or from the parent
   ingredient for `c2pa.opened`. Parents are classified recursively.
   - An ingredient without a manifest uses its own declared `digitalSourceType`. If it has
     none, its origin is unknown.
   - An ingredient that fails validation is unknown.
5. **Components.** `componentOf` ingredients are classified the same way. Any that a later
   `c2pa.deleted` action references are no longer part of the current content. `inputTo`
   ingredients (inputs to a model, such as prompts or reference images) aren't content and
   don't count.
6. **Later actions.** Each action from the specification's list is either one that preserves
   content or one that may add it:
   - Preserving: cropping, resizing, colour and orientation changes, enhancement, transcoding,
     deletion, publishing and similar.
   - May add: drawing, added text, filters and styles, placing, dubbing, translation, and the
     generic `c2pa.edited` and `c2pa.modified`.

   The spec doesn't require a generative edit to declare a source type, so an action that may
   add content and declares none makes the result unclear. A non-generative declaration
   (`humanEdits`) settles it. An adding action keeps an AI origin entirely AI only if its
   source type is generative (`trainedAlgorithmicMedia` or
   `compositeWithTrainedAlgorithmicMedia`). Otherwise, or with `compositeSynthetic`, the content
   may now be mixed. A `digitalSourceType` on `c2pa.placed` counts as evidence too, and stands
   in for a placed ingredient that declares none. An action that isn't in the specification's
   list (entity-specific) or is `c2pa.unknown`, and has no source type, also makes the result
   unclear: a complete history says what was recorded, not that the classifier understands it.

   Every actions assertion in the manifest counts, created or gathered.
7. **Redaction.** A manifest that redacts an ingredient or actions assertion from its history
   makes the result unclear. Redaction hides provenance, not content, and the spec has the
   redacted ingredient's manifest removed from the store, so its absence can't count as no AI.
8. **AI disclosure.** `c2pa.ai-disclosure` names AI/ML models but doesn't say they generated
   content, so on its own it doesn't count as generative AI. With a source type that already
   declares AI use (including `computationalCapture`), it changes nothing. Without one, it makes
   the result unclear.
9. **Combining:**
   - **ai** if every input is AI (an AI origin, or an `empty` origin with components) and every
     remaining component is AI. The history must also be complete (`allActionsIncluded: true`),
     with nothing unclear and no later action that may have added other content.
   - **includes-ai** if any generative AI is declared in the provenance of the current content:
     - an AI origin that fails the "entirely AI" test above;
     - `compositeWithTrainedAlgorithmicMedia` or `compositeSynthetic`;
     - an AI source type on any action;
     - an AI component.
   - **unknown** if the origin, any component or any source type is unclear, or the history may
     be incomplete. Only a v2 actions assertion with `allActionsIncluded: true` rules out
     unrecorded actions; v1 actions and an omitted field don't.
   - **no-ai** otherwise.

**Source types** are matched as full URIs: IPTC terms under
`http://cv.iptc.org/newscodes/digitalsourcetype/` and the two C2PA terms under
`http://c2pa.org/digitalsourcetype/`. Each falls into one of these groups:

| Group | Source types |
|---|---|
| AI | `trainedAlgorithmicMedia` |
| Part AI | `compositeWithTrainedAlgorithmicMedia`, `compositeSynthetic` |
| No generative AI | `digitalCapture`, `computationalCapture`, `negativeFilm`, `positiveFilm`, `print`, `digitalCreation`, `compositeCapture`, `algorithmicMedia` |
| Edits only | `humanEdits` and the retired `minorHumanEdits` describe edits, not an origin, so they are neutral on an edit and unclear as an origin. |
| Unclear | everything else, including `algorithmicallyEnhanced`, `screenCapture`, `virtualRecording`, unrecognised URIs, and `trainedAlgorithmicData` (data from a trained model, which may be a classifier as well as a generator) |

The labels answer "is generative AI in the provenance of the current content, with no
established removal". A consumer asking "was it *ever* involved" would answer n07 and n09
differently; their `classifierReason` shows the AI in the history.

The classifier ignores `signingCredential.untrusted`. See trust below. Cases c2pa-rs can't
build as files, such as published-only and metadata-only update manifests (its Builder always
records `c2pa.opened`), are covered by unit tests in `src/classify.rs`. So are an older v2
assertion that omits `allActionsIncluded` (read as possibly incomplete), and full template
precedence (`*`, then the named template, then the action).

## Schema checks

The generator also lints every manifest ([`src/lint.rs`](src/lint.rs)) against rules the SDK's
validator does not enforce:

- `allActionsIncluded` is present on every `c2pa.actions.v2`.
- `softwareAgent` is a `generator-info-map` in v2 and a string in v1, in actions, templates and
  nested `related` actions.
- Every `digitalSourceType` is a recognised URI.
- `c2pa.created` has a source type once templates are merged.
- An ingredient reference into another manifest (as `c2pa.deleted` may make, including through
  `templateParameters`) has the hash that manifest's claim records.
- `c2pa.ai-disclosure` has a `modelType`, a valid `humanOversightLevel`, and a non-empty array
  of arXiv-style `scientificDomain` values. The `files/invalid/` fixtures each break one of
  these, and the check expects exactly that lint error from each.
- No ingredient has both `activeManifest` and `digitalSourceType`.

The lint reads `Reader::detailed_json()`, because `Reader::json()` reports a v1 `c2pa.actions`
assertion as `c2pa.actions.v2`.

## Known c2pa-rs issues

The run found these against the specs-core text. It works around each one rather than writing a
non-conformant file:

| Issue | Effect here |
|---|---|
| c2pa-rs checks `c2pa.created` for a `digitalSourceType` before applying action templates, so a template-supplied source type is reported as `assertion.action.malformed`. Fix: [contentauth/c2pa-rs#3034](https://github.com/contentauth/c2pa-rs/pull/3034). | a07 is signed without verify-after-sign. `labels.json` records the SDK's verdict under `knownSdkIssue`, and the check confirms the label once that code is set aside. |
| `Builder` writes every actions assertion as `c2pa.actions.v2` (legal in a v1 claim), but keeps a string `softwareAgent`, which v2 doesn't allow. Fix: [contentauth/c2pa-rs#3035](https://github.com/contentauth/c2pa-rs/pull/3035). It also can't write a v1 `c2pa.actions` assertion. | u03 is a genuine 1.x fixture instead. |
| `Reader::json()` reports a v1 `c2pa.actions` assertion as `c2pa.actions.v2`. | The lint reads `Reader::detailed_json()`. |
| c2pa-rs rejects a `c2pa.placed` in an ingredient's manifest whose ingredient was later redacted (`assertion.action.ingredientMismatch`). Validate_Assertions allows a placed reference to resolve to a redacted ingredient assertion. | u17 is signed without verify-after-sign and recorded under `knownSdkIssue`. The check confirms its label with that code set aside. |
| `Reader::json()` prints a hashed URI's hash as base64 text, but a `Builder` given that JSON back stores the text's bytes, not the hash. c2pa-rs doesn't verify a `c2pa.deleted` reference into another manifest, although Validate_Assertions requires a valid hashed URI there, so the wrong hash goes unreported. | The deletion cases build `HashedUri` values from decoded bytes, and the lint checks every such hash. |

With `BuilderIntent::Update`, c2pa-rs always records `c2pa.opened`, so an update manifest with no
actions can't be produced. The generator turns on `builder.actions.auto_all_actions_included`, so
that `c2pa.opened` (a06) carries `allActionsIncluded: true`.

## Trust

Every file except u03 is signed with the public **C2PA test certificate** from c2pa-rs.
[`settings/c2pa-test-settings.json`](settings/c2pa-test-settings.json) holds the certificate,
its test private key and its trust anchors. With those anchors, the signed files validate as
`Trusted`. Standard tools such as `c2patool` and public viewers report
`signingCredential.untrusted`. Don't use these files to test trust decisions.

## Running it

The generator builds against the c2pa-rs checkout next to this repository (`../c2pa-rs/sdk`).
It needs Rust 1.96 (pinned in `rust-toolchain.toml`), and ffmpeg for the MP4 case, which is
skipped if ffmpeg is missing.

```bash
cargo run --release
```

The run rebuilds `files/` and `labels.json`, then reads every file back. It fails on any lint
problem, or if any file's verdict differs from its label. Signatures, instance IDs and
timestamps change each run; the source media are deterministic.

To lint and classify other files instead:

```bash
cargo run --release -- path/to/file.jpg
```

The classifier's unit tests:

```bash
cargo test --release
```

The committed files were built with c2pa-rs `f5ec249b`.
