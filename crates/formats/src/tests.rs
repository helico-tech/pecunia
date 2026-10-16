//! The parser tests from budget.rs `files/tests.rs`, plus the preview and hostile input. Made-up data only: never real
//! bank exports.

use std::collections::HashSet;

use chrono::NaiveDate;
use pecunia_model::{CsvProfile, DetectedFormat, FileFormat, Money, ParsedStatement, SignMode, StatementUnreadable};

use super::csv::{profiles, read_csv};
use super::mt940::read_details;
use super::samples::{self, BankFileAccount, BankFileFormat, BankFileTx};
use super::*;

fn account() -> BankFileAccount {
    BankFileAccount { key: "ing".into(), iban: "NL12INGB0001234567".into() }
}

const OPENING: Money = Money(250_000);

/// A synthetic current account: salary first, direct debits, two rows that differ only in the policy number, a card
/// payment, a Tikkie with the IBAN in the text, a balance below zero, quotes, `&` and a non-ASCII name.
fn expected() -> Vec<BankFileTx> {
    let rows: [(&str, i64, &str, Option<&str>, &str); 13] = [
        ("2026-09-01", 341260, "Werkgever B.V.", Some("NL20RABO0123456780"), "Salaris september 2026"),
        ("2026-09-02", -124500, "ABN AMRO Hypotheken", Some("NL86ABNA0555555555"), "Termijn 09-2026 leningdeel 1"),
        ("2026-09-02", -20000, "Oranje Spaarrekening", Some("NL55INGB0008810123"), "Naar Oranje spaarrekening V12345678"),
        ("2026-09-03", -16500, "Vattenfall Klantenservice N.V.", Some("NL60INGB0000123456"), "Termijnbedrag energie"),
        ("2026-09-04", -15620, "Zilveren Kruis N.V.", Some("NL51INGB0000654321"), "Premie polis 123456 september"),
        ("2026-09-04", -15620, "Zilveren Kruis N.V.", Some("NL51INGB0000654321"), "Premie polis 123457 september"),
        ("2026-09-06", -1399, "Netflix International B.V.", None, "Netflix.com september"),
        ("2026-09-08", -425000, "Autobedrijf Zuid & Zn.", Some("NL91ABNA0417164300"), "Factuur 118 \"APK + onderhoud\""),
        ("2026-09-08", -6418, "Albert Heijn 1427", None, "BEA, Apple Pay ALBERT HEIJN 1427,PAS123 NR:CT59X1, 08.09.26/12:47 UTRECHT"),
        (
            "2026-09-13",
            2500,
            "M. Jansen via Tikkie",
            Some("NL13ABNA0123123123"),
            "Tikkie ID 0123456789, Etentje vrijdag, Van M. Jansen, NL13ABNA0123123123",
        ),
        ("2026-09-20", 31200, "Belastingdienst", Some("NL86INGB0002445588"), "TOESLAGEN ZORGTOESLAG 09-2026"),
        ("2026-10-01", 341260, "Werkgever B.V.", Some("NL20RABO0123456780"), "Salaris oktober 2026"),
        ("2026-10-05", -750, "Café de Zwaan", None, "Koffie en taart"),
    ];
    rows.iter()
        .enumerate()
        .map(|(i, &(date, cents, name, iban, description))| {
            let date: NaiveDate = date.parse().unwrap();
            BankFileTx {
                seq: i as u32 + 1,
                date,
                amount: Money(cents),
                name: name.into(),
                iban: iban.map(str::to_string),
                description: description.into(),
                reference: format!("ING-{}-{:05}", date.format("%Y%m%d"), i + 1),
            }
        })
        .collect()
}

fn file(format: BankFileFormat) -> Vec<u8> {
    samples::write(format, &account(), &expected(), OPENING)
}

fn read(bytes: &[u8], format: &DetectedFormat) -> ParsedStatement {
    parse(bytes, format, &[]).unwrap()
}

#[test]
fn every_format_round_trips_dates_and_amounts_and_is_detected() {
    let expected = expected();
    for f in BankFileFormat::ALL {
        let bytes = file(f);
        let detected = detect(&bytes, &[]).unwrap_or_else(|| panic!("{f:?} not detected"));
        let profile = match f {
            BankFileFormat::Camt053 => None,
            BankFileFormat::Mt940 => None,
            BankFileFormat::IngCsv => Some("ING (CSV)"),
            BankFileFormat::RabobankCsv => Some("Rabobank (CSV)"),
            BankFileFormat::AbnAmroTab => Some("ABN AMRO (TAB)"),
            BankFileFormat::AsnCsv | BankFileFormat::SnsCsv => Some("ASN / SNS / RegioBank (CSV)"),
            BankFileFormat::TriodosCsv => Some("Triodos (CSV)"),
            BankFileFormat::BunqCsv => Some("bunq (CSV)"),
            BankFileFormat::KnabCsv => Some("Knab (CSV)"),
        };
        match f {
            BankFileFormat::Camt053 => assert_eq!(detected.format, FileFormat::Camt053),
            BankFileFormat::Mt940 => assert_eq!(detected.format, FileFormat::Mt940),
            _ => assert_eq!(detected.profile.as_deref(), profile, "{f:?}"),
        }
        let rows = read(&bytes, &detected).rows;
        assert_eq!(rows.len(), expected.len(), "{f:?} row count");
        for (e, r) in expected.iter().zip(&rows) {
            assert_eq!(r.date, e.date, "{f:?} date of {}", e.seq);
            assert_eq!(r.amount, e.amount, "{f:?} amount of {}", e.seq);
        }
        // The import hash is made from `raw_line`, so the raw lines must be there and tell the rows apart.
        assert!(rows.iter().all(|r| !r.raw_line.is_empty()), "{f:?} raw lines");
        assert_eq!(rows.iter().map(|r| &r.raw_line).collect::<HashSet<_>>().len(), rows.len(), "{f:?} raw lines");
    }
}

#[test]
fn structured_formats_keep_names_ibans_and_bank_references() {
    let (ing, expected) = (account(), expected());
    let camt = read(&file(BankFileFormat::Camt053), &DetectedFormat::camt053());
    assert_eq!(camt.account_ibans, vec![ing.iban.clone()]);
    assert_eq!(
        camt.rows.iter().map(|r| r.external_id.clone()).collect::<Vec<_>>(),
        expected.iter().map(|e| Some(e.reference.clone())).collect::<Vec<_>>()
    );
    assert_eq!(camt.rows[0].counterparty_name.as_ref(), Some(&expected[0].name));
    assert_eq!(camt.rows[7].counterparty_name.as_deref(), Some("Autobedrijf Zuid & Zn."));
    let mt = read(&file(BankFileFormat::Mt940), &DetectedFormat::mt940());
    assert_eq!(mt.rows[0].counterparty_name.as_ref(), Some(&expected[0].name));
    assert_eq!(mt.rows[0].counterparty_iban, expected[0].iban);
    assert!(mt.rows[0].description.starts_with("Salaris"), "{}", mt.rows[0].description);
    assert_eq!(mt.closing_balances.len(), 1);
    assert!(mt.warnings.is_empty(), "{:?}", mt.warnings);
    let rabo = read(&file(BankFileFormat::RabobankCsv), &DetectedFormat::csv("Rabobank (CSV)"));
    assert!(rabo.rows.iter().all(|r| r.external_id.is_some()));
    let ing_csv = read(&file(BankFileFormat::IngCsv), &DetectedFormat::csv("ING (CSV)"));
    assert_eq!(ing_csv.account_ibans, vec![ing.iban]);
    assert_eq!(ing_csv.rows[0].counterparty_name.as_deref(), Some("Werkgever B.V."));
}

const BROKEN_ING: &str = "\"Datum\";\"Naam / Omschrijving\";\"Rekening\";\"Tegenrekening\";\"Code\";\"Af Bij\";\"Bedrag (EUR)\";\"Mutatiesoort\";\"Mededelingen\"\n\
    \"20261001\";\"AH\";\"NL1\";\"\";\"BA\";\"Af\";\"12,50\";\"x\";\"y\"\n\
    \"2026-13-45\";\"AH\";\"NL1\";\"\";\"BA\";\"Af\";\"12,50\";\"x\";\"y\"\n";

#[test]
fn malformed_input_names_the_line() {
    let e = csv::parse(&profiles::ing(), BROKEN_ING.as_bytes()).unwrap_err();
    assert_eq!(e.line, Some(3), "{e}");
    assert!(camt053::parse(b"<nope/>").is_err());
    let xxe = camt053::parse(b"<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///etc/passwd\">]><x>&e;</x>").unwrap_err();
    assert!(xxe.reason.starts_with("not valid XML"), "{xxe}");
    assert!(detect(b"just,some,text\n1,2,3\n", &[]).is_none());
}

#[test]
fn failures_are_values() {
    let e = parse(BROKEN_ING.as_bytes(), &DetectedFormat::csv("ING (CSV)"), &[]).unwrap_err();
    assert_eq!(e.line, Some(3));
    assert!(e.to_string().starts_with("line 3: "), "{e}");
    let e = parse(b"x", &DetectedFormat::csv("Nope"), &[]).unwrap_err();
    assert_eq!(e, StatementUnreadable::new("unknown CSV profile 'Nope'"));
    let e = parse(b"no statement here", &DetectedFormat::mt940(), &[]).unwrap_err();
    assert_eq!(e.to_string(), "no :61: statement lines: this is not an MT940 file");
    let e = parse(b"", &DetectedFormat::csv("ING (CSV)"), &[]).unwrap_err();
    assert_eq!(e.to_string(), "the file is empty");
}

#[test]
fn csv_reader_handles_quotes_delimiters_and_newlines_in_cells() {
    let rows = read_csv("a;\"b;c\";\"say \"\"hi\"\"\"\r\n\"multi\nline\";x;\n", ';');
    assert_eq!(rows, vec![vec!["a", "b;c", "say \"hi\""], vec!["multi\nline", "x", ""]]);
    assert_eq!(read_csv("\u{feff}a,b\n\n,\n\rc", ','), vec![vec!["a", "b"], vec!["c"]], "BOM dropped, empty rows skipped");
    assert!(read_csv("", ',').is_empty());
}

// From budget.rs `Mt940StylesTest`: made-up examples in the shapes Dutch banks use. Lines wrap at 65 characters,
// even mid-word.

#[test]
fn abn_amro_structured_transfer_and_direct_debit() {
    let transfer = read_details(concat!(
        "/TRTP/SEPA OVERBOEKING/IBAN/NL13ABNA0123123123/BIC/ABNANL2A/NAME/",
        "AAB INZ TIKKIE/REMI/TIKKIE ID 0123456789, ETENTJE, VAN J JANSEN/EREF/12345",
    ));
    assert_eq!(transfer.name.as_deref(), Some("AAB INZ TIKKIE"));
    assert_eq!(transfer.iban.as_deref(), Some("NL13ABNA0123123123"));
    assert!(transfer.description.starts_with("TIKKIE ID 0123456789"));
    let debit = read_details(concat!(
        "/TRTP/SEPA INCASSO ALGEMEEN DOORLOPEND/CSID/NL00ZZZ123456780000/NAME/SELFSTORAGE B.V./MARF/123/REMI/",
        "FACTUUR 77//IBAN/NL92IN",
        "GB0001234567/BIC/INGBNL2A/EREF/680-1",
    ));
    assert_eq!(debit.name.as_deref(), Some("SELFSTORAGE B.V."));
    assert_eq!(debit.iban.as_deref(), Some("NL92INGB0001234567"), "a value wrapped mid-word is joined back");
}

#[test]
fn abn_amro_labelled_direct_debit() {
    let d = read_details(concat!(
        "SEPA INCASSO ALGEMEEN DOORLOPEND INCASSANT: NL00ZZZ123456780000",
        "NAAM: VERZEKERAAR NV             MACHTIGING: 98765           ",
        "OMSCHRIJVING: PREMIE PERIODE 01.01.2026 - 01.",
        "02.2026 VERZEKERAAR              IBAN: NL49ABNA0123456789     ",
        "KENMERK: 555",
    ));
    assert_eq!(d.name.as_deref(), Some("VERZEKERAAR NV"));
    assert_eq!(d.iban.as_deref(), Some("NL49ABNA0123456789"));
    assert!(d.description.starts_with("PREMIE PERIODE 01.01.2026 - 01.02.2026"), "{}", d.description);
}

#[test]
fn card_payment_and_cash_withdrawal() {
    let bea = read_details("BEA, APPLE PAY                   ALBERT HEIJN 1427,PAS555NR:BS123456, 02.01.26/12:47      NIJMEGEN");
    assert_eq!(bea.name.as_deref(), Some("ALBERT HEIJN 1427"));
    assert_eq!(bea.iban, None);
    let gea = read_details("GEA, BETAALPAS                   GELDAUTOMAAT CENTRUM,PAS555NR:123456, 19.05.26/17:03      MUENCHEN, LAND: DEU");
    assert_eq!(gea.name.as_deref(), Some("GELDAUTOMAAT CENTRUM"));
}

#[test]
fn statements_are_checked_against_their_opening_and_closing_balance() {
    let ok = [
        "ABNANL2A",
        "940",
        "ABNANL2A",
        ":20:ABN AMRO BANK NV",
        ":25:123456789",
        ":28:1/1",
        ":60F:C260102EUR100,00",
        ":61:2601020102D25,50N247NONREF",
        ":86:BEA, APPLE PAY                   ALBERT HEIJN 1427,PAS555",
        "NR:BS123456, 02.01.26/12:47      NIJMEGEN",
        ":61:2601020102C10,00N247NONREF",
        ":86:/TRTP/SEPA OVERBOEKING/IBAN/NL13ABNA0123123123/BIC/ABNANL2A/NAME/",
        "J JANSEN/REMI/TERUGBETALING/EREF/NOTPROVIDED",
        ":62F:C260102EUR84,50",
        "-",
    ]
    .join("\r\n");
    let parsed = mt940::parse(ok.as_bytes()).unwrap();
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows.iter().map(|r| r.counterparty_name.as_deref()).collect::<Vec<_>>(), vec![Some("ALBERT HEIJN 1427"), Some("J JANSEN")]);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.closing_balances.len(), 1);
    assert_eq!(parsed.closing_balances[0].amount, Money(8450));
    let wrong = mt940::parse(ok.replace(":62F:C260102EUR84,50", ":62F:C260102EUR84,00").as_bytes()).unwrap();
    assert_eq!(wrong.warnings, vec!["statement ending 2026-01-02: opening € 100,00 + lines −€ 15,50 ≠ closing € 84,00"]);
}

const ICS: &str = "Transactiedatum;Boekingsdatum;Omschrijving;Naam Card-houder;Card nummer;Debit/Credit;Bedrag;Merchant categorie;Land;Valuta;Bedrag in oorspronkelijke valuta;Type transactie;WalletProvider\n\
    20-09-2026;21-09-2026;WEBSHOP AMSTERDAM NLD;\"JANSEN, J\";****1234;D;12,71;Computer Software Stores;USA;USD;14,20;Transaction;null\n\
    22-09-2026;22-09-2026;BETALING ONTVANGEN;\"JANSEN, J\";****1234;C;100,00;;;EUR;100,00;Payment;null\n";

#[test]
fn ics_credit_card_csv_is_detected_and_parsed() {
    let format = detect(ICS.as_bytes(), &[]).unwrap();
    assert_eq!(format.profile.as_deref(), Some("ICS creditcard (CSV)"));
    let rows = read(ICS.as_bytes(), &format).rows;
    assert_eq!(rows.iter().map(|r| r.amount).collect::<Vec<_>>(), vec![Money(-1271), Money(10000)]);
    assert_eq!(rows[0].date.to_string(), "2026-09-21", "booking date");
    assert!(rows[0].description.contains("Computer Software Stores"));
    assert_eq!(rows[0].who.as_deref(), Some("JANSEN, J"), "card holder");
}

// New in Pecunia: saved profiles, the preview and hostile input.

const OWN: &str = "Wanneer|Hoeveel|Wat\n2026-09-01|-12.50|Bakker\n2026-09-02|100.00|Terugbetaling\n";

fn own_profile() -> CsvProfile {
    CsvProfile {
        delimiter: '|',
        decimal_comma: false,
        description_columns: vec!["Wat".into()],
        ..CsvProfile::new("Mijn bank", "Mijn bank", "Wanneer", "yyyy-MM-dd", "Hoeveel")
    }
}

#[test]
fn saved_profiles_are_tried_after_the_built_in_ones() {
    assert_eq!(built_in_profiles().len(), 8);
    assert_eq!(detect(OWN.as_bytes(), &[]), None);
    let format = detect(OWN.as_bytes(), &[own_profile()]).unwrap();
    assert_eq!(format, DetectedFormat::csv("Mijn bank"));
    let rows = parse(OWN.as_bytes(), &format, &[own_profile()]).unwrap().rows;
    assert_eq!(rows.iter().map(|r| r.amount).collect::<Vec<_>>(), vec![Money(-1250), Money(10000)]);
    assert_eq!(rows[0].description, "Bakker");
    // A saved copy of a built-in layout does not take the bank's file away from the built-in profile.
    let copy = CsvProfile { name: "Kopie".into(), ..profiles::ing() };
    assert_eq!(detect(&file(BankFileFormat::IngCsv), &[copy]), Some(DetectedFormat::csv("ING (CSV)")));
}

#[test]
fn amounts_beyond_the_cap_fail_the_row() {
    let csv = "Wanneer|Hoeveel|Wat\n2026-09-01|-12.50|Bakker\n2026-09-02|200000000000.00|Te veel\n";
    let e = parse(csv.as_bytes(), &DetectedFormat::csv("Mijn bank"), &[own_profile()]).unwrap_err();
    assert_eq!(e.line, Some(3), "{e}");
    let mt = [":20:X", ":25:NL12INGB0001234567", ":61:2601020102D99999999999999999,00NTRFNONREF", "-}"].join("\n");
    assert!(mt940::parse(mt.as_bytes()).is_err());
    let camt = String::from_utf8(file(BankFileFormat::Camt053)).unwrap().replacen("3412.60", "100000000001.00", 1);
    assert!(camt053::parse(camt.as_bytes()).is_err());
}

#[test]
fn preview_guesses_the_delimiter_and_shows_the_first_rows() {
    let p = preview_csv(&file(BankFileFormat::IngCsv), 2).unwrap();
    assert_eq!(p.delimiter, ';');
    assert_eq!(p.headers.first().map(String::as_str), Some("Datum"));
    assert_eq!(p.headers.len(), 11);
    assert_eq!(p.rows.len(), 2);
    assert_eq!(p.rows[0][0], "20260901");
    let rabo = preview_csv(&file(BankFileFormat::RabobankCsv), 100).unwrap();
    assert_eq!((rabo.delimiter, rabo.headers.len(), rabo.rows.len()), (',', 26, 13), "decimal commas inside quotes");
    assert_eq!(preview_csv(&file(BankFileFormat::AbnAmroTab), 1).unwrap().delimiter, '\t');
    let own = preview_csv(OWN.as_bytes(), 5).unwrap();
    assert_eq!((own.delimiter, own.headers.clone(), own.rows.len()), ('|', vec!["Wanneer".into(), "Hoeveel".into(), "Wat".into()], 2));
    // windows-1252 text is read as such: 0xE9 is é.
    let latin = b"Naam;Bedrag\nCaf\xe9;1,00\n";
    assert_eq!(preview_csv(latin, 5).unwrap().rows, vec![vec!["Café".to_string(), "1,00".to_string()]]);
    assert_eq!(preview_csv(b"", 5), None);
    assert_eq!(preview_csv(b"just one column\nof text\n", 5), None);
}

#[test]
fn deep_xml_is_refused_before_it_can_overflow_the_stack() {
    let nested =
        |depth: usize, open: &str, close: &str| format!("<BkToCstmrStmt>{}{}</BkToCstmrStmt>", open.repeat(depth - 1), close.repeat(depth - 1));
    assert_eq!(camt053::nesting_depth(&nested(10, "<a x='/>' y=\">\"><!-- <b> --><![CDATA[<c>]]>", "<?pi <d> ?></a>")), 10);
    assert_eq!(camt053::nesting_depth(&nested(10, "<a/><b x='1'/>", "")), 1, "empty elements do not nest");
    assert_eq!(camt053::nesting_depth(&String::from_utf8(file(BankFileFormat::Camt053)).unwrap()), 10);
    let e = camt053::parse(nested(100_000, "<a>", "</a>").as_bytes()).unwrap_err();
    assert!(e.reason.contains("nested more than"), "{e}");
    // The deepest file that is let through parses on a small stack (the browser has 1 MB).
    let at_limit = nested(camt053::MAX_DEPTH, "<a x='1'>", "</a>");
    let small_stack = std::thread::Builder::new().stack_size(512 * 1024).spawn(move || camt053::parse(at_limit.as_bytes()).map(|_| ()));
    let e = small_stack.unwrap().join().unwrap().unwrap_err();
    assert!(e.reason.starts_with("no <Stmt> found"), "{e}");
}

/// A small deterministic generator, so the garbage is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn bytes(&mut self, n: usize, alphabet: &[u8]) -> Vec<u8> {
        (0..n).map(|_| alphabet[self.next() as usize % alphabet.len()]).collect()
    }
}

fn hostile_profiles() -> Vec<CsvProfile> {
    let base = own_profile();
    vec![
        CsvProfile { encoding: "nonsense".into(), ..base.clone() },
        CsvProfile { encoding: "UTF-16LE".into(), ..base.clone() },
        CsvProfile { delimiter: '"', ..base.clone() },
        CsvProfile { delimiter: '\n', ..base.clone() },
        CsvProfile { skip_lines: usize::MAX, ..base.clone() },
        CsvProfile {
            has_header: false,
            date_column: "#18446744073709551615".into(),
            amount_column: "#99999999999999999999999".into(),
            ..base.clone()
        },
        CsvProfile { has_header: false, date_column: "#-1".into(), amount_column: "#".into(), ..base.clone() },
        CsvProfile { date_format: "yyyy/ww".into(), ..base.clone() },
        CsvProfile { date_column: "x".repeat(100_000), ..base.clone() },
        CsvProfile { sign: SignMode::Indicator, indicator_column: Some("Wat".into()), debit_values: vec![String::new()], ..base },
    ]
}

/// Runs everything on `content`; only a panic fails.
fn poke(content: &[u8], extra: &[CsvProfile]) {
    let _ = detect(content, extra);
    let _ = detect(content, &[]);
    let mut formats = vec![
        DetectedFormat::camt053(),
        DetectedFormat::mt940(),
        DetectedFormat::csv("nope"),
        DetectedFormat { format: FileFormat::Csv, profile: None },
    ];
    formats.extend(built_in_profiles().iter().chain(extra).map(|p| DetectedFormat::csv(&p.name)));
    for format in &formats {
        let _ = parse(content, format, extra);
    }
    for rows in [0, 3, usize::MAX] {
        let _ = preview_csv(content, rows);
    }
}

#[test]
fn hostile_input_never_panics() {
    let extra = hostile_profiles();
    let mut rng = Lcg(42);
    let mut inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"\xef\xbb\xbf".to_vec(),
        b"\xff\xfe\x00\x00".to_vec(),
        b"\n\r\n\r\r\n".repeat(1000),
        b"\"".repeat(10_000),
        b";;;;,,,,\t\t\t||||".repeat(1000),
        vec![0u8; 50_000],
        vec![b'a'; 3_000_000],
        b"Datum;Bedrag (EUR)\n".iter().chain(&[b'9'; 1_000_000]).copied().collect(),
        // Invalid UTF-8 and a wrong encoding: an ING file in UTF-16, a CAMT file in windows-1252 with an é in it.
        b"\xc3\x28\xa0\xa1\xe2\x28\xa1\xf0\x28\x8c\xbc\xf8\xa1\xa1\xa1\xa1".repeat(100),
        String::from_utf8(file(BankFileFormat::IngCsv)).unwrap().encode_utf16().flat_map(u16::to_le_bytes).collect(),
        String::from_utf8(file(BankFileFormat::Camt053)).unwrap().chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect(),
        // Mangled MT940.
        b":20:X\n:25:\n:60F:C999999EUR1,00\n:61:\n:61:991399D1,\n:62F:DXXXXXXEUR,\n".to_vec(),
        b":20:X\n:60F:C260102EUR99999999999999999999,00\n:61:2601020102C1,00\n:62F:C260102EUR1,00\n".to_vec(),
        b"{4:\n:20:\n:61:2601020102RD1,00NTRF\n:86:/CNTP/////REMI/\n:86:NAAM:IBAN:OMSCHRIJVING:\n-}".to_vec(),
        [b":20:X\n:61:2601020102D1,00NTRF\n:86:".as_slice(), &[b'/'; 100_000], b"NAME/".repeat(10_000).as_slice()].concat(),
        b":20:X\n:61:2601020102D1,00NTRF\n:86:BEA, \xe9\xff,PAS1\n".to_vec(),
        // Mangled CAMT.053: deep nesting, entries without parts, a huge amount, bad dates.
        format!("<BkToCstmrStmt>{}{}</BkToCstmrStmt>", "<a>".repeat(5_000), "</a>".repeat(5_000)).into_bytes(),
        b"<BkToCstmrStmt><Stmt><Ntry/><Ntry><Amt>1</Amt></Ntry></Stmt></BkToCstmrStmt>".to_vec(),
        b"<BkToCstmrStmt><Stmt><Ntry><Amt>1e9</Amt><BookgDt><DtTm>x</DtTm></BookgDt></Ntry></Stmt></BkToCstmrStmt>".to_vec(),
        b"<BkToCstmrStmt><Stmt><Ntry><Amt>1,00</Amt><BookgDt><Dt>2026-02-30</Dt></BookgDt></Ntry></Stmt></BkToCstmrStmt>".to_vec(),
        b"<?xml version=\"1.0\"?><!DOCTYPE lol [<!ENTITY a \"aaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;\">]><BkToCstmrStmt>&b;</BkToCstmrStmt>".to_vec(),
        // CSV rows that are almost right.
        b"\"Datum\";\"Bedrag (EUR)\"\n\"20269999\";\"1,00\"\n".to_vec(),
        b"Datum;Naam / Omschrijving;Rekening;Tegenrekening;Code;Af Bij;Bedrag (EUR);Mutatiesoort;Mededelingen\n20260101;;;;;Af;99999999999999999999999,99;;\n".to_vec(),
        "01-01-2026,NL1,,\"x\",,,,EUR,0.00,EUR,-1.00,,,,,,,\"\u{1F4A9}\",1\n".as_bytes().to_vec(),
        b"\"20260101\"\t\"EUR\"\t\"2026\xff\"\n".to_vec(),
    ];
    for f in BankFileFormat::ALL {
        let bytes = file(f);
        // Truncated at many points, including in the middle of a multi-byte character.
        inputs.extend((0..bytes.len()).step_by(97).chain(0..40).map(|cut| bytes.get(..cut).unwrap().to_vec()));
        // Random bytes flipped.
        for _ in 0..20 {
            let mut mangled = bytes.clone();
            for _ in 0..10 {
                let at = rng.next() as usize % mangled.len();
                mangled[at] = rng.next() as u8;
            }
            inputs.push(mangled);
        }
    }
    for n in [1, 10, 100, 10_000] {
        inputs.push(rng.bytes(n, &(0..=255).collect::<Vec<u8>>()));
        inputs.push(rng.bytes(n, b"0123456789,.;-\"\n:/CDNTRFBEA<>"));
    }
    for input in &inputs {
        poke(input, &extra);
    }
    // Whatever comes back must be well-formed: rows within the amount cap.
    for input in &inputs {
        if let Some(format) = detect(input, &extra)
            && let Ok(statement) = parse(input, &format, &extra)
        {
            assert!(statement.rows.iter().all(|r| r.amount.is_sane()));
        }
    }
}
