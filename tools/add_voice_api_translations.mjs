/**
 * add_voice_api_translations.mjs
 *
 * Adds the fork's UI strings to the English locale.
 *
 * Why a script instead of a hand edit: `translation.json` is a large file that
 * many keys reference, and a stray comma silently breaks i18next at runtime. This
 * parses, merges the new keys, and re-serialises, so a malformed result is
 * impossible.
 *
 * Only English is added. i18next falls back to the `en` locale for any missing
 * key in the other 25 locales, so the new screens are usable everywhere; adding
 * real translations is a separate, community-facing task.
 *
 * Usage: node tools/add_voice_api_translations.mjs [--check]
 *   --check  exit 1 if any key is missing (for CI)
 */

import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const LOCALE = 'src/i18n/locales/en/translation.json';

/** The keys this fork's UI needs, grouped to mirror the components. */
export const ADDITIONS = {
  sidebar: {
    transcript: 'Transcript',
    voiceApi: 'Voice API',
  },
  transcript: {
    title: 'Transcript',
    description:
      'Live recognition results. Italic text is still being recognized and may change; confirmed text has been finalized.',
    empty: 'No speech recognized yet. Start dictating to see results here.',
    confirmed: 'Confirmed',
    recognizing: 'Recognizing',
    actionsTitle: 'Transcript actions',
    finalsOnly: 'Confirmed text only',
    finalsOnlyDescription:
      'Copy and export only the finalized segments, excluding anything still being recognized.',
    copy: 'Copy',
    export: 'Export JSON',
    clear: 'Clear',
    copied: 'Copied to clipboard.',
    exported: 'Exported to {{path}}',
    save: 'Save',
    cancel: 'Cancel',
    editHint: 'Click to edit this segment. Editing marks it confirmed.',
    uploadTitle: 'Upload to your backend',
    uploadDescription:
      'Uploads the transcript as JSON to the endpoint configured in Voice API. Nothing is uploaded unless you press the button below.',
    willUpload: 'This will be sent',
    session: 'Session',
    language: 'Language',
    segments: 'Segments',
    confirmedShort: 'confirmed',
    nothingToUpload: 'Nothing to upload yet.',
    uploadNow: 'Upload now',
    uploading: 'Uploading…',
    uploadManualNote:
      'Manual only — this fork never uploads automatically.',
    uploaded: 'Uploaded (HTTP {{status}}, {{attempts}} attempt(s)).',
    uploadSkipped: 'Nothing was uploaded.',
  },
  voiceApi: {
    endpointTitle: 'Endpoint',
    endpointDescription:
      'Where transcripts are sent. HTTPS is required unless you explicitly allow plain HTTP for a local backend.',
    baseUrl: 'Base URL',
    baseUrlDescription:
      'For example https://api.example.com/v1. A trailing slash is ignored.',
    endpointPath: 'Endpoint path',
    endpointPathDescription:
      'Appended to the base URL. A leading slash resets to the host root; a full URL replaces the base entirely.',
    method: 'HTTP method',
    methodDescription: 'POST, PUT or PATCH.',
    contentType: 'Content-Type',
    contentTypeDescription: 'Sent as the request Content-Type header.',
    bodyTitle: 'Request body',
    bodyDescription:
      'Backends disagree about request shapes, so the body is configurable rather than assumed.',
    bodyFormat: 'Body format',
    bodyFormatDescription: 'How the transcript is placed in the request.',
    bodyRaw: 'Transcript document',
    bodyWrapped: 'Wrapped in a field',
    bodyTemplate: 'Custom JSON template',
    wrapField: 'Wrapping field',
    wrapFieldDescription:
      'The transcript is nested under this key, e.g. {"transcript": {…}}.',
    template: 'JSON template',
    templateDescription:
      'A JSON object containing {{transcript}}, which is replaced by the transcript document.',
    authTitle: 'Authentication',
    authDescription:
      'The API key is stored in the operating system credential store — never in a settings file, a transcript export, or a log.',
    apiKey: 'API key',
    apiKeyStoredIn: 'A key is stored in the {{where}}.',
    apiKeyNotSet: 'No API key stored.',
    saveKey: 'Save key',
    clearKey: 'Remove key',
    apiKeyNote:
      'The key is write-only: the app never displays it again after saving.',
    customHeaders: 'Custom headers',
    customHeadersDescription:
      'One header per line as Name: Value. Authorization is managed by the key above.',
    behaviourTitle: 'Behaviour',
    behaviourDescription:
      'Timeouts, retries and duplicate protection for uploads.',
    timeout: 'Timeout (ms)',
    timeoutDescription: 'How long a single request may take before it fails.',
    maxAttempts: 'Maximum attempts',
    maxAttemptsDescription:
      'Total tries including the first. Only timeouts, connection failures, HTTP 429 and 5xx are retried.',
    idempotency: 'Send idempotency key',
    idempotencyDescription:
      'Sends an Idempotency-Key derived from the session and the exact request body, so a backend can recognise a retry instead of storing it twice. Only helps if the server honours it.',
    allowHttp: 'Allow insecure HTTP',
    allowHttpDescription:
      'Off by default. Only enable this for a backend on localhost; it sends your transcript in cleartext.',
    save: 'Save configuration',
    working: 'Working…',
    testConnection: 'Test connection',
    saved: 'Configuration saved.',
    keySaved: 'API key saved to the OS credential store.',
    keyCleared: 'API key removed.',
    testResult: 'Test result',
  },
};

/** Merge `additions` into `target`, returning the set of missing paths. */
export function mergeAdditions(target, additions, prefix = '') {
  for (const [key, value] of Object.entries(additions)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === 'object') {
      if (target[key] === undefined) target[key] = {};
      if (typeof target[key] !== 'object' || Array.isArray(target[key])) {
        throw new Error(`cannot merge into non-object at '${path}'`);
      }
      mergeAdditions(target[key], value, path);
    } else {
      target[key] = value;
    }
  }
}

/** List dotted paths that are absent from `target`. */
export function findMissing(target, additions, prefix = '') {
  const missing = [];
  for (const [key, value] of Object.entries(additions)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === 'object') {
      missing.push(...findMissing(target[key] ?? {}, value, path));
    } else if (target[key] === undefined) {
      missing.push(path);
    }
  }
  return missing;
}

export async function main(argv = process.argv.slice(2)) {
  const check = argv.includes('--check');
  const raw = await readFile(LOCALE, 'utf8');
  const json = JSON.parse(raw);

  if (check) {
    const missing = findMissing(json, ADDITIONS);
    if (missing.length > 0) {
      console.error(`FAIL: ${missing.length} UI string(s) missing from ${LOCALE}:`);
      for (const m of missing.slice(0, 40)) console.error(`  ${m}`);
      return 1;
    }
    console.error(`OK: all fork UI strings present in ${LOCALE}`);
    return 0;
  }

  mergeAdditions(json, ADDITIONS);
  // Match the file's existing formatting: two-space indent, trailing newline.
  await writeFile(LOCALE, `${JSON.stringify(json, null, 2)}\n`, 'utf8');
  console.error(`updated ${LOCALE}`);
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
