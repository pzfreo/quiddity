# Correspondence fixtures

`cgb217.fingerprint4.json.gz`: `quiddity-rust/fingerprint/4` fingerprints of
`cadgenbench_inputs/cgb217.step.gz` (quiddity-corpus), as quiddity 0.5.5 wrote them. Made with:

```sh
python3 -m venv v055 && v055/bin/pip install quiddity==0.5.5
v055/bin/python -c "import quiddity,sys; sys.stdout.write(quiddity.fingerprints(sys.argv[1]))" \
    quiddity-corpus/cadgenbench_inputs/cgb217.step.gz | gzip -9 > cgb217.fingerprint4.json.gz
```

Specify #271: the unchanged part must correspond with its fresh fingerprints. The STEP file is
read from the corpus, so the test skips where there is none.
