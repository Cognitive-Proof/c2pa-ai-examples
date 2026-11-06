# How the classifier decides

The reference classifier ([`src/classify.rs`](../src/classify.rs)) answers one question: what does
the signed provenance declare about **generative** AI in the current content? These charts follow
the code. [The step-by-step guide](step-by-step-guide.md) explains it in plain language, and
[the policy](../README.md#the-policy) in the README gives the reasons.

## 1. The verdict

```mermaid
flowchart TD
    file([File]) --> readable{Readable C2PA manifest?}
    readable -->|No| unknownA[unknown]
    readable -->|Yes| valid{"Active manifest fails validation?<br/>(an untrusted signer is ignored)"}
    valid -->|Yes| unknownB[unknown]
    valid -->|No| update{"Update manifest?<br/>(no c2pa.hash.* hard binding)"}
    update -->|Yes| disclose{"Adds c2pa.ai-disclosure<br/>over a no-ai parent?"}
    disclose -->|Yes| unknownC[unknown]
    disclose -->|No| parent["Verdict of its parent ingredient<br/>(chart 2)"]
    update -->|No| gather["Gather the evidence:<br/>origin and components (chart 2),<br/>later actions (chart 3)"]
    gather --> whole{"Every input is AI, the history is<br/>complete, and nothing is unclear<br/>or added by a later action?"}
    whole -->|Yes| ai[ai]
    whole -->|No| any{"Any generative AI evidence?<br/>AI origin, AI component,<br/>or an AI source type on an action"}
    any -->|Yes| includes[includes-ai]
    any -->|No| doubt{"Origin unknown, anything unclear,<br/>undeclared processing,<br/>or history incomplete?"}
    doubt -->|Yes| unknown2[unknown]
    doubt -->|No| noai[no-ai]

    classDef ai fill:#fde2e1,stroke:#c0392b,color:#000
    classDef inc fill:#fdebd0,stroke:#d35400,color:#000
    classDef no fill:#d5f5e3,stroke:#1e8449,color:#000
    classDef unk fill:#e5e7e9,stroke:#566573,color:#000
    class ai ai
    class includes inc
    class noai no
    class unknownA,unknownB,unknownC,unknown2 unk
```

"History complete" means every actions assertion is `c2pa.actions.v2` with
`allActionsIncluded: true`. A v1 assertion, or an omitted field, may hide unrecorded actions.

## 2. Origin and ingredients

Actions are first merged as the specification requires: the `*` template, then same-named
templates, then the action itself (`templateParameters` count as `parameters`), and every
`related` action is merged onto its primary action.

```mermaid
flowchart LR
    inception{First action} -->|c2pa.created| dst{digitalSourceType}
    inception -->|c2pa.opened| parentIng[Classify the parentOf ingredient]
    inception -->|neither| oUnknown[origin: unknown]

    dst -->|trainedAlgorithmicMedia| oAi[origin: AI]
    dst -->|"compositeWithTrainedAlgorithmicMedia<br/>compositeSynthetic"| oInc[origin: includes AI]
    dst -->|"digitalCapture, computationalCapture,<br/>compositeCapture, digitalCreation,<br/>algorithmicMedia, film, print"| oNo[origin: no AI]
    dst -->|empty| oEmpty[origin: its components only]
    dst -->|"anything else, or missing<br/>(e.g. trainedAlgorithmicData,<br/>screenCapture, virtualRecording)"| oUnknown

    parentIng --> ing
    components["Each componentOf ingredient,<br/>except one a later c2pa.deleted<br/>references (inputTo is ignored)"] --> ing

    ing{Ingredient} -->|fails validation| iUnknown[unknown]
    ing -->|has a manifest| recurse["Classify that manifest<br/>(chart 1, recursively)"]
    ing -->|"no manifest, but declares a<br/>digitalSourceType (or its<br/>c2pa.placed action does)"| iDeclared[Map that source type]
    ing -->|no manifest, nothing declared| iUnknown
```

## 3. Later actions

Every action after the first, in every actions assertion, created or gathered:

```mermaid
flowchart LR
    act{Has a digitalSourceType?} -->|Yes| kind{Which kind?}
    kind -->|"trainedAlgorithmicMedia,<br/>compositeWithTrainedAlgorithmicMedia"| sigAi[AI evidence]
    kind -->|compositeSynthetic| sigMix[AI evidence, and content mixed]
    kind -->|"humanEdits or another non-AI type,<br/>on an action that adds content"| mixed[Content mixed]
    kind -->|"unclear: algorithmicallyEnhanced,<br/>screenCapture, unrecognised"| unclear[Unclear]

    act -->|No| effect{"What does the action do?<br/>(actions.yaml)"}
    effect -->|"preserves: crop, trim, orientation,<br/>delete, repackage, publish, metadata"| nothing[No effect]
    effect -->|"processes: resize, transcode,<br/>colour, enhance, master, speed"| proc["Undeclared processing<br/>(could be AI upscaling)"]
    effect -->|"adds: edit, modify, draw,<br/>text, filter, dub, translate"| addUnclear[Unclear, and content mixed]
    effect -->|"custom action or c2pa.unknown"| unclear

    other["Also unclear:<br/>redacting an ingredient or actions assertion,<br/>or a c2pa.ai-disclosure with no source type<br/>that declares AI use"] --> unclear
```

- **AI evidence** makes the result at least includes-ai.
- **Content mixed** stops an AI origin from counting as entirely AI.
- **Unclear** and **undeclared processing** rule out no-ai.
- A `digitalSourceType` on `c2pa.placed` counts unless its component was later deleted.
