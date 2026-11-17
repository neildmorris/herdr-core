//! The mesh: a machine added here with `mesh` on may use this core as its
//! hub. Its calls reach every terminal here and on the other machines added
//! here; on that machine, addresses this core doesn't know go to the hub.

use super::{To, forward_to, route};
use crate::machines::Machine;
use crate::rpc::RpcError;
use crate::runtime::{Core, State};
use serde_json::Value;
use std::sync::Arc;

/// What a mesh machine may not ask of its hub: changing the hub itself or its
/// machines, and the streams a relayed call can't carry yet.
pub(super) const NOT_OVER_MESH: &[&str] = &[
    "hello",
    "server.stop",
    "server.restart",
    "machine.add",
    "machine.rm",
    "machine.rename",
    "machine.connect",
    "machine.mesh",
    "terminal.attach",
    "events.subscribe",
    "terminal.put",
    "terminal.put_begin",
    "terminal.put_chunk",
    "terminal.put_finish",
    "terminal.put_abort",
];

/// A call from a mesh machine: anything but `NOT_OVER_MESH`, here or on the
/// machines added here, routed as a call made here would be.
pub(super) async fn from_mesh(
    core: &Arc<Core>,
    m: &Machine,
    method: &str,
    mut params: Value,
) -> Result<Value, RpcError> {
    if NOT_OVER_MESH.contains(&method) {
        return Err(RpcError::new("denied", format!("{method} is not offered over the mesh"), ""));
    }
    if let Some(caller) = params["caller"].as_str() {
        params["caller"] = format!("{}/{caller}", m.name).into();
    }
    match route(core, method, &params)? {
        // One hop: a name the hub doesn't know never goes on to a hub of its own.
        Some((To::Relay { .. }, _)) => {
            let prefix = params["terminal"].as_str().and_then(|t| t.split_once('/')).map(|(p, _)| p);
            let name = params["on"].as_str().or(prefix).unwrap_or_default();
            Err(crate::machines::unknown(&core.lock().machines, name))
        }
        Some((to, params)) => forward_to(core, &to, method, params).await,
        None => {
            let call = crate::api::Call { core, conn: None, plain: None };
            crate::api::dispatch(&call, method, params).await
        }
    }
}

/// The first connected mesh hub, if any: (its id, its name).
pub(super) fn hub(st: &State) -> Option<(String, String)> {
    st.peers
        .iter()
        .find(|(id, p)| p.mesh && st.peer_lines.contains_key(*id))
        .map(|(id, p)| (id.clone(), p.name.clone()))
}

/// Every connected mesh hub: (its id, its name).
pub fn hubs(core: &Core) -> Vec<(String, String)> {
    let st = core.lock();
    st.peers
        .iter()
        .filter(|(id, p)| p.mesh && st.peer_lines.contains_key(*id))
        .map(|(id, p)| (id.clone(), p.name.clone()))
        .collect()
}

pub(super) fn is_mesh_peer(st: &State, id: &str) -> bool {
    st.peers.get(id).is_some_and(|p| p.mesh)
}
