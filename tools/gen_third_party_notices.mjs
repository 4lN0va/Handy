#!/usr/bin/env node
/**
 * gen_third_party_notices.mjs
 *
 * Generates THIRD_PARTY_NOTICES.md from the machine-readable dependency licence
 * audit (tools/license_audit.json, produced by tools/audit_rust_licenses.mjs).
 *
 * Why generated: the Rust dependency graph has hundreds of crates. A
 * hand-maintained notice file is wrong the moment a dependency is bumped, and a
 * wrong notice file is worse than none. Generating it makes the document a
 * faithful projection of the lockfile.
 *
 * What this document is NOT: legal advice. It records declared licence
 * identifiers; confirming that each obligation is satisfied is a separate,
 * human task (see LICENSE_AUDIT.md in the project root).
 *
 * Usage:
 *   node tools/gen_third_party_notices.mjs [--audit <path>] [--out <path>] [--check]
 *
 *   --check   Exit non-zero if the generated content would differ from the file
 *             on disk, i.e. the notices are stale. Intended for CI.
 */

import { readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const DEFAULT_AUDIT = 'tools/license_audit.json';
const DEFAULT_OUT = 'THIRD_PARTY_NOTICES.md';

/**
 * Licences that carry obligations beyond attribution, or that need a human to
 * confirm compatibility. Matched case-insensitively as substrings.
 */
export const ATTENTION_PATTERNS = ['gpl', 'mpl', 'cdla', 'unlicense', 'bsl', 'cc0'];

/** True when the licence expression deserves an explicit mention. */
export function needsAttention(license) {
  const l = String(license ?? '').toLowerCase();
  return ATTENTION_PATTERNS.some((p) => l.includes(p));
}

/** True when the expression offers a purely permissive option. */
export function hasPermissiveOption(license) {
  const l = String(license ?? '').toLowerCase();
  return /(^|[^a-z])(mit|apache-2\.0|bsd|isc|zlib|0bsd)([^a-z]|$)/.test(l);
}

/** Group packages by licence expression, returning [license, names[]] sorted. */
export function groupByLicense(packages) {
  const groups = new Map();
  for (const p of packages) {
    const key = p.license ?? '<NO LICENSE DECLARED>';
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(`${p.name} ${p.version}`);
  }
  for (const names of groups.values()) names.sort();
  return [...groups.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
}

export function render(audit) {
  const packages = audit.packages ?? [];
  const groups = groupByLicense(packages);
  const attention = packages.filter((p) => needsAttention(p.license));
  const unresolved = audit.unresolved ?? [];

  const lines = [];
  lines.push('# Third-Party Notices');
  lines.push('');
  lines.push(
    '> **Generated file — do not edit by hand.** Regenerate with `node tools/gen_third_party_notices.mjs`.',
  );
  lines.push('>');
  lines.push(`> Source: \`${audit.lockfile ?? 'Cargo.lock'}\`, audited ${audit.generated_at ?? 'unknown'}.`);
  lines.push('');
  lines.push('This project is a fork of [Handy](https://github.com/cjpais/Handy), which is');
  lines.push('licensed under the MIT License, Copyright (c) 2025 CJ Pais. That notice, and');
  lines.push('the notices of every dependency below, must be retained in redistributions.');
  lines.push('');
  lines.push('## Scope and limitations');
  lines.push('');
  lines.push('* Covers the **Rust** dependency graph as resolved in `Cargo.lock`.');
  lines.push('* **Does not** cover the JavaScript/frontend dependency tree, fonts, or icons.');
  lines.push('* **Does not** cover speech **models**, which are downloaded separately and are');
  lines.push('  documented in `MODEL_LICENSES.md`.');
  lines.push('* Records *declared* licence identifiers. It is not a legal opinion, and it does');
  lines.push('  not verify that each obligation has been met.');
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(`* Registry packages audited: **${audit.registry_count ?? packages.length}**`);
  lines.push(`* Git/path dependencies (not on crates.io): **${(audit.git_and_path_packages ?? []).length}**`);
  lines.push(`* Distinct licence expressions: **${groups.length}**`);
  lines.push(`* Packages with no declared licence: **${packages.filter((p) => !p.license).length}**`);
  lines.push(`* Unresolved during audit: **${unresolved.length}**`);
  lines.push('');
  if (unresolved.length > 0) {
    lines.push('> ⚠️ Some packages could not be resolved during the audit. Their licences are');
    lines.push('> **unknown**, not permissive. Re-run the audit when the registry rate limit clears.');
    lines.push('>');
    for (const u of unresolved.slice(0, 50)) {
      lines.push(`> * \`${u.name} ${u.version}\` — ${u.status}`);
    }
    lines.push('');
  }

  const gitDeps = audit.git_and_path_packages ?? [];
  if (gitDeps.length > 0) {
    lines.push('## Git and path dependencies');
    lines.push('');
    lines.push('These are not published on crates.io. They are pinned by commit in');
    lines.push('`Cargo.lock`, and **must be vendored before release**, because a deleted or');
    lines.push('force-pushed upstream branch would make the project unbuildable.');
    lines.push('');
    lines.push('| Package | Version | Source |');
    lines.push('|---|---|---|');
    for (const d of gitDeps) {
      const src = String(d.source ?? '(local path)');
      const short = src.length > 90 ? `${src.slice(0, 87)}…` : src;
      lines.push(`| \`${d.name}\` | ${d.version} | \`${short}\` |`);
    }
    lines.push('');
  }

  lines.push('## Licence expressions needing attention');
  lines.push('');
  if (attention.length === 0) {
    lines.push('None.');
  } else {
    lines.push('These carry obligations beyond plain attribution, or need a human to confirm');
    lines.push('compatibility. Where an expression is disjunctive (contains `OR`), selecting a');
    lines.push('permissive branch avoids the stricter obligation — that choice must be made');
    lines.push('deliberately and recorded.');
    lines.push('');
    lines.push('| Package | Version | License | Permissive option available? |');
    lines.push('|---|---|---|---|');
    for (const p of attention.sort((a, b) => a.name.localeCompare(b.name))) {
      const yes = hasPermissiveOption(p.license) ? 'yes' : '**no**';
      lines.push(`| \`${p.name}\` | ${p.version} | \`${p.license}\` | ${yes} |`);
    }
  }
  lines.push('');
  lines.push('### Notes on specific families');
  lines.push('');
  lines.push('* **MPL-2.0** (the `symphonia` audio-decoding family, `cssparser`, `selectors`,');
  lines.push('  `cbindgen`, `epoll`, `option-ext`) is *file-level* copyleft. Using these');
  lines.push('  unmodified alongside proprietary code is fine; a **modified** MPL file must be');
  lines.push('  published under the MPL. Do not patch these dependencies.');
  lines.push('* **`r-efi`** offers `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. Select MIT or');
  lines.push('  Apache-2.0 so no LGPL obligation applies.');
  lines.push('* **`Unlicense OR MIT` / `Unlicense/MIT`** packages: either branch is permissive.');
  lines.push('* **BSD-3-Clause / ISC / Zlib / BSL-1.0 / CC0 / CDLA-Permissive-2.0**: permissive;');
  lines.push('  retain the notice and licence text.');
  lines.push('');
  lines.push('## Full inventory by licence');
  lines.push('');
  for (const [license, names] of groups) {
    lines.push(`### \`${license}\` (${names.length})`);
    lines.push('');
    lines.push(names.map((n) => `\`${n}\``).join(', '));
    lines.push('');
  }
  lines.push('## Non-Rust components');
  lines.push('');
  lines.push('These are linked or invoked but do not appear as crates. Each needs its own');
  lines.push('notice, and the list is maintained by hand because it cannot be derived from');
  lines.push('`Cargo.lock` (a `-sys` binding crate does not carry the C library\'s licence).');
  lines.push('');
  lines.push('| Component | License | Note |');
  lines.push('|---|---|---|');
  lines.push('| whisper.cpp / ggml | MIT | via `transcribe-cpp` |');
  lines.push('| ONNX Runtime | MIT | via `ort` / `ort-sys` |');
  lines.push('| Silero VAD (model + runtime) | MIT | via `vad-rs`; **the `vad-rs` repository ships no LICENSE file** — see LICENSE_AUDIT.md |');
  lines.push('| SQLite | Public domain | bundled via `libsqlite3-sys` |');
  lines.push('| OpenBLAS | BSD-3-Clause | Linux CPU backend |');
  lines.push('| GTK 3 (C library) | LGPL-2.1-or-later | Linux; must remain dynamically linked and replaceable |');
  lines.push('| WebKitGTK (C library) | LGPL-2.1-or-later + BSD | Linux; same requirement |');
  lines.push('| gtk-layer-shell | MIT | Linux overlay |');
  lines.push('| libopus | BSD-3-Clause | only if the `opus` crate is enabled |');
  lines.push('| Vulkan loader / headers | Apache-2.0 / MIT | system component on Windows and Linux |');
  lines.push('');
  lines.push('> This fork **does not use FFmpeg**, which avoids the GPL/LGPL questions that');
  lines.push('> arise when bundling an FFmpeg build. Audio decoding uses `symphonia` (MPL-2.0).');
  lines.push('> If FFmpeg is ever added, it must be an LGPL build, dynamically linked.');
  lines.push('');
  return lines.join('\n');
}

export function parseArgs(argv) {
  const opts = { audit: DEFAULT_AUDIT, out: DEFAULT_OUT, check: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--audit') opts.audit = argv[++i];
    else if (argv[i] === '--out') opts.out = argv[++i];
    else if (argv[i] === '--check') opts.check = true;
  }
  return opts;
}

export async function main(argv = process.argv.slice(2)) {
  const opts = parseArgs(argv);
  if (!existsSync(opts.audit)) {
    console.error(
      `FATAL: audit file '${opts.audit}' not found.\n` +
        `Generate it first: node tools/audit_rust_licenses.mjs <Cargo.lock> ${opts.audit}`,
    );
    return 2;
  }
  const audit = JSON.parse(await readFile(opts.audit, 'utf8'));
  const markdown = render(audit);

  if (opts.check) {
    if (!existsSync(opts.out)) {
      console.error(`FAIL: ${opts.out} does not exist; run the generator`);
      return 1;
    }
    const onDisk = await readFile(opts.out, 'utf8');
    if (onDisk !== markdown) {
      console.error(`FAIL: ${opts.out} is stale; re-run node tools/gen_third_party_notices.mjs`);
      return 1;
    }
    console.error(`OK: ${opts.out} is up to date`);
    return 0;
  }

  await writeFile(opts.out, markdown, 'utf8');
  console.error(`wrote ${opts.out} (${(audit.packages ?? []).length} packages)`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
    .then((code) => process.exit(code))
    .catch((e) => {
      console.error('FATAL', e);
      process.exit(2);
    });
}
