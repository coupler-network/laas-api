#!/usr/bin/env python3

from lib.containers import BITCOIN_CONTAINER
import typing as t
import argparse
from exec import exec


DEAD_ADDRESS = "bcrt1qgq4c3n9uxye5lxhcxphevj9xsytrv4nhrnjw4v"


def mine_blocks(num_blocks: int = 1, container_id: t.Optional[str] = None) -> t.Any:
    if container_id is not None:
        address_response = exec(container_id, "lncli", ["newaddress", "p2wkh"])
        address = address_response["address"]
    else:
        address = DEAD_ADDRESS

    return exec(BITCOIN_CONTAINER, "bitcoin-cli", ["generatetoaddress", str(num_blocks), address])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Mine blocks.")
    parser.add_argument("--count", type=int, help="number of block to mine", default=1)
    parser.add_argument("--target", type=str, help="LND container ID to mine into, omit this to mine into an inactive address")
    args = parser.parse_args()

    mine_blocks(args.count, args.target)
