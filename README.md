# LaaS

This directory contains code for the backend API of Lightning as a Service, AKA
[coupler.network](https://coupler.network).

## Getting started

To start up the local environment, you will need:

- Docker and Docker compose
- Python 3.6 or higher
- [Docker package for python](https://pypi.org/project/docker/), use
  `python3 -m pip install docker` to install it
- Bash

To start up the local environment, run `./scripts/up.sh` from the `docker/`
directory. To stop it, run `./scripts/down.sh`. To restart, run
`./scripts/restart.sh`. The entire startup process is scripted in Python and
very easy to modify. For reference, check `./scripts/setup.py` and
`./scripts/exec.py`.

Most scripts are designed so they can be imported from other scripts as well as
used from the terminal. For example, to run `lncli getinfo` on the local
instance of LND (container name `lnd1` in `docker-compose`), use

```bash
./scripts/exec.py lnd1 lncli getinfo
```

All executable scripts support `-h` flags for quick help summary. For example,
try `./scripts/mine_blocks.py -h`.

For development, you will need [Rust](https://rustup.rs/). The log level is controlled
from the `RUST_LOG` environment variable, e.g. `RUST_LOG=debug`.

If you like [adminer](https://www.adminer.org/), a local instance will be
running at [localhost:7402](http://localhost:7402). Username is `postgres`,
password is `password`.
