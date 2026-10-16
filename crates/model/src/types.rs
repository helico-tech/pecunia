//! The data of a book (ADR 0003): accounts in a tree, transactions with postings, payee rules. Plain data; the
//! rules live in [`crate::Book`] and the modules next to it. Field and value names are the file format.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::money::Money;

macro_rules! id_type {
    ($($(#[$doc:meta])* $name:ident),+) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    )+};
}

id_type!(
    /// An account's number in its book. Numbers are never reused.
    AccountId,
    /// A transaction's number in its book. Numbers are never reused.
    TxnId,
    /// A payee rule's number in its book.
    RuleId,
    /// One import of a bank file.
    ImportId
);

/// The four groups people see, plus hidden equity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// Money you have (assets).
    Have,
    /// Money you owe (liabilities).
    Owe,
    /// Where money goes (expenses).
    Goes,
    /// Where money comes from (income).
    Comes,
    /// Hidden: starting balance, value changes, not sorted yet.
    Equity,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Have, Group::Owe, Group::Goes, Group::Comes, Group::Equity];

    pub fn id(self) -> &'static str {
        match self {
            Group::Have => "have",
            Group::Owe => "owe",
            Group::Goes => "goes",
            Group::Comes => "comes",
            Group::Equity => "equity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::Have => "Money you have",
            Group::Owe => "Money you owe",
            Group::Goes => "Where money goes",
            Group::Comes => "Where money comes from",
            Group::Equity => "Equity",
        }
    }

    pub fn parse(s: &str) -> Option<Group> {
        let s = s.trim().to_ascii_lowercase();
        Group::ALL.into_iter().find(|g| g.id() == s).or(match s.as_str() {
            "expense" | "expenses" => Some(Group::Goes),
            "income" => Some(Group::Comes),
            "asset" | "assets" => Some(Group::Have),
            "liability" | "liabilities" => Some(Group::Owe),
            _ => None,
        })
    }

    /// The sign people see a balance in: postings are "money into this account", so what you owe and what came from
    /// somewhere show flipped.
    pub fn display_sign(self) -> i64 {
        match self {
            Group::Have | Group::Goes => 1,
            Group::Owe | Group::Comes | Group::Equity => -1,
        }
    }

    pub fn rank(self) -> usize {
        Group::ALL.iter().position(|g| *g == self).unwrap_or(Group::ALL.len())
    }
}

/// What an account is, within its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    // Physical accounts (roots of Have / Owe).
    Current,
    Savings,
    CreditCard,
    Cash,
    /// Something you own with an estimated value (a house).
    Property,
    /// Mortgage, study loan, other debt.
    Loan,
    /// A part of a Have account's money (child of a physical Have account).
    Pot,
    /// A heading in Goes / Comes (Home, Daily); takes no postings.
    CategoryGroup,
    /// Where money goes or comes from (child of a category group).
    Category,
    /// Equity accounts the app manages.
    System,
}

impl AccountKind {
    pub const PHYSICAL: [AccountKind; 6] =
        [AccountKind::Current, AccountKind::Savings, AccountKind::Cash, AccountKind::Property, AccountKind::CreditCard, AccountKind::Loan];

    pub fn id(self) -> &'static str {
        match self {
            AccountKind::Current => "current",
            AccountKind::Savings => "savings",
            AccountKind::CreditCard => "credit_card",
            AccountKind::Cash => "cash",
            AccountKind::Property => "property",
            AccountKind::Loan => "loan",
            AccountKind::Pot => "pot",
            AccountKind::CategoryGroup => "category_group",
            AccountKind::Category => "category",
            AccountKind::System => "system",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AccountKind::Current => "Current account",
            AccountKind::Savings => "Savings account",
            AccountKind::CreditCard => "Credit card",
            AccountKind::Cash => "Cash",
            AccountKind::Property => "Property",
            AccountKind::Loan => "Loan or mortgage",
            AccountKind::Pot => "Pot",
            AccountKind::CategoryGroup => "Category group",
            AccountKind::Category => "Category",
            AccountKind::System => "System",
        }
    }

    pub fn parse(s: &str) -> Option<AccountKind> {
        let norm: String = s.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let all = [
            AccountKind::Current,
            AccountKind::Savings,
            AccountKind::CreditCard,
            AccountKind::Cash,
            AccountKind::Property,
            AccountKind::Loan,
            AccountKind::Pot,
            AccountKind::CategoryGroup,
            AccountKind::Category,
            AccountKind::System,
        ];
        all.into_iter().find(|k| k.id().replace('_', "") == norm).or(match norm.as_str() {
            "card" => Some(AccountKind::CreditCard),
            "mortgage" => Some(AccountKind::Loan),
            "house" | "home" => Some(AccountKind::Property),
            "group" => Some(AccountKind::CategoryGroup),
            _ => None,
        })
    }

    /// A physical account: something a bank (or you) keeps a balance for. Imports touch only these.
    pub fn is_physical(self) -> bool {
        AccountKind::PHYSICAL.contains(&self)
    }

    /// The group a physical account of this kind belongs to.
    pub fn physical_group(self) -> Option<Group> {
        match self {
            AccountKind::Current | AccountKind::Savings | AccountKind::Cash | AccountKind::Property => Some(Group::Have),
            AccountKind::CreditCard | AccountKind::Loan => Some(Group::Owe),
            _ => None,
        }
    }
}

/// The system accounts in Equity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAccount {
    /// Opening balances.
    StartingBalance,
    /// Revaluations of estimated things (a house).
    ValueChanges,
    /// The counter side of a transaction nobody has classified yet; Review empties it.
    NotSortedYet,
}

impl SystemAccount {
    pub const ALL: [SystemAccount; 3] = [SystemAccount::StartingBalance, SystemAccount::ValueChanges, SystemAccount::NotSortedYet];

    pub fn key(self) -> &'static str {
        match self {
            SystemAccount::StartingBalance => "starting_balance",
            SystemAccount::ValueChanges => "value_changes",
            SystemAccount::NotSortedYet => "not_sorted_yet",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SystemAccount::StartingBalance => "Starting balance",
            SystemAccount::ValueChanges => "Value changes",
            SystemAccount::NotSortedYet => "Not sorted yet",
        }
    }
}

/// Where a transaction came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Csv,
    Camt053,
    Mt940,
    Manual,
}

impl SourceKind {
    pub fn id(self) -> &'static str {
        match self {
            SourceKind::Csv => "csv",
            SourceKind::Camt053 => "camt053",
            SourceKind::Mt940 => "mt940",
            SourceKind::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub group: Group,
    pub kind: AccountKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<AccountId>,
    pub name: String,
    /// For categories: what belongs here ("Supermarkets: Albert Heijn, Jumbo").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iban: Option<String>,
    /// The automatic Unassigned pot of a Have account.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unassigned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemAccount>,
    /// A pot's goal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_date: Option<NaiveDate>,
    /// Order among siblings (lower first).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sort: i32,
    /// Closed accounts keep their history but take no new money and leave the sidebar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

impl Account {
    pub fn is_physical(&self) -> bool {
        self.kind.is_physical()
    }
}

/// Review state of a transaction (ADR 0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxnStatus {
    /// Classified automatically by a rule or memory; no review needed.
    Auto,
    /// Needs a person: unknown, a guess, a likely transfer, a possible duplicate.
    ToReview,
    /// A person confirmed or entered it.
    Reviewed,
}

impl TxnStatus {
    pub fn id(self) -> &'static str {
        match self {
            TxnStatus::Auto => "auto",
            TxnStatus::ToReview => "to_review",
            TxnStatus::Reviewed => "reviewed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TxnStatus::Auto => "Sorted automatically",
            TxnStatus::ToReview => "To review",
            TxnStatus::Reviewed => "Checked",
        }
    }

    pub fn parse(s: &str) -> Option<TxnStatus> {
        let norm: String = s.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match norm.as_str() {
            "auto" => Some(TxnStatus::Auto),
            "toreview" | "review" => Some(TxnStatus::ToReview),
            "reviewed" | "checked" => Some(TxnStatus::Reviewed),
            _ => None,
        }
    }
}

/// Which step put the current counter account there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassSource {
    Rule,
    Memory,
    User,
}

/// What kind of booking a transaction is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxnKind {
    /// From a bank file.
    Bank,
    /// Entered by hand (cash, a card nobody imports).
    Manual,
    /// Between pots of one physical account; the bank sees nothing.
    PotMove,
    /// Opening balance of an account.
    Opening,
    /// A new estimate of something's value (a house).
    Adjustment,
}

impl TxnKind {
    pub fn id(self) -> &'static str {
        match self {
            TxnKind::Bank => "bank",
            TxnKind::Manual => "manual",
            TxnKind::PotMove => "pot_move",
            TxnKind::Opening => "opening",
            TxnKind::Adjustment => "adjustment",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TxnKind::Bank => "Bank",
            TxnKind::Manual => "Entered by hand",
            TxnKind::PotMove => "Pot move",
            TxnKind::Opening => "Starting balance",
            TxnKind::Adjustment => "New value",
        }
    }
}

/// One of the classifier's best guesses: a counter account and its probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guess {
    pub account: AccountId,
    pub probability: f64,
}

/// One leg of a transaction. Positive = money into this account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub account: AccountId,
    pub amount: Money,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memo: String,
}

impl Posting {
    pub fn new(account: AccountId, amount: Money) -> Posting {
        Posting { account, amount, memo: String::new() }
    }
}

/// A dedup key a bank row left on its account: `ext:CAMT053:abc` or `hash:<sha256>` (ADR 0002).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TxnKey {
    pub account: AccountId,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Txn {
    pub id: TxnId,
    pub kind: TxnKind,
    pub date: NaiveDate,
    /// The text the bank gave (or the person typed).
    pub payee: String,
    /// Normalised payee, the key for rules and memory.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub payee_norm: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterparty_iban: Option<String>,
    /// The person's own note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// For bank rows: the physical account the row belongs to, and its amount there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank_amount: Option<Money>,
    pub source: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportId>,
    pub status: TxnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_source: Option<ClassSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guesses: Vec<Guess>,
    /// Which step answered, for the "Sorted by a payee rule" line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
    /// The classifier thinks this moves money between own accounts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transfer_likely: bool,
    /// Set while this looks like a duplicate of another transaction and nobody has decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<TxnId>,
    /// Card holder or person, information only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    pub postings: Vec<Posting>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<TxnKey>,
    /// Unix milliseconds.
    #[serde(default)]
    pub created: i64,
}

impl Txn {
    /// Postings must sum to zero.
    pub fn is_balanced(&self) -> bool {
        self.postings.iter().map(|p| p.amount).sum::<Money>() == Money::ZERO
    }

    /// The amount that moved: the bank amount, else the sum of the positive postings.
    pub fn size(&self) -> Money {
        self.bank_amount.map(Money::abs).unwrap_or_else(|| self.postings.iter().map(|p| p.amount).filter(|m| m.is_positive()).sum())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOrigin {
    Learned,
    Manual,
}

/// A payee rule. `payee_prefix` matches the start of the normalised payee ("ALBERT HEIJN" matches "ALBERT HEIJN
/// UTRECHT"); `iban` matches the counterparty IBAN. A `mixed` rule has seen different answers and is skipped. The
/// answer is a counter account (ADR 0003).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iban: Option<String>,
    pub account: AccountId,
    pub origin: RuleOrigin,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mixed: bool,
    #[serde(default)]
    pub uses: u32,
}

impl Rule {
    pub fn matches(&self, payee_norm: &str, counterparty_iban: Option<&str>) -> bool {
        (self.iban.is_some() && self.iban.as_deref() == counterparty_iban)
            || self
                .payee_prefix
                .as_deref()
                .is_some_and(|p| !p.is_empty() && (payee_norm == p || payee_norm.strip_prefix(p).is_some_and(|rest| rest.starts_with(' '))))
    }
}

/// A balance the bank stated (MT940 `:62F:`, CAMT `CLBD`), to check the books against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatedBalance {
    pub account: AccountId,
    pub date: NaiveDate,
    pub amount: Money,
    /// Where it came from: the file name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

/// One import of a bank file, for the history and to find its transactions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecord {
    pub id: ImportId,
    pub account: AccountId,
    pub file: String,
    /// `CSV · ING (CSV)`, `CAMT.053`, `MT940`.
    pub format: String,
    /// Unix milliseconds.
    pub at: i64,
    pub rows: usize,
    pub new: usize,
    pub known: usize,
    pub duplicates: usize,
    pub transfers: usize,
}
