#!/usr/bin/env python3

import argparse
from open_channel import open_channel
from mine_blocks import mine_blocks
from exec import exec
from lib.containers import BITCOIN_CONTAINER, LND1_CONTAINER


def setup() -> None:
    _copy_admin_macaroon()
    _create_wallets()
    mine_blocks(400)
    mine_blocks(100, "lnd1")
    mine_blocks(100, "lnd2")
    mine_blocks(100, "lnd3")
    mine_blocks(100, "lnd4")
    exec("lnd1", "lncli", ["updatechanpolicy", "1000", "100", "18"])
    exec("lnd2", "lncli", ["updatechanpolicy", "1000", "100", "18"])
    exec("lnd3", "lncli", ["updatechanpolicy", "1000", "100", "18"])
    exec("lnd4", "lncli", ["updatechanpolicy", "1000", "100", "18"])
    open_channel("lnd2", "lnd1", 100_000, 50_000)
    open_channel("lnd3", "lnd2", 100_000, 50_000)
    mine_blocks(20, "lnd1")
    mine_blocks(20, "lnd2")
    mine_blocks(20, "lnd3")
    mine_blocks(20, "lnd4")


def _copy_admin_macaroon():
    while True:
        stat = exec(LND1_CONTAINER, "stat", ["/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon"], ignore_failure=True)
        if "No such file or directory" not in stat:
            break

    exec(LND1_CONTAINER, "mkdir", ["-p", "/lnd-data/data/chain/bitcoin/regtest"])
    exec(LND1_CONTAINER, "cp", ["/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon", "/lnd-data/data/chain/bitcoin/regtest/admin.macaroon"])
    exec(LND1_CONTAINER, "cp", ["/root/.lnd/tls.cert", "/lnd-data/"])
    exec(LND1_CONTAINER, "chmod", ["a+r", "/lnd-data/data/chain/bitcoin/regtest/admin.macaroon"])
    exec(LND1_CONTAINER, "chmod", ["a+r", "/lnd-data/tls.cert"])


def _create_wallets():
    exec(BITCOIN_CONTAINER, "bitcoin-cli", ["createwallet", "w1"])
    exec(BITCOIN_CONTAINER, "bitcoin-cli", ["createwallet", "w2"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Setup the cluster by funding and connecting nodes")
    setup()
