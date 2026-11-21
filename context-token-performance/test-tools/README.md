# Test tools

Reusable tooling for measuring what an animation costs a fresh agent. Each valid run leaves one GIF in `reports/` beside the report, named after the run.

```
prompts/<test>/    one prompt per run, with {repo} and {skill} placeholders
sessions.csv       test, run, prompt file, session id, status, product
report.py          rebuilds reports/token-economy.md from the session database
```

The `product` column names the finished animation in `reports/`, and the report links it. Any format is allowed, so a later test can compare a GIF with an SVG.

## Protocol

1. Run the baseline prompt first. It is a fresh session asked only to reply OK; its fill is the fixed overhead every other run in that test pays.
2. Vary one axis per pair of runs. Hold the other axis constant.
3. Every prompt is self-contained: exact input paths, the task rules the skill does not supply, one deliverable, one stop condition, and no request to report tokens.
4. Do not name the skill's own files. Loading SKILL.md and its references is the skill's job; naming them measures a forced read, not what the job costs on its own.
5. Run each prompt once in a new session and take the session id from the return.
6. Add the id to `sessions.csv` and run `python test-tools/report.py`.

Fill on one assistant message is input plus cache read plus cache write. The context token cost is the heaviest call of a run; the last call matched it in every run so far. Overhead is that number minus the baseline. Output and reasoning are not part of the fill. Per-call counters are never summed for this report; a sum measures billing, not context. Time is measured from session start to the last message.

The model window is not recorded in the local configuration, so the report prints tokens only. `token-report.py --context <id> --window N` prints a share when a window is given.

## Placeholders

`{repo}` is the workspace root and `{skill}` is the installed anikit skill folder. They are expanded when a prompt is sent, so the stored prompt is portable and the repository stays free of machine paths. The exact text a session received is reachable from its id in the database.

## Adding a test

1. Make `prompts/<test>/`.
2. Add a row with `run=baseline` and the baseline prompt, then one row per prompt.
3. Run each prompt once in a fresh session, fill in the session ids, and put the finished file in `reports/` with its name in the product column.
4. Run `python test-tools/report.py`; it writes one section per test.

`report.py` computes context fill and time; only rows marked valid are reported. A future test that measures something else keeps its prompts here and its GIF in `reports/`, with its own script and report beside `report.py`.

Draft runs stay in `sessions.csv` and leave no file; only valid runs put a GIF in `reports/`.
