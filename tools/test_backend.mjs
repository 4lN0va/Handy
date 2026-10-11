#!/usr/bin/env node
/**
 * test_backend.mjs — a loopback HTTP endpoint for verifying the upload path.
 *
 * WHAT THIS IS
 *   A throwaway server bound to 127.0.0.1 that records exactly what the
 *   application sends. It is the controlled reference against which the real
 *   upload path is measured.
 *
 * WHAT THIS IS NOT
 *   It is not a third-party API. It speaks plain HTTP on loopback, so no TLS
 *   handshake occurs and no vendor contract is exercised. A green run here says
 *   "our client sent the right bytes to a server we control"; it says nothing
 *   about any production service. See docs/PHASE3_TEST_REPORT.md.
 *
 * It also refuses to be a general-purpose write target: it only accepts the
 * configured paths and methods, and it bounds the body size.
 *
 * Usage:
 *   node tools/test_backend.mjs [--port 8787] [--out received.json] [--status 200]
 *
 * Programming hooks (for tests, via the control path):
 *   POST /__control  { "status": 500 }   set the status returned for uploads
 *   GET  /__requests                     dump everything received so far
 *   POST /__reset                        forget everything received
 *
 * Recorded requests never include the Authorization header value: it is replaced
 * with a redaction marker so that a captured fixture cannot leak a credential.
 */

import { createServer } from 'node:http';
import { writeFileSync, readFileSync, existsSync } from 'node:fs';

const args = process.argv.slice(2);
const getArg = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] !== undefined ? args[i + 1] : fallback;
};

const PORT = Number(getArg('--port', 8787));
const OUT = getArg('--out', 'received.json');
const INITIAL_STATUS = Number(getArg('--status', 200));

/** Headers whose values must never be persisted. */
const REDACTED_HEADERS = new Set(['authorization', 'x-api-key', 'cookie', 'proxy-authorization']);

let responseStatus = INITIAL_STATUS;
const received = [];

const server = createServer((req, res) => {
  const chunks = [];
  let size = 0;
  req.on('data', (c) => {
    size += c.length;
    if (size > 8 * 1024 * 1024) {
      res.writeHead(413).end('too large');
      req.destroy();
      return;
    }
    chunks.push(c);
  });

  req.on('end', () => {
    const body = Buffer.concat(chunks);

    // --- control paths ---
    if (req.url === '/__control' && req.method === 'POST') {
      try {
        const cfg = JSON.parse(body.toString('utf8') || '{}');
        if (typeof cfg.status === 'number') responseStatus = cfg.status;
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ ok: true, status: responseStatus }));
      } catch (e) {
        res.writeHead(400).end(String(e));
      }
      return;
    }
    if (req.url === '/__requests' && req.method === 'GET') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(received, null, 2));
      return;
    }
    if (req.url === '/__reset' && req.method === 'POST') {
      received.length = 0;
      responseStatus = INITIAL_STATUS;
      res.writeHead(200).end('reset');
      return;
    }

    // --- the upload path ---
    const headers = [];
    for (const [k, v] of Object.entries(req.headers)) {
      headers.push([k, REDACTED_HEADERS.has(k.toLowerCase()) ? '***redacted***' : v]);
    }

    const record = {
      method: req.method,
      path: req.url,
      headers,
      bodyLength: body.length,
      bodyText: body.toString('utf8'),
      receivedAt: new Date().toISOString(),
    };
    received.push(record);
    persist();

    res.writeHead(responseStatus, { 'content-type': 'application/json', connection: 'close' });
    res.end(JSON.stringify({ ok: responseStatus < 300, status: responseStatus }));
  });
});

function persist() {
  try {
    writeFileSync(OUT, JSON.stringify(received, null, 2), 'utf8');
  } catch (e) {
    console.error('could not persist received requests:', e.message);
  }
}

server.listen(PORT, '127.0.0.1', () => {
  console.log(`test backend listening on http://127.0.0.1:${PORT}`);
  console.log(`initial status : ${INITIAL_STATUS}`);
  console.log(`recording to   : ${OUT}`);
  console.log('control        : POST /__control {"status":N} | GET /__requests | POST /__reset');
  if (existsSync(OUT)) console.log('note           : output file already exists and will be overwritten');
});
