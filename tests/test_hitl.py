import pytest
from agent08.hitl import propose, decide, validate, get_action, ACTIONS, handle


class KV:
    def __init__(self): self.m = {}
    def get(self, k): return self.m.get(k)
    def put(self, k, v, ttl=None): self.m[k] = v


def env(reply):
    return (lambda m, i: {"response": reply}), KV()


refund = '{"action":"issue_refund","params":{"order_id":"A1","amount_usd":40,"reason":"late"},"rationale":"late delivery"}'
email = '{"action":"send_email","params":{"to":"a@b.co","subject":"Hi","body":"Hello"},"rationale":"r"}'


def test_proposal_waits_nothing_executes():
    ai, kv = env(refund)
    a = propose(ai, kv, "refund order A1")
    assert a["status"] == "pending_approval" and "outcome" not in a


def test_approve_once_second_conflicts():
    ai, kv = env(refund)
    a = propose(ai, kv, "refund A1")
    r = decide(kv, a["id"], "approve", note="ok")
    assert r["status"] == "executed" and r["outcome"]["simulated"]
    assert decide(kv, a["id"], "reject")["code"] == 409


def test_reject_and_edit_bounds():
    ai, kv = env(refund)
    a = propose(ai, kv, "x")
    assert decide(kv, a["id"], "reject", note="no")["status"] == "rejected"
    b = propose(ai, kv, "y")
    assert decide(kv, b["id"], "approve", params={"order_id": "A1", "amount_usd": 9999, "reason": "r"})["code"] == 400


def test_validation():
    assert "missing" in validate({"action": "send_email", "params": {"to": "a"}})
    assert validate({"action": "nuke"}) == "unknown action"
    assert validate({}) == "unknown action"


def test_catalog_risk_levels():
    assert all(a["risk"] in ("low", "medium", "high") for a in ACTIONS.values())


def test_refund_bounds():
    r = lambda amt: validate({"action": "issue_refund", "params": {"order_id": "1", "amount_usd": amt, "reason": "x"}})
    assert r(500) is None and "refund" in r(501) and "refund" in r(0) and "refund" in r(-5)


def test_invalid_output_never_approvable():
    ai, kv = env("I think you should refund them")
    a = propose(ai, kv, "x")
    assert a["status"] == "invalid" and decide(kv, a["id"], "approve")["code"] == 409


def test_unknown_decision_rejected_state_unchanged():
    ai, kv = env(email)
    a = propose(ai, kv, "x")
    assert decide(kv, a["id"], "yolo")["code"] == 400 and get_action(kv, a["id"])["status"] == "pending_approval"


def test_unknown_id_404():
    assert decide(KV(), "zzzzzzzz", "approve")["code"] == 404


def test_audit_trail():
    ai, kv = env(email)
    a = propose(ai, kv, "x")
    r = decide(kv, a["id"], "approve", params={"to": "c@d.co", "subject": "S", "body": "B"})
    assert [x["event"] for x in r["audit"]] == ["proposed", "edited", "approve"] and r["proposal"]["params"]["to"] == "c@d.co"


def test_simulated_only_and_rejected_has_no_outcome():
    ai, kv = env(email)
    assert decide(kv, propose(ai, kv, "x")["id"], "approve")["outcome"]["simulated"] is True
    assert "outcome" not in decide(kv, propose(ai, kv, "x")["id"], "reject")


def test_http_router():
    ai, kv = env(email)
    assert handle(ai, kv, "GET", "/api/catalog")[1] == ACTIONS
    assert handle(ai, kv, "POST", "/api/propose", {})[0] == 400
    assert handle(ai, kv, "POST", "/api/decide", {"id": "bad"})[0] == 400
    s, a = handle(ai, kv, "POST", "/api/propose", {"request": "hi"})
    assert s == 200 and handle(ai, kv, "GET", f"/api/actions/{a['id']}")[0] == 200
    assert handle(ai, kv, "POST", "/api/decide", {"id": a["id"], "decision": "approve"})[1]["status"] == "executed"
    assert handle(ai, kv, "POST", "/api/decide", {"id": a["id"], "decision": "reject"})[0] == 409
