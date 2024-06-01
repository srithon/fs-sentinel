# 0.2.4

- remove the "initially modified" behavior from the `fs-sentinel` library
    - note that this change has no observable effect from the CLI

# 0.2.3

- greatly improve error handling/messages
    - daemon now checks if fsnotifywait exists in `PATH` before running
- `list-modified` now only returns ids which were passed into the active daemon instance
- expand upon documentation in README.org and INSTALL.org

# 0.2.2

- filesystem list passed to `daemon` must now be non-empty
    - (before, you could pass in no filesystems and it would "work" but just wouldn't do anything)
- improved CLI help messages
- match `none` mountpoints for fs-sentinel-zfs-daemon.sh
    - `-` mountpoints are for zvols, while `none` mountpoints are for datasets.
- add `list-modified` CLI subcommand

# 0.2.1

- add help messages to subcommands
- add /zfs-linux directory with sample sanoid integration

# 0.2.0

- first functional release
