#!/usr/bin/env node
/**
 * apply_license_overrides.mjs
 *
 * Applies the fork's licence-driven catalog overrides.
 *
 * # Why this exists
 *
 * `src-tauri/src/catalog/catalog.json` is generated upstream by
 * `scripts/gen_catalog.py`. Editing it directly would be silently reverted the
 * next time the catalog is regenerated, and the reason for the edit would be
 * lost. This script applies reviewed overrides from `model_license_overrides.json`
 * so that:
 *
 *   * regeneration + re-apply is the reproducible workflow;
 *   * every override carries a written reason that is reviewable in the diff;
 *   * `--check` can fail CI when the on-disk catalog does not match the overrides.
 *
 * Usage:
 *   node tools/apply_license_overrides.mjs [--catalog <path>] [--overrides <path>]
 *   node tools/apply_license_overrides.mjs --check
 *
 * `--check` performs no writes and exits 1 if any override is not applied.
 */

import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const DEFAULT_CATALOG = 'src-tauri/src/catalog/catalog.json';
const DEFAULT_OVERRIDES = 'model_license_overrides.json';

/**
 * Set a top-level field of one model by editing the raw JSON *text*.
 *
 * Re-serializing the whole document would reformat every line the upstream
 * generator wrote (it uses a different layout for some arrays), turning a
 * one-field change into a multi-thousand-line diff. A targeted text edit keeps
 * the fork's diff against upstream honest and reviewable.
 *
 * Throws if the model id is absent or ambiguous, or if the field is not found,
 * so a renamed or restructured catalog fails loudly instead of silently doing
 * nothing.
 */
export function setFieldInRawText(raw, modelId, key, value) {
  const needle = `"id": "${modelId}"`;
  const first = raw.indexOf(needle);
  if (first < 0) {
    throw new Error(`model id '${modelId}' not found in the catalog text`);
  }
  if (raw.indexOf(needle, first + 1) >= 0) {
    throw new Error(`model id '${modelId}' appears more than once; refusing to guess`);
  }

  // Search for the field only within this model's block, i.e. after its id and
  // before the next model's id.
  const nextIdIdx = raw.indexOf('"id": "', first + needle.length);
  const blockEnd = nextIdIdx < 0 ? raw.length : nextIdIdx;

  const fieldRe = new RegExp(`"${key}"\\s*:\\s*(true|false|null|-?\\d+(?:\\.\\d+)?|"[^"]*")`);
  const block = raw.slice(first, blockEnd);
  const match = fieldRe.exec(block);
  if (!match) {
    throw new Error(`field '${key}' not found for model '${modelId}'`);
  }

  const absoluteStart = first + match.index;
  const absoluteEnd = absoluteStart + match[0].length;
  return {
    text: raw.slice(0, absoluteStart) + `"${key}": ${JSON.stringify(value)}` + raw.slice(absoluteEnd),
    from: match[1],
  };
}

/**
 * Apply `overrides` to the catalog *text*, returning the new text and the list
 * of changes. Pure: callers can test it without touching the filesystem.
 */
export function applyOverridesToText(raw, overridesFile) {
  const overrides = overridesFile?.overrides ?? [];
  // Parse only to validate ids exist; the edit itself stays textual.
  const catalog = JSON.parse(raw);
  const knownIds = new Set((catalog.models ?? []).map((m) => m.id));

  let text = raw;
  const changes = [];
  for (const override of overrides) {
    if (!knownIds.has(override.id)) {
      throw new Error(
        `override targets unknown model id '${override.id}' — the catalog may have been ` +
          `regenerated with different ids; update model_license_overrides.json`,
      );
    }
    for (const [key, value] of Object.entries(override.set ?? {})) {
      const result = setFieldInRawText(text, override.id, key, value);
      if (result.from !== JSON.stringify(value)) {
        changes.push({ id: override.id, key, from: result.from, to: JSON.stringify(value) });
        text = result.text;
      }
    }
  }
  return { text, changes };
}

/** Ids whose overrides are not currently reflected in the catalog. */
export function findUnapplied(catalog, overridesFile) {
  const overrides = overridesFile?.overrides ?? [];
  const byId = new Map((catalog.models ?? []).map((m) => [m.id, m]));
  const unapplied = [];
  for (const override of overrides) {
    const model = byId.get(override.id);
    if (!model) {
      unapplied.push({ id: override.id, key: '<model missing>', expected: null, actual: null });
      continue;
    }
    for (const [key, value] of Object.entries(override.set ?? {})) {
      if (model[key] !== value) {
        unapplied.push({ id: override.id, key, expected: value, actual: model[key] });
      }
    }
  }
  return unapplied;
}

export function parseArgs(argv) {
  const opts = { catalog: DEFAULT_CATALOG, overrides: DEFAULT_OVERRIDES, check: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--catalog') opts.catalog = argv[++i];
    else if (argv[i] === '--overrides') opts.overrides = argv[++i];
    else if (argv[i] === '--check') opts.check = true;
  }
  return opts;
}

export async function main(argv = process.argv.slice(2)) {
  const opts = parseArgs(argv);
  const rawCatalog = await readFile(opts.catalog, 'utf8');
  const catalog = JSON.parse(rawCatalog);
  const overridesFile = JSON.parse(await readFile(opts.overrides, 'utf8'));

  if (opts.check) {
    const unapplied = findUnapplied(catalog, overridesFile);
    if (unapplied.length > 0) {
      console.error('FAIL: catalogue overrides are not applied:');
      for (const u of unapplied) {
        console.error(
          `  ${u.id}: ${u.key} is ${JSON.stringify(u.actual)}, expected ${JSON.stringify(u.expected)}`,
        );
      }
      console.error('Run: node tools/apply_license_overrides.mjs');
      return 1;
    }
    console.error(`OK: ${(overridesFile.overrides ?? []).length} override(s) applied`);
    return 0;
  }

  const { text, changes } = applyOverridesToText(rawCatalog, overridesFile);
  if (changes.length === 0) {
    console.error('no changes needed; catalog already matches the overrides');
    return 0;
  }

  await writeFile(opts.catalog, text, 'utf8');

  for (const c of changes) {
    console.error(`applied: ${c.id} ${c.key}: ${c.from} -> ${c.to}`);
  }
  console.error(`wrote ${opts.catalog} (${changes.length} change(s))`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
    .then((code) => process.exit(code))
    .catch((e) => {
      console.error('FATAL', e.message ?? e);
      process.exit(2);
    });
}
