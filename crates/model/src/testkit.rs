//! A small book for unit tests of the ledger, the classifier and the reports. The full sample household is in the
//! engine (`sample.rs`). From budget.rs `ledger/testkit.rs`.

use std::sync::Arc;

use chrono::NaiveDate;

use crate::book::{Book, BookMeta};
use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::*;

pub const BETAAL: AccountId = AccountId(1);
pub const BETAAL_FREE: AccountId = AccountId(2);
pub const BILLS: AccountId = AccountId(3);
pub const SPAAR: AccountId = AccountId(4);
pub const SPAAR_FREE: AccountId = AccountId(5);
pub const HOLIDAY: AccountId = AccountId(6);
pub const WALLET: AccountId = AccountId(7);
pub const WALLET_FREE: AccountId = AccountId(8);
pub const CARD: AccountId = AccountId(9);
pub const MORTGAGE: AccountId = AccountId(10);
pub const DAILY: AccountId = AccountId(20);
pub const GROCERIES: AccountId = AccountId(21);
pub const INTEREST: AccountId = AccountId(22);
pub const INCOME: AccountId = AccountId(30);
pub const SALARY: AccountId = AccountId(31);
pub const STARTING: AccountId = AccountId(40);
pub const NOT_SORTED: AccountId = AccountId(41);
pub const VALUE_CHANGES: AccountId = AccountId(42);

pub fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

pub fn acc(id: AccountId, group: Group, kind: AccountKind, parent: Option<AccountId>, name: &str) -> Account {
    Account {
        id,
        group,
        kind,
        parent,
        name: name.into(),
        description: String::new(),
        iban: None,
        unassigned: false,
        system: None,
        goal: None,
        goal_date: None,
        sort: id.0 as i32,
        closed: false,
    }
}

pub fn accounts() -> Vec<Account> {
    use AccountKind::*;
    use Group::*;
    let unassigned = |id, parent| Account { unassigned: true, ..acc(id, Have, Pot, Some(parent), "Unassigned") };
    let system = |id, key: SystemAccount| Account { system: Some(key), ..acc(id, Equity, System, None, key.name()) };
    vec![
        Account { iban: Some("NL12INGB0001234567".into()), ..acc(BETAAL, Have, Current, None, "ING Betaalrekening") },
        unassigned(BETAAL_FREE, BETAAL),
        acc(BILLS, Have, Pot, Some(BETAAL), "Bills buffer"),
        Account { iban: Some("NL34INGB0007654321".into()), ..acc(SPAAR, Have, Savings, None, "ING Oranje Spaar") },
        unassigned(SPAAR_FREE, SPAAR),
        Account { goal: Some(Money(250_000)), goal_date: Some(d(2027, 6, 30)), ..acc(HOLIDAY, Have, Pot, Some(SPAAR), "Holiday") },
        acc(WALLET, Have, Cash, None, "Wallet"),
        unassigned(WALLET_FREE, WALLET),
        acc(CARD, Owe, CreditCard, None, "ABN AMRO Creditcard"),
        acc(MORTGAGE, Owe, Loan, None, "Mortgage"),
        acc(DAILY, Goes, CategoryGroup, None, "Daily"),
        acc(GROCERIES, Goes, Category, Some(DAILY), "Groceries"),
        acc(INTEREST, Goes, Category, Some(DAILY), "Mortgage interest"),
        acc(INCOME, Comes, CategoryGroup, None, "Income"),
        acc(SALARY, Comes, Category, Some(INCOME), "Salary"),
        system(STARTING, SystemAccount::StartingBalance),
        system(NOT_SORTED, SystemAccount::NotSortedYet),
        system(VALUE_CHANGES, SystemAccount::ValueChanges),
    ]
}

pub fn tree() -> AccountTree {
    AccountTree::new(accounts().into_iter().map(Arc::new))
}

/// The test accounts as a book with these transactions.
pub fn book(txns: Vec<Txn>) -> Book {
    let meta = BookMeta { name: "Test".into(), ..BookMeta::default() };
    let (b, notes) = Book::from_parts(meta, accounts(), txns, Vec::new());
    assert!(notes.is_empty(), "{notes:?}");
    b
}

/// The transactions as the reports take them.
pub fn refs(txns: &[Txn]) -> Vec<&Txn> {
    txns.iter().collect()
}

pub fn p(account: AccountId, cents: i64) -> Posting {
    Posting::new(account, Money(cents))
}

/// A transaction with defaults: reviewed, no payee. `source` makes it a bank transaction of that account with the sum
/// of its postings there as the bank amount.
pub fn txn(id: u64, date: NaiveDate, source: Option<AccountId>, payee: &str, postings: &[(AccountId, i64)]) -> Txn {
    let postings: Vec<Posting> = postings.iter().map(|(a, c)| p(*a, *c)).collect();
    let t = tree();
    let bank_amount = source.map(|s| postings.iter().filter(|x| t.physical_of(x.account) == Some(s)).map(|x| x.amount).sum());
    Txn {
        id: TxnId(id),
        kind: if source.is_some() { TxnKind::Bank } else { TxnKind::Manual },
        date,
        payee: payee.into(),
        payee_norm: payee.to_uppercase(),
        description: String::new(),
        counterparty_iban: None,
        note: String::new(),
        account: source,
        bank_amount,
        source: if source.is_some() { SourceKind::Csv } else { SourceKind::Manual },
        import: None,
        status: TxnStatus::Reviewed,
        class_source: None,
        guesses: vec![],
        via: String::new(),
        transfer_likely: false,
        duplicate_of: None,
        who: None,
        postings,
        keys: vec![],
        created: 0,
    }
}
