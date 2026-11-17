"""The agent may only PROPOSE actions. Nothing runs until a human approves it (or edits, then approves)."""
import json, math, re, time, uuid
from datetime import datetime, timezone

MODEL = "@cf/meta/llama-3.3-70b-instruct-fp8-fast"
ACTIONS = {
    "send_email": {"risk": "high", "needs": ["to", "subject", "body"]},
    "issue_refund": {"risk": "high", "needs": ["order_id", "amount_usd", "reason"]},
    "schedule_meeting": {"risk": "medium", "needs": ["with", "when", "topic"]},
    "reply_draft": {"risk": "low", "needs": ["text"]},
}
SYSTEM = ("You are a customer-operations agent. Given a request, propose exactly ONE action.\n"
          "Allowed actions and their params: " + "; ".join(f"{k}({', '.join(v['needs'])})" for k, v in ACTIONS.items()) + ".\n"
          'Reply ONLY JSON: {"action": string, "params": object, "rationale": string}')
dumps = lambda o: json.dumps(o, separators=(",", ":"), ensure_ascii=False)
iso = lambda ms=None: datetime.fromtimestamp((time.time() * 1000 if ms is None else ms) / 1000, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def text(r):
    v = r.get("response") if isinstance(r, dict) else None
    return v if isinstance(v, str) else dumps(v if v is not None else r)


def parse_json(s, fallback=None):
    m = re.search(r"[\[{][\s\S]*[\]}]", str(s))
    try:
        return json.loads(m.group(0)) if m else fallback
    except ValueError:
        return fallback


def cap(s, n=500):
    return isinstance(s, str) and bool(s.strip()) and len(s) <= n


def _num(v):
    """JS Number(v) for the values a model can produce."""
    if isinstance(v, bool):
        return 1.0 if v else 0.0
    if isinstance(v, (int, float)):
        return float(v)
    if v is None:
        return 0.0
    if isinstance(v, str):
        try:
            return float(v) if v.strip() else 0.0
        except ValueError:
            return math.nan
    return math.nan


def validate(p):
    spec = ACTIONS.get(p.get("action")) if isinstance(p, dict) and isinstance(p.get("action"), str) else None
    if not spec:
        return "unknown action"
    params = p.get("params")
    missing = [k for k in spec["needs"] if (not isinstance(params, dict) or k not in params) or params[k] == ""]
    if missing:
        return "missing params: " + ", ".join(missing)
    if p["action"] == "issue_refund" and not (0 < _num(params["amount_usd"]) <= 500):
        return "refund must be $0-500"
    return None


def execute_action(p):
    """Simulated executor: this demo never sends email or moves money, it records what WOULD happen."""
    return {"simulated": True, "result": f"{p['action']} executed with {dumps(p.get('params'))}"}


K = lambda i: f"act:{i}"


def _put(kv, a):
    kv.put(K(a["id"]), dumps(a), 86400 * 3)


def get_action(kv, i):
    raw = kv.get(K(i))
    return json.loads(raw) if raw else None


def propose(ai, kv, request, new_id=None, now=None):
    r = ai(MODEL, {"messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": request}], "max_tokens": 300})
    p = parse_json(text(r), {})
    error = validate(p)
    spec = ACTIONS.get(p.get("action")) if isinstance(p, dict) and isinstance(p.get("action"), str) else None
    a = {"id": new_id or str(uuid.uuid4())[:8], "request": request, "proposal": p, "risk": (spec or {}).get("risk") or "unknown",
         "status": "invalid" if error else "pending_approval", "error": error, "audit": [{"at": iso(now), "event": "proposed"}]}
    _put(kv, a)
    return a


def decide(kv, i, decision, note="", params=None, now=None):
    a = get_action(kv, i)
    if not a:
        return {"error": "not found", "code": 404}
    if a["status"] != "pending_approval":
        return {"error": f"already {a['status']}", "code": 409}
    if decision == "approve":
        if params:
            edited = {**a["proposal"], "params": params}
            err = validate(edited)
            if err:
                return {"error": err, "code": 400}
            a["proposal"] = edited
            a["audit"].append({"at": iso(now), "event": "edited"})
        a["outcome"] = execute_action(a["proposal"])
        a["status"] = "executed"
    elif decision == "reject":
        a["status"] = "rejected"
    else:
        return {"error": "decision must be approve or reject", "code": 400}
    a["audit"].append({"at": iso(now), "event": decision, "note": str(note)[:200]})
    _put(kv, a)
    return a


def handle(ai, kv, method, path, body=None, **kw):
    """Pure router: (status, json); status 0 = serve the page."""
    m = re.fullmatch(r"/api/actions/([a-z0-9]{8})", path)
    if method == "GET" and path == "/api/catalog":
        return 200, ACTIONS
    if method == "GET" and m:
        a = get_action(kv, m.group(1))
        return (200, a) if a else (404, {"error": "not found"})
    if method == "POST" and path in ("/api/propose", "/api/decide"):
        b = body if isinstance(body, dict) else {}
        if path == "/api/decide":
            bid = b.get("id") or ""
            if not (isinstance(bid, str) and re.fullmatch(r"[a-z0-9]{8}", bid)):
                return 400, {"error": "id required"}
            r = decide(kv, b["id"], b.get("decision"), b.get("note", ""), b.get("params"), **kw)
            return (r["code"], {"error": r["error"]}) if r.get("error") else (200, r)
        if not cap(b.get("request")):
            return 400, {"error": "request required (<=500 chars)"}
        try:
            return 200, propose(ai, kv, b["request"], **kw)
        except Exception as e:
            return 502, {"error": "Agent error: " + str(e)}
    return 0, None
