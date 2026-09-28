//! Native account records in consensus state (clients v1, E04 gap 8). Each
//! account's liquid balances and nonce are two state entries a client reads
//! through `/state/proof/{key}`. The chain writes them with bincode 1
//! (fixed-width little-endian integers, u64 lengths). These decoders share
//! no code with bincode and refuse any other encoding; the node's tests
//! check both agree.
use crate::address::AccountAddress;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

/// Liquid balances by denomination (`udgt`, `udrt`). The chain removes a
/// denomination whose balance reaches zero.
pub fn balances_key(address: &AccountAddress) -> Vec<u8> {
    format!("acct:balances:{}", address.encode()).into_bytes()
}
/// The account's native nonce, mirrored by its recovery record once it has one.
pub fn nonce_key(address: &AccountAddress) -> Vec<u8> {
    format!("acct:nonce:{}", address.encode()).into_bytes()
}

struct Input<'a>(&'a [u8]);
impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.0.len() >= n, "Native account record is truncated");
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(self.take(16)?.try_into()?))
    }
    fn finish(self) -> Result<()> {
        ensure!(
            self.0.is_empty(),
            "Native account record has trailing bytes"
        );
        Ok(())
    }
}

/// Decode a balance record: a count, then denomination and amount pairs in
/// strictly ascending denomination order.
pub fn decode_balances(bytes: &[u8]) -> Result<BTreeMap<String, u128>> {
    let mut input = Input(bytes);
    let count = input.u64()?;
    // Each entry takes at least 24 bytes; refuse a count the input cannot hold.
    ensure!(
        count <= (bytes.len() / 24) as u64,
        "Native balance count exceeds the record"
    );
    let mut balances = BTreeMap::new();
    let mut last: Option<String> = None;
    for _ in 0..count {
        let length = usize::try_from(input.u64()?).context("Denomination length overflow")?;
        let denomination = std::str::from_utf8(input.take(length)?)
            .context("Denomination is not UTF-8")?
            .to_owned();
        ensure!(
            last.as_deref()
                .is_none_or(|previous| previous < denomination.as_str()),
            "Native balances are not in ascending denomination order"
        );
        let amount = input.u128()?;
        last = Some(denomination.clone());
        balances.insert(denomination, amount);
    }
    input.finish()?;
    Ok(balances)
}

pub fn decode_nonce(bytes: &[u8]) -> Result<u64> {
    let mut input = Input(bytes);
    let nonce = input.u64()?;
    input.finish()?;
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_decode_exactly_and_refuse_other_encodings() {
        let mut record = 2u64.to_le_bytes().to_vec();
        for (name, amount) in [("udgt", 5u128), ("udrt", u128::MAX)] {
            record.extend((name.len() as u64).to_le_bytes());
            record.extend(name.as_bytes());
            record.extend(amount.to_le_bytes());
        }
        let balances = decode_balances(&record).unwrap();
        assert_eq!(balances["udgt"], 5);
        assert_eq!(balances["udrt"], u128::MAX);
        assert!(decode_balances(&[record.clone(), vec![0]].concat()).is_err());
        assert!(decode_balances(&record[..record.len() - 1]).is_err());
        assert_eq!(
            decode_balances(&0u64.to_le_bytes()).unwrap(),
            BTreeMap::new()
        );
        assert!(decode_balances(&u64::MAX.to_le_bytes()).is_err());
        // Descending order is not what the chain writes.
        let mut reversed = 2u64.to_le_bytes().to_vec();
        for (name, amount) in [("udrt", 1u128), ("udgt", 1u128)] {
            reversed.extend((name.len() as u64).to_le_bytes());
            reversed.extend(name.as_bytes());
            reversed.extend(amount.to_le_bytes());
        }
        assert!(decode_balances(&reversed).is_err());
        assert_eq!(decode_nonce(&7u64.to_le_bytes()).unwrap(), 7);
        assert!(decode_nonce(&[0; 7]).is_err());
        assert!(decode_nonce(&[0; 9]).is_err());
    }
}
