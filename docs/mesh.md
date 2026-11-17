# Build your own mesh

This fork of herdr-core adds the mesh: every machine you add can list and
drive the terminals on every other one, through a hub that already reaches
them. Any mix of macOS and Linux, arm64 and x86_64, works, as long as the
hub reaches each machine over ssh (or a `--via` command).

This page sets one up from nothing. [Machines](machines.md) covers adding a
single machine in detail, and [the mesh section](machines.md#the-mesh-every-machine-from-any-machine)
what a mesh machine may ask of its hub.

[Back to the README](../README.md) · [Command reference](commands.md#hc-machine)

## 1. Install this fork on the hub

Pick the hub: the machine that stays on and can ssh to all the others
(a home server, a cloud box, a desktop that never sleeps). Install there:

```sh
curl -fsSL https://raw.githubusercontent.com/neildmorris/herdr-core/channel/core/install.sh | sh
hc --version          # hc 0.1.21-mesh.1 or later: the -mesh part says it is this fork
```

The installer puts `herdr-core` in `~/.local/bin`, with `hc` beside it. Builds
from this fork say `-mesh.N` after the upstream version they are based on, and
`hc update` updates from this fork's channel, never from upstream's.
An upstream hc already there is replaced the planned way: its terminals keep
running.

## 2. Add every other machine to the mesh

On the hub, once per machine, with the target you would give ssh:

```sh
hc machine add gpu-box --mesh --install
hc machine add user@10.0.0.7 --name ops --mesh --install
hc machine ls
```

`--install` copies hc there when it has none, or one too old. When the OS and
CPU match the hub's, it copies the hub's own hc. Otherwise it downloads the
same version's build for that machine from this fork's channel, so every
machine runs the same fork build. That needs the hub to run a released build
(as the installer gives you), not one you built yourself with an unreleased
version.

Each machine hears that it is in the mesh at the next hello, which comes at
once. From any of them:

```sh
hc ls                          # its own terminals, the hub's (hub/t7), the other machines' (ops/t3)
hc run --bg --on ops --cwd /srv/app --name tests cargo test
hc read ops/tests --lines 100
hc send hub/t7 "the tests on ops are done"
```

The hub's own terminals are listed under its core name; `hc server status` on
the hub shows it. A machine you add without `--mesh` still runs work for the
hub, but sees only the hub terminals that started its work.

## Topologies

### One hub

The simplest: one always-on machine adds every other one with `--mesh`. Only
the hub needs ssh access to the others; they need none of their own, and the
hub need not be reachable from them.

### Several always-on hubs

Two hubs can add each other, both with `--mesh`, and each add the machines it
reaches. A machine that two hubs reach is listed once, by its core id, through
whichever hub answers first. Calls relay one hop: a hub forwards to the
machines it added, never on to a hub of its own.

```sh
# on hub-a
hc machine add hub-b --mesh --install
hc machine add ops --mesh
# on hub-b
hc machine add hub-a --mesh
hc machine add gpu-box --mesh --install
```

### A laptop that sleeps

A laptop makes a poor hub: when it sleeps, every line it keeps drops. Let an
always-on hub add the laptop instead:

```sh
# on the hub; the laptop needs an ssh server (macOS: System Settings, Sharing, Remote Login)
hc machine add laptop --mesh --install
```

While the laptop sleeps the hub shows it as down. It dials again with a
growing pause of up to 30 s, so the laptop is back in the mesh within half a
minute of waking.

Attach, events and put do not cross the mesh yet. To attach from the laptop
to a terminal on another machine, also add that machine (or the hub) on the
laptop with `hc machine add`. A hub added on the laptop still brings every
machine it reaches, each listed once.

## What crosses the mesh

From a mesh machine, through the hub: listing (`hc ls`, `hc ls --found`),
starting (`hc run --on`), reading, sending, waiting, `hc exec --on`, files
(`hc file read`, `hc file write`, `hc file edit`), labels and stopping
terminals.

What does not, yet: `hc attach`, `hc events`, `hc put`. They work on the hub
itself and on any machine you add directly. A mesh machine also cannot add,
remove or rename the hub's machines, change who is in the mesh, or stop or
restart the hub's core.

## Trust

The mesh is trust. A machine in it can type into every terminal its hub
reaches, start programs on every machine the hub has added, and read and
write their files, with the permissions of the user each core runs as. Put a
machine in the mesh only if you would give it a shell on every other one.

- Leave out machines that belong to someone else, shared boxes, and short-lived
  sandboxes. Add those without `--mesh`: they still run work for you.
- The hub holds the keys. Use an ssh key just for the hub, kept in its ssh agent
  or a key file only its user can read; the other machines need none.
- A private network such as a tailnet keeps the ssh ports off the internet.
  With Tailscale SSH, do not use its "check" mode for hub lines: it stops a
  connection to ask a person to approve it in a browser, which an always-on line
  can't do, so the machine looks hung. Use plain `accept` rules, or the
  machine's own sshd and an ssh key.
- `hc machine mesh <machine> --off` takes a machine out; it hears at once.

## What runs outside hc

`hc ls --found` also lists what each machine's `discover` command finds
outside hc: agents in tmux or as services, model servers, containers. Through
the mesh, it asks every machine the hub reaches.

On each machine whose outside-hc programs you want listed, install the example
script and point the config at it:

```sh
mkdir -p ~/.config/herdr-core
curl -fsSL https://raw.githubusercontent.com/neildmorris/herdr-core/mesh/contrib/discover/hc-discover.py \
  -o ~/.config/herdr-core/hc-discover.py
chmod +x ~/.config/herdr-core/hc-discover.py
~/.config/herdr-core/hc-discover.py   # one line of JSON
```

```toml
# ~/.config/herdr-core/config.toml
discover = ["~/.config/herdr-core/hc-discover.py"]
```

The command prints a JSON array of `{kind, name, status, pid, cwd, detail}`
within 5 s. [`hc-discover.py`](../contrib/discover/hc-discover.py) is a
starting point: edit its tables to look for what you run. Any program that
prints that array works. Found entries are for looking, not driving.

## Release channel

This fork publishes its own releases, so a mesh never mixes in upstream's
builds by accident. The channel is the `channel` branch of the fork, read as
`https://raw.githubusercontent.com/neildmorris/herdr-core/channel`:

| Path | What |
| --- | --- |
| `core/latest.json` | the latest release's manifest |
| `core/<version>/manifest.json` | one version's manifest, such as `core/0.1.21-mesh.1/manifest.json` |
| `core/install.sh`, `core/install.ps1` | the installers, defaulting to this channel |

A manifest is `{version, generation, notes, targets}`, with each target
(`linux-x86_64`, `linux-aarch64`, `macos-aarch64`, `macos-x86_64`,
`windows-x86_64`) giving the `url` of its archive on the fork's GitHub
release and its `sha256`. `hc update`, `hc machine add --install` and
`hc machine update` read it; `HERDR_RELEASES_URL` points one run elsewhere,
such as upstream's `https://releases.herdr.dev`.

### Releasing a fork of your own

The channel is chosen when hc is built: `HC_RELEASES_URL`, with this fork's
channel as the default. To run your own fork:

1. Fork the repository and enable Actions on it.
2. Set `version = "<upstream>-mesh.<n>"` in `crates/herdr-core/Cargo.toml`
   (for example `0.1.21-mesh.2`), commit, and push a matching tag
   (`v0.1.21-mesh.2`).
3. `.github/workflows/fork-release.yml` builds all five targets with
   `HC_RELEASES_URL` set to your fork's `channel` branch, creates the GitHub
   release, then publishes the manifests and installers to that branch. It
   needs no secrets. Upstream's `release.yml` skips itself on forks.
4. Install with `curl -fsSL https://raw.githubusercontent.com/<you>/herdr-core/channel/core/install.sh | sh`.

Without Apple secrets, macOS builds are ad-hoc signed, which hc accepts from
any channel but upstream's. With `APPLE_CERTIFICATE_P12` and
`APPLE_CERTIFICATE_PASSWORD` (a Developer ID Application certificate) they are
signed with your team, and with `APPLE_API_KEY`, `APPLE_API_KEY_ID` and
`APPLE_API_ISSUER` also notarized. To serve the channel from GitHub Pages or
elsewhere instead, set the repository variable `RELEASES_URL`.
A build you make yourself updates from this fork's channel unless you build
with `HC_RELEASES_URL=<your channel> cargo build --release`.

Versions compare by the upstream part, then the mesh number: `0.1.21-mesh.2`
is newer than `0.1.21-mesh.1` and `0.1.21`, older than `0.1.22`. Cores of this
fork and of upstream still talk to each other: compatibility is the protocol
generation and features, not the version.

## When something goes wrong

- Start with `hc machine ls` on the hub: each machine's line state and hc version.
- A machine that never lists the hub's terminals is not in the mesh: run
  `hc machine mesh <machine>` on the hub, or check that machine runs this fork
  (`hc --version` says `-mesh`). Upstream hc ignores the mesh.
- `machine_unknown` from a mesh machine means neither it nor its hub has that
  machine. Add it on the hub.
- `not offered over the mesh` is attach, events, put, or a change to the hub
  itself. Do it on the hub, or add the machine directly.
- `has no release to download` during `--install`: the hub runs an hc whose
  version this fork never released (a local build). Install the hub from the
  channel, or install hc on that machine yourself.
- A line that stays `connecting` with Tailscale SSH is often its check mode
  waiting for a browser approval; see [Trust](#trust).
- A line that closes at once: the remote login prints extra text to stdout.
  See [Machines](machines.md#when-something-goes-wrong) for that and for
  `hc machine login`.
- After `hc update` on the hub, run `hc machine update <machine>` for each
  machine so the whole mesh stays on one build. Terminals keep running.
