//! The account tree (ADR 0003): physical accounts are roots of Have/Owe, pots are children of a Have account,
//! categories hang under category groups in Goes/Comes, system accounts sit in Equity. Postings go to leaves only.
//! From budget.rs `ledger/tree.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::ledger::LedgerError;
use crate::types::{Account, AccountId, AccountKind, Group, SystemAccount};

/// All accounts with their parent/child links, in display order: roots by group (Have, Owe, Goes, Comes, Equity),
/// then `sort`, then id; children with the Unassigned pot first. Cheap to build: accounts are shared.
#[derive(Debug, Clone, Default)]
pub struct AccountTree {
    accounts: BTreeMap<AccountId, Arc<Account>>,
    children: BTreeMap<AccountId, Vec<AccountId>>,
    roots: Vec<AccountId>,
}

impl AccountTree {
    /// Builds the tree; it does not check it (see [`AccountTree::check`]). An account whose parent is missing counts as
    /// a root. Parent links that form a loop are cut where the loop closes.
    pub fn new(accounts: impl IntoIterator<Item = Arc<Account>>) -> AccountTree {
        let accounts: BTreeMap<AccountId, Arc<Account>> = accounts.into_iter().map(|a| (a.id, a)).collect();
        let mut children: BTreeMap<AccountId, Vec<AccountId>> = BTreeMap::new();
        let mut roots = Vec::new();
        for a in accounts.values() {
            match a.parent.filter(|p| accounts.contains_key(p) && !loops(&accounts, a.id)) {
                Some(p) => children.entry(p).or_default().push(a.id),
                None => roots.push(a.id),
            }
        }
        let key = |id: &AccountId| match accounts.get(id) {
            Some(a) => (a.group.rank(), !a.unassigned, a.sort, a.id),
            None => (usize::MAX, true, 0, *id),
        };
        roots.sort_by_key(key);
        for list in children.values_mut() {
            list.sort_by_key(key);
        }
        AccountTree { accounts, children, roots }
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn get(&self, id: AccountId) -> Option<&Account> {
        self.accounts.get(&id).map(|a| a.as_ref())
    }

    /// The account, or [`LedgerError::AccountNotFound`].
    pub fn account(&self, id: AccountId) -> Result<&Account, LedgerError> {
        self.get(id).ok_or(LedgerError::AccountNotFound(id))
    }

    pub fn name(&self, id: AccountId) -> String {
        self.get(id).map(|a| a.name.clone()).unwrap_or_else(|| format!("#{id}"))
    }

    /// Every account, depth first in display order.
    pub fn walk(&self) -> Vec<&Account> {
        let mut out = Vec::with_capacity(self.accounts.len());
        let mut stack: Vec<AccountId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if let Some(a) = self.get(id) {
                out.push(a);
            }
            if let Some(kids) = self.children.get(&id) {
                stack.extend(kids.iter().rev());
            }
        }
        out
    }

    /// Every account with its depth (0 for roots), depth first in display order.
    pub fn walk_depth(&self) -> Vec<(&Account, usize)> {
        let mut out = Vec::with_capacity(self.accounts.len());
        let mut stack: Vec<(AccountId, usize)> = self.roots.iter().rev().map(|r| (*r, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            if let Some(a) = self.get(id) {
                out.push((a, depth));
            }
            if let Some(kids) = self.children.get(&id) {
                stack.extend(kids.iter().rev().map(|k| (*k, depth + 1)));
            }
        }
        out
    }

    /// Root accounts of a group in display order (physical accounts, category groups, system accounts).
    pub fn roots(&self, group: Group) -> Vec<&Account> {
        self.roots.iter().filter_map(|id| self.get(*id)).filter(|a| a.group == group).collect()
    }

    pub fn children(&self, id: AccountId) -> Vec<&Account> {
        self.children.get(&id).map(|k| k.iter().filter_map(|c| self.get(*c)).collect()).unwrap_or_default()
    }

    pub fn is_leaf(&self, id: AccountId) -> bool {
        self.children.get(&id).is_none_or(Vec::is_empty)
    }

    /// Postings go to leaves only, never to a category group, and never to a Have account itself (its pots take them).
    pub fn takes_postings(&self, id: AccountId) -> bool {
        self.get(id).is_some_and(|a| self.is_leaf(id) && a.kind != AccountKind::CategoryGroup && !(a.group == Group::Have && a.kind.is_physical()))
    }

    /// The leaves under an account (the account itself when it is a leaf).
    pub fn leaves(&self, id: AccountId) -> Vec<AccountId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            if out.len() > self.accounts.len() {
                break; // a corrupt tree; `new` cuts loops, so this never happens
            }
            match self.children.get(&x) {
                Some(kids) if !kids.is_empty() => stack.extend(kids.iter().rev()),
                _ => out.push(x),
            }
        }
        out
    }

    /// The top of the branch an account is in (itself for a root).
    pub fn root_of(&self, id: AccountId) -> AccountId {
        let mut at = id;
        for _ in 0..self.accounts.len() {
            match self.get(at).and_then(|a| a.parent).filter(|p| self.children.get(p).is_some_and(|k| k.contains(&at))) {
                Some(p) => at = p,
                None => break,
            }
        }
        at
    }

    /// The physical account an account belongs to: itself for a physical account, the parent for a pot, none for
    /// categories and system accounts.
    pub fn physical_of(&self, id: AccountId) -> Option<AccountId> {
        let a = self.get(id)?;
        if a.kind.is_physical() {
            return Some(a.id);
        }
        if a.kind == AccountKind::Pot {
            return a.parent.filter(|p| self.get(*p).is_some_and(|pa| pa.kind.is_physical()));
        }
        None
    }

    /// Physical accounts of a group in display order.
    pub fn physical_accounts(&self, group: Group) -> Vec<&Account> {
        self.roots(group).into_iter().filter(|a| a.kind.is_physical()).collect()
    }

    /// All physical accounts (Have, then Owe), in display order.
    pub fn all_physical(&self) -> Vec<&Account> {
        let mut v = self.physical_accounts(Group::Have);
        v.extend(self.physical_accounts(Group::Owe));
        v
    }

    /// The pots of a Have account, Unassigned first.
    pub fn pots(&self, physical: AccountId) -> Vec<&Account> {
        self.children(physical).into_iter().filter(|a| a.kind == AccountKind::Pot).collect()
    }

    pub fn unassigned_pot(&self, physical: AccountId) -> Option<&Account> {
        self.pots(physical).into_iter().find(|p| p.unassigned)
    }

    /// Where a bank row of this physical account lands: the Unassigned pot of a Have account, the account itself for
    /// one in Owe.
    pub fn own_leaf(&self, physical: AccountId) -> Result<AccountId, LedgerError> {
        let a = self.account(physical)?;
        if !a.kind.is_physical() {
            return Err(LedgerError::NotPhysical { name: a.name.clone() });
        }
        if a.group != Group::Have {
            return Ok(a.id);
        }
        self.unassigned_pot(physical).map(|p| p.id).ok_or_else(|| LedgerError::NoUnassignedPot { name: a.name.clone() })
    }

    /// The leaf an answer stands for: a Have account means its Unassigned pot (a transfer "to Savings"); anything else
    /// must take postings itself.
    pub fn resolve_leaf(&self, id: AccountId) -> Result<AccountId, LedgerError> {
        let a = self.account(id)?;
        if a.group == Group::Have && a.kind.is_physical() {
            return self.own_leaf(id);
        }
        if !self.takes_postings(id) {
            return Err(LedgerError::NotALeaf { name: a.name.clone() });
        }
        Ok(id)
    }

    /// A system account by its key.
    pub fn system(&self, key: SystemAccount) -> Result<AccountId, LedgerError> {
        self.accounts.values().find(|a| a.system == Some(key)).map(|a| a.id).ok_or(LedgerError::SystemAccountMissing(key))
    }

    /// The own account with this IBAN (spaces and case ignored).
    pub fn find_by_iban(&self, iban: &str) -> Option<&Account> {
        let want = normalize_iban(iban);
        if want.is_empty() {
            return None;
        }
        self.accounts.values().find(|a| a.iban.as_deref().map(normalize_iban).as_deref() == Some(want.as_str())).map(|a| a.as_ref())
    }

    /// Categories (leaves in Goes and Comes) in display order, with their category group's name.
    pub fn categories(&self) -> Vec<(&Account, String)> {
        self.walk()
            .into_iter()
            .filter(|a| a.kind == AccountKind::Category && matches!(a.group, Group::Goes | Group::Comes))
            .map(|a| (a, a.parent.map(|p| self.name(p)).unwrap_or_default()))
            .collect()
    }

    /// Outline numbers as on the Accounts screen: Have = 1, Owe = 2, Goes = 3, Comes = 4, Equity = 5; `1.2.3` is the
    /// third child of the second account in Have.
    pub fn outline(&self) -> BTreeMap<AccountId, String> {
        let mut out = BTreeMap::new();
        for (gi, g) in Group::ALL.iter().enumerate() {
            let mut stack: Vec<(AccountId, String)> =
                self.roots(*g).iter().enumerate().rev().map(|(i, a)| (a.id, format!("{}.{}", gi + 1, i + 1))).collect();
            while let Some((id, num)) = stack.pop() {
                for (i, c) in self.children(id).iter().enumerate().rev() {
                    stack.push((c.id, format!("{num}.{}", i + 1)));
                }
                out.insert(id, num);
            }
        }
        out
    }

    /// Whether an account of this group and kind may sit under this parent (or at the root).
    pub fn check_placement(&self, group: Group, kind: AccountKind, parent: Option<AccountId>) -> Result<(), LedgerError> {
        let wrong_group = Err(LedgerError::WrongGroup { kind, group });
        let place = |reason| Err(LedgerError::WrongPlace { kind, reason });
        let parent = match parent {
            Some(p) => Some(self.account(p)?),
            None => None,
        };
        match kind {
            AccountKind::Current | AccountKind::Savings | AccountKind::Cash | AccountKind::Property => {
                if group != Group::Have {
                    return wrong_group;
                }
            }
            AccountKind::CreditCard | AccountKind::Loan => {
                if group != Group::Owe {
                    return wrong_group;
                }
            }
            AccountKind::Pot => {
                if group != Group::Have {
                    return wrong_group;
                }
                if !parent.is_some_and(|p| p.group == Group::Have && p.kind.is_physical()) {
                    return place("a pot belongs to one account in Money you have");
                }
                return Ok(());
            }
            AccountKind::CategoryGroup => {
                if !matches!(group, Group::Goes | Group::Comes) {
                    return wrong_group;
                }
            }
            AccountKind::Category => {
                if !matches!(group, Group::Goes | Group::Comes) {
                    return wrong_group;
                }
                if !parent.is_some_and(|p| p.kind == AccountKind::CategoryGroup && p.group == group) {
                    return place("a category sits under a category group of the same kind");
                }
                return Ok(());
            }
            AccountKind::System => {
                if group != Group::Equity {
                    return wrong_group;
                }
            }
        }
        if parent.is_some() {
            return place("it is a top-level account");
        }
        Ok(())
    }

    /// Checks the whole tree: every account in its place, and every Have account with exactly one Unassigned pot.
    pub fn check(&self) -> Result<(), LedgerError> {
        for a in self.accounts.values() {
            if let Some(p) = a.parent {
                self.account(p)?;
            }
            self.check_placement(a.group, a.kind, a.parent)?;
            if a.unassigned && a.kind != AccountKind::Pot {
                return Err(LedgerError::NotAPot { name: a.name.clone() });
            }
            if a.group == Group::Have && a.kind.is_physical() {
                let n = self.pots(a.id).iter().filter(|p| p.unassigned).count();
                if n != 1 {
                    return Err(LedgerError::UnassignedPotCount { name: a.name.clone() });
                }
            }
        }
        Ok(())
    }
}

/// Whether following parents from `id` comes back to `id`.
fn loops(accounts: &BTreeMap<AccountId, Arc<Account>>, id: AccountId) -> bool {
    let mut at = accounts.get(&id).and_then(|a| a.parent);
    for _ in 0..=accounts.len() {
        match at {
            Some(p) if p == id => return true,
            Some(p) => at = accounts.get(&p).and_then(|a| a.parent),
            None => return false,
        }
    }
    true
}

/// An IBAN without spaces, upper case: `NL55 INGB 0008 8101 23` → `NL55INGB0008810123`.
pub fn normalize_iban(iban: &str) -> String {
    iban.chars().filter(|c| !c.is_whitespace()).take(40).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn finds_leaves_pots_and_physical_accounts() {
        let t = tree();
        assert_eq!(t.own_leaf(BETAAL).unwrap(), BETAAL_FREE);
        assert_eq!(t.own_leaf(CARD).unwrap(), CARD);
        assert_eq!(t.resolve_leaf(SPAAR).unwrap(), SPAAR_FREE);
        assert_eq!(t.resolve_leaf(GROCERIES).unwrap(), GROCERIES);
        assert_eq!(t.resolve_leaf(DAILY), Err(LedgerError::NotALeaf { name: "Daily".into() }));
        assert_eq!(t.physical_of(HOLIDAY), Some(SPAAR));
        assert_eq!(t.physical_of(CARD), Some(CARD));
        assert_eq!(t.physical_of(GROCERIES), None);
        assert!(!t.takes_postings(BETAAL));
        assert!(t.takes_postings(NOT_SORTED));
        assert_eq!(t.leaves(SPAAR), vec![SPAAR_FREE, HOLIDAY]);
        assert_eq!(t.system(SystemAccount::NotSortedYet).unwrap(), NOT_SORTED);
        assert_eq!(t.find_by_iban("nl12 ingb 0001 2345 67").map(|a| a.id), Some(BETAAL));
        assert_eq!(t.root_of(GROCERIES), DAILY);
        assert_eq!(t.categories().len(), 3);
    }

    #[test]
    fn numbers_the_outline_like_the_accounts_screen() {
        let o = tree().outline();
        assert_eq!(o[&BETAAL], "1.1");
        assert_eq!(o[&BETAAL_FREE], "1.1.1");
        assert_eq!(o[&SPAAR], "1.2");
        assert_eq!(o[&HOLIDAY], "1.2.2");
        assert_eq!(o[&CARD], "2.1");
        assert_eq!(o[&GROCERIES], "3.1.1");
        assert_eq!(o[&SALARY], "4.1.1");
    }

    #[test]
    fn checks_where_accounts_sit() {
        let t = tree();
        assert_eq!(t.check(), Ok(()));
        assert!(t.check_placement(Group::Have, AccountKind::Pot, Some(SPAAR)).is_ok());
        assert!(matches!(t.check_placement(Group::Have, AccountKind::Pot, Some(CARD)), Err(LedgerError::WrongPlace { .. })));
        assert!(matches!(t.check_placement(Group::Owe, AccountKind::Pot, Some(SPAAR)), Err(LedgerError::WrongGroup { .. })));
        assert!(t.check_placement(Group::Goes, AccountKind::Category, Some(DAILY)).is_ok());
        assert!(t.check_placement(Group::Comes, AccountKind::Category, Some(DAILY)).is_err());
        assert!(t.check_placement(Group::Have, AccountKind::Savings, Some(BETAAL)).is_err());
        assert!(t.check_placement(Group::Owe, AccountKind::CreditCard, None).is_ok());

        let mut accounts: Vec<Arc<Account>> = t.walk().into_iter().map(|a| Arc::new(a.clone())).collect();
        accounts.retain(|a| a.id != SPAAR_FREE);
        assert_eq!(AccountTree::new(accounts).check(), Err(LedgerError::UnassignedPotCount { name: "ING Oranje Spaar".into() }));
    }

    #[test]
    fn parent_loops_do_not_hang() {
        let a = Account { parent: Some(AccountId(2)), ..acc(AccountId(1), Group::Goes, AccountKind::Category, None, "A") };
        let b = Account { parent: Some(AccountId(1)), ..acc(AccountId(2), Group::Goes, AccountKind::Category, None, "B") };
        let t = AccountTree::new([Arc::new(a), Arc::new(b)]);
        assert_eq!(t.walk().len(), 2);
        assert_eq!(t.leaves(AccountId(1)).len(), 1);
        let _ = t.root_of(AccountId(1));
    }
}
