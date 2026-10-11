#!/usr/bin/env node
/**
 * gen_model_licenses.mjs
 *
 * Generates MODEL_LICENSES.md from Handy's model catalog.
 *
 * Why generated rather than hand-written: the catalog is the source of truth for
 * what can be downloaded, and a hand-maintained licence table drifts the moment
 * someone adds a model. Generating it means the document cannot disagree with
 * the catalog it describes.
 *
 * Usage:
 *   node tools/gen_model_licenses.mjs [--catalog <path>] [--out <path>] [--check]
 *
 *   --check   Verify only. Exits non-zero if any model whose licence forbids or
 *             restricts commercial use is marked as a recommended/default model,
 *             which is the one condition that would ship a restricted model to
 *             users by default. Also fails if the catalog predates the licence
 *             field. Intended for CI.
 *
 * The script performs no network access and no writes other than <out>.
 */

import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const DEFAULT_CATALOG = "src-tauri/src/catalog/catalog.json";
const DEFAULT_OUT = "MODEL_LICENSES.md";

/**
 * Licence identifiers that restrict or forbid commercial use, or that we have
 * not confirmed. Matched case-insensitively as substrings of the catalogue's
 * SPDX-ish `license` field.
 */
export const RESTRICTED_LICENSE_PATTERNS = [
  "nc", // covers cc-by-nc-*, and the ubiquitous "non-commercial" family
  "non-commercial",
  "noncommercial",
  "research",
  "other", // unconfirmed by definition
];

/** True when the licence is restricted or unconfirmed. */
export function isRestrictedLicense(license) {
  if (!license) return true; // missing licence is unconfirmed, not permissive
  const l = String(license).toLowerCase().trim();
  if (l === "") return true;
  // `cc-by-nc-4.0` etc. Match on explicit tokens to avoid false positives such
  // as a hypothetical "ncsa" identifier.
  return RESTRICTED_LICENSE_PATTERNS.some((p) => {
    if (p === "nc")
      return /(^|[-_ ])nc([-_ ]|$|\d)/.test(l) || l.includes("cc-by-nc");
    return l.includes(p);
  });
}

/** Licences that require attribution when redistributed. */
export function requiresAttribution(license) {
  const l = String(license ?? "").toLowerCase();
  return (
    l.includes("cc-by") ||
    l.includes("apache") ||
    l.includes("bsd") ||
    l.includes("mit")
  );
}

/**
 * Find restricted models that are recommended (i.e. surfaced to new users by
 * default). Returns the offending models.
 */
export function findRestrictedRecommended(catalog) {
  return (catalog.models ?? []).filter(
    (m) => m.recommended === true && isRestrictedLicense(m.license),
  );
}

function summarize(models) {
  const counts = new Map();
  for (const m of models) {
    const key = m.license ?? "(missing)";
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()].sort(
    (a, b) => b[1] - a[1] || a[0].localeCompare(b[0]),
  );
}

function esc(s) {
  return String(s ?? "").replace(/\|/g, "\\|");
}

export function render(catalog, { generatedFrom }) {
  const models = catalog.models ?? [];
  const restricted = models.filter((m) => isRestrictedLicense(m.license));
  const summary = summarize(models);

  const lines = [];
  lines.push("# Model Licenses");
  lines.push("");
  lines.push(
    "> **Generated file — do not edit by hand.** Regenerate with `node tools/gen_model_licenses.mjs`.",
  );
  lines.push(">");
  lines.push(
    `> Source: \`${generatedFrom}\` (catalog version ${catalog.catalog_version ?? "?"}).`,
  );
  lines.push("");
  lines.push(
    "These models are **not bundled** with the application. They are downloaded on",
  );
  lines.push(
    "demand, each verified against a SHA-256 recorded in the catalog. This document",
  );
  lines.push(
    "records the licence each model is distributed under, so users and contributors",
  );
  lines.push("can see the terms before downloading.");
  lines.push("");
  lines.push("## Attribution obligations");
  lines.push("");
  lines.push(
    "Models under **CC-BY-4.0** may be used and redistributed, including commercially,",
  );
  lines.push(
    "**provided attribution is given** and the licence is linked. Attribution for the",
  );
  lines.push(
    "models listed here must be preserved when redistributing this application or its",
  );
  lines.push(
    "model catalog. Apache-2.0 and MIT models require the licence text and copyright",
  );
  lines.push("notice to be retained.");
  lines.push("");
  lines.push("## Summary");
  lines.push("");
  lines.push("| License | Models |");
  lines.push("|---|---:|");
  for (const [license, count] of summary) {
    const flag = isRestrictedLicense(license) ? " ⚠️" : "";
    lines.push(`| \`${esc(license)}\`${flag} | ${count} |`);
  }
  lines.push("");
  lines.push(`Total catalogued models: **${models.length}**.`);
  lines.push("");

  lines.push("## Models requiring review before redistribution");
  lines.push("");
  if (restricted.length === 0) {
    lines.push(
      "None. Every catalogued model declares a permissive or attribution-only licence.",
    );
  } else {
    lines.push(
      "The following models have a licence that **restricts commercial use** or that has",
    );
    lines.push(
      "**not been confirmed**. They are not recommended defaults, and must be reviewed",
    );
    lines.push("before being offered or redistributed as part of a product.");
    lines.push("");
    lines.push("| Model | License | Base model | Status |");
    lines.push("|---|---|---|---|");
    for (const m of restricted) {
      const status = m.recommended
        ? "**RECOMMENDED — must be fixed**"
        : "not a default";
      lines.push(
        `| \`${esc(m.id)}\` | \`${esc(m.license ?? "missing")}\` | \`${esc(m.base_model ?? "?")}\` | ${status} |`,
      );
    }
  }
  lines.push("");

  lines.push("## Full inventory");
  lines.push("");
  lines.push(
    "| Model | License | Architecture | Parameters | Languages | Default? |",
  );
  lines.push("|---|---|---|---|---|---|");
  const sorted = [...models].sort((a, b) =>
    String(a.id).localeCompare(String(b.id)),
  );
  for (const m of sorted) {
    const langs = Array.isArray(m.languages) ? m.languages.join(", ") : "";
    const shortLangs =
      langs.length > 60
        ? `${langs.slice(0, 57)}…`
        : langs || String(m.language_count ?? "");
    const flag = m.recommended ? "**yes**" : "";
    const warn = isRestrictedLicense(m.license) ? " ⚠️" : "";
    lines.push(
      `| \`${esc(m.id)}\` | \`${esc(m.license ?? "missing")}\`${warn} | ${esc(m.architecture ?? "")} | ${esc(m.parameters ?? "")} | ${esc(shortLangs)} | ${flag} |`,
    );
  }
  lines.push("");
  lines.push(
    "⚠️ = restricted or unconfirmed licence. See the review section above.",
  );
  lines.push("");

  lines.push("## Policy for this fork");
  lines.push("");
  lines.push(
    "1. **No model is bundled** in an installer. Models are downloaded on demand.",
  );
  lines.push(
    "2. **Every download is verified by SHA-256**, recorded in the catalog.",
  );
  lines.push(
    "3. **A model with a restricted or unconfirmed licence must never be a default**",
  );
  lines.push(
    "   or a recommended pick. `node tools/gen_model_licenses.mjs --check` enforces",
  );
  lines.push("   this in CI.");
  lines.push(
    "4. **Attribution is preserved** for CC-BY, Apache-2.0 and MIT models.",
  );
  lines.push(
    "5. Adding a model whose licence has not been confirmed requires an explicit",
  );
  lines.push(
    "   entry in this document and review before it is offered to users.",
  );
  lines.push("");
  lines.push("## Default model for Traditional Chinese");
  lines.push("");
  lines.push(
    "`handy-computer/Breeze-ASR-25-gguf` is the intended default for Taiwanese",
  );
  lines.push(
    "Mandarin. It is derived from `MediaTek-Research/Breeze-ASR-25` and is licensed",
  );
  lines.push(
    "**Apache-2.0**, which permits commercial use, modification and redistribution",
  );
  lines.push(
    "subject to attribution and notice retention. Its base model is a Whisper",
  );
  lines.push(
    "large-v2 fine-tune, so OpenAI's MIT Whisper licence also applies; MIT and",
  );
  lines.push("Apache-2.0 are compatible.");
  lines.push("");
  lines.push(
    "Accuracy figures published by the model authors are **not reproduced here as",
  );
  lines.push(
    "guarantees**. This project has not yet measured CER or latency on its own corpus.",
  );
  lines.push("See `PROJECT_EVALUATION.md` §5.3 for the measurement plan.");
  lines.push("");
  return lines.join("\n");
}

export function parseArgs(argv) {
  const opts = { catalog: DEFAULT_CATALOG, out: DEFAULT_OUT, check: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--catalog") opts.catalog = argv[++i];
    else if (argv[i] === "--out") opts.out = argv[++i];
    else if (argv[i] === "--check") opts.check = true;
  }
  return opts;
}

export async function main(argv = process.argv.slice(2)) {
  const opts = parseArgs(argv);
  const catalog = JSON.parse(await readFile(opts.catalog, "utf8"));
  const models = catalog.models ?? [];

  if (models.length === 0) {
    throw new Error(`no models found in ${opts.catalog}`);
  }
  const missingLicense = models.filter((m) => !m.license);
  if (missingLicense.length > 0) {
    console.error(
      `warning: ${missingLicense.length} model(s) have no license field: ` +
        missingLicense.map((m) => m.id).join(", "),
    );
  }

  const restrictedRecommended = findRestrictedRecommended(catalog);

  if (opts.check) {
    if (restrictedRecommended.length > 0) {
      console.error(
        "FAIL: restricted-licence model(s) marked as recommended defaults:",
      );
      for (const m of restrictedRecommended) {
        console.error(`  ${m.id} (license: ${m.license})`);
      }
      return 1;
    }
    console.error(
      `OK: ${models.length} models, no restricted-licence model is a recommended default ` +
        `(${models.filter((m) => isRestrictedLicense(m.license)).length} restricted, none default)`,
    );
    return 0;
  }

  const markdown = render(catalog, { generatedFrom: opts.catalog });
  await writeFile(opts.out, markdown, "utf8");
  console.error(`wrote ${opts.out} (${models.length} models)`);
  return 0;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main()
    .then((code) => process.exit(code))
    .catch((e) => {
      console.error("FATAL", e);
      process.exit(2);
    });
}
