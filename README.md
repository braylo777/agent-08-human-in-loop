# agent-08-human-in-loop

A **human-in-the-loop** AI agent: it proposes actions but cannot execute anything until a person approves, edits or rejects the proposal. It runs serverless on Cloudflare Workers with the free Workers AI tier and KV. Project 8 of a 10-part agentic AI portfolio.

**Live demo:** https://agent-08-human-in-loop.braylo777.workers.dev

## What it is
Describe a customer-ops request. The agent picks one action from a fixed catalog (`send_email`, `issue_refund`, `schedule_meeting`, `reply_draft`), fills in the parameters, and parks it as `pending_approval` with a risk level. A human then approves it (optionally with edited params), or rejects it with a note. Every event is written to an audit trail. **Execution is simulated**: the demo records what would happen and never sends or pays anything.

## How it works
```
request ─▶ LLM proposes {action, params, rationale}
              │ validate (catalog, required params, refund ≤ $500)
              ▼
         pending_approval (KV) ──human──▶ approve / edit+approve / reject
                                              │
                                    simulated execute + audit log
```
- `src/hitl.js`: action catalog with risk levels, the validator, propose/decide and the audit trail.
- Safety by design: the model can't call the executor. Only `/api/decide` can, once per action (a second decision returns 409), and edited params are re-validated.
- Guardrails: 5 requests/min per IP, 500-char cap, 3-day expiry.

## Run it
```bash
npm install
npm test            # offline unit tests (Node 20+), LLM is mocked
npx wrangler dev    # local; Workers AI runs via your Cloudflare login
npx wrangler deploy # free tier, no paid API keys
```
`wrangler.toml` needs your KV namespace id first: `npx wrangler kv namespace create KV`.

## Stack
Cloudflare Workers · Workers AI (`@cf/meta/llama-3.3-70b-instruct-fp8-fast`, Workers KV) · vanilla JS · node:test

## License
MIT
