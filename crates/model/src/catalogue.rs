//! Common Dutch household categories. From budget.rs `classify/catalogue.rs`: savings goals are pots, the mortgage
//! category is its interest (the repayment lowers the mortgage), and income has its own categories.

use crate::book::Book;
use crate::edit::NewAccount;
use crate::types::{AccountKind, Group};
use crate::{Result, invalid};

/// A category a household may want: its name, category group (by name), what belongs in it, and whether money goes
/// there or comes from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: &'static str,
    pub group_name: &'static str,
    pub description: &'static str,
    pub group: Group,
}

const fn e(name: &'static str, group_name: &'static str, description: &'static str, group: Group) -> Entry {
    Entry { name, group_name, description, group }
}

pub const ENTRIES: &[Entry] = &[
    // Fixed Costs
    e("Mortgage Interest", "Fixed Costs", "Interest part of mortgage payments to a bank (hypotheekrente)", Group::Goes),
    e("Rent", "Fixed Costs", "Monthly rent for a home (huur)", Group::Goes),
    e("Energy", "Fixed Costs", "Electricity and gas: Vattenfall, Eneco, Essent, Greenchoice", Group::Goes),
    e("Water", "Fixed Costs", "Drinking water company: Vitens, Evides, Waternet", Group::Goes),
    e("Health Insurance", "Fixed Costs", "Zorgverzekering: Zilveren Kruis, CZ, VGZ, Menzis", Group::Goes),
    e("Other Insurance", "Fixed Costs", "Home, contents, liability, car or travel insurance", Group::Goes),
    e("Internet & Phone", "Fixed Costs", "Internet, TV and mobile: Ziggo, KPN, Odido, Vodafone", Group::Goes),
    e("Municipal Taxes", "Fixed Costs", "Gemeentelijke belastingen, waterschap", Group::Goes),
    e("Childcare", "Fixed Costs", "Kinderopvang, daycare, after-school care", Group::Goes),
    e("Loan Repayment", "Fixed Costs", "Repayments of personal loans or study debt (DUO)", Group::Goes),
    // Daily Life
    e("Groceries", "Daily Life", "Supermarkets: Albert Heijn, Jumbo, Lidl, Aldi, Plus, Dirk", Group::Goes),
    e("Eating Out", "Daily Life", "Restaurants, cafés, takeaway: Thuisbezorgd, Uber Eats", Group::Goes),
    e("Transport", "Daily Life", "Public transport and fuel: NS, OV-chipkaart, GVB, Shell, parking", Group::Goes),
    e("Car Maintenance", "Daily Life", "Car service, tyres, APK, garage", Group::Goes),
    e("Personal Care", "Daily Life", "Drugstores and hairdresser: Kruidvat, Etos, hairdresser", Group::Goes),
    e("Kids", "Daily Life", "Children's clothes, toys, school costs", Group::Goes),
    e("Education", "Daily Life", "Courses, study costs, school fees", Group::Goes),
    e("Clothing", "Daily Life", "Clothes and shoes: Zalando, H&M, Zeeman", Group::Goes),
    e("Household", "Daily Life", "Household goods and DIY: Action, HEMA, IKEA, Gamma, Praxis", Group::Goes),
    e("Home Maintenance", "Daily Life", "Repairs and maintenance of the home", Group::Goes),
    e("Pet Care", "Daily Life", "Pets: vet, pet food, Zooplus, pet shop", Group::Goes),
    e("Medical", "Daily Life", "Pharmacy (apotheek), dentist, doctor, own risk (eigen risico)", Group::Goes),
    e("Online Shopping", "Daily Life", "General web shops: Bol.com, Amazon, Coolblue", Group::Goes),
    e("Gifts", "Daily Life", "Presents for birthdays and holidays", Group::Goes),
    e("Charity", "Daily Life", "Donations to charities and good causes", Group::Goes),
    e("Hobbies", "Daily Life", "Hobby supplies and courses", Group::Goes),
    // Fun
    e("Subscriptions", "Fun", "Streaming and apps: Netflix, Spotify, Disney+, Videoland", Group::Goes),
    e("Games", "Fun", "Video games and game stores: Steam, Nintendo, PlayStation, Xbox", Group::Goes),
    e("Sports", "Fun", "Gym, sports club, sports gear: Basic-Fit, Decathlon", Group::Goes),
    e("Going Out", "Fun", "Cinema, concerts, bars, events", Group::Goes),
    e("Books & Media", "Fun", "Books, newspapers, magazines", Group::Goes),
    // Income
    e("Salary", "Income", "Salary and wages from an employer (salaris)", Group::Comes),
    e("Allowances", "Income", "Benefits from the government: zorgtoeslag, huurtoeslag, kinderbijslag", Group::Comes),
    e("Other Income", "Income", "Interest, sales on Marktplaats, other money that comes in", Group::Comes),
];

/// The categories a new household starts with.
pub const STARTERS: [&str; 16] = [
    "Mortgage Interest",
    "Rent",
    "Energy",
    "Health Insurance",
    "Internet & Phone",
    "Municipal Taxes",
    "Groceries",
    "Transport",
    "Eating Out",
    "Personal Care",
    "Kids",
    "Car Maintenance",
    "Gifts",
    "Salary",
    "Allowances",
    "Other Income",
];

pub fn entry(name: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.name.eq_ignore_ascii_case(name.trim()))
}

/// Adds a catalogue category to the book, with its category group (made when missing). Returns the category.
pub fn add_entry(book: &mut Book, entry: &Entry) -> Result<crate::AccountId> {
    let existing = book
        .accounts()
        .find(|a| a.kind == AccountKind::CategoryGroup && a.group == entry.group && a.name.eq_ignore_ascii_case(entry.group_name))
        .map(|a| a.id);
    let group = match existing {
        Some(g) => g,
        None => book.create_account(NewAccount {
            kind: Some(AccountKind::CategoryGroup),
            name: entry.group_name.into(),
            group: Some(entry.group),
            ..Default::default()
        })?,
    };
    if book.accounts().any(|a| a.parent == Some(group) && a.name.eq_ignore_ascii_case(entry.name)) {
        return Err(invalid(format!("{} is already there", entry.name)));
    }
    book.create_account(NewAccount {
        kind: Some(AccountKind::Category),
        name: entry.name.into(),
        parent: Some(group),
        description: entry.description.into(),
        ..Default::default()
    })
}

pub(crate) fn add_starter_categories(book: &mut Book) -> Result<()> {
    for name in STARTERS {
        if let Some(e) = entry(name) {
            add_entry(book, e)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starters_are_in_the_catalogue_and_make_a_valid_book() {
        assert!(STARTERS.iter().all(|n| entry(n).is_some()));
        let b = Book::with_starter_categories("Home").unwrap();
        let tree = b.tree();
        assert_eq!(tree.categories().len(), STARTERS.len());
        let groups: Vec<&str> = tree.roots(Group::Goes).iter().map(|a| a.name.as_str()).collect();
        assert_eq!(groups, ["Fixed Costs", "Daily Life"]);
        assert_eq!(b.validate(), Ok(()));
    }
}
