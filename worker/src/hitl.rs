//! The agent may only PROPOSE actions. Nothing runs until a human approves it (or edits, then approves).
use regex::Regex;
use serde_json::{json, Value};

pub const MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";

#[allow(async_fn_in_trait)]
pub trait Ai {
    async fn run(&self, model: &str, input: Value) -> Result<Value, String>;
}
#[allow(async_fn_in_trait)]
pub trait Kv {
    async fn get(&self, key: &str) -> Option<String>;
    async fn put(&self, key: &str, val: &str, ttl: Option<u64>);
}

pub struct Action {
    pub name: &'static str,
    pub risk: &'static str,
    pub needs: &'static [&'static str],
}
pub const ACTIONS: [Action; 4] = [
    Action { name: "send_email", risk: "high", needs: &["to", "subject", "body"] },
    Action { name: "issue_refund", risk: "high", needs: &["order_id", "amount_usd", "reason"] },
    Action { name: "schedule_meeting", risk: "medium", needs: &["with", "when", "topic"] },
    Action { name: "reply_draft", risk: "low", needs: &["text"] },
];
fn spec(name: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.name == name)
}
pub fn catalog() -> Value {
    let mut o = serde_json::Map::new();
    for a in ACTIONS.iter() {
        o.insert(a.name.into(), json!({"risk": a.risk, "needs": a.needs}));
    }
    Value::Object(o)
}
pub fn system() -> String {
    let list: Vec<String> = ACTIONS.iter().map(|a| format!("{}({})", a.name, a.needs.join(", "))).collect();
    format!("You are a customer-operations agent. Given a request, propose exactly ONE action.\nAllowed actions and their params: {}.\nReply ONLY JSON: {{\"action\": string, \"params\": object, \"rationale\": string}}", list.join("; "))
}

pub fn text(r: &Value) -> String {
    match r.get("response") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => r.to_string(),
        Some(v) => v.to_string(),
    }
}
pub fn parse_json(s: &str) -> Option<Value> {
    Regex::new(r"[\[{][\s\S]*[\]}]").unwrap().find(s).and_then(|m| serde_json::from_str(m.as_str()).ok())
}
pub fn cap(s: Option<&str>, n: usize) -> bool {
    matches!(s, Some(s) if !s.trim().is_empty() && s.chars().count() <= n)
}
pub fn iso_ms(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", if m <= 2 { y + 1 } else { y }, m, d, rem / 3_600_000, rem % 3_600_000 / 60_000, rem % 60_000 / 1000, rem % 1000)
}

/// JS Number(v) for the values a model can produce.
fn num(v: &Value) -> f64 {
    match v {
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Null => 0.0,
        Value::String(s) => if s.trim().is_empty() { 0.0 } else { s.trim().parse().unwrap_or(f64::NAN) },
        _ => f64::NAN,
    }
}

pub fn validate(p: &Value) -> Option<String> {
    let Some(sp) = p.get("action").and_then(|a| a.as_str()).and_then(spec) else { return Some("unknown action".into()) };
    let params = p.get("params");
    let missing: Vec<&str> = sp.needs.iter().copied().filter(|k| match params.and_then(|x| x.as_object()) {
        Some(o) => o.get(*k).map_or(true, |v| v.as_str() == Some("")),
        None => true,
    }).collect();
    if !missing.is_empty() {
        return Some(format!("missing params: {}", missing.join(", ")));
    }
    if sp.name == "issue_refund" {
        let a = num(&params.unwrap()["amount_usd"]);
        if !(a > 0.0 && a <= 500.0) {
            return Some("refund must be $0-500".into());
        }
    }
    None
}

/// Simulated executor: this demo never sends email or moves money, it records what WOULD happen.
pub fn execute_action(p: &Value) -> Value {
    json!({"simulated": true, "result": format!("{} executed with {}", p["action"].as_str().unwrap_or(""), p.get("params").map_or("undefined".to_string(), |x| x.to_string()))})
}

fn k(id: &str) -> String {
    format!("act:{id}")
}
async fn put<K: Kv>(kv: &K, a: &Value) {
    kv.put(&k(a["id"].as_str().unwrap_or("")), &a.to_string(), Some(86400 * 3)).await;
}
pub async fn get_action<K: Kv>(kv: &K, id: &str) -> Option<Value> {
    kv.get(&k(id)).await.and_then(|r| serde_json::from_str(&r).ok())
}

pub async fn propose<A: Ai, K: Kv>(ai: &A, kv: &K, request: &str, new_id: &str, now: i64) -> Result<Value, String> {
    let r = ai.run(MODEL, json!({"messages":[{"role":"system","content":system()},{"role":"user","content":request}],"max_tokens":300})).await?;
    let p = parse_json(&text(&r)).unwrap_or(json!({}));
    let error = validate(&p);
    let risk = p.get("action").and_then(|a| a.as_str()).and_then(spec).map_or("unknown", |s| s.risk);
    let a = json!({"id": new_id, "request": request, "proposal": p, "risk": risk, "status": if error.is_some() { "invalid" } else { "pending_approval" }, "error": error, "audit": [{"at": iso_ms(now), "event": "proposed"}]});
    put(kv, &a).await;
    Ok(a)
}

pub async fn decide<K: Kv>(kv: &K, id: &str, decision: &str, note: &str, params: Option<&Value>, now: i64) -> Result<Value, (u16, String)> {
    let Some(mut a) = get_action(kv, id).await else { return Err((404, "not found".into())) };
    let status = a["status"].as_str().unwrap_or("").to_string();
    if status != "pending_approval" {
        return Err((409, format!("already {status}")));
    }
    if decision == "approve" {
        if let Some(pr) = params.filter(|p| !p.is_null() && **p != json!(false) && **p != json!(0) && **p != json!("")) {
            let mut edited = a["proposal"].clone();
            edited["params"] = pr.clone();
            if let Some(e) = validate(&edited) {
                return Err((400, e));
            }
            a["proposal"] = edited;
            a["audit"].as_array_mut().unwrap().push(json!({"at": iso_ms(now), "event": "edited"}));
        }
        a["outcome"] = execute_action(&a["proposal"]);
        a["status"] = json!("executed");
    } else if decision == "reject" {
        a["status"] = json!("rejected");
    } else {
        return Err((400, "decision must be approve or reject".into()));
    }
    a["audit"].as_array_mut().unwrap().push(json!({"at": iso_ms(now), "event": decision, "note": note.chars().take(200).collect::<String>()}));
    put(kv, &a).await;
    Ok(a)
}

/// Pure router: (status, json); status 0 = serve the page.
pub async fn handle<A: Ai, K: Kv>(ai: &A, kv: &K, method: &str, path: &str, body: &Value, new_id: &str, now: i64) -> (u16, Value) {
    let m = Regex::new(r"^/api/actions/([a-z0-9]{8})$").unwrap().captures(path);
    if method == "GET" && path == "/api/catalog" {
        return (200, catalog());
    }
    if method == "GET" {
        if let Some(c) = m {
            return match get_action(kv, &c[1]).await {
                Some(a) => (200, a),
                None => (404, json!({"error": "not found"})),
            };
        }
    }
    if method == "POST" && (path == "/api/propose" || path == "/api/decide") {
        if path == "/api/decide" {
            let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !Regex::new(r"^[a-z0-9]{8}$").unwrap().is_match(id) {
                return (400, json!({"error": "id required"}));
            }
            let note = match body.get("note") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(o) => o.to_string(),
            };
            let decision = body.get("decision").and_then(|v| v.as_str()).unwrap_or("");
            return match decide(kv, id, decision, &note, body.get("params"), now).await {
                Ok(a) => (200, a),
                Err((c, e)) => (c, json!({"error": e})),
            };
        }
        let r = body.get("request").and_then(|v| v.as_str());
        if !cap(r, 500) {
            return (400, json!({"error": "request required (<=500 chars)"}));
        }
        return match propose(ai, kv, r.unwrap(), new_id, now).await {
            Ok(a) => (200, a),
            Err(e) => (502, json!({"error": format!("Agent error: {e}")})),
        };
    }
    (0, Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::{cell::RefCell, collections::HashMap};

    #[derive(Default)]
    struct Map(RefCell<HashMap<String, String>>);
    impl Kv for Map {
        async fn get(&self, k: &str) -> Option<String> { self.0.borrow().get(k).cloned() }
        async fn put(&self, k: &str, v: &str, _t: Option<u64>) { self.0.borrow_mut().insert(k.into(), v.into()); }
    }
    struct Fixed(&'static str);
    impl Ai for Fixed {
        async fn run(&self, _m: &str, _i: Value) -> Result<Value, String> { Ok(json!({"response": self.0})) }
    }
    const REFUND: &str = r#"{"action":"issue_refund","params":{"order_id":"A1","amount_usd":40,"reason":"late"},"rationale":"late delivery"}"#;
    const EMAIL: &str = r#"{"action":"send_email","params":{"to":"a@b.co","subject":"Hi","body":"Hello"},"rationale":"r"}"#;
    fn prop(ai: &Fixed, kv: &Map, id: &str) -> Value { block_on(propose(ai, kv, "x", id, 0)).unwrap() }
    fn dec(kv: &Map, id: &str, d: &str, p: Option<&Value>) -> Result<Value, (u16, String)> { block_on(decide(kv, id, d, "ok", p, 0)) }

    #[test]
    fn proposal_waits_approve_once_second_conflicts() {
        let (ai, kv) = (Fixed(REFUND), Map::default());
        let a = prop(&ai, &kv, "abcd1234");
        assert_eq!(a["status"], "pending_approval");
        assert!(a.get("outcome").is_none());
        let r = dec(&kv, "abcd1234", "approve", None).unwrap();
        assert_eq!(r["status"], "executed");
        assert_eq!(r["outcome"]["simulated"], true);
        assert_eq!(dec(&kv, "abcd1234", "reject", None).unwrap_err().0, 409);
    }
    #[test]
    fn reject_edit_bounds_and_unknown_decision() {
        let (ai, kv) = (Fixed(REFUND), Map::default());
        prop(&ai, &kv, "aaaa0001");
        assert_eq!(dec(&kv, "aaaa0001", "reject", None).unwrap()["status"], "rejected");
        prop(&ai, &kv, "aaaa0002");
        let big = json!({"order_id":"A1","amount_usd":9999,"reason":"r"});
        assert_eq!(dec(&kv, "aaaa0002", "approve", Some(&big)).unwrap_err().0, 400);
        assert_eq!(dec(&kv, "aaaa0002", "yolo", None).unwrap_err().0, 400);
        assert_eq!(get_action_sync(&kv, "aaaa0002")["status"], "pending_approval");
        assert_eq!(dec(&kv, "zzzzzzzz", "approve", None).unwrap_err().0, 404);
    }
    fn get_action_sync(kv: &Map, id: &str) -> Value { block_on(get_action(kv, id)).unwrap() }
    #[test]
    fn validation_and_refund_bounds() {
        assert!(validate(&json!({"action":"send_email","params":{"to":"a"}})).unwrap().contains("missing"));
        assert_eq!(validate(&json!({"action":"nuke"})).unwrap(), "unknown action");
        assert_eq!(validate(&json!({})).unwrap(), "unknown action");
        let r = |a: Value| validate(&json!({"action":"issue_refund","params":{"order_id":"1","amount_usd":a,"reason":"x"}}));
        assert!(r(json!(500)).is_none());
        assert!(r(json!(501)).unwrap().contains("refund") && r(json!(0)).unwrap().contains("refund") && r(json!(-5)).unwrap().contains("refund"));
        assert!(ACTIONS.iter().all(|a| ["low", "medium", "high"].contains(&a.risk)));
    }
    #[test]
    fn invalid_output_never_approvable_and_audit_trail() {
        let (ai, kv) = (Fixed("I think you should refund them"), Map::default());
        let a = prop(&ai, &kv, "bbbb0001");
        assert_eq!(a["status"], "invalid");
        assert_eq!(dec(&kv, "bbbb0001", "approve", None).unwrap_err().0, 409);
        let (ai, kv) = (Fixed(EMAIL), Map::default());
        prop(&ai, &kv, "cccc0001");
        let p = json!({"to":"c@d.co","subject":"S","body":"B"});
        let r = dec(&kv, "cccc0001", "approve", Some(&p)).unwrap();
        let ev: Vec<&str> = r["audit"].as_array().unwrap().iter().map(|x| x["event"].as_str().unwrap()).collect();
        assert_eq!(ev, ["proposed", "edited", "approve"]);
        assert_eq!(r["proposal"]["params"]["to"], "c@d.co");
        prop(&ai, &kv, "cccc0002");
        assert!(dec(&kv, "cccc0002", "reject", None).unwrap().get("outcome").is_none());
    }
    #[test]
    fn http_router() {
        let (ai, kv) = (Fixed(EMAIL), Map::default());
        let go = |m: &str, p: &str, b: Value| block_on(handle(&ai, &kv, m, p, &b, "dddd0001", 0));
        assert_eq!(go("GET", "/api/catalog", json!({})).1, catalog());
        assert_eq!(go("POST", "/api/propose", json!({})).0, 400);
        assert_eq!(go("POST", "/api/decide", json!({"id":"bad"})).0, 400);
        let (s, _) = go("POST", "/api/propose", json!({"request":"hi"}));
        assert_eq!(s, 200);
        assert_eq!(go("GET", "/api/actions/dddd0001", json!({})).0, 200);
        assert_eq!(go("POST", "/api/decide", json!({"id":"dddd0001","decision":"approve"})).1["status"], "executed");
        assert_eq!(go("POST", "/api/decide", json!({"id":"dddd0001","decision":"reject"})).0, 409);
    }
}
