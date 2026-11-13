# Command line

Every `quiddity` command. The [README](../README.md#command-line) shows the main path.

```text
quiddity <file.step>                                  recognise the part's features
quiddity correspond <old.json|old.step> <new.json|new.step>   match features across revisions
quiddity evidence <file.step> [--framed]              the recognition evidence for each feature
quiddity fingerprints <file.step>                     the features' fingerprints
quiddity capabilities                                 this build's versioned capabilities
quiddity parts <file.step>                            list an assembly's parts
quiddity manifest <file.step> -o <manifest.json>       scan product ranges
quiddity recognise <file.step> --manifest <manifest.json> --out-dir <dir> [--parts p0,p7]
                                                       recognise products and write results
quiddity pmi read <file.step> [--part N]              the file's PMI as JSON
quiddity pmi check <file.step> <pmi.json>             validate a PMI document against the part
quiddity pmi write <file.step> <pmi.json> -o <out.step> [--mode add|replace|remove]
                   [--presentation refuse|remove]     write PMI into the file
quiddity brep <file.step> -o <out.step> [--pmi <pmi.json>]   the B-rep and PMI as a new AP242 file
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

`quiddity serve` keeps one process open for applications: one JSON request per line on
stdin, one response line per request on stdout, e.g. `{"id":1,"op":"recognise","step":"part.step"}`.
The operations are `recognise` and `correspond`.

**Shell completion.** `quiddity completions SHELL` prints a completion script for `bash`,
`zsh`, `fish`, `powershell` or `elvish`:

```sh
mkdir -p ~/.local/share/bash-completion/completions && quiddity completions bash > ~/.local/share/bash-completion/completions/quiddity
mkdir -p ~/.zfunc && quiddity completions zsh > ~/.zfunc/_quiddity
# zsh: add `fpath=(~/.zfunc $fpath)` before `compinit` in ~/.zshrc, then restart zsh
mkdir -p ~/.config/fish/completions && quiddity completions fish > ~/.config/fish/completions/quiddity.fish
```
