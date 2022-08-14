#!/usr/bin/env python3

import argparse
from exec import exec


def open_channel(source: str, target: str, local_amt: int, push_amt: int) -> None:
    exec(source, "lncli", ["walletbalance"])
    info = exec(target, "lncli", ["getinfo"])
    target_pubkey = info["identity_pubkey"]
    peers = exec(source, "lncli", ["listpeers"])
    peer_pubkeys = [peer["pub_key"] for peer in peers["peers"]]
    if target_pubkey not in peer_pubkeys:
        exec(source, "lncli", ["connect", f"{target_pubkey}@{target}", "--timeout=30s"])
    exec(source, "lncli", ["openchannel", target_pubkey, str(local_amt), str(push_amt)])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Open a channel between two LND nodes")
    parser.add_argument("source", type=str, help="source container name")
    parser.add_argument("target", type=str, help="target container name")
    parser.add_argument("local_amt", type=int, help="local amount")
    parser.add_argument("remote_amt", type=int, help="remote amount")
    args = parser.parse_args()

    open_channel(args.source, args.target, args.local_amt, args.remote_amt)
