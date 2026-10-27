import { test } from "node:test";
import assert from "node:assert/strict";
import { propose, decide, validate } from "../src/hitl.js";

const kv = () => { const m = new Map(); return { get: async (k) => (m.has(k) ? JSON.parse(m.get(k)) : null), put: async (k, v) => m.set(k, v) }; };
const env = (reply) => ({ KV: kv(), AI: { run: async () => ({ response: reply }) } });
const refund = '{"action":"issue_refund","params":{"order_id":"A1","amount_usd":40,"reason":"late"},"rationale":"late delivery"}';

test("proposal waits for approval; nothing executes", async () => {
  const a = await propose(env(refund), "refund order A1"); assert.equal(a.status, "pending_approval"); assert.equal(a.outcome, undefined);
});
test("approve executes once; second decision conflicts", async () => {
  const e = env(refund); const a = await propose(e, "refund A1");
  const r = await decide(e, a.id, "approve", { note: "ok" }); assert.equal(r.status, "executed"); assert.ok(r.outcome.simulated);
  assert.equal((await decide(e, a.id, "reject")).code, 409);
});
test("reject and edit-then-approve", async () => {
  const e = env(refund); const a = await propose(e, "x"); assert.equal((await decide(e, a.id, "reject", { note: "no" })).status, "rejected");
  const b = await propose(e, "y"); assert.equal((await decide(e, b.id, "approve", { params: { order_id: "A1", amount_usd: 9999, reason: "r" } })).code, 400);
});
test("validation", () => { assert.match(validate({ action: "send_email", params: { to: "a" } }), /missing/); assert.equal(validate({ action: "nuke" }), "unknown action"); });
