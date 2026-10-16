//! Writes transactions as bank files, for tests and the sample book: the sample book is built by importing these files
//! through the real parsers. From budget.rs `fakes/src/bank_files.rs`. The CSV layouts follow public descriptions of
//! Dutch bank exports. Synthetic data only: never real bank exports.

use chrono::NaiveDate;
use pecunia_model::Money;

/// The account a file belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct BankFileAccount {
    /// Short key, used in CAMT ids (`ing`).
    pub key: String,
    pub iban: String,
}

/// One transaction to write.
#[derive(Debug, Clone, PartialEq)]
pub struct BankFileTx {
    /// Sequence number (Rabobank Volgnr, Volksbank).
    pub seq: u32,
    pub date: NaiveDate,
    /// Signed for the account: negative is money out.
    pub amount: Money,
    pub name: String,
    pub iban: Option<String>,
    pub description: String,
    /// Stable bank reference (CAMT AcctSvcrRef, Knab Referentie, MT940 `:61:`).
    pub reference: String,
}

/// The kinds of bank file [`write`] can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BankFileFormat {
    IngCsv,
    RabobankCsv,
    AbnAmroTab,
    AsnCsv,
    SnsCsv,
    TriodosCsv,
    BunqCsv,
    KnabCsv,
    Camt053,
    Mt940,
}

impl BankFileFormat {
    pub const ALL: [BankFileFormat; 10] = [
        BankFileFormat::IngCsv,
        BankFileFormat::RabobankCsv,
        BankFileFormat::AbnAmroTab,
        BankFileFormat::AsnCsv,
        BankFileFormat::SnsCsv,
        BankFileFormat::TriodosCsv,
        BankFileFormat::BunqCsv,
        BankFileFormat::KnabCsv,
        BankFileFormat::Camt053,
        BankFileFormat::Mt940,
    ];

    pub fn file_name(self) -> &'static str {
        match self {
            BankFileFormat::IngCsv => "ing.csv",
            BankFileFormat::RabobankCsv => "rabobank.csv",
            BankFileFormat::AbnAmroTab => "abnamro.tab",
            BankFileFormat::AsnCsv => "asn.csv",
            BankFileFormat::SnsCsv => "sns.csv",
            BankFileFormat::TriodosCsv => "triodos.csv",
            BankFileFormat::BunqCsv => "bunq.csv",
            BankFileFormat::KnabCsv => "knab.csv",
            BankFileFormat::Camt053 => "camt053.xml",
            BankFileFormat::Mt940 => "mt940.sta",
        }
    }
}

/// The file bytes: UTF-8, except MT940 in Latin-1. `opening` is the balance before the first transaction.
pub fn write(format: BankFileFormat, account: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> Vec<u8> {
    match format {
        BankFileFormat::IngCsv => ing(account, txs, opening).into_bytes(),
        BankFileFormat::RabobankCsv => rabobank(account, txs, opening).into_bytes(),
        BankFileFormat::AbnAmroTab => abn_amro(account, txs, opening).into_bytes(),
        BankFileFormat::AsnCsv | BankFileFormat::SnsCsv => volksbank(account, txs).into_bytes(),
        BankFileFormat::TriodosCsv => triodos(account, txs).into_bytes(),
        BankFileFormat::BunqCsv => bunq(account, txs).into_bytes(),
        BankFileFormat::KnabCsv => knab(account, txs).into_bytes(),
        BankFileFormat::Camt053 => camt053(account, txs, opening).into_bytes(),
        // Characters outside Latin-1 become '?', as Java's encoder does.
        BankFileFormat::Mt940 => mt940(account, txs, opening).chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect(),
    }
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

fn dmy(d: NaiveDate) -> String {
    d.format("%d-%m-%Y").to_string()
}

/// `1234,56` without sign.
fn comma(m: Money) -> String {
    m.abs().to_decimal().replace('.', ",")
}

/// `-1234.56`.
fn dot(m: Money) -> String {
    format!("{}{}", if m.is_negative() { "-" } else { "" }, m.abs().to_decimal())
}

/// `-1234,56`.
fn signed(m: Money) -> String {
    format!("{}{}", if m.is_negative() { "-" } else { "" }, comma(m))
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Header and rows, cells quoted and joined by `delimiter`, one line each, ending in a newline.
fn table(header: &[&str], rows: Vec<Vec<String>>, delimiter: &str, quote: bool) -> String {
    let line = |cells: Vec<String>| cells.iter().map(|c| if quote { q(c) } else { c.clone() }).collect::<Vec<_>>().join(delimiter);
    let mut lines = vec![line(header.iter().map(|h| h.to_string()).collect())];
    lines.extend(rows.into_iter().map(line));
    lines.join("\n") + "\n"
}

fn ing(a: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> String {
    let header = [
        "Datum",
        "Naam / Omschrijving",
        "Rekening",
        "Tegenrekening",
        "Code",
        "Af Bij",
        "Bedrag (EUR)",
        "Mutatiesoort",
        "Mededelingen",
        "Saldo na mutatie",
        "Tag",
    ];
    let mut balance = opening;
    let rows = txs
        .iter()
        .map(|t| {
            balance += t.amount;
            let card = t.description.starts_with("BEA");
            vec![
                ymd(t.date),
                t.name.clone(),
                a.iban.clone(),
                t.iban.clone().unwrap_or_default(),
                (if card { "BA" } else { "IC" }).into(),
                (if t.amount.is_negative() { "Af" } else { "Bij" }).into(),
                comma(t.amount),
                (if card { "Betaalautomaat" } else { "Incasso" }).into(),
                t.description.clone(),
                signed(balance),
                String::new(),
            ]
        })
        .collect();
    table(&header, rows, ";", true)
}

fn rabobank(a: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> String {
    let header = [
        "IBAN/BBAN",
        "Munt",
        "BIC",
        "Volgnr",
        "Datum",
        "Rentedatum",
        "Bedrag",
        "Saldo na trn",
        "Tegenrekening IBAN/BBAN",
        "Naam tegenpartij",
        "Naam uiteindelijke partij",
        "Naam initiërende partij",
        "BIC tegenpartij",
        "Code",
        "Batch ID",
        "Transactiereferentie",
        "Machtigingskenmerk",
        "Incassant ID",
        "Betalingskenmerk",
        "Omschrijving-1",
        "Omschrijving-2",
        "Omschrijving-3",
        "Reden retour",
        "Oorspr bedrag",
        "Oorspr munt",
        "Koers",
    ];
    let plus_minus = |m: Money| format!("{}{}", if m.is_negative() { "-" } else { "+" }, comma(m));
    let mut balance = opening;
    let rows = txs
        .iter()
        .map(|t| {
            balance += t.amount;
            let mut row = vec![
                a.iban.clone(),
                "EUR".into(),
                "RABONL2U".into(),
                format!("{:018}", t.seq),
                t.date.to_string(),
                t.date.to_string(),
                plus_minus(t.amount),
                plus_minus(balance),
                t.iban.clone().unwrap_or_default(),
                t.name.clone(),
                String::new(),
                String::new(),
                String::new(),
                (if t.description.starts_with("BEA") { "bc" } else { "tb" }).into(),
            ];
            row.extend(std::iter::repeat_n(String::new(), 5));
            row.push(t.description.clone());
            row.extend(std::iter::repeat_n(String::new(), 6));
            row
        })
        .collect();
    table(&header, rows, ",", true)
}

/// ABN AMRO .TAB: no header, tab-separated.
fn abn_amro(a: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> String {
    let account: String = a.iban.chars().skip(a.iban.chars().count().saturating_sub(9)).collect();
    let mut balance = opening;
    txs.iter()
        .map(|t| {
            let start = balance;
            balance += t.amount;
            let cells = [
                account.clone(),
                "EUR".into(),
                ymd(t.date),
                signed(start),
                signed(balance),
                ymd(t.date),
                signed(t.amount),
                format!("{} {}", t.name, t.description).trim().to_string(),
            ];
            cells.join("\t") + "\n"
        })
        .collect()
}

/// ASN / SNS (de Volksbank): no header, comma-separated, decimal point, dd-MM-yyyy. 19 positional columns.
fn volksbank(a: &BankFileAccount, txs: &[BankFileTx]) -> String {
    txs.iter()
        .map(|t| {
            let cells = [
                dmy(t.date),
                a.iban.clone(),
                t.iban.clone().unwrap_or_default(),
                q(&t.name),
                String::new(),
                String::new(),
                String::new(),
                "EUR".into(),
                "0.00".into(),
                "EUR".into(),
                dot(t.amount),
                dmy(t.date),
                dmy(t.date),
                "1234".into(),
                "IOB".into(),
                t.seq.to_string(),
                String::new(),
                q(&t.description),
                "1".into(),
            ];
            cells.join(",") + "\n"
        })
        .collect()
}

/// Triodos: no header, comma-separated, quoted, Debet/Credit indicator, decimal comma.
fn triodos(a: &BankFileAccount, txs: &[BankFileTx]) -> String {
    txs.iter()
        .map(|t| {
            let cells = [
                dmy(t.date),
                a.iban.clone(),
                comma(t.amount),
                (if t.amount.is_negative() { "Debet" } else { "Credit" }).into(),
                t.name.clone(),
                t.iban.clone().unwrap_or_default(),
                "IC".into(),
                t.description.clone(),
            ];
            cells.iter().map(|c| q(c)).collect::<Vec<_>>().join(",") + "\n"
        })
        .collect()
}

fn bunq(a: &BankFileAccount, txs: &[BankFileTx]) -> String {
    let header = ["Date", "Interest Date", "Amount", "Account", "Counterparty", "Name", "Description"];
    let rows = txs
        .iter()
        .map(|t| {
            vec![
                t.date.to_string(),
                t.date.to_string(),
                dot(t.amount),
                a.iban.clone(),
                t.iban.clone().unwrap_or_default(),
                t.name.clone(),
                t.description.clone(),
            ]
        })
        .collect();
    table(&header, rows, ";", true)
}

/// Knab: a "KNAB EXPORT" line before the header, semicolons, no quotes.
fn knab(a: &BankFileAccount, txs: &[BankFileTx]) -> String {
    let header = [
        "Rekeningnummer",
        "Transactiedatum",
        "Valutacode",
        "CreditDebet",
        "Bedrag",
        "Tegenrekeningnummer",
        "Tegenrekeninghouder",
        "Valutadatum",
        "Betaalwijze",
        "Omschrijving",
        "Type betaling",
        "Machtigingsnummer",
        "Incassant ID",
        "Adres",
        "Referentie",
        "Boekdatum",
    ];
    let rows = txs
        .iter()
        .map(|t| {
            let mut row = vec![
                a.iban.clone(),
                dmy(t.date),
                "EUR".into(),
                (if t.amount.is_negative() { "D" } else { "C" }).into(),
                comma(t.amount),
                t.iban.clone().unwrap_or_default(),
                t.name.clone(),
                dmy(t.date),
                "Overboeking".into(),
                t.description.clone(),
            ];
            row.extend(std::iter::repeat_n(String::new(), 4));
            row.extend([t.reference.clone(), dmy(t.date)]);
            row
        })
        .collect();
    "KNAB EXPORT;;;;;;;;;;;;;;;\n".to_string() + &table(&header, rows, ";", false)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn camt053(a: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> String {
    let closing = opening + txs.iter().map(|t| t.amount).sum::<Money>();
    let cd = |m: Money| if m.is_negative() { "DBIT" } else { "CRDT" };
    let entries: String = txs
        .iter()
        .map(|t| {
            let (party, party_acct) = if t.amount.is_negative() { ("Cdtr", "CdtrAcct") } else { ("Dbtr", "DbtrAcct") };
            let acct = t.iban.as_ref().map(|iban| format!("<{party_acct}><Id><IBAN>{iban}</IBAN></Id></{party_acct}>")).unwrap_or_default();
            format!(
                r#"
      <Ntry>
        <Amt Ccy="EUR">{amount}</Amt>
        <CdtDbtInd>{cd}</CdtDbtInd>
        <Sts><Cd>BOOK</Cd></Sts>
        <BookgDt><Dt>{date}</Dt></BookgDt>
        <ValDt><Dt>{date}</Dt></ValDt>
        <AcctSvcrRef>{reference}</AcctSvcrRef>
        <NtryDtls><TxDtls>
          <Refs><EndToEndId>E2E-{reference}</EndToEndId></Refs>
          <RltdPties><{party}><Pty><Nm>{name}</Nm></Pty></{party}>{acct}</RltdPties>
          <RmtInf><Ustrd>{description}</Ustrd></RmtInf>
        </TxDtls></NtryDtls>
      </Ntry>"#,
                amount = dot(t.amount.abs()),
                cd = cd(t.amount),
                date = t.date,
                reference = xml(&t.reference),
                name = xml(&t.name),
                description = xml(&t.description),
            )
        })
        .collect();
    let created = txs.last().map_or("2026-01-01".to_string(), |t| t.date.to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>STMT-{key}</MsgId><CreDtTm>{created}T00:00:00</CreDtTm></GrpHdr>
    <Stmt>
      <Id>{key}-1</Id>
      <Acct><Id><IBAN>{iban}</IBAN></Id><Ccy>EUR</Ccy></Acct>
      <Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">{opening}</Amt><CdtDbtInd>{opening_cd}</CdtDbtInd></Bal>
      <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">{closing}</Amt><CdtDbtInd>{closing_cd}</CdtDbtInd></Bal>{entries}
    </Stmt>
  </BkToCstmrStmt>
</Document>
"#,
        key = xml(&a.key),
        iban = xml(&a.iban),
        opening = dot(opening.abs()),
        opening_cd = cd(opening),
        closing = dot(closing.abs()),
        closing_cd = cd(closing),
    )
}

/// MT940 (ING flavour): `:61:` statement lines with `:86:` details split over 65-character lines (at most 6).
fn mt940(a: &BankFileAccount, txs: &[BankFileTx], opening: Money) -> String {
    let yymmdd = |d: NaiveDate| d.format("%y%m%d").to_string();
    let bal = |tag: &str, d: Option<NaiveDate>, m: Money| {
        let d = d.or(NaiveDate::from_ymd_opt(2026, 1, 1)).unwrap_or_default();
        format!(":{tag}:{}{}EUR{}\n", if m.is_negative() { "D" } else { "C" }, yymmdd(d), comma(m))
    };
    let mut out = String::from("{1:F01INGBNL2AXXXX0000000000}{2:I940INGBNL2AXXXXN}{4:\n");
    out += &format!(":20:P{}\n", txs.first().map_or("0".to_string(), |t| ymd(t.date)));
    out += &format!(":25:{} EUR\n", a.iban);
    out += ":28C:1\n";
    out += &bal("60F", txs.first().map(|t| t.date), opening);
    let mut balance = opening;
    for t in txs {
        balance += t.amount;
        let dc = if t.amount.is_negative() { "D" } else { "C" };
        out += &format!(":61:{}{}{dc}{}NTRFNONREF//{}\n", yymmdd(t.date), t.date.format("%m%d"), comma(t.amount), t.reference);
        let details = format!("/CNTP/{}/INGBNL2A/{}//REMI/USTD//{}/", t.iban.as_deref().unwrap_or(""), t.name, t.description);
        let ascii: Vec<char> = details.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect();
        for (i, chunk) in ascii.chunks(65).take(6).enumerate() {
            out += if i == 0 { ":86:" } else { "" };
            out.extend(chunk);
            out.push('\n');
        }
    }
    out += &bal("62F", txs.last().map(|t| t.date), balance);
    out += "-}\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt940_details_wrap_at_65_characters_in_at_most_6_lines() {
        let account = BankFileAccount { key: "ing".into(), iban: "NL12INGB0001234567".into() };
        let tx = BankFileTx {
            seq: 1,
            date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            amount: Money(-1250),
            name: "Café Zuid".into(),
            iban: None,
            description: "x".repeat(500),
            reference: "ING-20260901-00001".into(),
        };
        let file = write(BankFileFormat::Mt940, &account, &[tx], Money(10000));
        let text: String = file.iter().map(|&b| char::from(b)).collect();
        let details: Vec<&str> =
            text.lines().skip_while(|l| !l.starts_with(":86:")).take_while(|l| !l.starts_with(':') || l.starts_with(":86:")).collect();
        assert_eq!(details.len(), 6);
        assert!(details[0].starts_with(":86:/CNTP//INGBNL2A/Caf? Zuid//REMI/"), "{}", details[0]);
        assert!(details.iter().all(|l| l.trim_start_matches(":86:").len() == 65));
        assert!(text.contains(":61:2609010901D12,50NTRFNONREF//ING-20260901-00001\n"));
        assert!(text.contains(":62F:C260901EUR87,50\n"));
    }
}
