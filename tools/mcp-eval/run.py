#!/usr/bin/env python3
"""Gives the typical tasks of crates/og-cli/tests/mcp_tasks.json to real assistants connected
to `og mcp`, one model at a time, and judges what they made by the checks of each task.

    python tools/mcp-eval/run.py --og target/release/og --models sonnet opus haiku

Each task runs in a folder of its own with copies of its data. By default the assistant is
Claude Code (`claude -p`), which must be installed and signed in; --agent gives another
command, with {prompt}, {model} and {config} in it, the last being a file of the form
{"mcpServers": {"ographis": {"command": ..., "args": [...]}}}.

The result is a table of tasks and models, and results.json beside this script with how
long each took and what went wrong.
"""

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TASKS = ROOT / "crates" / "og-cli" / "tests" / "mcp_tasks.json"
EXAMPLES = ROOT / "examples"

DEFAULT_AGENT = (
    "claude -p {prompt} --model {model} --mcp-config {config} --strict-mcp-config "
    "--allowedTools mcp__ographis --output-format json --max-turns 40"
)

# Said after every request, as no one is there to answer questions.
UNATTENDED = (
    " Use the tools of Ographis for all of it. Work on your own without asking me anything: "
    "when the trial looks right, run the model in full."
)


def judge(og: str, folder: Path, check: dict) -> str:
    """Empty when the check holds, else what is wrong."""
    f = folder / check["file"]
    if not f.exists():
        return f"{check['file']} was not written"
    info = ""
    if "features" in check or "crs" in check:
        info = subprocess.run([og, "info", str(f)], capture_output=True, text=True).stdout
    n = check.get("features")
    if n is not None and f"{n} features" not in info:
        return f"{check['file']} should have {n} features"
    if check.get("crs") and check["crs"] not in info:
        return f"{check['file']} should be in {check['crs']}"
    if check.get("contains") and check["contains"] not in f.read_text(errors="replace"):
        return f"{check['file']} should contain {check['contains']}"
    return ""


def run_task(og: str, agent: str, model: str, task: dict, keep: bool) -> dict:
    folder = Path(tempfile.mkdtemp(prefix=f"og-eval-{task['name']}-"))
    for d in task["data"]:
        shutil.copy(EXAMPLES / d, folder / d)
    config = folder / ".mcp.json"
    config.write_text(json.dumps({"mcpServers": {"ographis": {"command": og, "args": ["mcp", "--folder", str(folder)]}}}))
    env = dict(os.environ, OG_MCP_NO_WINDOW="1", OGRAPHIS_SETTINGS_DIR=str(folder / ".settings"))
    command = agent.format(prompt=shlex.quote(task["prompt"] + UNATTENDED), model=shlex.quote(model), config=shlex.quote(str(config)))
    started = time.time()
    try:
        done = subprocess.run(command, shell=True, cwd=folder, env=env, capture_output=True, text=True, timeout=900)
        output = done.stdout[-4000:] + done.stderr[-2000:]
    except subprocess.TimeoutExpired:
        output = "timed out after 15 minutes"
    seconds = round(time.time() - started)
    problems = [p for p in (judge(og, folder, c) for c in task["checks"]) if p]
    if not keep:
        shutil.rmtree(folder, ignore_errors=True)
    return {"task": task["name"], "model": model, "passed": not problems, "problems": problems, "seconds": seconds, "folder": str(folder) if keep else "", "output": output}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--og", default=str(ROOT / "target" / "release" / ("og.exe" if os.name == "nt" else "og")))
    ap.add_argument("--models", nargs="+", default=["sonnet", "opus", "haiku"])
    ap.add_argument("--agent", default=DEFAULT_AGENT)
    ap.add_argument("--tasks", nargs="*", help="only these tasks, by name")
    ap.add_argument("--keep", action="store_true", help="keep the folders of the tasks")
    a = ap.parse_args()
    if not Path(a.og).exists():
        print(f"There is no {a.og}; build it first, or give --og.", file=sys.stderr)
        return 2
    tasks = json.loads(TASKS.read_text(encoding="utf-8"))["tasks"]
    if a.tasks:
        tasks = [t for t in tasks if t["name"] in a.tasks]
    results = []
    for model in a.models:
        for task in tasks:
            r = run_task(a.og, a.agent, model, task, a.keep)
            results.append(r)
            print(f"{model:<10} {task['name']:<26} {'passed' if r['passed'] else 'FAILED'} {r['seconds']:>4} s  {'; '.join(r['problems'])}", flush=True)
    (Path(__file__).parent / "results.json").write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")
    print()
    print("task".ljust(26) + "".join(m.ljust(10) for m in a.models))
    for task in tasks:
        row = task["name"].ljust(26)
        for model in a.models:
            r = next(x for x in results if x["task"] == task["name"] and x["model"] == model)
            row += ("ok" if r["passed"] else "–").ljust(10)
        print(row)
    return 0 if all(r["passed"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
