# Shared host CLI

The independent [trames-cli](https://github.com/project-trames/trames-cli) repository owns
`trames-cli-common`. Mac/HVF, Linux/KVM and the interpreter wrapper pin its same
source through `cli/`. It has no runtime, CPU executor, AppKit or media dependency.
Product executables compose the library with their own engine and lifecycle.

```mermaid
flowchart LR
  Mac[Mac product] --> CLI[trames-cli-common]
  Linux[Linux product] --> CLI
  Runner[Interpreter runner] --> CLI
  Mac --> HVF
  Linux --> KVM
  Runner --> Interpreter
  HVF --> Runtime[trames-runtime]
  KVM --> Runtime
  Interpreter --> Runtime
```

The first extraction shares environment/argv selection, foreground options,
storage-size helpers, Unix terminal setup/restoration and the original session
and exec protocol clients. Mac retains environment management, its registry,
server side of those protocols, GUI and lifecycle. Linux gains explicit rootfs
shell/exec and portable socket clients. Named environment management on Linux
remains a separate implementation batch.

Project dependency boundaries, exact source pins and fixed target ABI selection are checked at build/test time. Release execution does not scan source trees or invoke Git to recheck these invariants.

CLI input is literal argv, never host shell syntax. Transfers preserve separate
binary stdin/stdout/stderr and guest exit status. Terminal settings retain a
duplicated original descriptor. Signal handlers retain a stable self-pipe and
preserve errno; sockets/stdin/signals/deadlines wake the same frontend thread.
There is no polling timer or new resource/coordinator thread. Programmatic
captures preserve embedding handlers; foreground transfers opt into forwarding.

The old interpreter package `trames-cli` and binary `trames-run` keep their names,
so existing build and compatibility scripts remain valid. Runtime semantics stay
in the separately pinned runtime project. Parsing/help do not initialize devices.

Linux examples:

```sh
trames shell --rootfs /path/to/rootfs
trames exec --rootfs /path/to/rootfs --cwd /tmp --setenv KEY='literal value' -- /bin/sh -c 'printf "%s\n" "$KEY"'
trames run --pty --timeout-ms 30000 -- /path/to/static-arm64-elf
trames exec --socket /path/to/console -- /bin/echo hello
trames session ls --socket /path/to/console --json
```

Socket commands connect to an already available compatible service. They do not
claim that Linux environment persistence or a daemon has been implemented.
