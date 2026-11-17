// Parity helper: same 12 model outputs x 5 decisions as the legacy JS harness; prints JSON.
use agent_08_human_in_loop::hitl::*;
use futures::executor::block_on;
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct Map(RefCell<HashMap<String, String>>);
impl Kv for Map {
    async fn get(&self, k: &str) -> Option<String> { self.0.borrow().get(k).cloned() }
    async fn put(&self, k: &str, v: &str, _t: Option<u64>) { self.0.borrow_mut().insert(k.into(), v.into()); }
}
struct Fixed<'a>(&'a str);
impl<'a> Ai for Fixed<'a> {
    async fn run(&self, _m: &str, _i: Value) -> Result<Value, String> { Ok(json!({"response": self.0})) }
}
fn strip(mut a: Value) -> Value {
    if let Some(au) = a.get("audit").and_then(|x| x.as_array()).cloned() {
        a["audit"] = Value::Array(au.iter().map(|x| { let mut o = json!({"event": x["event"]}); if x.get("note").is_some() { o["note"] = x["note"].clone(); } o }).collect());
    }
    a.as_object_mut().unwrap().remove("id");
    a
}
fn main() {
    let r: Vec<String> = vec![
        r#"{"action":"issue_refund","params":{"order_id":"A1","amount_usd":40,"reason":"late"},"rationale":"x"}"#,
        r#"Sure: {"action":"send_email","params":{"to":"a@b.co","subject":"Hi","body":"é"},"rationale":"r"}"#,
        r#"{"action":"issue_refund","params":{"order_id":"A1","amount_usd":"250","reason":"late"}}"#,
        r#"{"action":"issue_refund","params":{"order_id":"A1","amount_usd":501,"reason":"late"}}"#,
        r#"{"action":"issue_refund","params":{"order_id":"A1","amount_usd":"abc","reason":"late"}}"#,
        r#"{"action":"send_email","params":{"to":"a","subject":"","body":"b"}}"#,
        r#"{"action":"schedule_meeting","params":{"with":"Sam","when":null,"topic":"t"}}"#,
        r#"{"action":"nuke","params":{}}"#, "no json", r#"{"action":"reply_draft","params":{"text":"hi"}}"#, "[1,2]", r#"{"action":"reply_draft"}"#,
    ].into_iter().map(String::from).collect();
    let long = "n".repeat(300);
    let e: Vec<(&str, String, Option<Value>)> = vec![
        ("approve", String::new(), None),
        ("approve", String::new(), Some(json!({"to":"c@d.co","subject":"S","body":"B"}))),
        ("reject", long, None),
        ("yolo", String::new(), None),
        ("approve", String::new(), Some(json!({"text":""}))),
    ];
    let mut out = vec![];
    for reply in &r {
        for (d, note, params) in &e {
            let (kv, ai) = (Map::default(), Fixed(reply));
            let a = block_on(propose(&ai, &kv, "req é", "abcd1234", 0)).unwrap();
            let p = strip(a);
            let x = match block_on(decide(&kv, "abcd1234", d, note, params.as_ref(), 0)) { Ok(v) => v, Err((c, m)) => json!({"error": m, "code": c}) };
            out.push(json!({"p": p, "d": strip(x)}));
        }
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}
