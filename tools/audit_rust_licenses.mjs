#!/usr/bin/env node
/**
 * audit_rust_licenses.mjs
 *
 * Builds a verifiable third-party license inventory for a Rust project by
 * reading its Cargo.lock and querying the crates.io registry API for the
 * declared SPDX license expression of every *exact resolved version*.
 *
 * Why this exists: a project's own LICENSE file says nothing about its
 * dependency tree. Commercial distribution decisions need the actual resolved
 * versions, not the caret ranges in Cargo.toml.
 *
 * crates.io rate-limits unauthenticated traffic aggressively (HTTP 429), so
 * this tool:
 *   - honours Retry-After when present, else uses exponential backoff + jitter,
 *   - keeps a persistent on-disk cache so a re-run only fetches what is missing,
 *   - reports unresolved entries explicitly instead of silently treating a
 *     throttled response as "no license declared".
 *
 * Usage:
 *   node tools/audit_rust_licenses.mjs <path-to-Cargo.lock> <out.json>
 *        [--concurrency N] [--cache path] [--no-cache]
 *
 * Exit codes: 0 = complete, 1 = fatal error, 3 = completed with unresolved
 * entries (so CI can fail the build on an incomplete audit rather than
 * publishing a misleading one).
 *
 * Output JSON:
 *   { generated_at, lockfile, package_count, registry_count, cache_hits,
 *     git_and_path_packages: [...], license_summary: {...},
 *     unresolved: [...], packages: [ {name, version, license, repository} ] }
 */

import { readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const CONCURRENCY_DEFAULT = 4;
const REGISTRY_API = 'https://crates.io/api/v1/crates';
const USER_AGENT =
  'license-audit-script/1.1 (dependency license review; node fetch)';
const MAX_ATTEMPTS = 6;
const DEFAULT_CACHE = 'tools/.license_cache.json';

function parseArgs(argv) {
  const positional = [];
  let concurrency = CONCURRENCY_DEFAULT;
  let cachePath = DEFAULT_CACHE;
  let useCache = true;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--concurrency') concurrency = Number.parseInt(argv[++i], 10);
    else if (a === '--cache') cachePath = argv[++i];
    else if (a === '--no-cache') useCache = false;
    else positional.push(a);
  }
  const [lockPath, outPath] = positional;
  if (!lockPath || !outPath) {
    console.error(
      'usage: node audit_rust_licenses.mjs <Cargo.lock> <out.json> [--concurrency N] [--cache path] [--no-cache]',
    );
    process.exit(2);
  }
  if (!Number.isInteger(concurrency) || concurrency < 1) concurrency = CONCURRENCY_DEFAULT;
  return { lockPath, outPath, concurrency, cachePath, useCache };
}

/** Parse a Cargo.lock into [{name, version, source}]. */
export function parseCargoLock(text) {
  const packages = [];
  // Split on the [[package]] table header; each chunk holds that package's keys.
  const chunks = text.split(/^\[\[package\]\]\s*$/m).slice(1);
  for (const chunk of chunks) {
    const body = chunk.split(/^\[/m)[0]; // stop at the next table header
    const name = /^name\s*=\s*"([^"]+)"/m.exec(body)?.[1];
    const version = /^version\s*=\s*"([^"]+)"/m.exec(body)?.[1];
    const source = /^source\s*=\s*"([^"]+)"/m.exec(body)?.[1] ?? null;
    if (name && version) packages.push({ name, version, source });
  }
  return packages;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const cacheKey = (name, version) => `${name}@${version}`;

/**
 * Fetch one crate version's license. Returns
 * { status: 'ok'|'not-found'|'rate-limited'|'http-error'|'network-error', ... }.
 * Retries 429/5xx/network faults with Retry-After-aware exponential backoff.
 *
 * `sleepImpl` is injectable so tests can assert retry behaviour without
 * actually waiting out the backoff.
 */
export async function fetchLicense(
  name,
  version,
  { fetchImpl = fetch, sleepImpl = sleep } = {},
) {
  const url = `${REGISTRY_API}/${encodeURIComponent(name)}/${encodeURIComponent(version)}`;
  let last = { status: 'unknown' };

  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    try {
      const res = await fetchImpl(url, { headers: { 'User-Agent': USER_AGENT } });

      if (res.status === 404) return { status: 'not-found' };

      if (res.status === 429 || res.status >= 500) {
        // Retry-After is in seconds per RFC 9110; crates.io sends it on 429.
        const retryAfter = Number.parseInt(res.headers?.get?.('retry-after') ?? '', 10);
        const waitMs = Number.isFinite(retryAfter)
          ? retryAfter * 1000
          : Math.min(2 ** attempt * 400, 15000) + Math.random() * 400;
        last = { status: 'rate-limited', code: res.status };
        if (attempt < MAX_ATTEMPTS) await sleepImpl(waitMs);
        continue;
      }

      if (!res.ok) return { status: 'http-error', code: res.status };

      const json = await res.json();
      const license = json?.version?.license ?? null;
      // A 200 with no license field is a real "undeclared", not a throttle.
      return {
        status: 'ok',
        license,
        license_file: json?.version?.license_file ?? null,
        repository: json?.version?.repository ?? null,
      };
    } catch (err) {
      last = { status: 'network-error', error: String(err?.message ?? err) };
      if (attempt < MAX_ATTEMPTS) await sleepImpl(Math.min(2 ** attempt * 300, 10000));
    }
  }
  return last;
}

/** Run `worker` over `items` with a fixed concurrency ceiling. */
export async function mapLimit(items, limit, worker) {
  const results = new Array(items.length);
  let next = 0;
  const runners = Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async () => {
    for (;;) {
      const i = next++;
      if (i >= items.length) return;
      results[i] = await worker(items[i], i);
    }
  });
  await Promise.all(runners);
  return results;
}

export async function loadCache(cachePath, useCache) {
  if (!useCache || !existsSync(cachePath)) return {};
  try {
    return JSON.parse(await readFile(cachePath, 'utf8'));
  } catch {
    console.error(`warning: cache at ${cachePath} unreadable; ignoring`);
    return {};
  }
}

/**
 * Aggregate license expressions into a histogram.
 *
 * Entries explicitly marked `resolved: false` are SKIPPED: an unresolved
 * (e.g. throttled) crate has no known license, and counting it as
 * "<NO LICENSE DECLARED>" would report a throttling artefact as a legal fact.
 * Those are surfaced separately in the `unresolved` array instead.
 */
export function summarize(packages) {
  const summary = {};
  for (const p of packages) {
    if (p.resolved === false) continue;
    const key = p.license ?? '<NO LICENSE DECLARED>';
    summary[key] = (summary[key] ?? 0) + 1;
  }
  return summary;
}

export async function runAudit({
  lockPath,
  outPath,
  concurrency = CONCURRENCY_DEFAULT,
  cachePath = DEFAULT_CACHE,
  useCache = true,
  fetchImpl,
  log = console.error,
} = {}) {
  const lockText = await readFile(lockPath, 'utf8');
  const all = parseCargoLock(lockText);

  // Only registry packages are queryable on crates.io. Git/path dependencies
  // carry their own LICENSE in their repository and must be audited by hand.
  const registry = all.filter((p) => p.source?.startsWith('registry+'));
  const nonRegistry = all.filter((p) => !p.source?.startsWith('registry+'));

  const cache = await loadCache(cachePath, useCache);
  let cacheHits = 0;

  const toFetch = [];
  const known = new Map();
  for (const pkg of registry) {
    const hit = cache[cacheKey(pkg.name, pkg.version)];
    if (hit && hit.status === 'ok') {
      known.set(cacheKey(pkg.name, pkg.version), hit);
      cacheHits++;
    } else {
      toFetch.push(pkg);
    }
  }

  log(
    `parsed ${all.length} packages: ${registry.length} registry, ${nonRegistry.length} git/path; ` +
      `${cacheHits} from cache, ${toFetch.length} to fetch`,
  );

  let done = 0;
  const fetched = await mapLimit(toFetch, concurrency, async (pkg) => {
    const info = await fetchLicense(pkg.name, pkg.version, {
      ...(fetchImpl ? { fetchImpl } : {}),
      sleepImpl: async () => {},
    });
    if (++done % 25 === 0) log(`  ...${done}/${toFetch.length}`);
    if (info.status === 'ok') cache[cacheKey(pkg.name, pkg.version)] = info;
    return { ...pkg, ...info };
  });

  if (useCache) {
    try {
      await writeFile(cachePath, JSON.stringify(cache, null, 2), 'utf8');
    } catch (err) {
      log(`warning: could not persist cache: ${err.message}`);
    }
  }

  const merged = [];
  const unresolved = [];
  for (const pkg of registry) {
    // IMPORTANT: read name/version from the lockfile `pkg`, never from the
    // cache record. A cache record only stores {status, license, repository},
    // so `rec.name` is undefined and would crash the sort below.
    const hit = known.get(cacheKey(pkg.name, pkg.version));
    const rec =
      hit ?? fetched.find((f) => f.name === pkg.name && f.version === pkg.version);
    if (!rec) {
      unresolved.push({ name: pkg.name, version: pkg.version, status: 'missing-from-run' });
      merged.push({
        name: pkg.name,
        version: pkg.version,
        license: null,
        repository: null,
        resolved: false,
      });
      continue;
    }
    if (rec.status !== 'ok') {
      unresolved.push({
        name: pkg.name,
        version: pkg.version,
        status: rec.status,
        code: rec.code,
        error: rec.error,
      });
      merged.push({
        name: pkg.name,
        version: pkg.version,
        license: null,
        repository: null,
        resolved: false,
      });
      continue;
    }
    merged.push({
      name: pkg.name,
      version: pkg.version,
      license: rec.license,
      repository: rec.repository ?? null,
      resolved: true,
    });
  }

  merged.sort((a, b) => a.name.localeCompare(b.name));

  const out = {
    generated_at: new Date().toISOString(),
    lockfile: lockPath,
    package_count: all.length,
    registry_count: registry.length,
    cache_hits: cacheHits,
    unresolved_count: unresolved.length,
    git_and_path_packages: nonRegistry.map(({ name, version, source }) => ({ name, version, source })),
    license_summary: summarize(merged),
    unresolved,
    packages: merged,
  };

  await writeFile(outPath, JSON.stringify(out, null, 2), 'utf8');
  log(`wrote ${outPath}`);
  log(`--- license summary (${unresolved.length} unresolved) ---`);
  for (const [lic, count] of Object.entries(out.license_summary).sort((a, b) => b[1] - a[1])) {
    log(`  ${String(count).padStart(4)}  ${lic}`);
  }
  return out;
}

// Only run when invoked directly, so the module stays importable by tests.
// pathToFileURL is required (not naive string splicing): import.meta.url is
// percent-encoded, so any checkout path containing a space would never match.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const opts = parseArgs(process.argv.slice(2));
  runAudit(opts)
    .then((out) => {
      // Distinguish "audit finished" from "audit finished with holes".
      process.exit(out.unresolved.length > 0 ? 3 : 0);
    })
    .catch((err) => {
      console.error('FATAL', err);
      process.exit(1);
    });
}
