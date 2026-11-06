# Is it AI? A step-by-step guide

This guide explains, in plain language, how the classifier in this repository decides what a
file's Content Credentials (its C2PA manifest) say about generative AI. It follows the same
steps as the code. For the same logic as diagrams, see
[the flowcharts](classification-flow.md); for the reasons behind each rule, see
[the policy](../README.md#the-policy).

## What the answer means

Every file gets one of four answers:

| Answer | Meaning |
|---|---|
| **AI** | The credentials say the whole thing was made by generative AI, and the history is complete. |
| **Includes AI** | The credentials say generative AI made at least part of it, and nothing shows that part was removed. |
| **No AI** | The credentials give a complete history with no generative AI in what you see now. |
| **Can't tell** | The credentials don't say enough to decide either way. |

Two things to keep in mind:

- **This is about what the file *declares*, not proof.** Content Credentials record what the
  tools that made the file said happened. The guide reads those statements carefully; it
  doesn't inspect the pixels.
- **"AI" here means *generative* AI**: a model that creates content, such as text-to-image,
  generative fill or a synthetic voice. Non-generative AI, such as a phone camera merging
  frames into an HDR photo, doesn't count.

## The steps

### Step 1: Are there credentials at all?

Look for a C2PA manifest in the file.

- **None, or it can't be read** (for example, it lives at a web address that doesn't exist)
  → **Can't tell.** A missing manifest says nothing; many files lose theirs when they're
  re-saved or uploaded.

*Examples: u01, u02, u06.*

### Step 2: Do the credentials check out?

Validate the manifest: the signature, and the hashes that tie it to the file's content.

- **Something fails** (for example, the image was changed after it was signed)
  → **Can't tell.** You can't rely on anything a broken manifest says.

This guide ignores whether the *signer* is trusted, because every example file is signed with a
public test certificate. A real application should decide separately how much to trust the
signer.

*Example: u05.*

### Step 3: Is this just an update to the label?

An "update manifest" adds information, such as a review or publication note, without changing
the content. You can recognise one because it doesn't bind itself to the content with a hash.

- **Yes** → the answer is **the same as for the file it updates.** Go back to Step 1 with that
  earlier file.
- One exception: if the update adds an **AI model disclosure** to a file that was otherwise
  **No AI** → **Can't tell**, because someone now says a model was involved without saying how.

*Examples: a06 (same answer as its AI original); u19 (the exception).*

### Step 4: Read the history properly

The manifest lists **actions**: created, opened, cropped, edited and so on. Before reading
them, fill in the details the specification lets tools abbreviate:

- **Templates.** An action can inherit fields from a template, first a general (`*`) one, then
  one with the same name. The action's own fields win.
- **Related actions.** An action can list related sub-actions that only record what differs.
  Each one inherits everything else from its main action.

Read **every** actions list in the manifest, not just the first.

*Examples: a07 (source type from a template); i09 (AI only in a related action); i20 (AI only
in a second actions list).*

### Step 5: Where did it start?

The first action says where the content came from.

**If it was created**, look at its **digital source type**:

| Source type says | Starting point |
|---|---|
| made by a trained (generative) model | AI |
| a mix that includes generative AI | partly AI |
| a camera capture, an in-camera merge such as HDR, a mix of captures, hand-made with non-generative tools, made by a non-trained algorithm (such as a fractal), or scanned from film or print | no AI |
| a blank canvas (`empty`) | whatever was placed on it (Step 6) |
| anything else, or nothing | unknown |

"Anything else" includes a screen capture (which says how the screen was captured, not what was
on it), a virtual recording (which may include generative AI), and data produced by a trained
model (which might not be generative).

**If an earlier file was opened** (an edit of something that already existed), the starting
point is **that earlier file's answer**: repeat these steps for it.

- If the earlier file has no credentials but says what kind of source it is, use that.
- If it says nothing → unknown.
- If its credentials fail to validate → unknown.

*Examples: a01 (AI), n01 (camera), i03 (partly AI), a11 (blank canvas), u11 (screen capture),
a03 and n02 (edits of earlier files), u04 (earlier file with nothing), u09 (earlier file fails).*

### Step 6: What was placed in it?

Some files are built from pieces (**components**), such as a photo dropped into a layout. Work
out each piece's answer the same way as in Step 5.

- **Skip pieces that were later deleted completely**, but only if the deletion names that
  piece. A deletion that removed just part of something doesn't count.
- **Skip inputs to a model**, such as a prompt or a reference image (`inputTo`). They shaped
  the result but aren't in it.
- If a piece has no credentials, use the source type that the piece, or the action that placed
  it, declares.

*Examples: i02 (an AI piece placed in a photo); i16 (AI declared only on the placing action);
n07 and n12 (the AI piece deleted completely); i17 (only part deleted); a13 (a reference image
as input).*

### Step 7: What happened afterwards?

Go through every later action.

**If the action declares a source type:**

| It declares | Effect |
|---|---|
| generative AI, or generative augmentation (inpainting, generative fill, AI upscaling) | AI is present |
| a mix that includes generative AI | AI is present, and the content is now mixed |
| human or other non-generative work, on an action that adds content | the content is now mixed |
| something unclear (such as "algorithmically enhanced") | can't rule out AI |

**If it declares nothing**, look at what kind of action it is:

| Kind of action | Examples | Effect |
|---|---|---|
| Removes, rearranges or repackages | crop, trim, rotate, delete, convert, publish, metadata edits | no effect |
| Reprocesses what's there | resize, transcode, colour changes, enhance, master, change speed | can't rule out AI (a model could have done it, as in AI upscaling) |
| May add new content | edit, modify, draw, add text, filter or style, dub, translate | can't rule out AI, and the content may be mixed |
| Not a standard action | a company's own action name, or `c2pa.unknown` | can't tell what it did |

The specification doesn't require a generative edit to say so, which is why an edit that says
nothing can't be assumed to be human. A declared `humanEdits` settles it.

*Examples: i01 and i21 (generative edits); i13 (a hand drawing on an AI image); n13 and u18 (the
same enhancement with and without a declaration); u16 (an edit that says nothing); u15 (a custom
action).*

### Step 8: Check for red flags

Two things make the answer less certain:

- **Hidden history.** Someone **redacted** (removed) an earlier ingredient or action record.
  That hides provenance, not content, so its absence can't count as "no AI".
- **An AI model disclosure with no AI source type.** The AI disclosure names models, but not
  whether they generated anything. If no source type in the file mentions AI use, a model was
  used in some undeclared way. If a source type already explains it, such as an in-camera merge,
  the disclosure changes nothing.

*Examples: u17 (redaction); u14 and n10 (a disclosure without and with an explaining source
type).*

### Step 9: Is the history complete?

Each actions list should say whether it records **all** the actions (`allActionsIncluded:
true`). If any list says it doesn't, leaves the flag out, or uses the older (v1) format, there
may be steps nobody wrote down.

*Examples: u10 and i10 (marked incomplete); u03 (older format).*

### Step 10: Decide

Apply these rules in order. The first one that fits is the answer.

1. **AI**: the starting point is AI (or a blank canvas with only AI pieces), every remaining
   piece is AI, the history is complete, nothing is unclear, and no later action added other
   content.
2. **Includes AI**: there is *any* generative AI evidence. That means an AI or partly-AI
   starting point, an AI piece, or an AI source type on a later action.
3. **Can't tell**: the starting point is unknown, a placed piece is unknown, something was
   unclear, something was reprocessed without a declaration, or the history is incomplete.
4. **No AI**: everything else, which means a complete, declared history with no generative AI.

The order matters: evidence of AI wins over uncertainty. A file with a generative edit and
some unclear steps is still **Includes AI**, because the AI is known to be there.

## Worked examples

**a03: an AI image, then cropped.**
1. Credentials present and valid.
2. It opens an earlier file, so check that file first: a01 was created by a generative model,
   so a01 is AI.
3. The crop removes content without adding any.
4. The history is complete. → **AI**.

**i02: a camera photo with an AI image placed in it.**
1. It opens a camera photo (n01, no AI) and places a01 (AI).
2. No action declares AI, but a placed piece is AI.
3. Not everything is AI. → **Includes AI**.

**n07: the same, with the AI piece deleted.**
1. It opens i02, and the deletion names i02's AI piece, so that piece is skipped.
2. What's left is the camera photo, with a complete history. → **No AI**.

**u16: a camera photo, then "edited", with no source type.**
1. It starts as no AI.
2. The edit could have added content and doesn't say how it was made.
3. Nothing shows AI, but nothing rules it out. → **Can't tell**.

**i15: a mixed image cropped so the AI part is out of frame.**
1. A crop doesn't record what it removed, so the AI piece is still in the history, and nothing
   establishes it's gone. → **Includes AI**.
2. Only an explicit deletion of the piece (as in n07) changes that.

## Quick glossary

| Term | Meaning |
|---|---|
| Manifest | The signed record of a file's history, embedded in the file. |
| Action | One recorded step: created, opened, cropped, edited and so on. |
| Digital source type | A standard term (from IPTC) for where content came from, such as `digitalCapture` or `trainedAlgorithmicMedia`. |
| Ingredient | An earlier file this one was made from: the file it was opened from (parent), a piece placed into it (component), or an input to a model. |
| Update manifest | A manifest that adds information without changing the content. |
| Redaction | Removing a record from an earlier manifest. |
| AI disclosure | An optional record (`c2pa.ai-disclosure`) naming the AI models used and how much people oversaw them. |
