#!/usr/bin/env python3

from lib.util import pretty_print
import typing as t
import argparse
from exec import exec


DEAD_ADDRESS = "bcrt1qgq4c3n9uxye5lxhcxphevj9xsytrv4nhrnjw4v"


def wallet_balance(container_id: str) -> t.Any:
    return exec(container_id, "lncli", ["walletbalance"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Get wallet balance.")
    parser.add_argument("target", type=str, help="LND container ID")
    args = parser.parse_args()

    wallet_balance(args.target)
