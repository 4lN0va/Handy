#!/usr/bin/env node
/**
 * sync_translations.mjs — keep every locale structurally identical to `en`.
 *
 * WHY
 *   `scripts/check-translations.ts` (upstream) fails a run when any locale has a
 *   key `en` does not, or lacks one `en` has. This fork adds strings to `en` only
 *   and relies on i18next's English fallback, so the check fails by design — 25 of
 *   25 locales, including `en`'s own extras. That is not a bug in the strings; it
 *   is two different policies for the same file.
 *
 *   This tool adopts the upstream policy without pretending the missing text is
 *   translated: it rebuilds each locale from the reference's **key order and
 *   shape**, keeping whatever translation already exists and filling the rest
 *   with the English string. The result passes the check, and every untranslated
 *   string is visibly English rather than silently absent.
 *
 * NOT A TRANSLATOR
 *   It never rewrites a string that is already present in a locale. Running it
 *   again after a translator has worked changes nothing.
 *
 * Usage:
 *   node tools/sync_translations.mjs           # write
 *   node tools/sync_translations.mjs --check   # exit 1 if anything would change
 */

import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const LOCALES = join(here, "..", "src", "i18n", "locales");
const REFERENCE = "en";
const CHECK = process.argv.includes("--check");

const isPlainObject = (v) =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * Rebuild `target` from `reference`'s shape: reference key order, target value
 * where the key exists, reference value otherwise.
 *
 * Builds from the reference rather than patching the target, which is what makes
 * *extra* keys disappear as a side effect instead of needing a second pass.
 */
function rebuild(reference, target) {
  if (!isPlainObject(reference)) {
    // A leaf: keep a translated string, otherwise fall back to English. A
    // target that holds an object where the reference holds a string is
    // malformed, so it is replaced rather than preserved.
    return typeof target === "string" ? target : reference;
  }
  const out = {};
  for (const key of Object.keys(reference)) {
    out[key] = rebuild(
      reference[key],
      isPlainObject(target) ? target[key] : undefined,
    );
  }
  return out;
}

function countLeaves(obj) {
  if (!isPlainObject(obj)) return 1;
  return Object.values(obj).reduce((n, v) => n + countLeaves(v), 0);
}

const referencePath = join(LOCALES, REFERENCE, "translation.json");
if (!existsSync(referencePath)) {
  console.error(`reference locale not found: ${referencePath}`);
  process.exit(2);
}
const reference = JSON.parse(readFileSync(referencePath, "utf8"));

const languages = readdirSync(LOCALES, { withFileTypes: true })
  .filter((e) => e.isDirectory() && e.name !== REFERENCE)
  .map((e) => e.name)
  .sort();

let changed = 0;
let filled = 0;

for (const lang of languages) {
  const file = join(LOCALES, lang, "translation.json");
  if (!existsSync(file)) {
    console.log(`  ${lang.padEnd(10)} MISSING FILE`);
    changed += 1;
    continue;
  }

  let current;
  try {
    current = JSON.parse(readFileSync(file, "utf8"));
  } catch (e) {
    console.log(`  ${lang.padEnd(10)} INVALID JSON: ${e.message}`);
    changed += 1;
    continue;
  }

  const merged = rebuild(reference, current);
  const before = JSON.stringify(current);
  const after = JSON.stringify(merged);

  if (before === after) {
    console.log(
      `  ${lang.padEnd(10)} unchanged (${countLeaves(current)} keys)`,
    );
    continue;
  }

  // How much of this locale was actually translated, before we fill the rest.
  const alreadyEnglish = countEnglishOverlap(reference, current);
  filled += countLeaves(reference) - alreadyEnglish;

  if (CHECK) {
    console.log(`  ${lang.padEnd(10)} WOULD CHANGE`);
  } else {
    writeFileSync(file, JSON.stringify(merged, null, 2) + "\n", "utf8");
    console.log(`  ${lang.padEnd(10)} updated`);
  }
  changed += 1;
}

/** Leaves present in the target with a non-English value, i.e. real translations. */
function countEnglishOverlap(reference, target) {
  if (!isPlainObject(reference)) {
    return typeof target === "string" && target !== reference ? 1 : 0;
  }
  let n = 0;
  for (const key of Object.keys(reference)) {
    n += countEnglishOverlap(
      reference[key],
      isPlainObject(target) ? target[key] : undefined,
    );
  }
  return n;
}

console.log("");
if (CHECK) {
  console.log(
    changed === 0
      ? `OK: all ${languages.length} locales match the reference shape`
      : `FAIL: ${changed} locale(s) differ from the reference shape`,
  );
  process.exit(changed === 0 ? 0 : 1);
} else {
  console.log(
    changed === 0
      ? `OK: all ${languages.length} locales already match the reference shape`
      : `updated ${changed} locale(s); ${filled} string(s) filled with English`,
  );
}
