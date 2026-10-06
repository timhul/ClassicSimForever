// The live viewer's server side in the page: csim-web (WebAssembly, bound by
// `wasm-bindgen --target web` into csim_web.js next to this file). The page (index.html) waits
// for `csimReady`, then sends its requests to `csimTransport` instead of over HTTP, and the
// Sim view's (`api/sim/*`) to `csimSimTransport`, which runs them in Web Workers
// (sim-worker.js), one per lane, so that a sim does not hold up the page and its shares run side
// by side.
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

// The workers, one per lane: the shares of a run over several threads run side by side, a
// lane each. A lane's worker starts at its first request (it loads the data again: the Live
// view alone does not need any).
const workers = new Map();
// The requests sent to a worker and not answered yet, by id: their lane and promise's settlers.
const pending = new Map();
let nextId = 0;

function simWorker(lane) {
  if (workers.has(lane)) {
    return workers.get(lane);
  }
  const worker = new Worker(new URL("./sim-worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (event) => {
    const { id, fatal, ...reply } = event.data;
    pending.get(id)?.resolve(reply);
    pending.delete(id);
    if (fatal) {
      dropWorker(lane, new Error(reply.body));
    }
  };
  // The worker did not load or died.
  worker.onerror = (event) => {
    event.preventDefault();
    dropWorker(lane, new Error(`the sim worker failed: ${event.message || "it did not load"}`));
  };
  workers.set(lane, worker);
  return worker;
}

// Ends the worker of `lane`: its requests fail with `error`, and its next one starts a new
// worker.
function dropWorker(lane, error) {
  for (const [id, request] of pending) {
    if (request.lane === lane) {
      request.reject(error);
      pending.delete(id);
    }
  }
  workers.get(lane)?.terminate();
  workers.delete(lane);
}

// As `csimTransport`, in the worker of `lane` (0 without): a promise of
// {status, contentType, body}.
window.csimSimTransport = (method, path, body, lane = 0) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { lane, resolve, reject });
  simWorker(lane).postMessage({ id, method, path, body });
});

// How many workers can usefully run side by side.
window.csimSimLanes = Math.max(1, navigator.hardwareConcurrency || 1);
