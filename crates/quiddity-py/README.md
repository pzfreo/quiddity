# quiddity

Rust-built STEP manufacturing feature recognition for CPython 3.11+, with no Python
runtime dependencies. This replaces Python quiddity 0.3.11 with a new file-based API.

```python
import json, quiddity
result = json.loads(quiddity.recognise("part.step"))
features = json.loads(quiddity.evidence("part.step"))["features"]
```

See [the API and migration guide](https://github.com/pzfreo/quiddity/blob/main/docs/python.md).
The `quiddity` console script and `python -m quiddity` run the Rust CLI.
