use crate::btc;
use crate::hex;
use crate::seconds::Seconds;
use futures::stream::BoxStream;
use futures::StreamExt;
use proto::lnrpc;
use proto::lnrpc::payment::PaymentStatus;
use proto::routerrpc::SendPaymentRequest;
use rand::Rng;
use rustls::internal::pemfile;
use std::collections::HashMap;
use std::time::Duration;
use std::{io::BufReader, str::FromStr, sync::Arc};
use thiserror::Error;
use tonic::Response;
use tonic::Streaming;
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, ClientTlsConfig, Uri},
};
use url::Url;

use self::proto::lnrpc::InvoiceSubscription;
use self::proto::lnrpc::PaymentFailureReason;
use self::proto::lnrpc::PaymentHash;

use super::RawInvoice;

type LightningClient = proto::lnrpc::lightning_client::LightningClient<Channel>;
type RouterClient = proto::routerrpc::router_client::RouterClient<Channel>;

/// Provides an interface for communicating with our Lightning node. We currently run an LND node,
/// so this type is implemented against LND.
pub struct Node {
    lightning: LightningClient,
    router: RouterClient,
    macaroon: hex::Hex,
    first_block: u32,
}

impl Node {
    const DEFAULT_TIMEOUT_SECS: i32 = 20;

    pub(super) async fn connect(
        endpoint: &Url,
        macaroon: hex::Hex,
        cert: Vec<u8>,
        first_block: u32,
    ) -> Self {
        let mut tls_config = rustls::ClientConfig::new();
        tls_config
            .dangerous()
            .set_certificate_verifier(Arc::new(LndCertVerifier::new(cert)));
        tls_config.set_protocols(&["h2".into()]);
        let channel = Channel::builder(Uri::try_from(endpoint.to_string()).unwrap())
            .tls_config(ClientTlsConfig::new().rustls_client_config(tls_config))
            .unwrap()
            .connect()
            .await
            .unwrap();
        Node {
            lightning: LightningClient::new(channel.clone()),
            router: RouterClient::new(channel),
            macaroon,
            first_block,
        }
    }

    pub async fn generate_address(&mut self) -> btc::Address {
        let resp = self
            .lightning
            .new_address(self.req(lnrpc::NewAddressRequest {
                r#type: lnrpc::AddressType::WitnessPubkeyHash.into(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        btc::Address::from_str(&resp.address).unwrap()
    }

    /// Returns tx outs in certain block range. If the block range runs over the last confirmed
    /// block, unconfirmed tx outs will be returned as well.
    pub async fn get_tx_outs(&mut self, query: TransactionsQuery) -> Vec<btc::TxOut> {
        // -1 because LND's end_height parameter is inclusive
        let end_height = query.start_height + query.num_blocks - 1;
        let confirmed_tx_outs = self
            .get_tx_outs_start_end(
                query.start_height.try_into().unwrap(),
                end_height.try_into().unwrap(),
                None,
            )
            .await;
        let highest_block = Self::get_highest_block(&confirmed_tx_outs).unwrap_or(0);
        if highest_block < end_height {
            self.get_tx_outs_start_end(query.start_height.try_into().unwrap(), -1, None)
                .await
        } else {
            confirmed_tx_outs
        }
    }

    pub async fn send_onchain(
        &mut self,
        address: &btc::Address,
        amount: btc::Sats,
        label: &str,
    ) -> btc::TxOut {
        let tx_id = self
            .lightning
            .send_coins(self.req(lnrpc::SendCoinsRequest {
                addr: address.to_string(),
                amount: amount.0,
                target_conf: 1,
                label: label.to_owned(),
                spend_unconfirmed: true,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .txid;
        let tx_id = btc::TxId::from_str(&tx_id).unwrap();
        let unconfirmed_tx_outs = self.get_tx_outs_start_end(i32::MAX, -1, None).await;
        unconfirmed_tx_outs
            .into_iter()
            .find(|tx_out| tx_out.tx.id == tx_id && tx_out.address == *address)
            .unwrap()
    }

    pub async fn get_tx(
        &mut self,
        address: &btc::Address,
        amount: btc::Sats,
        label: &str,
    ) -> Option<btc::TxOut> {
        const NUM_BLOCKS_ONE_MONTH: u32 = 4320;
        for start_height in (self.first_block..).step_by(NUM_BLOCKS_ONE_MONTH as usize) {
            let tx_outs = self
                .get_tx_outs_start_end(
                    start_height.try_into().unwrap(),
                    (start_height + NUM_BLOCKS_ONE_MONTH).try_into().unwrap(),
                    Some(label),
                )
                .await;
            if tx_outs.is_empty() {
                break;
            }
            let tx_out = tx_outs
                .into_iter()
                .find(|tx_out| tx_out.address == *address && tx_out.amount == amount);
            if tx_out.is_some() {
                return tx_out;
            }
        }
        self.get_tx_outs_start_end(i32::MAX, -1, Some(label))
            .await
            .into_iter()
            .find(|tx_out| tx_out.address == *address && tx_out.amount == amount)
    }

    pub async fn estimate_fee(&mut self, amount: btc::Sats, address: &btc::Address) -> btc::Sats {
        let resp = self
            .lightning
            .estimate_fee(self.req(lnrpc::EstimateFeeRequest {
                addr_to_amount: HashMap::from([(address.to_string(), amount.0)]),
                target_conf: 1,
                spend_unconfirmed: true,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        btc::Sats(resp.fee_sat)
    }

    async fn get_tx_outs_start_end(
        &mut self,
        start_height: i32,
        end_height: i32,
        label: Option<&str>,
    ) -> Vec<btc::TxOut> {
        let resp = self
            .lightning
            .get_transactions(self.req(lnrpc::GetTransactionsRequest {
                start_height,
                end_height,
                account: "default".to_owned(),
            }))
            .await
            .unwrap()
            .into_inner();
        log::debug!(
            "calling LND GetTransactions from {} to {}, got {} transactions",
            start_height,
            end_height,
            resp.transactions.len()
        );
        resp.transactions
            .into_iter()
            .filter(|t| match label {
                Some(label) => t.label == label,
                None => true,
            })
            .flat_map(|t| {
                let tx = btc::Tx {
                    id: btc::TxId::from_str(&t.tx_hash).unwrap(),
                    block_height: if t.block_height == 0 {
                        None
                    } else {
                        Some(t.block_height.try_into().unwrap())
                    },
                };
                t.output_details
                    .into_iter()
                    // Filtering out empty addresses here because currently
                    // business logic does not need to deal with transactions
                    // that don't pay to an address (https://en.bitcoin.it/wiki/Invoice_address).
                    // If this need ever arises, adjustments will need to be made to the way
                    // transactions are modelled.
                    .filter(|output| !output.address.is_empty())
                    .map(move |output| btc::TxOut {
                        tx: tx.clone(),
                        address: btc::Address::from_str(&output.address).unwrap(),
                        v_out: output.output_index,
                        amount: btc::Sats(output.amount),
                    })
            })
            .collect()
    }

    /// Attempts to route a payment for a lightning invoice. If the invoice specifies an amount,
    /// the amount parameter must be None.
    pub async fn pay_invoice(
        &mut self,
        invoice: &super::RawInvoice,
        amount: Option<btc::MilliSats>,
        fee_limit: btc::MilliSats,
    ) -> Result<(), PaymentError> {
        let amount = amount.unwrap_or_default();
        let resp = self
            .router
            .send_payment_v2(self.req(SendPaymentRequest {
                payment_request: invoice.0.clone(),
                amt_msat: amount.0,
                no_inflight_updates: true,
                timeout_seconds: Self::DEFAULT_TIMEOUT_SECS,
                fee_limit_msat: fee_limit.0,
                allow_self_payment: true,
                ..Default::default()
            }))
            .await;
        let resp = Self::handle_payment_error(resp)?;
        let payment = resp.into_inner().message().await.unwrap();
        Self::handle_payment_status(payment).await
    }

    const MAX_PROBE_RETRIES: i32 = 5;

    pub async fn probe_fee(
        &mut self,
        invoice: &super::ParsedInvoice,
        amount: Option<btc::MilliSats>,
    ) -> Result<btc::MilliSats, PaymentError> {
        for _ in 0..Self::MAX_PROBE_RETRIES {
            let resp = self
                .router
                .send_payment_v2(
                    self.req(SendPaymentRequest {
                        dest: invoice
                            .payee_pub_key()
                            .cloned()
                            .unwrap_or_else(|| invoice.recover_payee_pub_key())
                            .serialize()
                            .into_iter()
                            .collect(),
                        amt_msat: amount.map(|amount| amount.0).unwrap_or_else(|| {
                            invoice
                                .amount_milli_satoshis()
                                .unwrap_or_default()
                                .try_into()
                                .unwrap()
                        }),
                        // TODO Configurable fee limit
                        fee_limit_msat: i64::MAX,
                        // TODO Test that this works (private channels)
                        route_hints: invoice
                            .route_hints()
                            .into_iter()
                            .map(|hint| lnrpc::RouteHint {
                                hop_hints: hint
                                    .0
                                    .into_iter()
                                    .map(|hop| lnrpc::HopHint {
                                        node_id: hop.src_node_id.to_string(),
                                        chan_id: hop.short_channel_id,
                                        fee_base_msat: hop.fees.base_msat,
                                        fee_proportional_millionths: hop
                                            .fees
                                            .proportional_millionths,
                                        cltv_expiry_delta: hop
                                            .cltv_expiry_delta
                                            .try_into()
                                            .unwrap(),
                                    })
                                    .collect(),
                            })
                            .collect(),
                        no_inflight_updates: true,
                        // TODO Test that this works
                        final_cltv_delta: invoice.min_final_cltv_expiry().try_into().unwrap(),
                        payment_hash: (0..32).map(|_| rand::thread_rng().gen()).collect(),
                        timeout_seconds: 30,
                        allow_self_payment: true,
                        ..Default::default()
                    }),
                )
                .await;
            let resp = Self::handle_payment_error(resp)?;
            let payment = resp.into_inner().message().await.unwrap();
            match Self::handle_payment_status(payment).await {
                Err(PaymentError::InvalidPaymentDetails(payment)) => {
                    return Ok(btc::MilliSats(
                        payment
                            .htlcs
                            .iter()
                            .map(|htlc| {
                                htlc.route.as_ref().map_or(0, |route| route.total_fees_msat)
                            })
                            .sum(),
                    ))
                }
                Err(PaymentError::NoRouteFound) => {
                    // Delay and retry
                    tokio::time::sleep(Duration::from_millis(500)).await
                }
                Err(e) => return Err(e),
                Ok(()) => unreachable!("should never succeed with a random payment hash"),
            }
        }
        Err(PaymentError::NoRouteFound)
    }

    pub async fn create_invoice(
        &mut self,
        amount: btc::MilliSats,
        memo: Option<String>,
        expiry: Seconds,
    ) -> RawInvoice {
        let resp = self
            .lightning
            .add_invoice(self.req(proto::lnrpc::Invoice {
                memo: memo.unwrap_or_default(),
                value_msat: amount.0,
                private: true,
                expiry: expiry.0,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        RawInvoice(resp.payment_request)
    }

    pub async fn get_invoice_status(&mut self, invoice: &RawInvoice) -> InvoiceStatus {
        let invoice = self
            .lightning
            .lookup_invoice(
                self.req(PaymentHash {
                    r_hash: invoice
                        .parse()
                        .unwrap()
                        .payment_hash()
                        .iter()
                        .copied()
                        .collect(),
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_inner();
        if invoice.settle_date != 0 {
            InvoiceStatus::Settled(SettledInvoice {
                amount: btc::MilliSats(invoice.amt_paid_msat),
                raw: RawInvoice(invoice.payment_request),
                settle_index: invoice.settle_index,
            })
        } else {
            InvoiceStatus::Pending
        }
    }

    pub async fn stream_settled_invoices(
        &mut self,
        settle_index: u64,
    ) -> BoxStream<'_, SettledInvoice> {
        let one_month = Duration::from_secs(2_629_746);
        let stream = self
            .lightning
            .subscribe_invoices(self.req_timeout(
                InvoiceSubscription {
                    settle_index,
                    ..Default::default()
                },
                one_month,
            ))
            .await
            .unwrap()
            .into_inner();
        futures::stream::unfold(stream, |mut stream| async move {
            let resp = stream.message().await;
            Some((resp, stream))
        })
        .filter_map(|update| async move {
            match update.unwrap() {
                Some(update) if update.settle_date != 0 => Some(SettledInvoice {
                    amount: btc::MilliSats(update.amt_paid_msat),
                    settle_index: update.settle_index,
                    raw: RawInvoice(update.payment_request),
                }),
                _ => None,
            }
        })
        .boxed()
    }

    fn handle_payment_error(
        resp: Result<Response<Streaming<lnrpc::Payment>>, tonic::Status>,
    ) -> Result<Response<Streaming<lnrpc::Payment>>, PaymentError> {
        resp.map_err(|e| {
            let msg = e.message().to_lowercase();
            if msg.contains("invoice is already paid") {
                PaymentError::InvoiceAlreadyPaid
            } else if msg.contains("invoice expired") {
                PaymentError::InvoiceExpired
            } else {
                panic!("{:?}", e);
            }
        })
    }

    async fn handle_payment_status(payment: Option<lnrpc::Payment>) -> Result<(), PaymentError> {
        match payment {
            Some(payment) => match payment.status() {
                PaymentStatus::Unknown => Err(PaymentError::Unknown),
                PaymentStatus::Failed => match payment.failure_reason() {
                    PaymentFailureReason::FailureReasonTimeout => Err(PaymentError::TimedOut),
                    PaymentFailureReason::FailureReasonNoRoute => Err(PaymentError::NoRouteFound),
                    PaymentFailureReason::FailureReasonIncorrectPaymentDetails => {
                        Err(PaymentError::InvalidPaymentDetails(payment))
                    }
                    PaymentFailureReason::FailureReasonInsufficientBalance => {
                        log::error!("insufficient liquidity error");
                        Err(PaymentError::InsufficientLiquidity)
                    }
                    PaymentFailureReason::FailureReasonNone => Err(PaymentError::Unknown),
                    PaymentFailureReason::FailureReasonError => Err(PaymentError::Unknown),
                },
                PaymentStatus::InFlight => Err(PaymentError::Unknown),
                PaymentStatus::Succeeded => Ok(()),
            },
            None => Err(PaymentError::Unknown),
        }
    }

    fn get_highest_block(tx_outs: &[btc::TxOut]) -> Option<u32> {
        tx_outs
            .iter()
            .flat_map(|tx_out| tx_out.tx.block_height)
            .max()
    }

    fn req<T>(&self, msg: T) -> tonic::Request<T> {
        self.req_timeout(
            msg,
            Duration::from_secs(Self::DEFAULT_TIMEOUT_SECS.try_into().unwrap()),
        )
    }

    fn req_timeout<T>(&self, msg: T, timeout: Duration) -> tonic::Request<T> {
        let mut req = tonic::Request::new(msg);
        req.metadata_mut().insert(
            "macaroon",
            MetadataValue::from_str(self.macaroon.as_str()).unwrap(),
        );
        req.set_timeout(timeout);
        req
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionsQuery {
    pub start_height: u32,
    pub num_blocks: u32,
}

#[derive(Debug, Error, Clone)]
pub enum PaymentError {
    #[error("payment outcome is unknown")]
    Unknown,
    #[error("invoice has expired")]
    InvoiceExpired,
    #[error("invoice has already been paid")]
    InvoiceAlreadyPaid,
    #[error("payment timed out")]
    TimedOut,
    #[error("payment could not be routed")]
    NoRouteFound,
    #[error("invalid payment details")]
    InvalidPaymentDetails(lnrpc::Payment),
    #[error("insufficient node liquidity")]
    InsufficientLiquidity,
}

pub enum InvoiceStatus {
    Pending,
    Settled(SettledInvoice),
}

pub struct SettledInvoice {
    pub amount: btc::MilliSats,
    pub settle_index: u64,
    pub raw: RawInvoice,
}

mod proto {
    pub mod lnrpc {
        #![allow(clippy::all)]
        tonic::include_proto!("lnrpc");
    }

    pub mod routerrpc {
        #![allow(clippy::all)]
        tonic::include_proto!("routerrpc");
    }
}

struct LndCertVerifier {
    cert: rustls::Certificate,
}

impl LndCertVerifier {
    fn new(cert: Vec<u8>) -> Self {
        let mut reader = BufReader::new(cert.as_slice());
        let mut certs = pemfile::certs(&mut reader).unwrap();

        if certs.len() != 1 {
            panic!(
                "tls.cert contains {} certificates, expected one",
                certs.len()
            )
        } else {
            Self {
                cert: certs.swap_remove(0),
            }
        }
    }
}

impl rustls::ServerCertVerifier for LndCertVerifier {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        match presented_certs {
            [presented_cert] if *presented_cert == self.cert => {
                Ok(rustls::ServerCertVerified::assertion())
            }
            [presented_cert] if *presented_cert != self.cert => Err(rustls::TLSError::General(
                "server certificate doesn't match ours".to_owned(),
            )),
            _ => Err(rustls::TLSError::General(format!(
                "server sent {} certificates, expected one",
                presented_certs.len()
            ))),
        }
    }
}
