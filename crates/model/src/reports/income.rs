//! Income and expenses (profit and loss in plain words) for a month or any range.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::{AccountId, AccountKind, Group, SystemAccount, Txn};

/// What came in and went out from `from` to `to` (both included). Only postings on categories count, so transfers
/// between own accounts and pot moves fall out by themselves, and a mortgage repayment (it lowers what you owe) is not
/// a cost. Money still on *Not sorted yet* is kept apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncomeExpenses {
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Per category, biggest first.
    pub income: Vec<CategoryAmount>,
    pub expenses: Vec<CategoryAmount>,
    /// Per category group, biggest first.
    pub income_groups: Vec<CategoryAmount>,
    pub expense_groups: Vec<CategoryAmount>,
    pub total_income: Money,
    pub total_expenses: Money,
    /// What you saved, paid off or kept: income − expenses.
    pub left_over: Money,
    /// Paid off loans in this range (the mortgage repayment), for the note under the report.
    pub loan_repayments: Money,
    /// Money in and out that waits on *Not sorted yet*, not in the totals.
    pub not_sorted_in: Money,
    pub not_sorted_out: Money,
}

/// A category (or category group) with its amount: spent for Goes, received for Comes. A refund lowers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryAmount {
    pub account: AccountId,
    pub name: String,
    /// The category group (for a group line: none).
    pub category_group: Option<AccountId>,
    pub amount: Money,
}

pub fn income_and_expenses(tree: &AccountTree, txns: &[&Txn], from: NaiveDate, to: NaiveDate) -> IncomeExpenses {
    let not_sorted = tree.system(SystemAccount::NotSortedYet).ok();
    let mut per_account: BTreeMap<AccountId, Money> = BTreeMap::new();
    let (mut loan_repayments, mut not_sorted_in, mut not_sorted_out) = (Money::ZERO, Money::ZERO, Money::ZERO);
    for p in txns.iter().filter(|t| t.date >= from && t.date <= to).flat_map(|t| &t.postings) {
        let Some(a) = tree.get(p.account) else { continue };
        match a.group {
            Group::Goes | Group::Comes => *per_account.entry(a.id).or_default() += p.amount,
            Group::Owe if a.kind == AccountKind::Loan && p.amount.is_positive() => loan_repayments += p.amount,
            Group::Equity if Some(a.id) == not_sorted => {
                if p.amount.is_positive() {
                    not_sorted_out += p.amount
                } else {
                    not_sorted_in -= p.amount
                }
            }
            _ => {}
        }
    }
    let lines = |group: Group| -> (Vec<CategoryAmount>, Vec<CategoryAmount>) {
        let sign = group.display_sign();
        let mut cats = Vec::new();
        let mut groups: BTreeMap<AccountId, Money> = BTreeMap::new();
        for (id, sum) in &per_account {
            let Some(a) = tree.get(*id) else { continue };
            if a.group != group || sum.is_zero() {
                continue;
            }
            let amount = Money(sum.cents().saturating_mul(sign));
            let top = tree.root_of(*id);
            let category_group = (top != *id).then_some(top);
            cats.push(CategoryAmount { account: *id, name: a.name.clone(), category_group, amount });
            *groups.entry(top).or_default() += amount;
        }
        let mut groups: Vec<CategoryAmount> =
            groups.into_iter().map(|(id, amount)| CategoryAmount { account: id, name: tree.name(id), category_group: None, amount }).collect();
        cats.sort_by(|a, b| b.amount.cmp(&a.amount).then(a.name.cmp(&b.name)));
        groups.sort_by(|a, b| b.amount.cmp(&a.amount).then(a.name.cmp(&b.name)));
        (cats, groups)
    };
    let (income, income_groups) = lines(Group::Comes);
    let (expenses, expense_groups) = lines(Group::Goes);
    let total_income: Money = income.iter().map(|c| c.amount).sum();
    let total_expenses: Money = expenses.iter().map(|c| c.amount).sum();
    IncomeExpenses {
        from,
        to,
        income,
        expenses,
        income_groups,
        expense_groups,
        total_income,
        total_expenses,
        left_over: total_income - total_expenses,
        loan_repayments,
        not_sorted_in,
        not_sorted_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn counts_categories_only() {
        let txns = [
            txn(1, d(2026, 9, 1), Some(BETAAL), "Werkgever", &[(BETAAL_FREE, 341_260), (SALARY, -341_260)]),
            txn(2, d(2026, 9, 2), Some(BETAAL), "Hypotheek", &[(BETAAL_FREE, -124_500), (INTEREST, 61_230), (MORTGAGE, 63_270)]),
            txn(3, d(2026, 9, 2), Some(BETAAL), "Naar Spaar", &[(BETAAL_FREE, -20_000), (SPAAR_FREE, 20_000)]),
            txn(4, d(2026, 9, 5), Some(CARD), "AH", &[(CARD, -6_418), (GROCERIES, 6_418)]),
            txn(5, d(2026, 9, 6), Some(BETAAL), "AH retour", &[(BETAAL_FREE, 418), (GROCERIES, -418)]),
            txn(6, d(2026, 9, 30), Some(BETAAL), "Bol.com", &[(BETAAL_FREE, -4_799), (NOT_SORTED, 4_799)]),
            txn(7, d(2026, 10, 1), Some(BETAAL), "Werkgever", &[(BETAAL_FREE, 341_260), (SALARY, -341_260)]),
        ];
        let r = income_and_expenses(&tree(), &refs(&txns), d(2026, 9, 1), d(2026, 9, 30));
        assert_eq!(r.total_income, Money(341_260));
        assert_eq!(r.total_expenses, Money(67_230));
        assert_eq!(r.left_over, Money(274_030));
        assert_eq!(r.loan_repayments, Money(63_270));
        assert_eq!((r.not_sorted_in, r.not_sorted_out), (Money::ZERO, Money(4_799)));
        assert_eq!(r.expenses.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Mortgage interest", "Groceries"]);
        assert_eq!(r.expenses[1].amount, Money(6_000), "the refund lowers groceries");
        assert_eq!(r.expenses[1].category_group, Some(DAILY));
        assert_eq!(r.expense_groups, vec![CategoryAmount { account: DAILY, name: "Daily".into(), category_group: None, amount: Money(67_230) }]);
        assert_eq!(r.income[0].amount, Money(341_260), "income shows positive");
    }
}
