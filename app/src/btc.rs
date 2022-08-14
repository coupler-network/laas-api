//! This module contains definitions for Bitcoin-specific entities and routines.

use std::ops::{Add, AddAssign, Sub, SubAssign};

#[cfg(all(feature = "mainnet", feature = "testnet"))]
compile_error!("mainnet and testnet cannot be enabled at the same time");

#[cfg(feature = "mainnet")]
const NETWORK: bitcoin::Network = bitcoin::Network::Bitcoin;

#[cfg(feature = "testnet")]
const NETWORK: bitcoin::Network = bitcoin::Network::Testnet;

#[cfg(all(not(feature = "mainnet"), not(feature = "testnet")))]
const NETWORK: bitcoin::Network = bitcoin::Network::Regtest;

pub use bitcoin::Address;
pub use bitcoin::Txid as TxId;

#[derive(Debug, Clone)]
pub struct Tx {
    pub id: TxId,
    pub block_height: Option<u32>,
}

impl Tx {
    pub(crate) fn is_confirmed(&self) -> bool {
        self.block_height.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct TxOut {
    pub tx: Tx,
    pub address: Address,
    pub v_out: i64,
    pub amount: Sats,
}

#[derive(Debug, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq)]
pub struct MilliSats(pub i64);

#[derive(Debug, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq)]
pub struct Sats(pub i64);

impl MilliSats {
    pub fn sats_floor(&self) -> Sats {
        Sats(self.0 / 1000)
    }
}

impl Add for MilliSats {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for MilliSats {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl AddAssign for MilliSats {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for MilliSats {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Sats {
    pub fn msats(self) -> MilliSats {
        MilliSats(self.0 * 1000)
    }
}
