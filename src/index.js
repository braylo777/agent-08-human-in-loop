import { json, limited, body, cap } from "./core.js";
import { propose, decide, getAction, ACTIONS } from "./hitl.js";
import page from "./page.html";

export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    const m = url.pathname.match(/^\/api\/actions\/([a-z0-9]{8})$/);
    if (req.method === "GET" && url.pathname === "/api/catalog") return json(ACTIONS);
    if (req.method === "GET" && m) { const a = await getAction(env, m[1]); return a ? json(a) : json({ error: "not found" }, 404); }
    if (req.method === "POST" && ["/api/propose", "/api/decide"].includes(url.pathname)) {
      if (await limited(req, env)) return json({ error: "Rate limit: try again in a minute." }, 429);
      const b = await body(req);
      if (url.pathname === "/api/decide") {
        if (!/^[a-z0-9]{8}$/.test(b.id || "")) return json({ error: "id required" }, 400);
        const r = await decide(env, b.id, b.decision, b); return r.error ? json({ error: r.error }, r.code) : json(r);
      }
      if (!cap(b.request)) return json({ error: "request required (<=500 chars)" }, 400);
      try { return json(await propose(env, b.request)); } catch (e) { return json({ error: "Agent error: " + e.message }, 502); }
    }
    return new Response(page, { headers: { "content-type": "text/html;charset=utf-8" } });
  },
};
