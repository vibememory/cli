# Contributing

Issues and pull requests are welcome.

Keep the CLI dependency-free and compatible with Python 3.10+. Commands should emit compact JSON by default, avoid exposing credentials, reject cross-origin redirects, and preserve the rule that analysis and inference happen in the connected agent.

Before opening a pull request, run:

```sh
python3 -m unittest discover -s tests -v
python3 -m compileall -q bin scripts tests
```

Changes to the distribution manifest or update protocol must remain compatible with previously published launchers. Security-sensitive changes should include focused tests.
