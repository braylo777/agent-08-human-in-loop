import { test } from "node:test";
import assert from "node:assert/strict";
import { propose, decide, validate, getAction, ACTIONS } from "../src/hitl.js";

const kv = () => { const m = new Map(); return { get: async (k) => (m.has(k) ? JSON.parse(m.get(k)) : null), put: async (k, v) => m.set(k, v) }; };
const env = (reply) => ({ KV: kv(), AI: { run: async () => ({ response: reply }) } });
const email = '{"action":"send_email","params":{"to":"a@b.co","subject":"Hi","body":"Hello"},"rationale":"r"}';

test("catalog has risk levels", () => { for (const a of Object.values(ACTIONS)) assert.ok(["low", "medium", "high"].includes(a.risk)); });
test("refund bounds", () => {
  const r = (amt) => validate({ action: "issue_refund", params: { order_id: "1", amount_usd: amt, reason: "x" } });
  assert.equal(r(500), null); assert.match(r(501), /refund/); assert.match(r(0), /refund/); assert.match(r(-5), /refund/);
});
test("invalid model output is stored as invalid and can never be approved", async () => {
  const e = env("I think you should refund them"); const a = await propose(e, "x");
  assert.equal(a.status, "invalid"); assert.equal((await decide(e, a.id, "approve")).code, 409);
});
test("unknown decision is rejected without changing state", async () => {
  const e = env(email); const a = await propose(e, "x");
  assert.equal((await decide(e, a.id, "yolo")).code, 400); assert.equal((await getAction(e, a.id)).status, "pending_approval");
});
test("unknown id returns 404", async () => assert.equal((await decide(env(email), "zzzzzzzz", "approve")).code, 404));
test("audit trail records propose, edit and approve", async () => {
  const e = env(email); const a = await propose(e, "x");
  const r = await decide(e, a.id, "approve", { params: { to: "c@d.co", subject: "S", body: "B" } });
  assert.deepEqual(r.audit.map((x) => x.event), ["proposed", "edited", "approve"]); assert.equal(r.proposal.params.to, "c@d.co");
});
test("execution is simulated only", async () => { const e = env(email); const a = await propose(e, "x"); assert.equal((await decide(e, a.id, "approve")).outcome.simulated, true); });
test("rejected action has no outcome", async () => { const e = env(email); const a = await propose(e, "x"); assert.equal((await decide(e, a.id, "reject")).outcome, undefined); });
