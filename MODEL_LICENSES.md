# Model Licenses

> **Generated file — do not edit by hand.** Regenerate with `node tools/gen_model_licenses.mjs`.
>
> Source: `src-tauri/src/catalog/catalog.json` (catalog version 2).

These models are **not bundled** with the application. They are downloaded on
demand, each verified against a SHA-256 recorded in the catalog. This document
records the licence each model is distributed under, so users and contributors
can see the terms before downloading.

## Attribution obligations

Models under **CC-BY-4.0** may be used and redistributed, including commercially,
**provided attribution is given** and the licence is linked. Attribution for the
models listed here must be preserved when redistributing this application or its
model catalog. Apache-2.0 and MIT models require the licence text and copyright
notice to be retained.

## Summary

| License | Models |
|---|---:|
| `apache-2.0` | 25 |
| `mit` | 21 |
| `cc-by-4.0` | 15 |
| `other` ⚠️ | 7 |
| `cc-by-nc-4.0` ⚠️ | 1 |

Total catalogued models: **69**.

## Models requiring review before redistribution

The following models have a licence that **restricts commercial use** or that has
**not been confirmed**. They are not recommended defaults, and must be reviewed
before being offered or redistributed as part of a product.

| Model | License | Base model | Status |
|---|---|---|---|
| `handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf` | `other` | `nvidia/nemotron-3.5-asr-streaming-0.6b` | not a default |
| `handy-computer/Fun-ASR-MLT-Nano-2512-gguf` | `other` | `FunAudioLLM/Fun-ASR-MLT-Nano-2512` | not a default |
| `handy-computer/canary-1b-gguf` | `cc-by-nc-4.0` | `nvidia/canary-1b` | not a default |
| `handy-computer/Fun-ASR-Nano-2512-gguf` | `other` | `FunAudioLLM/Fun-ASR-Nano-2512` | not a default |
| `handy-computer/medasr-gguf` | `other` | `google/medasr` | not a default |
| `handy-computer/nemotron-speech-streaming-en-0.6b-gguf` | `other` | `nvidia/nemotron-speech-streaming-en-0.6b` | not a default |
| `handy-computer/multitalker-parakeet-streaming-0.6b-v1-gguf` | `other` | `nvidia/multitalker-parakeet-streaming-0.6b-v1` | not a default |
| `handy-computer/SenseVoiceSmall-gguf` | `other` | `FunAudioLLM/SenseVoiceSmall` | not a default |

## Full inventory

| Model | License | Architecture | Parameters | Languages | Default? |
|---|---|---|---|---|---|
| `handy-computer/Breeze-ASR-25-gguf` | `apache-2.0` | whisper | 1.5B | zh, en |  |
| `handy-computer/canary-180m-flash-gguf` | `cc-by-4.0` | canary | 180M | en, de, es, fr | **yes** |
| `handy-computer/canary-1b-flash-gguf` | `cc-by-4.0` | canary | 1B | en, de, es, fr |  |
| `handy-computer/canary-1b-gguf` | `cc-by-nc-4.0` ⚠️ | canary | 1B | en, de, es, fr |  |
| `handy-computer/canary-1b-v2-gguf` | `cc-by-4.0` | canary | 1B | bg, hr, cs, da, nl, en, et, fi, fr, de, el, hu, it, lv, l… |  |
| `handy-computer/canary-qwen-2.5b-gguf` | `cc-by-4.0` | canary_qwen | 2.5B | en |  |
| `handy-computer/cohere-transcribe-03-2026-gguf` | `apache-2.0` | cohere_asr | 2.0B | en, fr, de, es, it, pt, nl, pl, el, ar, ja, zh, vi, ko | **yes** |
| `handy-computer/cohere-transcribe-arabic-07-2026-gguf` | `apache-2.0` | cohere_asr | 2.0B | ar, en |  |
| `handy-computer/Fun-ASR-MLT-Nano-2512-gguf` | `other` ⚠️ | funasr_nano | 985M | zh, en, yue, ja, ko, vi, id, th, ms, tl, ar, hi, bg, hr, … |  |
| `handy-computer/Fun-ASR-Nano-2512-gguf` | `other` ⚠️ | funasr_nano | 985M | zh, en, ja |  |
| `handy-computer/gigaam-v3-ctc-gguf` | `mit` | gigaam | 221M | ru |  |
| `handy-computer/gigaam-v3-e2e-ctc-gguf` | `mit` | gigaam | 221M | ru |  |
| `handy-computer/gigaam-v3-e2e-rnnt-gguf` | `mit` | gigaam | 223M | ru |  |
| `handy-computer/gigaam-v3-rnnt-gguf` | `mit` | gigaam | 222M | ru |  |
| `handy-computer/granite-4.0-1b-speech-gguf` | `apache-2.0` | granite_speech | 2.3B | en, fr, de, es, pt, ja |  |
| `handy-computer/granite-speech-4.1-2b-gguf` | `apache-2.0` | granite_speech | 2.3B | en, fr, de, es, pt, ja |  |
| `handy-computer/granite-speech-4.1-2b-nar-gguf` | `apache-2.0` | granite_speech_nar | 2.3B | en, fr, de, es, pt |  |
| `handy-computer/granite-speech-4.1-2b-plus-gguf` | `apache-2.0` | granite_speech | 2.1B | en, fr, de, es, pt |  |
| `handy-computer/medasr-gguf` | `other` ⚠️ | medasr | 105M | en |  |
| `handy-computer/moonshine-base-ar-gguf` | `mit` | moonshine | 62M | ar |  |
| `handy-computer/moonshine-base-gguf` | `mit` | moonshine | 62M | en |  |
| `handy-computer/moonshine-base-ja-gguf` | `mit` | moonshine | 62M | ja |  |
| `handy-computer/moonshine-base-ko-gguf` | `mit` | moonshine | 62M | ko |  |
| `handy-computer/moonshine-base-uk-gguf` | `mit` | moonshine | 62M | uk |  |
| `handy-computer/moonshine-base-vi-gguf` | `mit` | moonshine | 62M | vi |  |
| `handy-computer/moonshine-base-zh-gguf` | `mit` | moonshine | 62M | zh |  |
| `handy-computer/moonshine-streaming-medium-gguf` | `mit` | moonshine_streaming | 266M | en |  |
| `handy-computer/moonshine-streaming-small-gguf` | `mit` | moonshine_streaming | 140M | en |  |
| `handy-computer/moonshine-streaming-tiny-gguf` | `mit` | moonshine_streaming | 44M | en |  |
| `handy-computer/moonshine-tiny-ar-gguf` | `mit` | moonshine | 27M | ar |  |
| `handy-computer/moonshine-tiny-gguf` | `mit` | moonshine | 27M | en |  |
| `handy-computer/moonshine-tiny-ja-gguf` | `mit` | moonshine | 27M | ja |  |
| `handy-computer/moonshine-tiny-ko-gguf` | `mit` | moonshine | 27M | ko |  |
| `handy-computer/moonshine-tiny-uk-gguf` | `mit` | moonshine | 27M | uk |  |
| `handy-computer/moonshine-tiny-vi-gguf` | `mit` | moonshine | 27M | vi |  |
| `handy-computer/moonshine-tiny-zh-gguf` | `mit` | moonshine | 27M | zh |  |
| `handy-computer/moss-transcribe-diarize-gguf` | `apache-2.0` | moss | 909M | en, zh |  |
| `handy-computer/multitalker-parakeet-streaming-0.6b-v1-gguf` | `other` ⚠️ | parakeet | 0.6B | en |  |
| `handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf` | `other` ⚠️ | parakeet | 0.6B | en, es, fr, it, pt, nl, de, tr, ru, ar, hi, ja, ko, vi, u… |  |
| `handy-computer/nemotron-speech-streaming-en-0.6b-gguf` | `other` ⚠️ | parakeet | 0.6B | en |  |
| `handy-computer/parakeet-ctc-0.6b-gguf` | `cc-by-4.0` | parakeet | 0.6B | en |  |
| `handy-computer/parakeet-ctc-1.1b-gguf` | `cc-by-4.0` | parakeet | 1.1B | en |  |
| `handy-computer/parakeet-primeline-gguf` | `cc-by-4.0` | parakeet | 0.6B | de, bg, hr, cs, da, nl, en, et, fi, fr, el, hu, it, lv, l… |  |
| `handy-computer/parakeet-rnnt-0.6b-gguf` | `cc-by-4.0` | parakeet | 0.6B | en |  |
| `handy-computer/parakeet-rnnt-1.1b-gguf` | `cc-by-4.0` | parakeet | 1.1B | en |  |
| `handy-computer/parakeet-tdt_ctc-1.1b-gguf` | `cc-by-4.0` | parakeet | 1.1B | en |  |
| `handy-computer/parakeet-tdt_ctc-110m-gguf` | `cc-by-4.0` | parakeet | 110M | en |  |
| `handy-computer/parakeet-tdt-0.6b-v2-gguf` | `cc-by-4.0` | parakeet | 0.6B | en |  |
| `handy-computer/parakeet-tdt-0.6b-v3-gguf` | `cc-by-4.0` | parakeet | 0.6B | bg, hr, cs, da, nl, en, et, fi, fr, de, el, hu, it, lv, l… |  |
| `handy-computer/parakeet-tdt-1.1b-gguf` | `cc-by-4.0` | parakeet | 1.1B | en |  |
| `handy-computer/parakeet-unified-en-0.6b-gguf` | `cc-by-4.0` | parakeet | 0.6B | en | **yes** |
| `handy-computer/Qwen3-ASR-0.6B-gguf` | `apache-2.0` | qwen3_asr | 782M | zh, en, yue, ar, de, fr, es, pt, id, it, ko, ru, th, vi, … |  |
| `handy-computer/Qwen3-ASR-1.7B-gguf` | `apache-2.0` | qwen3_asr | 2.0B | zh, en, yue, ar, de, fr, es, pt, id, it, ko, ru, th, vi, … |  |
| `handy-computer/SenseVoiceSmall-gguf` | `other` ⚠️ | sensevoice | 234M | zh, yue, en, ja, ko |  |
| `handy-computer/Voxtral-Mini-3B-2507-gguf` | `apache-2.0` | voxtral | 4.7B | en, fr, de, es, it, pt, nl, hi |  |
| `handy-computer/Voxtral-Mini-4B-Realtime-2602-gguf` | `apache-2.0` | voxtral_realtime | 4.8B | en, fr, es, de, ru, zh, ja, it, pt, nl, ar, hi, ko |  |
| `handy-computer/Voxtral-Small-24B-2507-gguf` | `apache-2.0` | voxtral | 24.3B | en, fr, de, es, it, pt, nl, hi |  |
| `handy-computer/whisper-base-gguf` | `apache-2.0` | whisper | 73M | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-base.en-gguf` | `apache-2.0` | whisper | 73M | en |  |
| `handy-computer/whisper-large-gguf` | `apache-2.0` | whisper | 1.5B | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-large-v2-gguf` | `apache-2.0` | whisper | 1.5B | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-large-v3-gguf` | `apache-2.0` | whisper | 1.5B | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-large-v3-turbo-gguf` | `apache-2.0` | whisper | 809M | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-medium-gguf` | `apache-2.0` | whisper | 764M | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… | **yes** |
| `handy-computer/whisper-medium.en-gguf` | `apache-2.0` | whisper | 764M | en |  |
| `handy-computer/whisper-small-gguf` | `apache-2.0` | whisper | 242M | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-small.en-gguf` | `apache-2.0` | whisper | 242M | en |  |
| `handy-computer/whisper-tiny-gguf` | `apache-2.0` | whisper | 38M | en, zh, de, es, ru, ko, fr, ja, pt, tr, pl, ca, nl, ar, s… |  |
| `handy-computer/whisper-tiny.en-gguf` | `apache-2.0` | whisper | 38M | en |  |

⚠️ = restricted or unconfirmed licence. See the review section above.

## Policy for this fork

1. **No model is bundled** in an installer. Models are downloaded on demand.
2. **Every download is verified by SHA-256**, recorded in the catalog.
3. **A model with a restricted or unconfirmed licence must never be a default**
   or a recommended pick. `node tools/gen_model_licenses.mjs --check` enforces
   this in CI.
4. **Attribution is preserved** for CC-BY, Apache-2.0 and MIT models.
5. Adding a model whose licence has not been confirmed requires an explicit
   entry in this document and review before it is offered to users.

## Default model for Traditional Chinese

`handy-computer/Breeze-ASR-25-gguf` is the intended default for Taiwanese
Mandarin. It is derived from `MediaTek-Research/Breeze-ASR-25` and is licensed
**Apache-2.0**, which permits commercial use, modification and redistribution
subject to attribution and notice retention. Its base model is a Whisper
large-v2 fine-tune, so OpenAI's MIT Whisper licence also applies; MIT and
Apache-2.0 are compatible.

Accuracy figures published by the model authors are **not reproduced here as
guarantees**. This project has not yet measured CER or latency on its own corpus.
See `PROJECT_EVALUATION.md` §5.3 for the measurement plan.
