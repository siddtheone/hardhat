//! transaction related data

use crate::{
    access_list::{AccessList, AccessListItem},
    signature::{Signature, SignatureError},
    utils::enveloped,
    Address, Bytes, H256, U256,
};
use revm::common::keccak256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

/// Container type for various Ethereum transaction requests
///
/// Its variants correspond to specific allowed transactions:
/// 1. Legacy (pre-EIP2718) [`LegacyTransactionRequest`]
/// 2. EIP2930 (state access lists) [`EIP2930TransactionRequest`]
/// 3. EIP1559 [`EIP1559TransactionRequest`]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TransactionRequest {
    Legacy(LegacyTransactionRequest),
    EIP2930(EIP2930TransactionRequest),
    EIP1559(EIP1559TransactionRequest),
}

/// Represents _all_ transaction requests received from RPC
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct EthTransactionRequest {
    /// from address
    pub from: Option<Address>,
    /// to address
    pub to: Option<Address>,
    /// legacy, gas Price
    #[cfg_attr(feature = "serde", serde(default))]
    pub gas_price: Option<U256>,
    /// max base fee per gas sender is willing to pay
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_fee_per_gas: Option<U256>,
    /// miner tip
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_priority_fee_per_gas: Option<U256>,
    /// gas
    pub gas: Option<u64>,
    /// value of th tx in wei
    pub value: Option<U256>,
    /// Any additional data sent
    pub data: Option<Bytes>,
    /// Transaction nonce
    pub nonce: Option<u64>,
    /// warm storage access pre-payment
    #[cfg_attr(feature = "serde", serde(default))]
    pub access_list: Option<Vec<AccessListItem>>,
    /// EIP-2718 type
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    pub transaction_type: Option<U256>,
}

impl EthTransactionRequest {
    /// Converts the request into a [TypedTransactionRequest]
    pub fn into_typed_request(self) -> Option<TransactionRequest> {
        let EthTransactionRequest {
            to,
            gas_price,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            gas,
            value,
            data,
            nonce,
            mut access_list,
            ..
        } = self;
        match (gas_price, max_fee_per_gas, access_list.take()) {
            // legacy transaction
            (Some(_), None, None) => Some(TransactionRequest::Legacy(LegacyTransactionRequest {
                nonce: nonce.unwrap_or(0),
                gas_price: gas_price.unwrap_or_default(),
                gas_limit: gas.unwrap_or_default(),
                value: value.unwrap_or(U256::ZERO),
                input: data.unwrap_or_default(),
                kind: match to {
                    Some(to) => TransactionKind::Call(to),
                    None => TransactionKind::Create,
                },
                chain_id: None,
            })),
            // EIP2930
            (_, None, Some(access_list)) => {
                Some(TransactionRequest::EIP2930(EIP2930TransactionRequest {
                    nonce: nonce.unwrap_or(0),
                    gas_price: gas_price.unwrap_or_default(),
                    gas_limit: gas.unwrap_or_default(),
                    value: value.unwrap_or(U256::ZERO),
                    input: data.unwrap_or_default(),
                    kind: match to {
                        Some(to) => TransactionKind::Call(to),
                        None => TransactionKind::Create,
                    },
                    chain_id: 0,
                    access_list,
                }))
            }
            // EIP1559
            (None, Some(_), access_list) | (None, None, access_list @ None) => {
                // Empty fields fall back to the canonical transaction schema.
                Some(TransactionRequest::EIP1559(EIP1559TransactionRequest {
                    nonce: nonce.unwrap_or(0),
                    max_fee_per_gas: max_fee_per_gas.unwrap_or_default(),
                    max_priority_fee_per_gas: max_priority_fee_per_gas.unwrap_or(U256::ZERO),
                    gas_limit: gas.unwrap_or_default(),
                    value: value.unwrap_or(U256::ZERO),
                    input: data.unwrap_or_default(),
                    kind: match to {
                        Some(to) => TransactionKind::Call(to),
                        None => TransactionKind::Create,
                    },
                    chain_id: 0,
                    access_list: access_list.unwrap_or_default(),
                }))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TransactionKind {
    Call(Address),
    Create,
}

impl TransactionKind {
    /// If this transaction is a call this returns the address of the callee
    pub fn as_call(&self) -> Option<&Address> {
        match self {
            TransactionKind::Call(to) => Some(to),
            TransactionKind::Create => None,
        }
    }
}

impl Encodable for TransactionKind {
    fn rlp_append(&self, s: &mut RlpStream) {
        match self {
            TransactionKind::Call(address) => {
                s.encoder().encode_value(&address[..]);
            }
            TransactionKind::Create => s.encoder().encode_value(&[]),
        }
    }
}

impl Decodable for TransactionKind {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.is_empty() {
            if rlp.is_data() {
                Ok(TransactionKind::Create)
            } else {
                Err(DecoderError::RlpExpectedToBeData)
            }
        } else {
            Ok(TransactionKind::Call(rlp.as_val()?))
        }
    }
}

#[cfg(feature = "fastrlp")]
impl open_fastrlp::Encodable for TransactionKind {
    fn length(&self) -> usize {
        match self {
            TransactionKind::Call(to) => to.length(),
            TransactionKind::Create => ([]).length(),
        }
    }
    fn encode(&self, out: &mut dyn open_fastrlp::BufMut) {
        match self {
            TransactionKind::Call(to) => to.encode(out),
            TransactionKind::Create => ([]).encode(out),
        }
    }
}

#[cfg(feature = "fastrlp")]
impl open_fastrlp::Decodable for TransactionKind {
    fn decode(buf: &mut &[u8]) -> Result<Self, open_fastrlp::DecodeError> {
        use bytes::Buf;

        if let Some(&first) = buf.first() {
            if first == 0x80 {
                buf.advance(1);
                Ok(TransactionKind::Create)
            } else {
                let addr = <Address as open_fastrlp::Decodable>::decode(buf)?;
                Ok(TransactionKind::Call(addr))
            }
        } else {
            Err(open_fastrlp::DecodeError::InputTooShort)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "fastrlp",
    derive(open_fastrlp::RlpEncodable, open_fastrlp::RlpDecodable)
)]
pub struct EIP2930TransactionRequest {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub access_list: Vec<AccessListItem>,
}

impl EIP2930TransactionRequest {
    pub fn hash(&self) -> H256 {
        let encoded = rlp::encode(self);
        let mut out = vec![0; 1 + encoded.len()];
        out[0] = 1;
        out[1..].copy_from_slice(&encoded);
        keccak256(&out)
    }
}

impl From<EIP2930SignedTransaction> for EIP2930TransactionRequest {
    fn from(tx: EIP2930SignedTransaction) -> Self {
        Self {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas_limit: tx.gas_limit,
            kind: tx.kind,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list.0,
        }
    }
}

impl Encodable for EIP2930TransactionRequest {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(8);
        s.append(&self.chain_id);
        s.append(&self.nonce);
        s.append(&self.gas_price);
        s.append(&self.gas_limit);
        s.append(&self.kind);
        s.append(&self.value);
        s.append(&self.input.as_ref());
        s.append_list(&self.access_list);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTransactionRequest {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub chain_id: Option<u64>,
}

impl LegacyTransactionRequest {
    pub fn hash(&self) -> H256 {
        keccak256(&rlp::encode(self))
    }
}

impl From<LegacySignedTransaction> for LegacyTransactionRequest {
    fn from(tx: LegacySignedTransaction) -> Self {
        let chain_id = tx.chain_id();
        Self {
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas_limit: tx.gas_limit,
            kind: tx.kind,
            value: tx.value,
            input: tx.input,
            chain_id,
        }
    }
}

impl Encodable for LegacyTransactionRequest {
    fn rlp_append(&self, s: &mut RlpStream) {
        if let Some(chain_id) = self.chain_id {
            s.begin_list(9);
            s.append(&self.nonce);
            s.append(&self.gas_price);
            s.append(&self.gas_limit);
            s.append(&self.kind);
            s.append(&self.value);
            s.append(&self.input.as_ref());
            s.append(&chain_id);
            s.append(&0u8);
            s.append(&0u8);
        } else {
            s.begin_list(6);
            s.append(&self.nonce);
            s.append(&self.gas_price);
            s.append(&self.gas_limit);
            s.append(&self.kind);
            s.append(&self.value);
            s.append(&self.input.as_ref());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "fastrlp",
    derive(open_fastrlp::RlpEncodable, open_fastrlp::RlpDecodable)
)]
pub struct EIP1559TransactionRequest {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub access_list: Vec<AccessListItem>,
}

impl EIP1559TransactionRequest {
    pub fn hash(&self) -> H256 {
        let encoded = rlp::encode(self);
        let mut out = vec![0; 1 + encoded.len()];
        out[0] = 2;
        out[1..].copy_from_slice(&encoded);
        keccak256(&out)
    }
}

impl From<EIP1559SignedTransaction> for EIP1559TransactionRequest {
    fn from(t: EIP1559SignedTransaction) -> Self {
        Self {
            chain_id: t.chain_id,
            nonce: t.nonce,
            max_priority_fee_per_gas: t.max_priority_fee_per_gas,
            max_fee_per_gas: t.max_fee_per_gas,
            gas_limit: t.gas_limit,
            kind: t.kind,
            value: t.value,
            input: t.input,
            access_list: t.access_list.0,
        }
    }
}

impl Encodable for EIP1559TransactionRequest {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(9);
        s.append(&self.chain_id);
        s.append(&self.nonce);
        s.append(&self.max_priority_fee_per_gas);
        s.append(&self.max_fee_per_gas);
        s.append(&self.gas_limit);
        s.append(&self.kind);
        s.append(&self.value);
        s.append(&self.input.as_ref());
        s.append_list(&self.access_list);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignedTransaction {
    /// Legacy transaction type
    Legacy(LegacySignedTransaction),
    /// EIP-2930 transaction
    EIP2930(EIP2930SignedTransaction),
    /// EIP-1559 transaction
    EIP1559(EIP1559SignedTransaction),
}

impl SignedTransaction {
    pub fn gas_price(&self) -> U256 {
        match self {
            SignedTransaction::Legacy(tx) => tx.gas_price,
            SignedTransaction::EIP2930(tx) => tx.gas_price,
            SignedTransaction::EIP1559(tx) => tx.max_fee_per_gas,
        }
    }

    pub fn gas_limit(&self) -> u64 {
        match self {
            SignedTransaction::Legacy(tx) => tx.gas_limit,
            SignedTransaction::EIP2930(tx) => tx.gas_limit,
            SignedTransaction::EIP1559(tx) => tx.gas_limit,
        }
    }

    pub fn value(&self) -> U256 {
        match self {
            SignedTransaction::Legacy(tx) => tx.value,
            SignedTransaction::EIP2930(tx) => tx.value,
            SignedTransaction::EIP1559(tx) => tx.value,
        }
    }

    pub fn data(&self) -> &Bytes {
        match self {
            SignedTransaction::Legacy(tx) => &tx.input,
            SignedTransaction::EIP2930(tx) => &tx.input,
            SignedTransaction::EIP1559(tx) => &tx.input,
        }
    }

    /// Max cost of the transaction
    pub fn max_cost(&self) -> U256 {
        U256::from(self.gas_limit()).saturating_mul(self.gas_price())
    }

    /// Returns a helper type that contains commonly used values as fields
    pub fn essentials(&self) -> TransactionEssentials {
        match self {
            SignedTransaction::Legacy(t) => TransactionEssentials {
                kind: t.kind,
                input: t.input.clone(),
                nonce: t.nonce,
                gas_limit: t.gas_limit,
                gas_price: Some(t.gas_price),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                value: t.value,
                chain_id: t.chain_id(),
                access_list: Default::default(),
            },
            SignedTransaction::EIP2930(t) => TransactionEssentials {
                kind: t.kind,
                input: t.input.clone(),
                nonce: t.nonce,
                gas_limit: t.gas_limit,
                gas_price: Some(t.gas_price),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                value: t.value,
                chain_id: Some(t.chain_id),
                access_list: t.access_list.clone(),
            },
            SignedTransaction::EIP1559(t) => TransactionEssentials {
                kind: t.kind,
                input: t.input.clone(),
                nonce: t.nonce,
                gas_limit: t.gas_limit,
                gas_price: None,
                max_fee_per_gas: Some(t.max_fee_per_gas),
                max_priority_fee_per_gas: Some(t.max_priority_fee_per_gas),
                value: t.value,
                chain_id: Some(t.chain_id),
                access_list: t.access_list.clone(),
            },
        }
    }

    pub fn nonce(&self) -> &u64 {
        match self {
            SignedTransaction::Legacy(t) => t.nonce(),
            SignedTransaction::EIP2930(t) => t.nonce(),
            SignedTransaction::EIP1559(t) => t.nonce(),
        }
    }

    pub fn chain_id(&self) -> Option<u64> {
        match self {
            SignedTransaction::Legacy(t) => t.chain_id(),
            SignedTransaction::EIP2930(t) => Some(t.chain_id),
            SignedTransaction::EIP1559(t) => Some(t.chain_id),
        }
    }

    pub fn as_legacy(&self) -> Option<&LegacySignedTransaction> {
        match self {
            SignedTransaction::Legacy(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns true whether this tx is a legacy transaction
    pub fn is_legacy(&self) -> bool {
        matches!(self, SignedTransaction::Legacy(_))
    }

    /// Returns true whether this tx is a EIP1559 transaction
    pub fn is_eip1559(&self) -> bool {
        matches!(self, SignedTransaction::EIP1559(_))
    }

    pub fn hash(&self) -> H256 {
        match self {
            SignedTransaction::Legacy(t) => t.hash(),
            SignedTransaction::EIP2930(t) => t.hash(),
            SignedTransaction::EIP1559(t) => t.hash(),
        }
    }

    /// Recovers the Ethereum address which was used to sign the transaction.
    pub fn recover(&self) -> Result<Address, SignatureError> {
        match self {
            SignedTransaction::Legacy(tx) => tx.recover(),
            SignedTransaction::EIP2930(tx) => tx.recover(),
            SignedTransaction::EIP1559(tx) => tx.recover(),
        }
    }

    /// Returns what kind of transaction this is
    pub fn kind(&self) -> &TransactionKind {
        match self {
            SignedTransaction::Legacy(tx) => &tx.kind,
            SignedTransaction::EIP2930(tx) => &tx.kind,
            SignedTransaction::EIP1559(tx) => &tx.kind,
        }
    }

    /// Returns the callee if this transaction is a call
    pub fn to(&self) -> Option<&Address> {
        self.kind().as_call()
    }

    /// Returns the Signature of the transaction
    pub fn signature(&self) -> Signature {
        match self {
            SignedTransaction::Legacy(tx) => tx.signature,
            SignedTransaction::EIP2930(tx) => {
                let v = tx.odd_y_parity as u8;
                let r = U256::from_be_bytes(tx.r.0);
                let s = U256::from_be_bytes(tx.s.0);
                Signature { r, s, v: v.into() }
            }
            SignedTransaction::EIP1559(tx) => {
                let v = tx.odd_y_parity as u8;
                let r = U256::from_be_bytes(tx.r.0);
                let s = U256::from_be_bytes(tx.s.0);
                Signature { r, s, v: v.into() }
            }
        }
    }
}

impl Encodable for SignedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        match self {
            SignedTransaction::Legacy(tx) => tx.rlp_append(s),
            SignedTransaction::EIP2930(tx) => enveloped(1, tx, s),
            SignedTransaction::EIP1559(tx) => enveloped(2, tx, s),
        }
    }
}

impl Decodable for SignedTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        let data = rlp.data()?;
        let first = *data.first().ok_or(DecoderError::Custom("empty slice"))?;
        if rlp.is_list() {
            return Ok(SignedTransaction::Legacy(rlp.as_val()?));
        }
        let s = data.get(1..).ok_or(DecoderError::Custom("no tx body"))?;
        if first == 0x01 {
            return rlp::decode(s).map(SignedTransaction::EIP2930);
        }
        if first == 0x02 {
            return rlp::decode(s).map(SignedTransaction::EIP1559);
        }
        Err(DecoderError::Custom("invalid tx type"))
    }
}

#[cfg(feature = "fastrlp")]
impl open_fastrlp::Encodable for SignedTransaction {
    fn length(&self) -> usize {
        match self {
            SignedTransaction::Legacy(tx) => tx.length(),
            tx => {
                let payload_len = match tx {
                    SignedTransaction::EIP2930(tx) => tx.length() + 1,
                    SignedTransaction::EIP1559(tx) => tx.length() + 1,
                    _ => unreachable!("legacy tx length already matched"),
                };
                // we include a string header for signed types txs, so include the length here
                payload_len + open_fastrlp::length_of_length(payload_len)
            }
        }
    }
    fn encode(&self, out: &mut dyn open_fastrlp::BufMut) {
        match self {
            SignedTransaction::Legacy(tx) => tx.encode(out),
            tx => {
                let payload_len = match tx {
                    SignedTransaction::EIP2930(tx) => tx.length() + 1,
                    SignedTransaction::EIP1559(tx) => tx.length() + 1,
                    _ => unreachable!("legacy tx length already matched"),
                };

                match tx {
                    SignedTransaction::EIP2930(tx) => {
                        let tx_string_header = open_fastrlp::Header {
                            list: false,
                            payload_length: payload_len,
                        };

                        tx_string_header.encode(out);
                        out.put_u8(0x01);
                        tx.encode(out);
                    }
                    SignedTransaction::EIP1559(tx) => {
                        let tx_string_header = open_fastrlp::Header {
                            list: false,
                            payload_length: payload_len,
                        };

                        tx_string_header.encode(out);
                        out.put_u8(0x02);
                        tx.encode(out);
                    }
                    _ => unreachable!("legacy tx encode already matched"),
                }
            }
        }
    }
}

#[cfg(feature = "fastrlp")]
impl open_fastrlp::Decodable for SignedTransaction {
    fn decode(buf: &mut &[u8]) -> Result<Self, open_fastrlp::DecodeError> {
        use bytes::Buf;
        use std::cmp::Ordering;

        let first = *buf
            .first()
            .ok_or(open_fastrlp::DecodeError::Custom("empty slice"))?;

        // a signed transaction is either encoded as a string (non legacy) or a list (legacy).
        // We should not consume the buffer if we are decoding a legacy transaction, so let's
        // check if the first byte is between 0x80 and 0xbf.
        match first.cmp(&open_fastrlp::EMPTY_LIST_CODE) {
            Ordering::Less => {
                // strip out the string header
                // NOTE: typed transaction encodings either contain a "rlp header" which contains
                // the type of the payload and its length, or they do not contain a header and
                // start with the tx type byte.
                //
                // This line works for both types of encodings because byte slices starting with
                // 0x01 and 0x02 return a Header { list: false, payload_length: 1 } when input to
                // Header::decode.
                // If the encoding includes a header, the header will be properly decoded and
                // consumed.
                // Otherwise, header decoding will succeed but nothing is consumed.
                let _header = open_fastrlp::Header::decode(buf)?;
                let tx_type = *buf.first().ok_or(open_fastrlp::DecodeError::Custom(
                    "typed tx cannot be decoded from an empty slice",
                ))?;
                if tx_type == 0x01 {
                    buf.advance(1);
                    <EIP2930SignedTransaction as open_fastrlp::Decodable>::decode(buf)
                        .map(SignedTransaction::EIP2930)
                } else if tx_type == 0x02 {
                    buf.advance(1);
                    <EIP1559SignedTransaction as open_fastrlp::Decodable>::decode(buf)
                        .map(SignedTransaction::EIP1559)
                } else {
                    Err(open_fastrlp::DecodeError::Custom("invalid tx type"))
                }
            }
            Ordering::Equal => Err(open_fastrlp::DecodeError::Custom(
                "an empty list is not a valid transaction encoding",
            )),
            Ordering::Greater => <LegacySignedTransaction as open_fastrlp::Decodable>::decode(buf)
                .map(SignedTransaction::Legacy),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "fastrlp",
    derive(open_fastrlp::RlpEncodable, open_fastrlp::RlpDecodable)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LegacySignedTransaction {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub signature: Signature,
}

impl LegacySignedTransaction {
    pub fn nonce(&self) -> &u64 {
        &self.nonce
    }

    pub fn hash(&self) -> H256 {
        keccak256(&rlp::encode(self))
    }

    /// Recovers the Ethereum address which was used to sign the transaction.
    pub fn recover(&self) -> Result<Address, SignatureError> {
        self.signature
            .recover(LegacyTransactionRequest::from(self.clone()).hash())
    }

    pub fn chain_id(&self) -> Option<u64> {
        if self.signature.v > 36 {
            Some((self.signature.v - 35) / 2)
        } else {
            None
        }
    }

    /// See <https://github.com/ethereum/EIPs/blob/master/EIPS/eip-155.md>
    /// > If you do, then the v of the signature MUST be set to {0,1} + CHAIN_ID * 2 + 35 where
    /// > {0,1} is the parity of the y value of the curve point for which r is the x-value in the
    /// > secp256k1 signing process.
    pub fn meets_eip155(&self, chain_id: u64) -> bool {
        let double_chain_id = chain_id.saturating_mul(2);
        let v = self.signature.v;
        v == double_chain_id + 35 || v == double_chain_id + 36
    }
}

impl Encodable for LegacySignedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(9);
        s.append(&self.nonce);
        s.append(&self.gas_price);
        s.append(&self.gas_limit);
        s.append(&self.kind);
        s.append(&self.value);
        s.append(&self.input.as_ref());
        s.append(&self.signature.v);
        s.append(&self.signature.r);
        s.append(&self.signature.s);
    }
}

impl Decodable for LegacySignedTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 9 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        let v = rlp.val_at(6)?;
        let r = rlp.val_at::<U256>(7)?;
        let s = rlp.val_at::<U256>(8)?;

        Ok(Self {
            nonce: rlp.val_at(0)?,
            gas_price: rlp.val_at(1)?,
            gas_limit: rlp.val_at(2)?,
            kind: rlp.val_at(3)?,
            value: rlp.val_at(4)?,
            input: rlp.val_at::<Vec<u8>>(5)?.into(),
            signature: Signature { v, r, s },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "fastrlp",
    derive(open_fastrlp::RlpEncodable, open_fastrlp::RlpDecodable)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EIP2930SignedTransaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub access_list: AccessList,
    pub odd_y_parity: bool,
    pub r: H256,
    pub s: H256,
}

impl EIP2930SignedTransaction {
    pub fn nonce(&self) -> &u64 {
        &self.nonce
    }

    pub fn hash(&self) -> H256 {
        let encoded = rlp::encode(self);
        let mut out = vec![0; 1 + encoded.len()];
        out[0] = 1;
        out[1..].copy_from_slice(&encoded);
        keccak256(&out)
    }

    /// Recovers the Ethereum address which was used to sign the transaction.
    pub fn recover(&self) -> Result<Address, SignatureError> {
        let mut sig = [0u8; 65];
        sig[0..32].copy_from_slice(&self.r[..]);
        sig[32..64].copy_from_slice(&self.s[..]);
        sig[64] = self.odd_y_parity as u8;
        let signature = Signature::try_from(&sig[..])?;
        signature.recover(EIP2930TransactionRequest::from(self.clone()).hash())
    }
}

impl rlp::Encodable for EIP2930SignedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(11);
        s.append(&self.chain_id);
        s.append(&self.nonce);
        s.append(&self.gas_price);
        s.append(&self.gas_limit);
        s.append(&self.kind);
        s.append(&self.value);
        s.append(&self.input.as_ref());
        s.append(&self.access_list);
        s.append(&self.odd_y_parity);
        s.append(&U256::from_be_bytes(self.r.0));
        s.append(&U256::from_be_bytes(self.s.0));
    }
}

impl rlp::Decodable for EIP2930SignedTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 11 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(Self {
            chain_id: rlp.val_at(0)?,
            nonce: rlp.val_at(1)?,
            gas_price: rlp.val_at(2)?,
            gas_limit: rlp.val_at(3)?,
            kind: rlp.val_at(4)?,
            value: rlp.val_at(5)?,
            input: rlp.val_at::<Vec<u8>>(6)?.into(),
            access_list: rlp.val_at(7)?,
            odd_y_parity: rlp.val_at(8)?,
            r: {
                let rarr = rlp.val_at::<U256>(9)?.to_be_bytes();
                H256::from(rarr)
            },
            s: {
                let sarr = rlp.val_at::<U256>(10)?.to_be_bytes();
                H256::from(sarr)
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "fastrlp",
    derive(open_fastrlp::RlpEncodable, open_fastrlp::RlpDecodable)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EIP1559SignedTransaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub kind: TransactionKind,
    pub value: U256,
    pub input: Bytes,
    pub access_list: AccessList,
    pub odd_y_parity: bool,
    pub r: H256,
    pub s: H256,
}

impl EIP1559SignedTransaction {
    pub fn nonce(&self) -> &u64 {
        &self.nonce
    }

    pub fn hash(&self) -> H256 {
        let encoded = rlp::encode(self);
        let mut out = vec![0; 1 + encoded.len()];
        out[0] = 2;
        out[1..].copy_from_slice(&encoded);
        keccak256(&out)
    }

    /// Recovers the Ethereum address which was used to sign the transaction.
    pub fn recover(&self) -> Result<Address, SignatureError> {
        let mut sig = [0u8; 65];
        sig[0..32].copy_from_slice(&self.r[..]);
        sig[32..64].copy_from_slice(&self.s[..]);
        sig[64] = self.odd_y_parity as u8;
        let signature = Signature::try_from(&sig[..])?;
        signature.recover(EIP1559TransactionRequest::from(self.clone()).hash())
    }
}

impl Encodable for EIP1559SignedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(12);
        s.append(&self.chain_id);
        s.append(&self.nonce);
        s.append(&self.max_priority_fee_per_gas);
        s.append(&self.max_fee_per_gas);
        s.append(&self.gas_limit);
        s.append(&self.kind);
        s.append(&self.value);
        s.append(&self.input.as_ref());
        s.append(&self.access_list);
        s.append(&self.odd_y_parity);
        s.append(&U256::from_be_bytes(self.r.0));
        s.append(&U256::from_be_bytes(self.s.0));
    }
}

impl Decodable for EIP1559SignedTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 12 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(Self {
            chain_id: rlp.val_at(0)?,
            nonce: rlp.val_at(1)?,
            max_priority_fee_per_gas: rlp.val_at(2)?,
            max_fee_per_gas: rlp.val_at(3)?,
            gas_limit: rlp.val_at(4)?,
            kind: rlp.val_at(5)?,
            value: rlp.val_at(6)?,
            input: rlp.val_at::<Vec<u8>>(7)?.into(),
            access_list: rlp.val_at(8)?,
            odd_y_parity: rlp.val_at(9)?,
            r: {
                let rarr = rlp.val_at::<U256>(10)?.to_be_bytes();
                H256::from(rarr)
            },
            s: {
                let sarr = rlp.val_at::<U256>(11)?.to_be_bytes();
                H256::from(sarr)
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionEssentials {
    pub kind: TransactionKind,
    pub input: Bytes,
    pub nonce: u64,
    pub gas_limit: u64,
    pub gas_price: Option<U256>,
    pub max_fee_per_gas: Option<U256>,
    pub max_priority_fee_per_gas: Option<U256>,
    pub value: U256,
    pub chain_id: Option<u64>,
    pub access_list: AccessList,
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    #[test]
    fn can_recover_sender() {
        let bytes = hex::decode("f85f800182520894095e7baea6a6c7c4c2dfeb977efac326af552d870a801ba048b55bfa915ac795c431978d8a6a992b628d557da5ff759b307d495a36649353a0efffd310ac743f371de3b9f7f9cb56c0b28ad43601b4ab949f53faa07bd2c804").unwrap();

        let tx: SignedTransaction = rlp::decode(&bytes).expect("decoding TypedTransaction failed");
        let tx = match tx {
            SignedTransaction::Legacy(tx) => tx,
            _ => panic!("Invalid typed transaction"),
        };
        assert_eq!(tx.input, Bytes::new());
        assert_eq!(tx.gas_price, U256::from(0x01u64));
        assert_eq!(tx.gas_limit, 0x5208u64);
        assert_eq!(tx.nonce, 0x00u64);
        if let TransactionKind::Call(ref to) = tx.kind {
            assert_eq!(
                *to,
                "095e7baea6a6c7c4c2dfeb977efac326af552d87".parse().unwrap()
            );
        } else {
            panic!();
        }
        assert_eq!(tx.value, U256::from(0x0au64));
        assert_eq!(
            tx.recover().unwrap(),
            "0f65fe9276bc9a24ae7083ae28e2660ef72df99e".parse().unwrap()
        );
    }

    #[test]
    #[cfg(feature = "fastrlp")]
    fn test_decode_fastrlp_create() {
        use bytes::BytesMut;
        use open_fastrlp::Encodable;

        // tests that a contract creation tx encodes and decodes properly

        let tx = SignedTransaction::EIP2930(EIP2930SignedTransaction {
            chain_id: 1u64,
            nonce: 0,
            gas_price: U256::from(1),
            gas_limit: 2,
            kind: TransactionKind::Create,
            value: U256::from(3),
            input: Bytes::from(vec![1, 2]),
            odd_y_parity: true,
            r: H256::default(),
            s: H256::default(),
            access_list: vec![].into(),
        });

        let mut encoded = BytesMut::new();
        tx.encode(&mut encoded);

        let decoded =
            <SignedTransaction as open_fastrlp::Decodable>::decode(&mut &*encoded).unwrap();
        assert_eq!(decoded, tx);
    }

    #[test]
    #[cfg(feature = "fastrlp")]
    fn test_decode_fastrlp_create_goerli() {
        // test that an example create tx from goerli decodes properly
        let tx_bytes =
              hex::decode("02f901ee05228459682f008459682f11830209bf8080b90195608060405234801561001057600080fd5b50610175806100206000396000f3fe608060405234801561001057600080fd5b506004361061002b5760003560e01c80630c49c36c14610030575b600080fd5b61003861004e565b604051610045919061011d565b60405180910390f35b60606020600052600f6020527f68656c6c6f2073746174656d696e64000000000000000000000000000000000060405260406000f35b600081519050919050565b600082825260208201905092915050565b60005b838110156100be5780820151818401526020810190506100a3565b838111156100cd576000848401525b50505050565b6000601f19601f8301169050919050565b60006100ef82610084565b6100f9818561008f565b93506101098185602086016100a0565b610112816100d3565b840191505092915050565b6000602082019050818103600083015261013781846100e4565b90509291505056fea264697066735822122051449585839a4ea5ac23cae4552ef8a96b64ff59d0668f76bfac3796b2bdbb3664736f6c63430008090033c080a0136ebffaa8fc8b9fda9124de9ccb0b1f64e90fbd44251b4c4ac2501e60b104f9a07eb2999eec6d185ef57e91ed099afb0a926c5b536f0155dd67e537c7476e1471")
                  .unwrap();
        let _decoded =
            <SignedTransaction as open_fastrlp::Decodable>::decode(&mut &tx_bytes[..]).unwrap();
    }

    #[test]
    #[cfg(feature = "fastrlp")]
    fn test_decode_fastrlp_call() {
        use bytes::BytesMut;
        use open_fastrlp::Encodable;

        let tx = SignedTransaction::EIP2930(EIP2930SignedTransaction {
            chain_id: 1u64,
            nonce: 0,
            gas_price: U256::from(1),
            gas_limit: 2,
            kind: TransactionKind::Call(Address::default()),
            value: U256::from(3),
            input: Bytes::from(vec![1, 2]),
            odd_y_parity: true,
            r: H256::default(),
            s: H256::default(),
            access_list: vec![].into(),
        });

        let mut encoded = BytesMut::new();
        tx.encode(&mut encoded);

        let decoded =
            <SignedTransaction as open_fastrlp::Decodable>::decode(&mut &*encoded).unwrap();
        assert_eq!(decoded, tx);
    }

    #[test]
    #[cfg(feature = "fastrlp")]
    fn decode_transaction_consumes_buffer() {
        let bytes = &mut &hex::decode("b87502f872041a8459682f008459682f0d8252089461815774383099e24810ab832a5b2a5425c154d58829a2241af62c000080c001a059e6b67f48fb32e7e570dfb11e042b5ad2e55e3ce3ce9cd989c7e06e07feeafda0016b83f4f980694ed2eee4d10667242b1f40dc406901b34125b008d334d47469").unwrap()[..];
        let _transaction_res =
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes).unwrap();
        assert_eq!(
            bytes.len(),
            0,
            "did not consume all bytes in the buffer, {:?} remaining",
            bytes.len()
        );
    }

    #[test]
    #[cfg(feature = "fastrlp")]
    fn decode_multiple_network_txs() {
        use std::str::FromStr;

        let bytes_first = &mut &hex::decode("f86b02843b9aca00830186a094d3e8763675e4c425df46cc3b5c0f6cbdac39604687038d7ea4c68000802ba00eb96ca19e8a77102767a41fc85a36afd5c61ccb09911cec5d3e86e193d9c5aea03a456401896b1b6055311536bf00a718568c744d8c1f9df59879e8350220ca18").unwrap()[..];
        let expected = SignedTransaction::Legacy(LegacySignedTransaction {
            nonce: 2u64,
            gas_price: 1000000000u64.into(),
            gas_limit: 100000,
            kind: TransactionKind::Call(Address::from_slice(
                &hex::decode("d3e8763675e4c425df46cc3b5c0f6cbdac396046").unwrap()[..],
            )),
            value: 1000000000000000u64.into(),
            input: Bytes::default(),
            signature: Signature {
                v: 43,
                r: U256::from_str(
                    "eb96ca19e8a77102767a41fc85a36afd5c61ccb09911cec5d3e86e193d9c5ae",
                )
                .unwrap(),
                s: U256::from_str(
                    "3a456401896b1b6055311536bf00a718568c744d8c1f9df59879e8350220ca18",
                )
                .unwrap(),
            },
        });
        assert_eq!(
            expected,
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes_first).unwrap()
        );

        let bytes_second = &mut &hex::decode("f86b01843b9aca00830186a094d3e8763675e4c425df46cc3b5c0f6cbdac3960468702769bb01b2a00802ba0e24d8bd32ad906d6f8b8d7741e08d1959df021698b19ee232feba15361587d0aa05406ad177223213df262cb66ccbb2f46bfdccfdfbbb5ffdda9e2c02d977631da").unwrap()[..];
        let expected = SignedTransaction::Legacy(LegacySignedTransaction {
            nonce: 1,
            gas_price: 1000000000u64.into(),
            gas_limit: 100000,
            kind: TransactionKind::Call(Address::from_slice(
                &hex::decode("d3e8763675e4c425df46cc3b5c0f6cbdac396046").unwrap()[..],
            )),
            value: 693361000000000u64.into(),
            input: Bytes::default(),
            signature: Signature {
                v: 43,
                r: U256::from_str(
                    "e24d8bd32ad906d6f8b8d7741e08d1959df021698b19ee232feba15361587d0a",
                )
                .unwrap(),
                s: U256::from_str(
                    "5406ad177223213df262cb66ccbb2f46bfdccfdfbbb5ffdda9e2c02d977631da",
                )
                .unwrap(),
            },
        });
        assert_eq!(
            expected,
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes_second).unwrap()
        );

        let bytes_third = &mut &hex::decode("f86b0384773594008398968094d3e8763675e4c425df46cc3b5c0f6cbdac39604687038d7ea4c68000802ba0ce6834447c0a4193c40382e6c57ae33b241379c5418caac9cdc18d786fd12071a03ca3ae86580e94550d7c071e3a02eadb5a77830947c9225165cf9100901bee88").unwrap()[..];
        let expected = SignedTransaction::Legacy(LegacySignedTransaction {
            nonce: 3,
            gas_price: 2000000000u64.into(),
            gas_limit: 10000000,
            kind: TransactionKind::Call(Address::from_slice(
                &hex::decode("d3e8763675e4c425df46cc3b5c0f6cbdac396046").unwrap()[..],
            )),
            value: 1000000000000000u64.into(),
            input: Bytes::default(),
            signature: Signature {
                v: 43,
                r: U256::from_str(
                    "ce6834447c0a4193c40382e6c57ae33b241379c5418caac9cdc18d786fd12071",
                )
                .unwrap(),
                s: U256::from_str(
                    "3ca3ae86580e94550d7c071e3a02eadb5a77830947c9225165cf9100901bee88",
                )
                .unwrap(),
            },
        });
        assert_eq!(
            expected,
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes_third).unwrap()
        );

        let bytes_fourth = &mut &hex::decode("b87502f872041a8459682f008459682f0d8252089461815774383099e24810ab832a5b2a5425c154d58829a2241af62c000080c001a059e6b67f48fb32e7e570dfb11e042b5ad2e55e3ce3ce9cd989c7e06e07feeafda0016b83f4f980694ed2eee4d10667242b1f40dc406901b34125b008d334d47469").unwrap()[..];
        let expected = SignedTransaction::EIP1559(EIP1559SignedTransaction {
            chain_id: 4,
            nonce: 26,
            max_priority_fee_per_gas: 1500000000u64.into(),
            max_fee_per_gas: 1500000013u64.into(),
            gas_limit: 21000,
            kind: TransactionKind::Call(Address::from_slice(
                &hex::decode("61815774383099e24810ab832a5b2a5425c154d5").unwrap()[..],
            )),
            value: 3000000000000000000u64.into(),
            input: Bytes::default(),
            access_list: AccessList::default(),
            odd_y_parity: true,
            r: H256::from_str("59e6b67f48fb32e7e570dfb11e042b5ad2e55e3ce3ce9cd989c7e06e07feeafd")
                .unwrap(),
            s: H256::from_str("016b83f4f980694ed2eee4d10667242b1f40dc406901b34125b008d334d47469")
                .unwrap(),
        });
        assert_eq!(
            expected,
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes_fourth).unwrap()
        );

        let bytes_fifth = &mut &hex::decode("f8650f84832156008287fb94cf7f9e66af820a19257a2108375b180b0ec491678204d2802ca035b7bfeb9ad9ece2cbafaaf8e202e706b4cfaeb233f46198f00b44d4a566a981a0612638fb29427ca33b9a3be2a0a561beecfe0269655be160d35e72d366a6a860").unwrap()[..];
        let expected = SignedTransaction::Legacy(LegacySignedTransaction {
            nonce: 15u64,
            gas_price: 2200000000u64.into(),
            gas_limit: 34811,
            kind: TransactionKind::Call(Address::from_slice(
                &hex::decode("cf7f9e66af820a19257a2108375b180b0ec49167").unwrap()[..],
            )),
            value: 1234u64.into(),
            input: Bytes::default(),
            signature: Signature {
                v: 44,
                r: U256::from_str(
                    "35b7bfeb9ad9ece2cbafaaf8e202e706b4cfaeb233f46198f00b44d4a566a981",
                )
                .unwrap(),
                s: U256::from_str(
                    "612638fb29427ca33b9a3be2a0a561beecfe0269655be160d35e72d366a6a860",
                )
                .unwrap(),
            },
        });
        assert_eq!(
            expected,
            <SignedTransaction as open_fastrlp::Decodable>::decode(bytes_fifth).unwrap()
        );
    }

    // <https://github.com/gakonst/ethers-rs/issues/1732>
    #[test]
    fn test_recover_legacy_tx() {
        let raw_tx = "f9015482078b8505d21dba0083022ef1947a250d5630b4cf539739df2c5dacb4c659f2488d880c46549a521b13d8b8e47ff36ab50000000000000000000000000000000000000000000066ab5a608bd00a23f2fe000000000000000000000000000000000000000000000000000000000000008000000000000000000000000048c04ed5691981c42154c6167398f95e8f38a7ff00000000000000000000000000000000000000000000000000000000632ceac70000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006c6ee5e31d828de241282b9606c8e98ea48526e225a0c9077369501641a92ef7399ff81c21639ed4fd8fc69cb793cfa1dbfab342e10aa0615facb2f1bcf3274a354cfe384a38d0cc008a11c2dd23a69111bc6930ba27a8";

        let tx: SignedTransaction = rlp::decode(&hex::decode(raw_tx).unwrap()).unwrap();
        let recovered = tx.recover().unwrap();
        let expected: Address = "0xa12e1462d0ced572f396f58b6e2d03894cd7c8a4"
            .parse()
            .unwrap();
        assert_eq!(expected, recovered);
    }
}