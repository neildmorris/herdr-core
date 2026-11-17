//! The mesh in lists: what hubs see, and what runs outside hc (`found`).

use super::*;

/// What this machine's `discover` command reports, or nothing (none set, it
/// failed, or it took over 5 s). Entries it can't read are skipped.
pub(super) async fn discover_found(core: &Core) -> Vec<Found> {
    let Some(argv) = crate::config::load(&core.paths.config).ok().and_then(|c| c.discover) else { return Vec::new() };
    let Some((program, args)) = argv.split_first() else { return Vec::new() };
    let mut cmd = tokio::process::Command::new(match (program.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::Path::new(&home).join(rest),
        _ => program.into(),
    });
    cmd.args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let out = match tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => out.stdout,
        _ => return Vec::new(),
    };
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| serde_json::from_value::<Found>(row).ok())
        .map(|f| Found { machine: String::new(), ..f })
        .collect()
}

/// Found on the machine `name`: theirs is `name`'s; what they reached through
/// a hub keeps its machine.
pub(super) fn found_on(name: &str, rows: &serde_json::Value) -> Vec<Found> {
    let rows = rows.as_array().cloned().unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| serde_json::from_value::<Found>(row).ok())
        .map(|f| Found { machine: if f.machine.is_empty() { name.into() } else { f.machine.clone() }, ..f })
        .collect()
}

pub(super) struct FromHubs {
    pub terminals: Vec<serde_json::Value>,
    pub hidden_finished: usize,
    pub mesh: Vec<MeshMachine>,
    pub found: Vec<Found>,
}

/// Each mesh hub's list, as this core names things: the hub's own terminals
/// as `hub/t7`, its machines' as they are, ours (it lists them too) left out.
pub(super) async fn from_hubs(core: &Arc<Core>, p: &ListParams) -> FromHubs {
    let mut out = FromHubs { terminals: Vec::new(), hidden_finished: 0, mesh: Vec::new(), found: Vec::new() };
    let asks: Vec<_> = crate::remote::hubs(core)
        .into_iter()
        .map(|(id, name)| {
            let (core, params) = (core.clone(), serde_json::to_value(p).unwrap_or_default());
            let to = crate::remote::To::Peer { id, name: name.clone() };
            // The hub asks its machines first (3 s each, side by side).
            let ask = async move { crate::remote::forward_to(&core, &to, "terminal.list", params).await };
            (name, tokio::spawn(async move { tokio::time::timeout(Duration::from_secs(6), ask).await }))
        })
        .collect();
    for (hub, ask) in asks {
        let Ok(Ok(Ok(r))) = ask.await else {
            out.mesh.push(MeshMachine { name: hub, state: "unreachable".into(), via: None });
            continue;
        };
        let calls_me = core.lock().peers.values().find(|peer| peer.name == hub).map(|peer| peer.calls_me.clone());
        let theirs = |address: &str| address.contains('/');
        out.terminals.extend(
            r["terminals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["address"].as_str().is_some_and(theirs))
                .cloned(),
        );
        out.hidden_finished += r["hidden_finished"].as_u64().unwrap_or(0) as usize;
        out.found.extend(found_on(&hub, &r["found"]).into_iter().filter(|f| Some(&f.machine) != calls_me.as_ref()));
        out.mesh.push(MeshMachine { name: hub.clone(), state: "connected".into(), via: None });
        for m in r["machines"].as_array().into_iter().flatten().filter(|m| m["id"] != core.info.id.as_str()) {
            let (name, state) = (m["name"].as_str().unwrap_or_default(), m["state"].as_str().unwrap_or_default());
            out.mesh.push(MeshMachine { name: name.into(), state: state.into(), via: Some(hub.clone()) });
        }
    }
    out
}

pub(super) async fn machine_mesh(core: Ctx<'_>, p: MachineMeshParams) -> Result<crate::machines::Machine, RpcError> {
    let mut st = core.lock();
    let i = st
        .machines
        .iter()
        .position(|m| m.name == p.machine)
        .ok_or_else(|| crate::machines::unknown(&st.machines, &p.machine))?;
    let machine = &mut st.machines[i];
    if machine.mesh != !p.off {
        machine.mesh = !p.off;
        let id = machine.id.clone();
        crate::machines::save(&core.paths.state, &st.machines);
        // It hears at its next hello: dial it again now.
        if let Some(crate::remote::LineState::Connected(line)) = st.lines.get(&id) {
            line.retired.notify_one();
        }
    }
    Ok(st.machines[i].clone())
}
