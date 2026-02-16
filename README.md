# jjt

A lightweight task tracker that stores tasks as [Jujutsu](https://github.com/jj-vcs/jj) commits.

Tasks are commits in the jj commit graph, parented under a `jjt` bookmark. ChangeIDs are task IDs. `jj describe` handles mutations. `jj op log` provides audit history. `jj abandon` handles cleanup.

## Install

```
cargo install --path .
```

Requires `jj` on your PATH.

## Usage

```
jjt init                              # create jjt bookmark in current jj repo
jjt new "Fix auth bug" --change @     # create task, link to current change
jjt list                              # list open tasks
jjt list --ready                      # only unblocked tasks
jjt claim <id>                        # assign to $JJT_AGENT or $USER
jjt done <id> --note "was a null check"
jjt block <id> --on <other>           # add dependency
jjt unblock <id> --from <other>
jjt note <id> "discovered edge case"
jjt link <id> --relates-to <other>
jjt show <id>                         # full task detail
jjt decay --before 7d                 # jj abandon old done tasks
```

All commands accept `--json` for agent consumption. Task IDs are jj ChangeIDs and support prefix matching.

## How it works

```
root()
 └── jjt (bookmark)
      ├── task commit: "jjt: Fix auth bug\nstatus: open\npriority: 2\n..."
      ├── task commit: "jjt: Write tests\nstatus: claimed\nagent: claude\n..."
      └── task commit: "jjt: Update docs\nstatus: done\n..."
```

Each task is an empty commit whose description holds structured metadata. Mutations are `jj describe` calls. History is `jj op log`. Decay is `jj abandon`. No files, no database, no sync protocol — jj is the storage layer.
