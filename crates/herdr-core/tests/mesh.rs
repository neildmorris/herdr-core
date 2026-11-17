#![cfg(unix)] // Windows: the same code paths, exercised through tests/machines.rs.

mod common;

use common::*;
use serde_json::Value;

/// A hub with `spark` (in the mesh) and `ops` (not) added.
fn hub_with_two() -> (Session, Session, Session) {
    let spark = Session::new();
    let ops = Session::new();
    let mut hub = Session::new();
    hub.env.push(fake_ssh(&hub, &[("spark", &spark), ("ops", &ops)]));
    hub.env.push(("HC_RECONNECT_MS".into(), "200".into()));
    assert_eq!(hub.json(&["machine", "add", "spark", "--mesh"])["mesh"], true);
    hub.json(&["machine", "add", "ops"]);
    (hub, spark, ops)
}

fn addresses(list: &Value) -> Vec<String> {
    list["terminals"].as_array().into_iter().flatten().filter_map(|t| t["address"].as_str().map(String::from)).collect()
}

/// What a core that dialed in is called here: its name, or its id when the
/// name is taken (every test core has this host's name).
fn hub_name(hub: &Session) -> String {
    hub.json(&["server", "status"])["core"]["id"].as_str().unwrap().to_string()
}

#[test]
fn a_mesh_machine_sees_and_drives_every_machine_through_its_hub() {
    let (hub, spark, ops) = hub_with_two();
    let name = hub_name(&hub);
    let here = run(&hub, &["--name", "h", "sh"]);
    let there = run(&ops, &["--name", "o", "sh"]);
    let mine = run(&spark, &["sh"]);
    wait_until("spark hears its hub", || addresses(&spark.json(&["ls"])).contains(&format!("ops/{there}")));

    let ls = spark.json(&["ls"]);
    let seen = addresses(&ls);
    assert!(seen.contains(&format!("{name}/{here}")), "the hub's own terminal: {seen:?}");
    assert!(seen.contains(&mine), "its own, once and bare: {seen:?}");
    assert_eq!(seen.iter().filter(|a| **a == mine).count(), 1, "once: {seen:?}");
    assert!(!seen.iter().any(|a| a.starts_with("spark/")), "not again through the hub: {seen:?}");
    let mesh: Vec<(&str, Option<&str>)> =
        ls["mesh"].as_array().unwrap().iter().map(|m| (m["name"].as_str().unwrap(), m["via"].as_str())).collect();
    assert_eq!(mesh, [(name.as_str(), None), ("ops", Some(name.as_str()))], "the hub and what it reaches");

    // Driving a machine spark never added, and one on the hub.
    spark.json(&["send", &format!("ops/{there}"), "echo mesh-$((6*7))"]);
    wait_until("ops ran what spark sent", || text(&ops, &there).contains("mesh-42"));
    spark.json(&["send", &format!("{name}/{here}"), "echo hub-$((6*7))"]);
    wait_until("the hub ran it", || text(&hub, &here).contains("hub-42"));
    let started = spark.json(&["run", "--bg", "--on", "ops", "--name", "fromspark", "sh"]);
    let address = started["address"].as_str().unwrap();
    assert!(address.starts_with("ops/"), "started on ops, named from spark: {started}");
    assert!(addresses(&ops.json(&["ls"])).iter().any(|a| address == format!("ops/{a}")), "it runs there");
    let read = spark.json(&["read", &format!("ops/{there}"), "--lines", "50"]);
    assert!(read["text"].as_str().unwrap().contains("mesh-42"), "{read}");

    let (_, unknown) = spark.err(&["--json", "read", "nowhere/t1"]);
    assert_eq!(unknown["error"]["kind"], "machine_unknown", "the hub doesn't know it either: {unknown}");

    // Not in the mesh: ops still reaches nothing it didn't start.
    assert!(ops.json(&["ls"]).get("mesh").is_none());
    let (_, denied) = ops.err(&["--json", "read", &format!("{name}/{here}")]);
    assert_eq!(denied["error"]["kind"], "denied", "{denied}");

    // Out again: spark hears it at the next hello.
    assert_eq!(hub.json(&["machine", "mesh", "spark", "--off"])["mesh"], false);
    wait_until("spark is out", || spark.json(&["ls"]).get("mesh").is_none());
    let (_, denied) = spark.err(&["--json", "read", &format!("ops/{there}")]);
    assert_eq!(denied["error"]["kind"], "machine_unknown", "{denied}");
}

#[test]
fn found_lists_what_each_machine_discovers_outside_hc() {
    let (hub, spark, ops) = hub_with_two();
    let script = executable(
        &ops,
        "#!/bin/sh\necho '[{\"kind\":\"hermes\",\"name\":\"crystal\",\"status\":\"active\",\"detail\":\"gateway\"},{\"bad\":1}]'\n",
    );
    let config = ops.root.join("config/herdr-core/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, format!("discover = [{script:?}]\n")).unwrap();
    wait_until("spark hears its hub", || spark.json(&["ls"]).get("mesh").is_some());

    let found = |s: &Session| spark_found(&s.json(&["ls", "--found"]));
    assert_eq!(found(&hub), [("ops".to_string(), "hermes".to_string(), "crystal".to_string())]);
    assert_eq!(found(&spark), found(&hub), "the same, through the hub");
    assert!(spark.json(&["ls"]).get("found").is_none(), "only when asked");
}

fn spark_found(list: &Value) -> Vec<(String, String, String)> {
    let s = |f: &Value, k: &str| f[k].as_str().unwrap_or_default().to_string();
    list["found"].as_array().into_iter().flatten().map(|f| (s(f, "machine"), s(f, "kind"), s(f, "name"))).collect()
}

#[test]
fn two_hubs_for_each_other_list_every_core_once_and_promptly() {
    let ops = Session::new();
    let mut spark = Session::new();
    let mut hub = Session::new();
    // spark's core starts with its own ssh first, so it can dial out too.
    spark.env.push(fake_ssh(&spark, &[("mainhub", &hub), ("ops", &ops)]));
    spark.env.push(("HC_RECONNECT_MS".into(), "200".into()));
    spark.json(&["server", "status"]);
    hub.env.push(fake_ssh(&hub, &[("spark", &spark), ("ops", &ops)]));
    hub.env.push(("HC_RECONNECT_MS".into(), "200".into()));
    hub.json(&["machine", "add", "spark", "--mesh"]);
    hub.json(&["machine", "add", "ops", "--mesh"]);
    // The always-on box is a hub too: it adds the first hub and ops, both meshed.
    spark.json(&["machine", "add", "mainhub", "--name", "mac", "--mesh"]);
    spark.json(&["machine", "add", "ops", "--mesh"]);
    let on_hub = run(&hub, &["--name", "h", "sh"]);
    let on_ops = run(&ops, &["--name", "o", "sh"]);
    let on_spark = run(&spark, &["--name", "s", "sh"]);
    // ops has two hubs: it lists through both, each core once.
    wait_until("ops hears both hubs", || addresses(&ops.json(&["ls"])).len() == 3);

    for (who, s) in [("hub", &hub), ("spark", &spark), ("ops", &ops)] {
        let started = std::time::Instant::now();
        let seen = addresses(&s.json(&["ls"]));
        assert!(started.elapsed() < std::time::Duration::from_secs(8), "{who} listed in {:?}", started.elapsed());
        // Each core's terminal exactly once, whatever it is called here.
        for t in [&on_hub, &on_ops, &on_spark] {
            let copies = seen.iter().filter(|a| a.rsplit('/').next() == Some(t.as_str())).count();
            assert!(copies >= 1, "{who} misses {t}: {seen:?}");
        }
        assert_eq!(seen.len(), 3, "{who} lists each core once: {seen:?}");
    }
}
