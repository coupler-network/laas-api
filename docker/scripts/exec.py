#!/usr/bin/env python3

from json.decoder import JSONDecodeError
from lib.client import client
from lib.util import pretty_print
import typing as t
import argparse
import json
import time


def _exec_raw(id: str, command: t.List[str]) -> t.Tuple[int, str]:
    containers: t.List[t.Any] = client.containers.list()
    container: t.Any = next(c for c in containers if c.name == id)
    print(f"{id} ({container.id[:5]}): {' '.join(command)}")
    code, output = container.exec_run(command, stdin=True, tty=True)
    return code, output.decode()


def _check_code(code: int) -> None:
    if code != 0:
        exit(code)


def _expand_numbers(params: t.List[str]) -> t.List[str]:
    return [_expand_number(p) for p in params]


def _expand_number(n: str) -> str:
    try:
        return str(int(float(n)))
    except ValueError:
        return n


def exec(id: str, command: str, params: t.List[str], *, ignore_failure: bool = False) -> t.Any:
    if command == "lncli":
        params = ["--rpcserver=localhost:10010", "--network=regtest", "--macaroonpath=/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon", *params]
    if command == "bitcoin-cli":
        additional_params = ["-rpcport=43782", "-rpcuser=user", "-rpcpassword=pass"]
        if not any("-rpcwallet" in param for param in params):
            additional_params.append("-rpcwallet=w1")
        params = [*additional_params, *params]

    params = _expand_numbers(params)

    while True:
        code, output = _exec_raw(id, [command, *params])

        known_errors = [
            "before the wallet is fully synced",
            "server is still in the process of starting",
            "the RPC server is in the process of starting up, but not yet ready to accept calls"
        ]
        if code != 0 and any(e in output for e in known_errors):
            print(f"{output.strip()}, retrying...")
            time.sleep(2)
        else:
            break

    try:
        response = json.loads(output)
        pretty_print(response)
        if not ignore_failure:
            _check_code(code)
        return response
    except JSONDecodeError:
        print(output)
        if not ignore_failure:
            _check_code(code)
        return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Execute an lncli or bitcoin-cli command.")
    parser.add_argument("id", type=str, help="container name")
    parser.add_argument("command", type=str, help="command to execute", nargs="+")
    args = parser.parse_args()

    command, *params = args.command
    exec(args.id, command, params)
