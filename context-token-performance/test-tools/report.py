#!/usr/bin/env python3
"""Token economy report over the OpenCode session database.

Reads test-tools/sessions.csv, which names one test set per row group, and
rewrites reports/token-economy.md. Fill on one assistant message is input plus
cache read plus cache write. The peak is the largest call in a run, the final
is the last call. Per-call counters are not summed: a sum measures billing,
not context.

A test group needs one row with run=baseline; every other run in the group is
reported against it. Runs without a session id are listed as pending.

Usage:
  python test-tools/report.py
  python test-tools/report.py --db PATH --csv PATH --out PATH
"""

import argparse
import csv
import json
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DEFAULT_DB = os.path.join(
    os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share"),
    "opencode", "opencode.db",
)
DEFAULT_CSV = os.path.join(HERE, "sessions.csv")
DEFAULT_OUT = os.path.join(ROOT, "reports", "token-economy.md")


def connect(path):
    if not os.path.exists(path):
        sys.exit("database not found: " + path)
    uri = "file:" + os.path.abspath(path).replace("\\", "/") + "?mode=ro"
    return sqlite3.connect(uri, uri=True)


def fills(db, sid):
    """Fill and completion time per assistant message.

    Fill is input plus cache read plus cache write. The completion time is what
    the message record carries; the created time is the fallback.
    """
    rows = []
    for created, data in db.execute(
        """select time_created, data from session_message
           where session_id = ? and type = 'assistant' order by time_created""",
        (sid,),
    ):
        msg = json.loads(data)
        tok = msg.get("tokens")
        if not tok:
            continue
        cache = tok.get("cache") or {}
        end = (msg.get("time") or {}).get("completed") or created
        rows.append((created, tok.get("input", 0) + cache.get("read", 0)
                     + cache.get("write", 0), end))
    return rows


def model_name(raw):
    names = {"deepseek-v4.1-flash": "DeepSeek V4.1 Flash"}
    try:
        info = json.loads(raw or "")
    except ValueError:
        return raw or ""
    mid = info.get("id", "") if isinstance(info, dict) else str(info)
    return names.get(mid, mid)


def span(ms):
    if ms is None:
        return "-"
    sec = int(round(ms / 1000.0))
    if sec < 60:
        return "%ds" % sec
    return "%dm %02ds" % (sec // 60, sec % 60)


def load(csv_path, db):
    with open(csv_path, newline="", encoding="utf-8") as fh:
        rows = list(csv.DictReader(fh))
    for row in rows:
        row["model"] = ""
        row["duration"] = None
        values = []
        if row["session"]:
            meta = db.execute(
                "select time_created, model from session_v2 where id = ?",
                (row["session"],),
            ).fetchone()
            if meta:
                created, raw = meta
                row["model"] = model_name(raw)
                values = fills(db, row["session"])
                if values:
                    row["duration"] = max(v[2] for v in values) - created
        row["calls"] = len(values)
        row["peak"] = max((v[1] for v in values), default=None)
        row["final"] = values[-1][1] if values else None
    return rows


def groups(rows):
    order = []
    for row in rows:
        if row["test"] not in order:
            order.append(row["test"])
    return [(name, [r for r in rows if r["test"] == name]) for name in order]


def product_cell(row, report_dir):
    name = row.get("product") or ""
    if not name:
        return "-"
    if os.path.exists(os.path.join(report_dir, name)):
        return "[%s](%s)" % (name, name)
    return "%s (missing)" % name


def write_report(rows, report_dir):
    out = []
    out.append("# Token economy")
    out.append("")
    out.append("Every session pays a fixed context cost before work starts: the system prompt, project instructions, skills index and tool list. The baseline run measures it. Context token cost is the heaviest call of a run: input plus cache read plus cache write, in tokens. Overhead is that number minus the fixed cost, the context the task itself added. Time runs from session start to the last message. The product column links the finished animation, kept in the same folder.")
    out.append("")

    for name, runs in groups(rows):
        valid = [r for r in runs if r["status"] == "valid" and r["peak"]]
        base = next((r["peak"] for r in valid if r["run"] == "baseline"), None)
        models = []
        for r in valid:
            if r["model"] and r["model"] not in models:
                models.append(r["model"])
        out.append("## " + name)
        out.append("")
        if models:
            out.append("Model: {}. Fixed overhead: **{} tokens**.".format(
                ", ".join(models), "{:,}".format(base) if base else "-"))
        elif base:
            out.append("Fixed overhead: **{:,} tokens**.".format(base))
        out.append("")
        extra = os.path.join(HERE, "constraints", name + ".md")
        if os.path.exists(extra):
            with open(extra, encoding="utf-8") as fh:
                out.extend(fh.read().rstrip("\n").split("\n"))
            out.append("")
        if not valid:
            out.append("No valid runs yet.")
            out.append("")
            continue
        out.append("| run | context token cost | overhead | time | product |")
        out.append("|---|---:|---:|---:|---|")
        for r in valid:
            overhead = "-" if base is None else "{:,}".format(r["peak"] - base)
            out.append("| {} | {:,} | {} | {} | {} |".format(
                r["run"], r["peak"], overhead, span(r["duration"]),
                product_cell(r, report_dir)))
        out.append("")
        out.append("Overhead is the part the task added on top of the fixed cost.")
        out.append("")

    out.append("## Method")
    out.append("")
    out.append("The figures are the provider's own counters, read from `session_message.data` in the OpenCode database in read-only mode. The window size is not recorded in the local configuration, so the report prints tokens, not a share. Only valid runs are listed; draft runs and every prompt stay in `test-tools/sessions.csv`.")
    out.append("")
    out.append("Generated by `test-tools/report.py` from `test-tools/sessions.csv`.")
    out.append("")
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--csv", default=DEFAULT_CSV)
    ap.add_argument("--out", default=DEFAULT_OUT)
    args = ap.parse_args()

    db = connect(os.path.expandvars(os.path.expanduser(args.db)))
    rows = load(args.csv, db)
    text = write_report(rows, os.path.dirname(os.path.abspath(args.out)))
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", newline="\n", encoding="utf-8") as fh:
        fh.write(text)
    print("wrote " + args.out)


if __name__ == "__main__":
    main()
