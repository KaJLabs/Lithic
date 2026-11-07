//! Opt-in bank-backed state. Never applies an external balance update itself.
use crate::Address;
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

pub(super) const MAX_BALANCES: usize = 128;
pub(super) type ReadBalance = unsafe extern "C" fn(usize, *const u8, *mut u8) -> i32;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BalanceChange {
    address: String,
    before: String,
    after: String,
}

pub(super) struct Bank {
    read: ReadBalance,
    context: usize,
    originals: RefCell<BTreeMap<Address, [u8; 32]>>,
    pub committed: RefCell<Vec<BalanceChange>>,
}

impl Bank {
    pub fn new(read: ReadBalance, context: usize) -> Self {
        Self {
            read,
            context,
            originals: RefCell::new(BTreeMap::new()),
            committed: RefCell::new(Vec::new()),
        }
    }

    pub fn balance(&self, address: &Address) -> Result<[u8; 32], String> {
        if address[..12] != [0; 12] {
            return Err("noncanonical bank address".into());
        }
        if let Some(value) = self.originals.borrow().get(address) {
            return Ok(*value);
        }
        if self.originals.borrow().len() >= MAX_BALANCES {
            return Err("bank footprint exceeds limit".into());
        }
        let mut value = [0; 32];
        // Fixed-size synchronous pointers: address20 and output word32. The
        // embedding callback validates the address and meters the SDK reads.
        if unsafe { (self.read)(self.context, address[12..].as_ptr(), value.as_mut_ptr()) } != 1 {
            return Err("bank balance read failed".into());
        }
        self.originals.borrow_mut().insert(*address, value);
        Ok(value)
    }

    pub fn commit(&self, staged: &BTreeMap<Address, [u8; 32]>) -> Result<(), String> {
        if staged.len() > MAX_BALANCES {
            return Err("bank footprint exceeds limit".into());
        }
        let mut before_total = [0; 33];
        let mut after_total = [0; 33];
        let mut changes = Vec::new();
        for (address, after) in staged {
            let before = self.balance(address)?;
            add(&mut before_total, &before);
            add(&mut after_total, after);
            if before != *after {
                changes.push(BalanceChange {
                    address: crate::hex(&address[12..]),
                    before: crate::hex(&before),
                    after: crate::hex(after),
                });
            }
        }
        if before_total != after_total {
            return Err("native bank balance conservation failed".into());
        }
        *self.committed.borrow_mut() = changes;
        Ok(())
    }
}

// The footprint bound permits at most 128 uint256 words; a 264-bit accumulator
// cannot overflow and never rejects a legitimate sum merely exceeding uint256.
fn add(total: &mut [u8; 33], value: &[u8; 32]) {
    let mut carry = 0u16;
    for index in (0..32).rev() {
        let next = total[index + 1] as u16 + value[index] as u16 + carry;
        total[index + 1] = next as u8;
        carry = next >> 8;
    }
    total[0] = total[0].checked_add(carry as u8).expect("bounded bank sum");
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn maximum(_: usize, _: *const u8, out: *mut u8) -> i32 {
        std::ptr::write_bytes(out, 255, 32);
        1
    }
    fn address(n: u8) -> Address {
        let mut a = [0; 32];
        a[31] = n;
        a
    }
    #[test]
    fn wide_conservation_and_bounded_footprint_reject_mint_and_burn() {
        let bank = Bank::new(maximum, 0);
        let mut staged = BTreeMap::new();
        for n in 0..128 {
            staged.insert(address(n), [255; 32]);
        }
        // The sum is greater than uint256; valid unchanged state must work.
        bank.commit(&staged).unwrap();
        assert!(bank.committed.borrow().is_empty());
        staged.get_mut(&address(0)).unwrap()[31] = 254;
        assert!(bank.commit(&staged).unwrap_err().contains("conservation"));
        assert!(bank.committed.borrow().is_empty());
        assert!(bank
            .balance(&address(128))
            .unwrap_err()
            .contains("footprint"));
        staged.insert(address(128), [255; 32]);
        assert!(bank.commit(&staged).unwrap_err().contains("footprint"));
        let mut alias = address(0);
        alias[0] = 1;
        assert!(bank.balance(&alias).unwrap_err().contains("noncanonical"));
    }
    #[test]
    fn conservation_uses_more_than_the_low_256_bits() {
        // 2 * (2^256 - 1) differs from 2^256 - 2 by exactly 2^256.
        let mut before = [0; 33];
        add(&mut before, &[255; 32]);
        add(&mut before, &[255; 32]);
        let mut after = [0; 33];
        let mut low = [255; 32];
        low[31] = 254;
        add(&mut after, &low);
        assert_eq!(before[1..], after[1..]);
        assert_ne!(before, after);
    }

    #[test]
    fn zero_bank_cannot_mint_even_one_unit() {
        unsafe extern "C" fn zero(_: usize, _: *const u8, out: *mut u8) -> i32 {
            std::ptr::write_bytes(out, 0, 32);
            1
        }
        let bank = Bank::new(zero, 0);
        let mut one = [0; 32];
        one[31] = 1;
        assert!(bank
            .commit(&BTreeMap::from([(address(1), one)]))
            .unwrap_err()
            .contains("conservation"));
        assert!(bank.committed.borrow().is_empty());
    }
}
