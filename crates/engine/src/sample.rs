//! The sample household: a year of a Dutch household's money, made up (no real bank data). It is built the way a
//! person would build it: accounts with starting balances, bank files imported in two rounds, the first round answered
//! in Review (so the classifier learns rules and memory), the second sorted partly by itself. Review keeps some work:
//! new payees, a mixed payee, a transfer and a possible duplicate.

use chrono::{Datelike, Days, Months, NaiveDate};
use pecunia_model::{
    AccountId, AccountKind, Book, ImportSource, Money, NewAccount, NewTxn, Posting, RawTransaction, SourceKind, StatementBalance, TxnStatus,
    bank_row, catalogue,
};

const ING_IBAN: &str = "NL12INGB0001234567";
const SPAAR_IBAN: &str = "NL34INGB0007654321";

/// A small deterministic random source, so the sample is the same every time.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// A whole number of cents between `lo` and `hi` euros.
    fn cents(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo).max(1) as u64 * 100;
        lo * 100 + (self.next() % span) as i64
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

struct Accounts {
    ing: AccountId,
    spaar: AccountId,
    holiday: AccountId,
    emergency: AccountId,
    bills: AccountId,
    wallet: AccountId,
    card: AccountId,
    mortgage: AccountId,
    house: AccountId,
}

fn day(d: NaiveDate, n: u32) -> NaiveDate {
    d.with_day(n.min(28)).unwrap_or(d)
}

/// The sample book, with "today" taken from `now` (Unix milliseconds).
pub fn book(now: i64) -> Book {
    build(now).unwrap_or_else(|e| {
        log::warn!("the sample could not be built: {e}");
        Book::new("Sample household").unwrap_or_else(|_| Book::from_parts(Default::default(), vec![], vec![], vec![]).0)
    })
}

fn build(now: i64) -> pecunia_model::Result<Book> {
    let today = pecunia_model::date_of(now).max(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap_or_default());
    let start = today.checked_sub_months(Months::new(12)).map(|d| day(d, 1)).unwrap_or(today);
    let mut b = Book::with_starter_categories("Sample household")?;
    for extra in ["Subscriptions", "Clothing", "Online Shopping", "Pet Care"] {
        if let Some(e) = catalogue::entry(extra) {
            catalogue::add_entry(&mut b, e)?;
        }
    }
    let new = |kind, name: &str, parent: Option<AccountId>, iban: Option<&str>, goal: Option<i64>| NewAccount {
        kind: Some(kind),
        name: name.into(),
        parent,
        iban: iban.map(str::to_string),
        goal: goal.map(|g| Money(g * 100)),
        goal_date: goal.and_then(|_| today.checked_add_months(Months::new(8))),
        ..Default::default()
    };
    let ing = b.create_account(new(AccountKind::Current, "ING Betaalrekening", None, Some(ING_IBAN), None))?;
    let bills = b.create_account(new(AccountKind::Pot, "Bills", Some(ing), None, None))?;
    let spaar = b.create_account(new(AccountKind::Savings, "ING Oranje Spaar", None, Some(SPAAR_IBAN), None))?;
    let holiday = b.create_account(new(AccountKind::Pot, "Holiday", Some(spaar), None, Some(2_500)))?;
    let emergency = b.create_account(new(AccountKind::Pot, "Emergency fund", Some(spaar), None, Some(10_000)))?;
    let wallet = b.create_account(new(AccountKind::Cash, "Wallet", None, None, None))?;
    let card = b.create_account(new(AccountKind::CreditCard, "ICS Creditcard", None, None, None))?;
    let mortgage = b.create_account(new(AccountKind::Loan, "Mortgage", None, None, None))?;
    let house = b.create_account(new(AccountKind::Property, "House", None, None, None))?;
    let a = Accounts { ing, spaar, holiday, emergency, bills, wallet, card, mortgage, house };

    let opening = start.pred_opt().unwrap_or(start);
    for (acct, euros) in
        [(ing, 2_400), (bills, 600), (spaar, 2_400), (holiday, 600), (emergency, 5_000), (wallet, 80), (mortgage, 285_000), (house, 395_000)]
    {
        b.set_starting_balance(acct, Money(euros * 100), opening, now)?;
    }

    // Two rounds: everything up to three weeks ago, answered; then the rest, partly sorted by itself.
    let cutoff = today.checked_sub_days(Days::new(21)).unwrap_or(today);
    let mut rng = Lcg(42);
    let rows = rows(&a, start, today, &mut rng);
    let (old, recent): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(_, r)| r.date < cutoff);
    import_round(&mut b, &a, &old, "2025", now)?;
    answer_everything(&mut b, &a)?;
    import_round(&mut b, &a, &recent, "recent", now)?;
    answer_transfers(&mut b, &a, false)?;

    // A copy of one supermarket row from another export of the same account: a possible duplicate.
    if let Some((_, r)) = recent.iter().find(|(acct, r)| *acct == a.ing && r.description.contains("Albert Heijn")) {
        let mut dup = bank_row(r.date, r.amount.cents(), "ALBERT HEIJN 1427", "Betaalautomaat");
        dup.raw_line = format!("mt940|{}", r.raw_line);
        let src = ImportSource { file: "ING-export.sta".into(), format: "MT940".into(), balances: vec![] };
        b.ingest(a.ing, &[dup], SourceKind::Mt940, &src, now, false)?;
    }

    // Cash at the market, pot moves and a new value for the house.
    let mut month = start;
    while month <= today {
        for d in [6, 20] {
            let date = day(month, d);
            if date <= today {
                let groceries = b.resolve_account("Groceries").unwrap_or(a.wallet);
                let amount = Money(rng.cents(8, 30));
                b.add_txn(NewTxn { date, payee: "Market".into(), from: a.wallet, to: groceries, amount, note: String::new(), who: None }, now)?;
            }
        }
        let date = day(month, 27);
        if date <= today {
            let spaar_free = b.tree().own_leaf(a.spaar)?;
            b.pot_move(spaar_free, a.holiday, Money(10_000), date, "Saving for the summer", now)?;
            b.pot_move(spaar_free, a.emergency, Money(50_00), date, "", now)?;
        }
        month = month.checked_add_months(Months::new(1)).unwrap_or(today.succ_opt().unwrap_or(today));
    }
    let mid = start.checked_add_months(Months::new(6)).unwrap_or(start);
    b.set_value(a.house, Money(41_000_000), mid, now)?;
    Ok(b)
}

/// One year of bank rows per account, in date order.
fn rows(a: &Accounts, start: NaiveDate, today: NaiveDate, rng: &mut Lcg) -> Vec<(AccountId, RawTransaction)> {
    let mut out: Vec<(AccountId, RawTransaction)> = Vec::new();
    let mut push = |acct: AccountId, date: NaiveDate, cents: i64, name: &str, desc: &str, iban: Option<&str>| {
        if date <= today && date >= start {
            let mut r = bank_row(date, cents, name, desc);
            r.counterparty_iban = iban.map(str::to_string);
            out.push((acct, r));
        }
    };
    let mut month = start;
    let mut n = 0u32;
    while month <= today {
        n += 1;
        push(a.ing, day(month, 1), -124_500, "ABN AMRO Hypotheken", "Maandtermijn hypotheek", Some("NL02ABNA0123456789"));
        push(a.ing, day(month, 3), -rng.cents(130, 160), "Vattenfall Klantenservice N.V.", "Termijnbedrag energie", Some("NL44RABO0123456789"));
        push(a.ing, day(month, 5), -13_850, "Zilveren Kruis Zorgverzekeringen N.V.", "Premie zorgverzekering", Some("NL55INGB0000012345"));
        push(a.ing, day(month, 8), -5_995, "Ziggo Services BV", "Internet en tv", None);
        push(a.ing, day(month, 12), -1_399, "Netflix International B.V.", "Netflix abonnement", None);
        push(a.ing, day(month, 12), -1_199, "Spotify AB", "Spotify Premium", None);
        if (2..=11).contains(&month.month()) {
            push(a.ing, day(month, 15), -4_520, "Gemeente Utrecht", "Termijn gemeentelijke belastingen", Some("NL66BNGH0285001234"));
        }
        push(a.ing, day(month, 25), 345_000, "Werkgever BV", "Salaris", Some("NL91ABNA0417164300"));
        push(a.ing, day(month, 26), -30_000, "ING Oranje Spaar", "Naar spaarrekening", Some(SPAAR_IBAN));
        push(a.spaar, day(month, 26), 30_000, "ING Betaalrekening", "Van betaalrekening", Some(ING_IBAN));
        push(a.ing, day(month, 10), -5_000, "Geldautomaat", "Geldopname Utrecht Centraal", None);
        // Weekly groceries, now and then something else.
        for week in 0..4u32 {
            let d = day(month, 2 + week * 7);
            if rng.chance(70) {
                push(a.ing, d, -rng.cents(35, 95), "Albert Heijn 1427", "BEA, Apple Pay Albert Heijn 1427 Utrecht", None);
            } else {
                push(a.ing, d, -rng.cents(30, 80), "Jumbo Utrecht", "BEA, Betaalpas Jumbo Utrecht", None);
            }
            if rng.chance(40) {
                push(a.ing, day(d, d.day() + 1), -rng.cents(18, 45), "Thuisbezorgd.nl", "iDEAL Thuisbezorgd", None);
            }
            if rng.chance(45) {
                push(a.ing, day(d, d.day() + 2), -rng.cents(4, 25), "NS Groep", "NS reizen", None);
            }
        }
        push(a.ing, day(month, 18), -rng.cents(12, 40), "Kruidvat 7731", "BEA, Apple Pay Kruidvat 7731 Utrecht", None);
        if n.is_multiple_of(2) {
            // Sometimes a present, sometimes for the kids: a payee the classifier can't learn one answer for.
            push(a.ing, day(month, 21), -rng.cents(15, 60), "Bol.com B.V.", "Bestelling bol.com", None);
        }
        if n.is_multiple_of(3) {
            push(a.ing, day(month, 23), -rng.cents(10, 40), "HEMA", "BEA, Betaalpas HEMA Utrecht", None);
        }
        if month.month().is_multiple_of(3) {
            push(a.spaar, day(month, 28), rng.cents(3, 9), "", "Rente", None);
        }
        // The credit card: purchases, then the monthly payment from ING.
        let spent = [rng.cents(30, 120), rng.cents(20, 90)];
        push(a.card, day(month, 9), -spent[0], "Zalando SE", "Zalando", None);
        push(a.card, day(month, 16), -spent[1], "Coolblue B.V.", "Coolblue bestelling", None);
        let next = month.checked_add_months(Months::new(1)).unwrap_or(month);
        push(a.ing, day(next, 4), -(spent[0] + spent[1]), "International Card Services", "Incasso creditcard", None);
        push(a.card, day(next, 4), spent[0] + spent[1], "", "Betaling ontvangen", None);
        month = next;
    }
    // Something the household has never seen before, in the last weeks.
    push(a.ing, today.checked_sub_days(Days::new(4)).unwrap_or(today), -6_850, "Dierenkliniek Utrecht", "Consult en vaccinatie", None);
    push(a.ing, today.checked_sub_days(Days::new(9)).unwrap_or(today), 4_500, "Marktplaats", "Verkoop fiets", None);
    push(a.ing, today.checked_sub_days(Days::new(6)).unwrap_or(today), -2_350, "Tikkie J. de Vries", "Etentje vrijdag", None);
    // A payee answered both ways before (a suggestion), and money to savings the savings file doesn't show yet.
    push(a.ing, today.checked_sub_days(Days::new(3)).unwrap_or(today), -2_999, "Bol.com B.V.", "Bestelling bol.com", None);
    push(a.ing, today.checked_sub_days(Days::new(2)).unwrap_or(today), -15_000, "ING Oranje Spaar", "Extra sparen", Some(SPAAR_IBAN));
    out.sort_by_key(|(_, r)| r.date);
    out
}

fn import_round(b: &mut Book, a: &Accounts, rows: &[(AccountId, RawTransaction)], tag: &str, now: i64) -> pecunia_model::Result<()> {
    for (acct, source, file) in [(a.ing, SourceKind::Csv, "ING"), (a.spaar, SourceKind::Mt940, "Oranje Spaar"), (a.card, SourceKind::Csv, "ICS")] {
        let mine: Vec<RawTransaction> = rows.iter().filter(|(x, _)| *x == acct).map(|(_, r)| r.clone()).collect();
        if mine.is_empty() {
            continue;
        }
        let tree = b.tree();
        let last = mine.iter().map(|r| r.date).max().unwrap_or_default();
        let closing = b.balances_at(last).shown(&tree, acct) + mine.iter().map(|r| r.amount).sum::<Money>();
        let balances = if source == SourceKind::Mt940 { vec![StatementBalance { date: last, amount: closing }] } else { vec![] };
        let format = if source == SourceKind::Mt940 { "MT940" } else { "CSV" };
        let src = ImportSource {
            file: format!("{file} {tag}.{}", if source == SourceKind::Mt940 { "sta" } else { "csv" }),
            format: format.into(),
            balances,
        };
        b.ingest(acct, &mine, source, &src, now, false)?;
    }
    Ok(())
}

/// Where the household says its money went, by payee.
fn answer_for(b: &Book, payee_norm: &str, n: usize) -> Option<AccountId> {
    let name = match payee_norm {
        p if p.starts_with("WERKGEVER") => "Salary",
        p if p.starts_with("VATTENFALL") => "Energy",
        p if p.starts_with("ZILVEREN KRUIS") => "Health Insurance",
        p if p.starts_with("ZIGGO") => "Internet & Phone",
        p if p.starts_with("NETFLIX") || p.starts_with("SPOTIFY") => "Subscriptions",
        p if p.starts_with("GEMEENTE") => "Municipal Taxes",
        p if p.starts_with("ALBERT HEIJN") || p.starts_with("JUMBO") => "Groceries",
        p if p.starts_with("THUISBEZORGD") => "Eating Out",
        p if p.starts_with("NS") => "Transport",
        p if p.starts_with("KRUIDVAT") => "Personal Care",
        p if p.starts_with("HEMA") => "Kids",
        p if p.starts_with("BOL.COM") => {
            if n.is_multiple_of(2) {
                "Gifts"
            } else {
                "Kids"
            }
        }
        p if p.starts_with("ZALANDO") => "Clothing",
        p if p.starts_with("COOLBLUE") => "Online Shopping",
        "RENTE" => "Other Income",
        _ => return None,
    };
    b.resolve_account(name)
}

fn answer_everything(b: &mut Book, a: &Accounts) -> pecunia_model::Result<()> {
    answer_transfers(b, a, true)?;
    let interest = b.resolve_account("Mortgage Interest");
    let waiting: Vec<_> =
        b.txns().filter(|t| t.status == TxnStatus::ToReview && t.duplicate_of.is_none()).map(|t| (t.id, t.payee_norm.clone())).collect();
    for (n, (id, payee)) in waiting.into_iter().enumerate() {
        if payee.starts_with("ABN AMRO HYPOTHEKEN") {
            if let Some(interest) = interest {
                b.split_txn(id, &[Posting::new(interest, Money(61_230)), Posting::new(a.mortgage, Money(63_270))])?;
                b.move_to_pot(id, a.bills)?;
            }
            continue;
        }
        if let Some(to) = answer_for(b, &payee, n) {
            b.answer(id, to, false)?;
        }
    }
    Ok(())
}

/// Card payments and cash withdrawals (and, with `savings`, savings transfers) are money between own accounts.
fn answer_transfers(b: &mut Book, a: &Accounts, savings: bool) -> pecunia_model::Result<()> {
    let waiting: Vec<_> =
        b.txns().filter(|t| t.status == TxnStatus::ToReview && t.duplicate_of.is_none()).map(|t| (t.id, t.payee_norm.clone(), t.account)).collect();
    for (id, payee, acct) in waiting {
        let to = match payee.as_str() {
            p if p.starts_with("INTERNATIONAL CARD") => Some(a.card),
            p if p.starts_with("GELDAUTOMAAT") => Some(a.wallet),
            p if p.starts_with("BETALING ONTVANGEN") && acct == Some(a.card) => Some(a.ing),
            p if savings && p.starts_with("ING ORANJE SPAAR") && acct == Some(a.ing) => Some(a.spaar),
            p if savings && p.starts_with("ING BETAALREKENING") && acct == Some(a.spaar) => Some(a.ing),
            _ => None,
        };
        if let Some(to) = to {
            b.answer(id, to, false)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_is_valid_and_has_work_in_review() {
        let now = 1_791_936_000_000; // 2026-10-14
        let b = book(now);
        assert_eq!(b.validate(), Ok(()));
        assert!(b.txn_count() > 250, "{}", b.txn_count());
        assert!(b.rules().count() >= 8, "rules were learned: {}", b.rules().count());
        let review = b.review();
        let kinds: std::collections::BTreeSet<_> = review.iter().map(|i| i.kind).collect();
        assert!(kinds.len() >= 3, "{kinds:?}");
        assert!(review.len() < 30, "{}", review.len());
        assert!(b.txns().any(|t| t.status == TxnStatus::Auto), "the second round was partly sorted");
        let tree = b.tree();
        let ing = b.balances().shown(&tree, b.resolve_account("ING Betaalrekening").unwrap());
        assert!(ing.is_positive(), "{ing}");
        let (_, notes) = Book::from_parts(
            b.meta.clone(),
            b.accounts().map(|a| (**a).clone()).collect(),
            b.txns().map(|t| (**t).clone()).collect(),
            b.rules().map(|r| (**r).clone()).collect(),
        );
        assert!(notes.is_empty(), "{notes:?}");
    }
}
