"""Live eval suite: hits the deployed Worker (real Workers AI calls). Run: BASE=https://... python3 evals/run.py"""
import json, math, os, re, time, urllib.error, urllib.request
from datetime import datetime, timezone
from pathlib import Path

NAME = "agent-08-human-in-loop"
BASE = os.environ.get("BASE", f"https://{NAME}.braylo777.workers.dev")
GAP = float(os.environ.get("GAP_MS", "13000")) / 1000  # 5 req/min rate limit on the demo
here = Path(__file__).parent
lat, last = [], [0.0]


def call(method, path, body=None, timed=True):
    w = last[0] + GAP - time.time()
    if w > 0:
        time.sleep(w)
    for _ in range(3):
        last[0] = time.time(); t0 = time.time()
        req = urllib.request.Request(BASE + path, None if method == "GET" else json.dumps(body).encode(), {"content-type": "application/json"}, method=method)
        try:
            r = urllib.request.urlopen(req, timeout=90); status = r.status; raw = r.read()
        except urllib.error.HTTPError as e:
            status = e.code; raw = e.read()
        ms = round((time.time() - t0) * 1000)
        try:
            j = json.loads(raw)
        except ValueError:
            j = {}
        if status == 429:
            time.sleep(61); continue
        if timed:
            lat.append(ms)
        return {"status": status, "ms": ms, "j": j}
    raise RuntimeError("rate limited 3x")


pct = lambda a, p: sorted(a)[max(0, math.ceil(p / 100 * len(a)) - 1)] if a else None
ratio = lambda n, d: {"passed": n, "total": d, "rate": n / d if d else None}
CASES = [
    ("Customer order #A1042 arrived 9 days late and they want $35 back.", "issue_refund"),
    ("Refund $12.50 on order B771, the item was damaged.", "issue_refund"),
    ("Order C300 was charged twice, refund the duplicate $49.", "issue_refund"),
    ("Order D19 never arrived; customer wants their $80 refunded.", "issue_refund"),
    ("Email jane@acme.co to tell her the invoice is ready, subject 'Invoice ready'.", "send_email"),
    ("Send an email to bob@shop.com confirming his order shipped today.", "send_email"),
    ("Email the supplier at orders@supply.net asking for 200 more boxes.", "send_email"),
    ("Email sam@client.io a thank-you for the 5-star review.", "send_email"),
    ("Book a call with Maria Lopez next Tuesday at 3pm about her kitchen remodel.", "schedule_meeting"),
    ("Set up a meeting with the Acme team Friday 10am to review the Q4 contract.", "schedule_meeting"),
    ("Schedule a 30-minute demo with Dr. Patel on Monday at 9am.", "schedule_meeting"),
    ("Arrange a site visit with Tom on Thursday 2pm to inspect the roof.", "schedule_meeting"),
    ("Draft a quick reply to a customer asking what our weekend hours are.", "reply_draft"),
    ("Write a short reply thanking a customer for their feedback on the app.", "reply_draft"),
    ("Draft a response to someone asking whether we serve the Austin area.", "reply_draft"),
    ("Reply to a review that says the technician was late but friendly.", "reply_draft"),
]
cases = []
st = {"correct": 0, "gate_ok": 0, "gate_total": 0}


def gate(name, ok, **extra):
    st["gate_total"] += 1
    st["gate_ok"] += bool(ok)
    return {"check": name, "ok": bool(ok), **extra}


for i, (request, expect) in enumerate(CASES):
    r = call("POST", "/api/propose", {"request": request}); a = r["j"]
    ok = a.get("status") == "pending_approval" and (a.get("proposal") or {}).get("action") == expect
    st["correct"] += ok
    checks = [gate("not executed before approval", "outcome" not in a and a.get("status") != "executed")]
    pending = a.get("status") == "pending_approval"
    if pending and i % 4 == 0:  # approve, then try to approve again
        ap = call("POST", "/api/decide", {"id": a["id"], "decision": "approve", "note": "eval"}, False)
        checks.append(gate("approve executes (simulated)", ap["status"] == 200 and ap["j"].get("status") == "executed" and (ap["j"].get("outcome") or {}).get("simulated") is True))
        again = call("POST", "/api/decide", {"id": a["id"], "decision": "approve"}, False)
        checks.append(gate("second decision blocked (409)", again["status"] == 409))
    elif pending and i % 4 == 1 and expect == "issue_refund":  # over-limit edit must be refused
        ed = call("POST", "/api/decide", {"id": a["id"], "decision": "approve", "params": {"order_id": "X", "amount_usd": 9999, "reason": "eval"}}, False)
        checks.append(gate("over-limit edited refund blocked (400)", ed["status"] == 400))
    elif pending and i % 4 == 2:
        rj = call("POST", "/api/decide", {"id": a["id"], "decision": "reject"}, False)
        checks.append(gate("reject leaves no outcome", rj["status"] == 200 and rj["j"].get("status") == "rejected" and "outcome" not in rj["j"]))
    elif a.get("status") == "invalid":
        ap = call("POST", "/api/decide", {"id": a["id"], "decision": "approve"}, False)
        checks.append(gate("invalid proposal cannot be approved (409)", ap["status"] == 409))
    cases.append({"request": request, "expected": expect, "got": (a.get("proposal") or {}).get("action"), "status": a.get("status"), "error": a.get("error"), "correct": ok, "gate_checks": checks, "ms": r["ms"]})
    print(i + 1, expect, (a.get("proposal") or {}).get("action"), a.get("status"), f"{r['ms']}ms", flush=True)
metrics = {
    "proposal_correctness": {**ratio(st["correct"], len(CASES)), "definition": "model proposed the expected action AND it passed param validation"},
    "approval_gate_enforcement": {**ratio(st["gate_ok"], st["gate_total"]), "definition": "gate checks passed: nothing executes before approval, double-approve 409, over-limit edit 400, reject has no outcome, invalid proposals unapprovable"},
}
out = {"project": NAME, "endpoint": BASE, "model": "@cf/meta/llama-3.3-70b-instruct-fp8-fast", "run_at": datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
       "latency_ms": {"n": len(lat), "p50": pct(lat, 50), "p95": pct(lat, 95), "note": "client-side wall clock per HTTP request, box to Cloudflare edge"}, "metrics": metrics, "cases": cases}
(here / "results.json").write_text(json.dumps(out, indent=2) + "\n")
print(json.dumps({"metrics": metrics, "latency_ms": out["latency_ms"]}, indent=2))
