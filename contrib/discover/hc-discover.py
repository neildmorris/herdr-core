#!/usr/bin/env python3
"""What runs on this machine outside hc, as `hc ls --found` wants it.

hc runs this (config: discover = ["~/.config/herdr-core/hc-discover.py"]) and
relays what it prints: a JSON array of {kind, name, status, pid, cwd, detail}.
Everything vendor-specific lives here, not in hc. Stdlib only; Linux and macOS.
Edit the tables below to taste.
"""
import json, os, platform, re, shutil, subprocess

# Agent command lines: the program's basename, and what to call it.
AGENTS = {
    "claude": "claude-code", "claude.exe": "claude-code", "codex": "codex", "gemini": "gemini", "agy": "agy",
    "cursor-agent": "cursor", "opencode": "opencode", "aider": "aider", "goose": "goose", "amp": "amp",
    "kimi": "kimi", "qwen": "qwen", "droid": "droid", "hermes": "hermes", "crush": "crush",
}
# Helper processes of an agent, not agents themselves.
HELPERS = ("bg-pty-host", "bg-spare", "daemon run", "shell-snapshots", "--chrome-native-host", "mcp serve")
# Desktop and IDE agents: a substring of the command line.
APPS = {"antigravity": "antigravity", "Cursor.app": "cursor-ide", "Windsurf": "windsurf", "Claude.app": "claude-desktop"}
# systemd units and containers worth listing: a pattern, and the kind.
UNITS = [(r"^hermes-gateway-(.+)\.service$", "hermes"), (r"^(hermes-.+)\.service$", "hermes"),
         (r"^(claude.*)\.service$", "claude-code"), (r"^(paperclip.*)\.service$", "paperclip"),
         (r"^(ollama|vllm.*|litellm.*)\.service$", "model-server")]
CONTAINERS = [(r"hermes", "hermes"), (r"claude-os|claude", "claude-os"), (r"^vllm|litellm|ollama|lifeboat|open-webui", "model-server"),
              (r"n8n", "automation"), (r"agent", "agent")]


def sh(*cmd, timeout=3):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout).stdout
    except Exception:
        return ""


def processes():
    """pid -> (ppid, args), from ps (works on both OSes)."""
    out = {}
    for line in sh("ps", "-axo", "pid=,ppid=,args=").splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and parts[0].isdigit():
            out[int(parts[0])] = (int(parts[1]), parts[2])
    return out


def ancestors(pid, procs):
    seen = []
    while pid in procs and pid > 1 and len(seen) < 64:
        pid = procs[pid][0]
        seen.append(pid)
    return seen


def cwd_of(pid):
    try:
        return os.readlink(f"/proc/{pid}/cwd").replace(os.path.expanduser("~"), "~")
    except Exception:
        return None


def unit_of(pid):
    try:
        for line in open(f"/proc/{pid}/cgroup"):
            m = re.search(r"/([^/]+\.(service|scope))$", line.strip())
            if m and not m.group(1).startswith(("session-", "user@", "init.scope")):
                return m.group(1)
    except Exception:
        pass
    return None


def tmux_sessions(procs):
    """tmux pane pid -> session name, so agents in tmux say where they are."""
    out = {}
    for line in sh("tmux", "list-panes", "-a", "-F", "#{pane_pid} #{session_name}").splitlines():
        pid, _, name = line.partition(" ")
        if pid.isdigit():
            out[int(pid)] = name
    return out


def agents(procs):
    rows, tmux = [], tmux_sessions(procs)
    agent_pids = set()
    for pid, (ppid, args) in procs.items():
        prog = os.path.basename(args.split()[0]) if args.split() else ""
        if prog in AGENTS and not any(h in args for h in HELPERS):
            agent_pids.add(pid)
    for pid in sorted(agent_pids):
        ppid, args = procs[pid]
        up = ancestors(pid, procs)
        if any(a in agent_pids for a in up):
            continue  # a child of an agent already listed
        if any("herdr-core" in procs.get(a, (0, ""))[1] for a in up):
            continue  # hc already lists it
        kind = AGENTS[os.path.basename(args.split()[0])]
        words = args.split()[1:]
        mode = "remote-control" if words[:1] in (["remote-control"], ["rc"]) else "resumed" if "--resume" in words else "interactive"
        unit = unit_of(pid)
        if unit and unit.startswith("hc-hold-"):
            unit = "started from an hc terminal"
        where = next((f"tmux {tmux[a]}" for a in [pid] + up if a in tmux), None) or unit
        rows.append({"kind": kind, "name": mode, "status": "running", "pid": pid, "cwd": cwd_of(pid),
                     "detail": " · ".join(x for x in [where, " ".join(words)[:60] or None] if x)})
    return rows


def apps(procs):
    rows, seen = [], set()
    for pid, (ppid, args) in procs.items():
        for needle, kind in APPS.items():
            if needle.lower() in args.lower() and kind not in seen and needle.lower() not in procs.get(ppid, (0, ""))[1].lower():
                seen.add(kind)
                rows.append({"kind": kind, "name": kind, "status": "running", "pid": pid})
    return rows


def units():
    if platform.system() != "Linux" or not shutil.which("systemctl"):
        return []
    rows = []
    for scope in (["--user"], []):
        for line in sh("systemctl", *scope, "list-units", "--type=service", "--state=running", "--no-legend", "--plain").splitlines():
            unit = line.split()[0] if line.split() else ""
            for pattern, kind in UNITS:
                m = re.match(pattern, unit)
                if m:
                    rows.append({"kind": kind, "name": m.group(1), "status": "active",
                                 "detail": ("user unit " if scope else "unit ") + unit})
                    break
    return rows


def containers():
    if not shutil.which("docker"):
        return []
    groups = {}
    for line in sh("docker", "ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Status}}", timeout=5).splitlines():
        name, image, status = (line.split("\t") + ["", ""])[:3]
        for pattern, kind in CONTAINERS:
            if re.search(pattern, name) or re.search(pattern, image.split("/")[-1]):
                # Many of one image with generated names (sandboxes): one row.
                key = (kind, image) if re.search(r"-[0-9a-f]{8}$", name) else (kind, name)
                g = groups.setdefault(key, {"kind": kind, "name": name, "status": status.split(" (")[0].lower(),
                                            "detail": f"container {image}", "n": 0})
                g["n"] += 1
                break
    rows = []
    for g in groups.values():
        n = g.pop("n")
        if n > 1:
            g["name"] = f"{g['detail'].split()[-1].split('/')[-1]} ×{n}"
        rows.append(g)
    return rows


def main():
    procs = processes()
    rows = agents(procs) + apps(procs) + units() + containers()
    print(json.dumps(rows))


if __name__ == "__main__":
    main()
