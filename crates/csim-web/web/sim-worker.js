// The Sim view's sim in a Web Worker: a LiveApp of its own (csim-web, the same WebAssembly
// module as the page's, from csim_web.js next to this file), so that the iterations run off the
// page's thread. web.js starts it and sends it the `api/sim/*` requests:
// `{id, method, path, body}` in, `{id, status, contentType, body}` out. A thrown error (a panic,
// which leaves the module unusable) is a 500 marked `fatal`: web.js then starts a new worker.
import init, { LiveApp } from "./csim_web.js";

const ready = (async () => {
  await init();
  // The seed of the sims the page names none for.
  const [high, low] = crypto.getRandomValues(new Uint32Array(2));
  return new LiveApp(high, low);
})();

// The requests in the order they came: a request waits for the ones before it.
let chain = Promise.resolve();

self.onmessage = (event) => {
  const { id, method, path, body } = event.data;
  chain = chain.then(async () => {
    try {
      const app = await ready;
      const reply = app.handle(method, path, body);
      try {
        self.postMessage({ id, status: reply.status, contentType: reply.contentType, body: reply.body });
      } finally {
        reply.free();
      }
    } catch (error) {
      self.postMessage({
        id,
        status: 500,
        contentType: "text/plain; charset=utf-8",
        body: `the sim worker failed: ${error?.message ?? error}`,
        fatal: true,
      });
    }
  });
};
