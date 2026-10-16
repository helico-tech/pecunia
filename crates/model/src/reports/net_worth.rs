//! Net worth at a date (have − owe, per account and pot), pot goals and free to spend.

use chrono::NaiveDate;
use serde::Serialize;

use crate::ledger::Balances;
use crate::money::Money;
use crate::reports::{AccountAmount, months_between};
use crate::tree::AccountTree;
use crate::types::{Account, AccountId, AccountKind, Group, Txn};

/// The balance sheet in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetWorth {
    pub date: NaiveDate,
    pub have: Vec<PhysicalBalance>,
    pub owe: Vec<PhysicalBalance>,
    pub total_have: Money,
    /// What you owe, positive.
    pub total_owe: Money,
    pub net_worth: Money,
}

/// A physical account with its shown balance and pots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhysicalBalance {
    pub account: AccountId,
    pub name: String,
    pub kind: AccountKind,
    /// What you have, or what you owe, positive.
    pub balance: Money,
    /// The part in pots other than Unassigned ("of which in pots").
    pub in_pots: Money,
    /// Have accounts only, Unassigned first.
    pub pots: Vec<PotBalance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PotBalance {
    pub account: AccountId,
    pub name: String,
    pub unassigned: bool,
    pub balance: Money,
    pub goal: Option<GoalProgress>,
}

/// How far a pot is towards its goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoalProgress {
    pub goal: Money,
    pub goal_date: Option<NaiveDate>,
    /// Whole percent of the goal, rounded down (can pass 100).
    pub percent: i64,
    pub reached: bool,
    /// When the balance last reached the goal and stayed there.
    pub reached_on: Option<NaiveDate>,
    pub to_go: Money,
    /// Months from this month to the goal month, when there is a goal date in the future.
    pub months_left: Option<i32>,
    /// What to add each month to make it, rounded up to the cent.
    pub per_month: Option<Money>,
}

/// Net worth at the end of `date`. Closed accounts with nothing on them are left out.
pub fn net_worth(tree: &AccountTree, txns: &[&Txn], date: NaiveDate) -> NetWorth {
    let balances = Balances::at(txns.iter().copied(), date);
    let lines = |group| -> Vec<PhysicalBalance> {
        tree.physical_accounts(group)
            .into_iter()
            .filter(|a| !(a.closed && balances.total(tree, a.id).is_zero()))
            .map(|a| physical_balance(tree, txns, &balances, a, date))
            .collect()
    };
    let (have, owe) = (lines(Group::Have), lines(Group::Owe));
    let total_have: Money = have.iter().map(|l| l.balance).sum();
    let total_owe: Money = owe.iter().map(|l| l.balance).sum();
    NetWorth { date, have, owe, total_have, total_owe, net_worth: total_have - total_owe }
}

fn physical_balance(tree: &AccountTree, txns: &[&Txn], b: &Balances, a: &Account, date: NaiveDate) -> PhysicalBalance {
    let pots: Vec<PotBalance> = tree
        .pots(a.id)
        .into_iter()
        .map(|p| PotBalance {
            account: p.id,
            name: p.name.clone(),
            unassigned: p.unassigned,
            balance: b.shown(tree, p.id),
            goal: goal_progress(tree, txns, p.id, date),
        })
        .collect();
    PhysicalBalance {
        account: a.id,
        name: a.name.clone(),
        kind: a.kind,
        balance: b.shown(tree, a.id),
        in_pots: pots.iter().filter(|p| !p.unassigned).map(|p| p.balance).sum(),
        pots,
    }
}

/// A pot's progress towards its goal at the end of `date`; `None` without a goal.
pub fn goal_progress(tree: &AccountTree, txns: &[&Txn], pot: AccountId, date: NaiveDate) -> Option<GoalProgress> {
    let a = tree.get(pot)?;
    let goal = a.goal.filter(|g| g.is_positive())?;
    let mut touching: Vec<&Txn> = txns.iter().copied().filter(|t| t.date <= date && t.postings.iter().any(|p| p.account == pot)).collect();
    touching.sort_by_key(|t| (t.date, t.id));
    let (mut balance, mut reached_on) = (Money::ZERO, None);
    for t in touching {
        balance += t.postings.iter().filter(|p| p.account == pot).map(|p| p.amount).sum();
        match (balance >= goal, reached_on) {
            (true, None) => reached_on = Some(t.date),
            (false, _) => reached_on = None,
            _ => {}
        }
    }
    let to_go = if balance >= goal { Money::ZERO } else { goal - balance };
    let months_left = a.goal_date.map(|g| months_between(date, g)).filter(|m| *m > 0);
    let per_month = months_left.filter(|_| to_go.is_positive()).map(|m| {
        let m = i64::from(m);
        Money(to_go.cents().saturating_add(m - 1) / m)
    });
    Some(GoalProgress {
        goal,
        goal_date: a.goal_date,
        percent: balance.cents().max(0).saturating_mul(100) / goal.cents(),
        reached: balance >= goal,
        reached_on,
        to_go,
        months_left,
        per_month,
    })
}

/// What you can spend freely: the Unassigned pots of current accounts and cash, minus credit-card debt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FreeToSpend {
    pub date: NaiveDate,
    pub amount: Money,
    /// The Unassigned pot per current account and cash (the line names the account).
    pub unassigned: Vec<AccountAmount>,
    /// What you owe on credit cards, positive.
    pub card_debt: Money,
}

pub fn free_to_spend(tree: &AccountTree, txns: &[&Txn], date: NaiveDate) -> FreeToSpend {
    let b = Balances::at(txns.iter().copied(), date);
    let unassigned: Vec<AccountAmount> = tree
        .physical_accounts(Group::Have)
        .into_iter()
        .filter(|a| matches!(a.kind, AccountKind::Current | AccountKind::Cash))
        .filter_map(|a| {
            let pot = tree.unassigned_pot(a.id)?;
            Some(AccountAmount { account: a.id, name: a.name.clone(), amount: b.shown(tree, pot.id) })
        })
        .collect();
    let card_debt: Money = tree
        .physical_accounts(Group::Owe)
        .into_iter()
        .filter(|a| a.kind == AccountKind::CreditCard)
        .map(|a| b.shown(tree, a.id))
        .filter(|m| m.is_positive())
        .sum();
    let amount = unassigned.iter().map(|u| u.amount).sum::<Money>() - card_debt;
    FreeToSpend { date, amount, unassigned, card_debt }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn books() -> Vec<Txn> {
        vec![
            txn(1, d(2026, 1, 1), None, "", &[(BETAAL_FREE, 200_000), (BILLS, 100_000), (STARTING, -300_000)]),
            txn(2, d(2026, 1, 1), None, "", &[(SPAAR_FREE, 300_000), (HOLIDAY, 100_000), (STARTING, -400_000)]),
            txn(3, d(2026, 1, 1), None, "", &[(MORTGAGE, -10_000_000), (STARTING, 10_000_000)]),
            txn(4, d(2026, 2, 1), Some(CARD), "Shop", &[(CARD, -5_000), (GROCERIES, 5_000)]),
            txn(5, d(2026, 3, 2), None, "", &[(SPAAR_FREE, -150_000), (HOLIDAY, 150_000)]),
            txn(6, d(2026, 4, 2), None, "", &[(HOLIDAY, -20_000), (SPAAR_FREE, 20_000)]),
            txn(7, d(2026, 5, 1), None, "", &[(WALLET_FREE, 4_000), (BETAAL_FREE, -4_000)]),
        ]
    }

    #[test]
    fn sums_have_and_owe_with_pots() {
        let txns = books();
        let txns = refs(&txns);
        let nw = net_worth(&tree(), &txns, d(2026, 5, 31));
        assert_eq!(nw.total_have, Money(700_000));
        assert_eq!(nw.total_owe, Money(10_005_000));
        assert_eq!(nw.net_worth, Money(-9_305_000));
        let spaar = nw.have.iter().find(|l| l.account == SPAAR).unwrap();
        assert_eq!((spaar.balance, spaar.in_pots), (Money(400_000), Money(230_000)));
        assert_eq!(spaar.pots[0].name, "Unassigned");
        let holiday = spaar.pots[1].goal.clone().unwrap();
        assert_eq!(holiday.percent, 92);
        assert!(!holiday.reached);
        assert_eq!(holiday.reached_on, None, "reached in March, dropped below in April");
        assert_eq!(holiday.to_go, Money(20_000));
        assert_eq!(holiday.months_left, Some(13));
        assert_eq!(holiday.per_month, Some(Money(1539)), "rounded up");
        let march = goal_progress(&tree(), &txns, HOLIDAY, d(2026, 3, 31)).unwrap();
        assert_eq!((march.reached, march.reached_on, march.percent), (true, Some(d(2026, 3, 2)), 100));
        assert_eq!(goal_progress(&tree(), &txns, BILLS, d(2026, 3, 31)), None);
    }

    #[test]
    fn leaves_out_closed_accounts_with_nothing_on_them() {
        let mut accounts = accounts();
        for a in accounts.iter_mut().filter(|a| a.id == WALLET || a.id == CARD) {
            a.closed = true;
        }
        let t = crate::tree::AccountTree::new(accounts.into_iter().map(std::sync::Arc::new));
        let txns = books();
        let nw = net_worth(&t, &refs(&txns), d(2026, 1, 31));
        assert!(nw.have.iter().all(|l| l.account != WALLET), "closed and empty");
        assert!(nw.have.iter().any(|l| l.account == BETAAL));
        let nw = net_worth(&t, &refs(&txns), d(2026, 5, 31));
        assert!(nw.have.iter().any(|l| l.account == WALLET) && nw.owe.iter().any(|l| l.account == CARD), "closed but not empty");
    }

    #[test]
    fn free_to_spend_is_unassigned_current_and_cash_minus_cards() {
        let txns = books();
        let f = free_to_spend(&tree(), &refs(&txns), d(2026, 5, 31));
        assert_eq!(f.unassigned.iter().map(|u| u.amount).collect::<Vec<_>>(), vec![Money(196_000), Money(4_000)]);
        assert_eq!(f.card_debt, Money(5_000));
        assert_eq!(f.amount, Money(195_000));
    }
}
