# Command line

Every `quiddity` command. The [README](../README.md#command-line) shows the main path.

```text
quiddity <file.step> [--k-factor K]                    recognise the part's features
quiddity correspond <old.json|old.step> <new.json|new.step>   match features across revisions
quiddity evidence <file.step> [--framed] [--k-factor K]  the recognition evidence for each feature
quiddity fingerprints <file.step>                     the features' fingerprints
quiddity capabilities                                 this build's versioned capabilities
quiddity parts <file.step>                            list an assembly's parts
quiddity manifest <file.step> -o <manifest.json>       scan product ranges
quiddity recognise <file.step> --manifest <manifest.json> --out-dir <dir> [--parts p0,p7] [--k-factor K]
                                                       recognise products and write results
quiddity pmi read <file.step> [--part N]              the file's PMI as JSON
quiddity pmi check <file.step> <pmi.json>             validate a PMI document against the part
quiddity pmi write <file.step> <pmi.json> -o <out.step> [--mode add|replace|remove]
                   [--presentation refuse|remove]     write PMI into the file
quiddity brep <file.step> -o <out.step> [--pmi <pmi.json>] [--inertia]   the B-rep and PMI as a new AP242 file
quiddity serve                                        JSON-lines requests on stdin
quiddity completions <SHELL>                          a shell completion script
quiddity --help
```

`quiddity <command> --help` gives a command's arguments.

Recognition and inspection commands print JSON; `manifest` and `recognise --manifest`
write their requested files. `completions`, help and version print text. STEP inputs may be
`.step`, `.stp` or gzipped; manifest product reads require an uncompressed source.
The exit code is 0 for success, 1 for an unreadable file or a refused operation,
and 2 for wrong usage.

`--k-factor K` sets the neutral-axis fraction of the sheet thickness for flat
patterns. It defaults to 0.5 and accepts finite numbers greater than zero and at
most one. An invalid value exits with code 2. Each development reports its value at
`flat_pattern.k_factor`.

`quiddity serve` keeps one process open for applications: one JSON request per line on
stdin, one response line per request on stdout, e.g. `{"id":1,"op":"recognise","step":"part.step"}`.
The operations are `recognise` and `correspond`. A `recognise` request can include the
numeric field `"k_factor": 0.45`. Omit it or set it to `null` to use 0.5.
An invalid value returns a `bad_request` response for that line and the process
continues serving.

## Correspondence

Compare both revisions at the same K. A change in K can switch
`flat_pattern_status` between `checked` and `overlap`. This trait is part of the
fingerprint, so `correspond` can report a matched sheet as `adapted`.
`quiddity fingerprints` uses K = 0.5 for STEP inputs, as does `correspond` in
both the CLI and serve. For a custom K, generate both recognition results with the same
`--k-factor` or serve `k_factor` and pass those results to `correspond`.
Where a development's validity changes, K can also change flange adoption and
the suppression of duplicate slots, pockets and pads. See
[Sheet-metal K-factor](recognition-6.md#sheet-metal-k-factor-for-358).

**Shell completion.** `quiddity completions SHELL` prints a completion script for `bash`,
`zsh`, `fish`, `powershell` or `elvish`:

```sh
mkdir -p ~/.local/share/bash-completion/completions && quiddity completions bash > ~/.local/share/bash-completion/completions/quiddity
mkdir -p ~/.zfunc && quiddity completions zsh > ~/.zfunc/_quiddity
# zsh: add `fpath=(~/.zfunc $fpath)` before `compinit` in ~/.zshrc, then restart zsh
mkdir -p ~/.config/fish/completions && quiddity completions fish > ~/.config/fish/completions/quiddity.fish
```
