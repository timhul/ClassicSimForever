// The live viewer's server side in the page: csim-web (WebAssembly, bound by
// `wasm-bindgen --target web` into csim_web.js next to this file). The page (index.html) waits
// for `csimReady`, then sends its requests to `csimTransport` instead of over HTTP, and the
// Sim view's (`api/sim/*`) to `csimSimTransport`, which runs them in a Web Worker
// (sim-worker.js) so that a sim does not hold up the page.
import init, { LiveApp } from "./csim_web.js";

window.csimReady = (async () => {
  await init();
  // The seed of the iterations the page names none for.
  const [high, low] = crypto.getRandomValues(new Uint32Array(2));
  const app = new LiveApp(high, low);
  // `method` `path` (`/api/...`) with the text `body`: {status, contentType, body}.
  window.csimTransport = (method, path, body) => {
    const reply = app.handle(method, path, body);
    try {
      return { status: reply.status, contentType: reply.contentType, body: reply.body };
    } finally {
      reply.free();
    }
  };
})();

// The worker, started at the first sim request (it loads the data again: the Live view alone
// does not need it).
let worker = null;
// The requests sent to the worker and not answered yet, by id: their promise's settlers.
const pending = new Map();
let nextId = 0;

function simWorker() {
  if (worker) {
    return worker;
  }
  worker = new Worker(new URL("./sim-worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (event) => {
    const { id, fatal, ...reply } = event.data;
    pending.get(id)?.resolve(reply);
    pending.delete(id);
    if (fatal) {
      dropWorker(new Error(reply.body));
    }
  };
  // The worker did not load or died.
  worker.onerror = (event) => {
    event.preventDefault();
    dropWorker(new Error(`the sim worker failed: ${event.message || "it did not load"}`));
  };
  return worker;
}

// Ends the worker: its requests fail with `error`, and the next one starts a new worker.
function dropWorker(error) {
  for (const { reject } of pending.values()) {
    reject(error);
  }
  pending.clear();
  worker?.terminate();
  worker = null;
}

// As `csimTransport`, in the worker: a promise of {status, contentType, body}.
window.csimSimTransport = (method, path, body) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  simWorker().postMessage({ id, method, path, body });
});
