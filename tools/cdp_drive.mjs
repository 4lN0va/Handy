#!/usr/bin/env node
/**
 * cdp_drive.mjs — drive the running Handy window through the Chrome DevTools
 * Protocol.
 *
 * WHY THIS EXISTS
 *   Handy renders into WebView2. Windows UI Automation can find and invoke
 *   controls, but it cannot read arbitrary rendered text or emit an application
 *   event from inside the page. CDP can do both, which makes it the only way to
 *   verify the event bridge end to end without a microphone.
 *
 * SCOPE AND SAFETY
 *   Every command targets one page of the app's own WebView2 instance, located by
 *   querying the debug endpoint the app was launched with. Nothing else on the
 *   desktop is touched, and no file outside the process is written.
 *
 *   The app must have been started with
 *     WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
 *
 * Usage:
 *   node tools/cdp_drive.mjs targets
 *   node tools/cdp_drive.mjs text                       # visible text of the page
 *   node tools/cdp_drive.mjs eval  "<javascript>"
 *   node tools/cdp_drive.mjs click "<css selector>"
 *   node tools/cdp_drive.mjs emit  <event-name> '<json payload>'
 *   node tools/cdp_drive.mjs wait  "<css selector>" [timeoutMs]
 *
 * Exit codes: 0 success, 1 command failed, 2 could not reach the debug endpoint.
 */

const DEBUG_PORT = process.env.CDP_PORT ? Number(process.env.CDP_PORT) : 9222;
const ENDPOINT = `http://127.0.0.1:${DEBUG_PORT}/json`;

/** Find the app's page target. */
async function findTarget() {
  let list;
  try {
    const res = await fetch(ENDPOINT);
    list = await res.json();
  } catch (err) {
    console.error(
      `cannot reach the DevTools endpoint at ${ENDPOINT}: ${err.message}\n` +
        `Start the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=${DEBUG_PORT}`,
    );
    process.exit(2);
  }
  // Prefer the main application window. Handy also creates a small always-on-top
  // "overlay" page for the recording indicator, and it usually registers first;
  // targeting it by accident would make every assertion read the wrong document.
  const pages = list.filter((t) => t.type === 'page' && t.webSocketDebuggerUrl);
  if (pages.length === 0) {
    console.error('no page targets found. Targets seen:');
    for (const t of list) console.error(`  ${t.type}  ${t.url}`);
    process.exit(2);
  }
  const wanted = process.env.CDP_TARGET_URL; // optional substring filter
  if (wanted) {
    const match = pages.find((t) => t.url.includes(wanted));
    if (!match) {
      console.error(`no page target matching CDP_TARGET_URL=${wanted}. Seen:`);
      for (const t of pages) console.error(`  ${t.url}`);
      process.exit(2);
    }
    return match;
  }
  return pages.find((t) => !t.url.includes('overlay')) ?? pages[0];
}

/** Minimal CDP client over the target's WebSocket. */
class Cdp {
  constructor(url) {
    this.url = url;
    this.nextId = 1;
    this.pending = new Map();
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);
      this.ws.addEventListener('open', () => resolve());
      this.ws.addEventListener('error', (e) =>
        reject(new Error(`websocket error: ${e.message ?? 'unknown'}`)),
      );
      this.ws.addEventListener('message', (ev) => {
        let msg;
        try {
          msg = JSON.parse(ev.data);
        } catch {
          return;
        }
        if (msg.id && this.pending.has(msg.id)) {
          const { resolve: res, reject: rej } = this.pending.get(msg.id);
          this.pending.delete(msg.id);
          if (msg.error) rej(new Error(JSON.stringify(msg.error)));
          else res(msg.result);
        }
      });
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`timeout waiting for ${method}`));
        }
      }, 20000);
    });
  }

  /** Evaluate an expression in the page, awaiting promises. */
  async evaluate(expression) {
    const result = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(
        `page exception: ${result.exceptionDetails.exception?.description ?? JSON.stringify(result.exceptionDetails)}`,
      );
    }
    return result.result?.value;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* ignore */
    }
  }
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

const [action, ...rest] = process.argv.slice(2);
if (!action) {
  fail('usage: cdp_drive.mjs <targets|text|eval|click|emit|wait> [args]');
}

const target = await findTarget();
const cdp = new Cdp(target.webSocketDebuggerUrl);
await cdp.connect();
await cdp.send('Runtime.enable');

try {
  switch (action) {
    case 'targets': {
      const res = await fetch(ENDPOINT);
      for (const t of await res.json()) console.log(`${t.type}\t${t.url}`);
      break;
    }

    case 'text': {
      const text = await cdp.evaluate(
        `document.body ? document.body.innerText : '(no body)'`,
      );
      console.log(text);
      break;
    }

    case 'eval': {
      const expr = rest.join(' ');
      if (!expr) fail('eval needs an expression');
      const out = await cdp.evaluate(expr);
      console.log(typeof out === 'string' ? out : JSON.stringify(out, null, 2));
      break;
    }

    case 'click': {
      const selector = rest[0];
      if (!selector) fail('click needs a CSS selector');
      const clicked = await cdp.evaluate(`(() => {
        const el = document.querySelector(${JSON.stringify(selector)});
        if (!el) return 'NOT_FOUND';
        el.scrollIntoView({ block: 'center' });
        el.click();
        return 'CLICKED';
      })()`);
      if (clicked === 'NOT_FOUND') fail(`no element matches ${selector}`);
      console.log(clicked);
      break;
    }

    case 'emit': {
      const eventName = rest[0];
      const payloadJson = rest[1] ?? 'null';
      if (!eventName) fail('emit needs an event name');
      // Tauri 2 exposes its IPC entry point on window.__TAURI_INTERNALS__. The
      // event plugin's emit command is `plugin:event|emit`.
      const result = await cdp.evaluate(`(async () => {
        const internals = window.__TAURI_INTERNALS__;
        if (!internals || typeof internals.invoke !== 'function') {
          return 'NO_TAURI_INTERNALS';
        }
        await internals.invoke('plugin:event|emit', {
          event: ${JSON.stringify(eventName)},
          payload: ${payloadJson},
        });
        return 'EMITTED';
      })()`);
      if (result === 'NO_TAURI_INTERNALS') {
        fail('window.__TAURI_INTERNALS__ is not available in this page');
      }
      console.log(result);
      break;
    }

    case 'wait': {
      const selector = rest[0];
      const timeout = Number(rest[1] ?? 10000);
      if (!selector) fail('wait needs a CSS selector');
      const found = await cdp.evaluate(`(async () => {
        const deadline = Date.now() + ${timeout};
        while (Date.now() < deadline) {
          if (document.querySelector(${JSON.stringify(selector)})) return true;
          await new Promise(r => setTimeout(r, 100));
        }
        return false;
      })()`);
      console.log(found ? 'FOUND' : 'TIMEOUT');
      if (!found) process.exitCode = 1;
      break;
    }

    default:
      fail(`unknown action: ${action}`);
  }
} finally {
  cdp.close();
}
