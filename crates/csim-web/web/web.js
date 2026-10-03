// The live viewer's server side in the page: csim-web (WebAssembly, bound by
// `wasm-bindgen --target web` into csim_web.js next to this file). The page (index.html) waits
// for `csimReady`, then sends its requests to `csimTransport` instead of over HTTP.
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
