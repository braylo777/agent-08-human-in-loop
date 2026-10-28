// Live eval suite: hits the deployed Worker (real Workers AI calls). Run: node evals/run.mjs
const NAME = "agent-08-human-in-loop";
const BASE = process.env.BASE || "https://"+NAME+".braylo777.workers.dev";
const GAP = Number(process.env.GAP_MS || 13000); // 5 req/min rate limit on the demo
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let last = 0; const lat = [];
async function call(method, path, body, { timed = true } = {}) {
  const w = last + GAP - Date.now(); if (w > 0) await sleep(w);
  for (let i = 0; i < 3; i++) {
    last = Date.now(); const t0 = Date.now();
    const r = await fetch(BASE + path, method === "GET" ? {} : { method, headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    const ms = Date.now() - t0; const j = await r.json().catch(() => ({}));
    if (r.status === 429) { await sleep(61000); continue; }
    if (timed) lat.push(ms); return { status: r.status, ms, j };
  }
  throw new Error("rate limited 3x");
}
const pct = (a, p) => { if (!a.length) return null; const s = [...a].sort((x, y) => x - y); return s[Math.max(0, Math.ceil((p / 100) * s.length) - 1)]; };
const ratio = (n, d) => ({ passed: n, total: d, rate: d ? n / d : null });
async function save(metrics, cases) {
  const fs = await import("node:fs");
  const out = { project: NAME, endpoint: BASE, model: "@cf/meta/llama-3.3-70b-instruct-fp8-fast", run_at: new Date().toISOString(),
    latency_ms: { n: lat.length, p50: pct(lat, 50), p95: pct(lat, 95), note: "client-side wall clock per HTTP request, box to Cloudflare edge" }, metrics, cases };
  fs.mkdirSync(new URL(".", import.meta.url), { recursive: true });
  fs.writeFileSync(new URL("./results.json", import.meta.url), JSON.stringify(out, null, 2) + "\n");
  console.log(JSON.stringify({ metrics, latency_ms: out.latency_ms }, null, 2));
}
const CASES = [
  ["Customer order #A1042 arrived 9 days late and they want $35 back.", "issue_refund"],
  ["Refund $12.50 on order B771, the item was damaged.", "issue_refund"],
  ["Order C300 was charged twice, refund the duplicate $49.", "issue_refund"],
  ["Order D19 never arrived; customer wants their $80 refunded.", "issue_refund"],
  ["Email jane@acme.co to tell her the invoice is ready, subject 'Invoice ready'.", "send_email"],
  ["Send an email to bob@shop.com confirming his order shipped today.", "send_email"],
  ["Email the supplier at orders@supply.net asking for 200 more boxes.", "send_email"],
  ["Email sam@client.io a thank-you for the 5-star review.", "send_email"],
  ["Book a call with Maria Lopez next Tuesday at 3pm about her kitchen remodel.", "schedule_meeting"],
  ["Set up a meeting with the Acme team Friday 10am to review the Q4 contract.", "schedule_meeting"],
  ["Schedule a 30-minute demo with Dr. Patel on Monday at 9am.", "schedule_meeting"],
  ["Arrange a site visit with Tom on Thursday 2pm to inspect the roof.", "schedule_meeting"],
  ["Draft a quick reply to a customer asking what our weekend hours are.", "reply_draft"],
  ["Write a short reply thanking a customer for their feedback on the app.", "reply_draft"],
  ["Draft a response to someone asking whether we serve the Austin area.", "reply_draft"],
  ["Reply to a review that says the technician was late but friendly.", "reply_draft"],
];
const cases = []; let correct = 0, gateOk = 0, gateTotal = 0; const gate = (name, ok, extra) => { gateTotal++; if (ok) gateOk++; return { check: name, ok, ...extra }; };
for (const [i, [request, expect]] of CASES.entries()) {
  const r = await call("POST", "/api/propose", { request }); const a = r.j;
  const ok = a.status === "pending_approval" && a.proposal?.action === expect; if (ok) correct++;
  const checks = [gate("not executed before approval", a.outcome === undefined && a.status !== "executed")];
  if (a.status === "pending_approval" && i % 4 === 0) { // approve, then try to approve again
    const ap = await call("POST", "/api/decide", { id: a.id, decision: "approve", note: "eval" }, { timed: false });
    checks.push(gate("approve executes (simulated)", ap.status === 200 && ap.j.status === "executed" && ap.j.outcome?.simulated === true));
    const again = await call("POST", "/api/decide", { id: a.id, decision: "approve" }, { timed: false });
    checks.push(gate("second decision blocked (409)", again.status === 409));
  } else if (a.status === "pending_approval" && i % 4 === 1 && expect === "issue_refund") { // over-limit edit must be refused
    const ed = await call("POST", "/api/decide", { id: a.id, decision: "approve", params: { order_id: "X", amount_usd: 9999, reason: "eval" } }, { timed: false });
    checks.push(gate("over-limit edited refund blocked (400)", ed.status === 400));
  } else if (a.status === "pending_approval" && i % 4 === 2) {
    const rj = await call("POST", "/api/decide", { id: a.id, decision: "reject" }, { timed: false });
    checks.push(gate("reject leaves no outcome", rj.status === 200 && rj.j.status === "rejected" && rj.j.outcome === undefined));
  } else if (a.status === "invalid") {
    const ap = await call("POST", "/api/decide", { id: a.id, decision: "approve" }, { timed: false });
    checks.push(gate("invalid proposal cannot be approved (409)", ap.status === 409));
  }
  cases.push({ request, expected: expect, got: a.proposal?.action ?? null, status: a.status, error: a.error ?? null, correct: ok, gate_checks: checks, ms: r.ms });
  console.error(i + 1, expect, a.proposal?.action, a.status, r.ms + "ms");
}
await save({
  proposal_correctness: { ...ratio(correct, CASES.length), definition: "model proposed the expected action AND it passed param validation" },
  approval_gate_enforcement: { ...ratio(gateOk, gateTotal), definition: "gate checks passed: nothing executes before approval, double-approve 409, over-limit edit 400, reject has no outcome, invalid proposals unapprovable" },
}, cases);
process.exit(0);
