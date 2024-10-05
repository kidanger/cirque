# cirque

Cirque is a minimal IRC server. Many IRC features are not implemented by design, and it is only suitable for small-scale communities.

Only two user modes are supported: voice (v) and channel op (o). Four channel modes are supported: secret (s), topic protected (t), moderated (m), no_external (n).

Besides the restricted feature set, cirque has two main design points:

- mostly safe to expose to the internet: uses TLS, support server password, exponential rate limiting for incoming connections per IP, rate limiting for commands, and limits to the TCP send buffer. And the server does not leak information about the clients.
- employ zero-copy to reduce allocations are much as possible.

The server does not use a database.


## Installation

With cargo:
```
cargo install --git https://github.com/kidanger/cirque.git --locked
```

With nix: (NixOS module not yet provided)
```
nix run github:kidanger/cirque
```


## Configuration

See [./config.yml](./config.yml).

The configuration can be live reloaded, including the listening address and port, by modifying the configuration file and sending SIGHUP to the process. Make sure the reload was successful by monitoring the logs.
