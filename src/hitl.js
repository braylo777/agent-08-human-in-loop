import { ask, parseJSON } from "./core.js";

// The agent may only PROPOSE actions. Nothing runs until a human approves it (or edits, then approves).
export const ACTIONS = {
  send_email: { risk: "high", needs: ["to", "subject", "body"] },
  issue_refund: { risk: "high", needs: ["order_id", "amount_usd", "reason"] },
  schedule_meeting: { risk: "medium", needs: ["with", "when", "topic"] },
  reply_draft: { risk: "low", needs: ["text"] },
};
const SYSTEM = `You are a customer-operations agent. Given a request, propose exactly ONE action.
Allowed actions and their params: ${Object.entries(ACTIONS).map(([k, v]) => `${k}(${v.needs.join(", ")})`).join("; ")}.
Reply ONLY JSON: {"action": string, "params": object, "rationale": string}`;

export function validate(p) {
  const spec = p && ACTIONS[p.action];
  if (!spec) return "unknown action";
  const missing = spec.needs.filter((k) => p.params?.[k] === undefined || p.params[k] === "");
  if (missing.length) return "missing params: " + missing.join(", ");
  if (p.action === "issue_refund" && !(Number(p.params.amount_usd) > 0 && Number(p.params.amount_usd) <= 500)) return "refund must be $0-500";
  return null;
}
// Simulated executor: this demo never sends email or moves money, it records what WOULD happen.
export const execute = (p) => ({ simulated: true, result: `${p.action} executed with ${JSON.stringify(p.params)}` });

const K = (id) => `act:${id}`;
const put = (env, a) => env.KV.put(K(a.id), JSON.stringify(a), { expirationTtl: 86400 * 3 });
export const getAction = (env, id) => env.KV.get(K(id), "json");

export async function propose(env, request) {
  const p = parseJSON(await ask(env, SYSTEM, request, { max_tokens: 300 }), {});
  const error = validate(p);
  const a = { id: crypto.randomUUID().slice(0, 8), request, proposal: p, risk: ACTIONS[p.action]?.risk || "unknown", status: error ? "invalid" : "pending_approval", error, audit: [{ at: new Date().toISOString(), event: "proposed" }] };
  await put(env, a); return a;
}
export async function decide(env, id, decision, { note = "", params } = {}) {
  const a = await getAction(env, id);
  if (!a) return { error: "not found", code: 404 };
  if (a.status !== "pending_approval") return { error: `already ${a.status}`, code: 409 };
  if (decision === "approve") {
    if (params) { const edited = { ...a.proposal, params }; const err = validate(edited); if (err) return { error: err, code: 400 }; a.proposal = edited; a.audit.push({ at: new Date().toISOString(), event: "edited" }); }
    a.outcome = execute(a.proposal); a.status = "executed";
  } else if (decision === "reject") a.status = "rejected";
  else return { error: "decision must be approve or reject", code: 400 };
  a.audit.push({ at: new Date().toISOString(), event: decision, note: String(note).slice(0, 200) });
  await put(env, a); return a;
}
