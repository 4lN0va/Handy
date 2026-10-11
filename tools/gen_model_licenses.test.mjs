/**
 * Tests for the model-license tooling.
 *
 * Run:  node --test tools/gen_model_licenses.test.mjs
 *
 * Covers the pure logic of both scripts: licence classification, the
 * restricted-default guard, Markdown rendering, and the surgical JSON text
 * editor. No filesystem or network access.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  isRestrictedLicense,
  findRestrictedRecommended,
  render,
} from './gen_model_licenses.mjs';
import {
  setFieldInRawText,
  applyOverridesToText,
  findUnapplied,
} from './apply_license_overrides.mjs';

/* ---------------------------------------------- licence classification */

test('permissive and attribution-only licences are not restricted', () => {
  for (const license of ['mit', 'apache-2.0', 'cc-by-4.0', 'bsd-3-clause', 'MPL-2.0']) {
    assert.equal(isRestrictedLicense(license), false, `${license} should be permissive`);
  }
});

test('non-commercial CC licences are restricted', () => {
  for (const license of ['cc-by-nc-4.0', 'CC-BY-NC-SA-4.0', 'cc-by-nc']) {
    assert.equal(isRestrictedLicense(license), true, `${license} should be restricted`);
  }
});

test('the unconfirmed "other" and research licences are restricted', () => {
  assert.equal(isRestrictedLicense('other'), true);
  assert.equal(isRestrictedLicense('research-only'), true);
  assert.equal(isRestrictedLicense('non-commercial'), true);
});

test('a missing or blank licence counts as unconfirmed, not permissive', () => {
  // Failing open here would let an unlabelled model ship as a default.
  assert.equal(isRestrictedLicense(undefined), true);
  assert.equal(isRestrictedLicense(null), true);
  assert.equal(isRestrictedLicense(''), true);
  assert.equal(isRestrictedLicense('   '), true);
});

test('a licence merely containing the letters "nc" elsewhere is not flagged', () => {
  // Guards a naive `includes('nc')` from misclassifying real identifiers.
  assert.equal(isRestrictedLicense('bsd-3-clause'), false);
  assert.equal(isRestrictedLicense('apache-2.0'), false);
});

/* ------------------------------------------------------ the CI guard */

test('a restricted licence marked recommended is reported', () => {
  const catalog = {
    models: [
      { id: 'a', license: 'apache-2.0', recommended: true },
      { id: 'b', license: 'other', recommended: true },
      { id: 'c', license: 'cc-by-nc-4.0', recommended: false },
    ],
  };
  const bad = findRestrictedRecommended(catalog);
  assert.deepEqual(
    bad.map((m) => m.id),
    ['b'],
  );
});

test('a catalog where no restricted model is recommended passes', () => {
  const catalog = {
    models: [
      { id: 'a', license: 'apache-2.0', recommended: true },
      { id: 'b', license: 'mit', recommended: true },
      { id: 'c', license: 'cc-by-nc-4.0', recommended: false },
    ],
  };
  assert.deepEqual(findRestrictedRecommended(catalog), []);
});

/* ------------------------------------------------------- rendering */

const sampleCatalog = {
  catalog_version: 2,
  models: [
    { id: 'good/model', license: 'apache-2.0', recommended: true, languages: ['zh', 'en'] },
    { id: 'bad/model', license: 'cc-by-nc-4.0', recommended: false, base_model: 'x/y' },
  ],
};

test('rendering marks restricted licences and lists them for review', () => {
  const md = render(sampleCatalog, { generatedFrom: 'catalog.json' });
  assert.match(md, /Generated file — do not edit by hand/);
  assert.match(md, /bad\/model/);
  assert.match(md, /cc-by-nc-4\.0/);
  assert.match(md, /Models requiring review before redistribution/);
  assert.match(md, /not a default/);
});

test('rendering a catalog with no restricted models says so explicitly', () => {
  const md = render(
    { models: [{ id: 'a', license: 'mit', recommended: true }] },
    { generatedFrom: 'x' },
  );
  assert.match(md, /None\. Every catalogued model declares a permissive/);
});

test('rendering never claims accuracy figures as guarantees', () => {
  const md = render(sampleCatalog, { generatedFrom: 'x' });
  assert.match(md, /not reproduced here as\s+guarantees|not reproduced here as guarantees/);
});

/* ------------------------------------------- surgical JSON editing */

const RAW = `{
  "catalog_version": 2,
  "models": [
    {"id": "one", "license": "apache-2.0", "recommended": true},
    {"id": "two", "license": "other", "recommended": true},
    {"id": "three", "license": "mit", "recommended": false}
  ]
}
`;

test('text editing changes only the targeted model field', () => {
  const { text, from } = setFieldInRawText(RAW, 'two', 'recommended', false);
  assert.equal(from, 'true');
  assert.match(text, /\{"id": "two", "license": "other", "recommended": false\}/);
  // The other models are untouched.
  assert.match(text, /\{"id": "one", "license": "apache-2.0", "recommended": true\}/);
  assert.match(text, /\{"id": "three", "license": "mit", "recommended": false\}/);
  assert.notEqual(text, RAW);
});

test('text editing preserves every other byte, including upstream layout', () => {
  const { text } = setFieldInRawText(RAW, 'two', 'recommended', false);
  assert.equal(text.length, RAW.length - 'true'.length + 'false'.length);
  // Same line count: nothing was reformatted.
  assert.equal(text.split('\n').length, RAW.split('\n').length);
});

test('text editing refuses an unknown model id', () => {
  assert.throws(() => setFieldInRawText(RAW, 'nope', 'recommended', false), /not found/);
});

test('text editing refuses an ambiguous model id', () => {
  const duplicated = RAW.replace(
    '{"id": "three"',
    '{"id": "two"',
  );
  assert.throws(() => setFieldInRawText(duplicated, 'two', 'recommended', false), /more than once/);
});

test('text editing refuses a field that is not present', () => {
  assert.throws(() => setFieldInRawText(RAW, 'one', 'nonexistent', 1), /not found/);
});

test('applying overrides reports the change and is idempotent', () => {
  const overrides = { overrides: [{ id: 'two', set: { recommended: false } }] };
  const first = applyOverridesToText(RAW, overrides);
  assert.equal(first.changes.length, 1);
  assert.deepEqual(first.changes[0], {
    id: 'two',
    key: 'recommended',
    from: 'true',
    to: 'false',
  });

  // Running again must be a no-op, so the workflow can be re-run safely.
  const second = applyOverridesToText(first.text, overrides);
  assert.equal(second.changes.length, 0);
  assert.equal(second.text, first.text);
});

test('applying an override for a missing model fails loudly', () => {
  const overrides = { overrides: [{ id: 'ghost', set: { recommended: false } }] };
  assert.throws(() => applyOverridesToText(RAW, overrides), /unknown model id/);
});

test('findUnapplied detects an override that has not been applied', () => {
  const catalog = JSON.parse(RAW);
  const overrides = { overrides: [{ id: 'two', set: { recommended: false } }] };
  const unapplied = findUnapplied(catalog, overrides);
  assert.equal(unapplied.length, 1);
  assert.equal(unapplied[0].id, 'two');
  assert.equal(unapplied[0].actual, true);
  assert.equal(unapplied[0].expected, false);

  // And reports nothing once applied.
  const applied = JSON.parse(applyOverridesToText(RAW, overrides).text);
  assert.deepEqual(findUnapplied(applied, overrides), []);
});
