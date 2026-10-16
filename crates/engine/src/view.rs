//! What the person is looking at: the screen, the register filter, the report and its period. Kept in the engine so the
//! CLI, MCP and snapshots can set it too; it is not saved in the book.

use pecunia_model::register::Filter;
use pecunia_model::{NaiveDate, ReviewKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    #[default]
    Overview,
    Transactions,
    Review,
    Pots,
    Accounts,
    Reports,
    Rules,
    Imports,
}

impl Screen {
    pub const ALL: [Screen; 8] =
        [Screen::Overview, Screen::Review, Screen::Transactions, Screen::Pots, Screen::Reports, Screen::Accounts, Screen::Rules, Screen::Imports];

    pub fn id(self) -> &'static str {
        match self {
            Screen::Overview => "overview",
            Screen::Transactions => "transactions",
            Screen::Review => "review",
            Screen::Pots => "pots",
            Screen::Accounts => "accounts",
            Screen::Reports => "reports",
            Screen::Rules => "rules",
            Screen::Imports => "imports",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Screen::Overview => "Overview",
            Screen::Transactions => "Transactions",
            Screen::Review => "Review",
            Screen::Pots => "Pots",
            Screen::Accounts => "Accounts and categories",
            Screen::Reports => "Reports",
            Screen::Rules => "Payee rules",
            Screen::Imports => "Imports",
        }
    }

    pub fn parse(s: &str) -> Option<Screen> {
        let s = s.trim().to_ascii_lowercase();
        Screen::ALL.into_iter().find(|x| x.id() == s).or(match s.as_str() {
            "register" | "txns" => Some(Screen::Transactions),
            "categories" => Some(Screen::Accounts),
            _ => None,
        })
    }
}

/// Which report the Reports screen shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReportKind {
    #[default]
    IncomeExpenses,
    NetWorth,
    CashFlow,
    FixedCosts,
    BankCheck,
}

impl ReportKind {
    pub const ALL: [ReportKind; 5] =
        [ReportKind::IncomeExpenses, ReportKind::NetWorth, ReportKind::CashFlow, ReportKind::FixedCosts, ReportKind::BankCheck];

    pub fn id(self) -> &'static str {
        match self {
            ReportKind::IncomeExpenses => "incomeExpenses",
            ReportKind::NetWorth => "netWorth",
            ReportKind::CashFlow => "cashFlow",
            ReportKind::FixedCosts => "fixedCosts",
            ReportKind::BankCheck => "bankCheck",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ReportKind::IncomeExpenses => "Income and spending",
            ReportKind::NetWorth => "Net worth",
            ReportKind::CashFlow => "Cash flow",
            ReportKind::FixedCosts => "Fixed costs",
            ReportKind::BankCheck => "Bank check",
        }
    }

    pub fn parse(s: &str) -> Option<ReportKind> {
        let norm: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        ReportKind::ALL.into_iter().find(|k| k.id().to_ascii_lowercase() == norm)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ViewState {
    pub screen: Screen,
    /// The register's filter (Transactions screen).
    pub filter: Filter,
    /// The Review screen's tab; `None` shows all.
    pub review_tab: Option<ReviewKind>,
    pub report: ReportKind,
    /// The report period; `None` means this month (or, for net worth, today).
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}
