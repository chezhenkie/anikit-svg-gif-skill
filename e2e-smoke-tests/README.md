# e2e-smoke-tests

Route-level checks that drive a built `anikit` binary. Fixtures come from `rust/crates/anikit-cli/tests/fixtures`, so the folder carries no data of its own.

```sh
python e2e-smoke-tests/smoke.py path/to/anikit
python e2e-smoke-tests/smoke.py path/to/anikit --full --out /tmp/smoke
```

The default run covers the routes that need no browser: ASCII to GIF and APNG, an image folder to GIF, and a tween scene to SVG. `--full` adds the vector and tween browser routes, then chains extract, the timed image rebuild and import back into the tween route. Output goes to a temp folder that is removed on success, or to `--out`. A run ends with a PASS or FAIL line per case and exits 1 when any case failed.

Python 3 with the standard library only, so the same script runs against a static musl asset, a Windows build and macOS.
