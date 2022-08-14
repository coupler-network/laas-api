//! Contains code related to integrating with the Lightning network. The most important abstraction
//! exposed by this module is [`Node`], which allows us to communicate with our Lightning node.

use crate::hex::Hex;
use std::{fs, str::FromStr};
use thiserror::Error;
use url::Url;

mod node;

pub(crate) use lightning_invoice::Invoice as ParsedInvoice;
pub use node::{InvoiceStatus, Node, PaymentError, SettledInvoice, TransactionsQuery};

#[derive(Debug, Error)]
#[error("{0}")]
pub struct InvoiceError(pub String);

/// An unparsed BOLT11 invoice. These invoices are also commonly referred to as "payment requests".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInvoice(pub String);

impl RawInvoice {
    pub(crate) fn parse(&self) -> Result<ParsedInvoice, InvoiceError> {
        ParsedInvoice::from_str(&self.0).map_err(|e| InvoiceError(e.to_string()))
    }
}

pub struct Config {
    pub endpoint: Url,
    pub macaroon_path: String,
    pub cert_path: String,
    pub first_block: u32,
}

/// Represents a gateway into the Lightning network.
#[derive(Debug, Clone)]
pub struct Lightning {
    endpoint: Url,
    cert: Vec<u8>,
    macaroon: Hex,
    first_block: u32,
}

impl Lightning {
    pub async fn new(config: Config) -> Self {
        let macaroon = fs::read(config.macaroon_path).unwrap();
        let cert = fs::read(config.cert_path).unwrap();
        Self {
            endpoint: config.endpoint,
            cert,
            macaroon: Hex::encode(&macaroon),
            first_block: config.first_block,
        }
    }

    /// Opens a new connection to our node.
    pub async fn create_node(&self) -> Node {
        Node::connect(
            &self.endpoint,
            self.macaroon.clone(),
            self.cert.clone(),
            self.first_block,
        )
        .await
    }
}
