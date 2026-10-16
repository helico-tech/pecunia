//! CSV statements: the built-in bank profiles, an RFC 4180 reader, the parser, the profile detector and the preview for
//! a column-mapping dialog. From budget.rs `files/csv.rs`.

use std::iter::Peekable;
use std::str::Chars;

use chrono::NaiveDate;
use pecunia_model::{CsvProfile, Money, ParsedStatement, RawTransaction, SignMode, StatementUnreadable};

use crate::{find_iban, is_blank, lines, non_blank, push_unique};

/// The CSV layouts of Dutch banks.
pub(crate) mod profiles {
    use pecunia_model::{CsvProfile, SignMode};

    fn col(name: &str) -> Option<String> {
        Some(name.to_string())
    }

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    pub fn ing() -> CsvProfile {
        CsvProfile {
            delimiter: ';',
            sign: SignMode::Indicator,
            indicator_column: col("Af Bij"),
            debit_values: cols(&["Af"]),
            name_column: col("Naam / Omschrijving"),
            counter_iban_column: col("Tegenrekening"),
            description_columns: cols(&["Mededelingen"]),
            own_iban_column: col("Rekening"),
            ..CsvProfile::new("ING (CSV)", "ING", "Datum", "yyyyMMdd", "Bedrag (EUR)")
        }
    }

    pub fn rabobank() -> CsvProfile {
        CsvProfile {
            name_column: col("Naam tegenpartij"),
            counter_iban_column: col("Tegenrekening IBAN/BBAN"),
            description_columns: cols(&["Omschrijving-1", "Omschrijving-2", "Omschrijving-3"]),
            own_iban_column: col("IBAN/BBAN"),
            external_id_column: col("Volgnr"),
            ..CsvProfile::new("Rabobank (CSV)", "Rabobank", "Datum", "yyyy-MM-dd", "Bedrag")
        }
    }

    pub fn abn_amro() -> CsvProfile {
        CsvProfile {
            delimiter: '\t',
            has_header: false,
            description_columns: cols(&["#7"]),
            ..CsvProfile::new("ABN AMRO (TAB)", "ABN AMRO", "#2", "yyyyMMdd", "#6")
        }
    }

    pub fn volksbank() -> CsvProfile {
        CsvProfile {
            has_header: false,
            decimal_comma: false,
            name_column: col("#3"),
            counter_iban_column: col("#2"),
            description_columns: cols(&["#17"]),
            own_iban_column: col("#1"),
            ..CsvProfile::new("ASN / SNS / RegioBank (CSV)", "de Volksbank", "#0", "dd-MM-yyyy", "#10")
        }
    }

    pub fn triodos() -> CsvProfile {
        CsvProfile {
            has_header: false,
            sign: SignMode::Indicator,
            indicator_column: col("#3"),
            debit_values: cols(&["Debet"]),
            name_column: col("#4"),
            counter_iban_column: col("#5"),
            description_columns: cols(&["#7"]),
            own_iban_column: col("#1"),
            ..CsvProfile::new("Triodos (CSV)", "Triodos", "#0", "dd-MM-yyyy", "#2")
        }
    }

    pub fn bunq() -> CsvProfile {
        CsvProfile {
            delimiter: ';',
            decimal_comma: false,
            name_column: col("Name"),
            counter_iban_column: col("Counterparty"),
            description_columns: cols(&["Description"]),
            own_iban_column: col("Account"),
            ..CsvProfile::new("bunq (CSV)", "bunq", "Date", "yyyy-MM-dd", "Amount")
        }
    }

    pub fn knab() -> CsvProfile {
        CsvProfile {
            delimiter: ';',
            skip_lines: 1,
            sign: SignMode::Indicator,
            indicator_column: col("CreditDebet"),
            debit_values: cols(&["D"]),
            name_column: col("Tegenrekeninghouder"),
            counter_iban_column: col("Tegenrekeningnummer"),
            description_columns: cols(&["Omschrijving"]),
            own_iban_column: col("Rekeningnummer"),
            external_id_column: col("Referentie"),
            ..CsvProfile::new("Knab (CSV)", "Knab", "Transactiedatum", "dd-MM-yyyy", "Bedrag")
        }
    }

    /// ICS credit cards (the cards behind ABN AMRO credit cards). Merchant category and the original currency go into
    /// the description: strong signals for the classifier. The card holder goes into `who`.
    pub fn ics() -> CsvProfile {
        CsvProfile {
            delimiter: ';',
            sign: SignMode::Indicator,
            indicator_column: col("Debit/Credit"),
            debit_values: cols(&["D"]),
            name_column: col("Omschrijving"),
            description_columns: cols(&["Omschrijving", "Merchant categorie", "Land", "Valuta", "Bedrag in oorspronkelijke valuta"]),
            who_column: col("Naam Card-houder"),
            ..CsvProfile::new("ICS creditcard (CSV)", "ICS", "Boekingsdatum", "dd-MM-yyyy", "Bedrag")
        }
    }

    /// In detection order.
    pub fn built_in() -> Vec<CsvProfile> {
        vec![ing(), rabobank(), abn_amro(), volksbank(), triodos(), bunq(), knab(), ics()]
    }
}

/// The rows of CSV text, one at a time: RFC 4180 quoting, any delimiter, `\n`, `\r\n` or `\r` line ends, a leading BOM
/// dropped. Rows with only empty cells are skipped. An unclosed quote runs to the end of the text.
pub(crate) struct CsvRows<'t> {
    chars: Peekable<Chars<'t>>,
    delimiter: char,
}

impl<'t> CsvRows<'t> {
    pub(crate) fn new(text: &'t str, delimiter: char) -> Self {
        CsvRows { chars: text.strip_prefix('\u{feff}').unwrap_or(text).chars().peekable(), delimiter }
    }
}

impl Iterator for CsvRows<'_> {
    type Item = Vec<String>;

    fn next(&mut self) -> Option<Vec<String>> {
        loop {
            let mut row = Vec::new();
            let mut cell = String::new();
            let mut quoted = false;
            let mut line_ended = false;
            while let Some(c) = self.chars.next() {
                if quoted && c == '"' && self.chars.next_if_eq(&'"').is_some() {
                    cell.push('"');
                } else if c == '"' && (cell.is_empty() || quoted) {
                    quoted = !quoted;
                } else if !quoted && c == self.delimiter {
                    row.push(std::mem::take(&mut cell));
                } else if !quoted && (c == '\n' || c == '\r') {
                    if c == '\r' {
                        self.chars.next_if_eq(&'\n');
                    }
                    line_ended = true;
                    break;
                } else {
                    cell.push(c);
                }
            }
            if !line_ended && cell.is_empty() && row.is_empty() {
                return None;
            }
            row.push(cell);
            if row.iter().any(|c| !c.is_empty()) {
                return Some(row);
            }
            if !line_ended {
                return None;
            }
        }
    }
}

/// All rows of CSV text; see [`CsvRows`].
pub(crate) fn read_csv(text: &str, delimiter: char) -> Vec<Vec<String>> {
    CsvRows::new(text, delimiter).collect()
}

const DATE_FORMATS: [&str; 3] = ["yyyyMMdd", "yyyy-MM-dd", "dd-MM-yyyy"];

/// Reads `content` with `profile`. Every data row must have a readable date and amount; the first one that does not
/// fails the file, with its line.
pub(crate) fn parse(profile: &CsvProfile, content: &[u8]) -> Result<ParsedStatement, StatementUnreadable> {
    let encoding = encoding_rs::Encoding::for_label(profile.encoding.trim().as_bytes())
        .ok_or_else(|| StatementUnreadable::new(format!("unknown encoding '{}'", shorten(&profile.encoding))))?;
    let text = encoding.decode_without_bom_handling(content).0;
    let all = lines(&text);
    let body = all.get(profile.skip_lines..).unwrap_or(&[]).join("\n");
    let table = read_csv(&body, profile.delimiter);
    let Some(first) = table.first() else { return Err(StatementUnreadable::new("the file is empty")) };
    let header: Vec<&str> = if profile.has_header { first.iter().map(|h| h.trim()).collect() } else { Vec::new() };
    let data_start = usize::from(profile.has_header);
    let data = table.get(data_start..).unwrap_or(&[]);
    if !data.is_empty() && !DATE_FORMATS.contains(&profile.date_format.as_str()) {
        return Err(StatementUnreadable::new(format!("unknown date format {}", shorten(&profile.date_format))));
    }
    // The rows exactly as read, for the import hash. A cell with a line break shifts them.
    let raw_lines: Vec<&str> = lines(&body).into_iter().filter(|l| !is_blank(l)).collect();
    let header_line = profile.skip_lines.saturating_add(1);

    let idx = |col: &str| -> Result<usize, StatementUnreadable> {
        match col.strip_prefix('#') {
            Some(pos) => pos.parse().map_err(|_| StatementUnreadable::new(format!("column '{}' is not a position", shorten(col)))),
            None => header.iter().position(|h| *h == col).ok_or_else(|| {
                StatementUnreadable::at(header_line, format!("column '{}' not found; this is not a {} file", shorten(col), shorten(&profile.name)))
            }),
        }
    };
    let opt = |col: &Option<String>| col.as_deref().map(idx).transpose();
    let date_i = idx(&profile.date_column)?;
    let amount_i = idx(&profile.amount_column)?;
    let ind_i = opt(&profile.indicator_column)?;
    let name_i = opt(&profile.name_column)?;
    let iban_i = opt(&profile.counter_iban_column)?;
    let desc_i = profile.description_columns.iter().map(|c| idx(c)).collect::<Result<Vec<_>, _>>()?;
    let own_i = opt(&profile.own_iban_column)?;
    let ext_i = opt(&profile.external_id_column)?;
    let who_i = opt(&profile.who_column)?;

    let mut statement = ParsedStatement::default();
    for (n, cells) in data.iter().enumerate() {
        let line_no = profile.skip_lines.saturating_add(data_start + n + 1);
        let cell = |i: Option<usize>| i.and_then(|i| cells.get(i)).map(|c| c.trim()).unwrap_or("");
        let date = parse_date(cell(Some(date_i)), &profile.date_format).map_err(|e| StatementUnreadable::at(line_no, e))?;
        let mut amount = parse_amount(profile, cell(Some(amount_i))).map_err(|e| StatementUnreadable::at(line_no, e))?;
        if profile.sign == SignMode::Indicator {
            let ind = cell(ind_i).to_lowercase();
            let debit = profile.debit_values.iter().any(|v| v.to_lowercase() == ind);
            amount = if debit { -amount.abs() } else { amount.abs() };
        }
        let description = desc_i.iter().map(|&i| cell(Some(i))).filter(|c| !is_blank(c)).collect::<Vec<_>>().join(" ");
        let own = non_blank(cell(own_i).replace(' ', "").to_uppercase());
        if let Some(own) = &own {
            push_unique(&mut statement.account_ibans, own.clone());
        }
        statement.rows.push(RawTransaction {
            date,
            amount,
            counterparty_name: non_blank(cell(name_i)),
            counterparty_iban: non_blank(cell(iban_i).replace(' ', "").to_uppercase()).or_else(|| find_iban(&description)),
            external_id: non_blank(cell(ext_i)).map(|id| format!("{}:{id}", own.as_deref().unwrap_or("x"))),
            raw_line: raw_lines.get(data_start + n).map(|l| l.to_string()).unwrap_or_else(|| cells.join(&profile.delimiter.to_string())),
            own_iban: own,
            who: non_blank(cell(who_i)),
            description,
        });
    }
    Ok(statement)
}

fn parse_amount(profile: &CsvProfile, text: &str) -> Result<Money, String> {
    if profile.decimal_comma { Money::parse(text) } else { Money::parse(&text.replace(',', "")) }
}

/// At most 60 characters of a text from a file or profile, for an error message.
fn shorten(text: &str) -> String {
    text.chars().take(60).collect()
}

/// Reads a date in one of the profile date formats: `yyyyMMdd`, `yyyy-MM-dd` (the first 10 characters) or
/// `dd-MM-yyyy` (also with `/` or `.`).
pub(crate) fn parse_date(text: &str, format: &str) -> Result<NaiveDate, String> {
    let t = text.trim();
    let bad = || format!("not a date: '{}'", shorten(t));
    let ymd = |y: &str, m: &str, d: &str| -> Result<NaiveDate, String> {
        let (y, m, d) = (y.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
        NaiveDate::from_ymd_opt(y, m, d).ok_or_else(bad)
    };
    match format {
        "yyyyMMdd" => match (t.get(0..4), t.get(4..6), t.get(6..8)) {
            (Some(y), Some(m), Some(d)) => ymd(y, m, d),
            _ => Err(bad()),
        },
        "yyyy-MM-dd" => {
            let first: String = t.chars().take(10).collect();
            NaiveDate::parse_from_str(&first, "%Y-%m-%d").map_err(|_| bad())
        }
        "dd-MM-yyyy" => {
            let mut parts = t.split(['-', '/', '.']);
            match (parts.next(), parts.next(), parts.next()) {
                (Some(d), Some(m), Some(y)) => ymd(y, m, d),
                _ => Err(bad()),
            }
        }
        other => Err(format!("unknown date format {}", shorten(other))),
    }
}

/// Picks the first profile that reads the file with at least one row and fits its shape; `None` when no profile fits
/// (the person maps the columns).
pub(crate) fn detect<'p>(content: &[u8], profiles: &'p [CsvProfile]) -> Option<&'p CsvProfile> {
    let text = String::from_utf8_lossy(content);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    profiles.iter().find(|p| parse(p, content).is_ok_and(|s| !s.rows.is_empty()) && fits_shape(p, text))
}

/// Extra checks so headerless profiles don't claim each other's files.
fn fits_shape(p: &CsvProfile, text: &str) -> bool {
    let skip = p.skip_lines.saturating_add(usize::from(p.has_header));
    let Some(first_data) = lines(text).into_iter().skip(skip).find(|l| !is_blank(l)) else { return false };
    let Some(cells) = CsvRows::new(first_data, p.delimiter).next() else { return false };
    if *p == profiles::abn_amro() {
        cells.len() == 8
    } else if *p == profiles::volksbank() {
        cells.len() >= 18
    } else if *p == profiles::triodos() {
        cells.len() == 8 && cells.get(3).is_some_and(|c| c == "Debet" || c == "Credit")
    } else {
        true
    }
}

/// The first lines of a CSV, for a column-mapping dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvPreview {
    /// The delimiter that splits the rows best: `;`, `,`, tab or `|`.
    pub delimiter: char,
    /// The cells of the first row. For a file without a header these are data; the dialog then maps positions.
    pub headers: Vec<String>,
    /// The rows after the first, at most as many as asked for.
    pub rows: Vec<Vec<String>>,
}

/// In order of preference when two split the rows equally well.
const DELIMITERS: [char; 4] = [';', ',', '\t', '|'];
/// Rows looked at to guess the delimiter.
const SAMPLE_ROWS: usize = 20;

pub(crate) fn preview(content: &[u8], rows: usize) -> Option<CsvPreview> {
    let text = decode_text(content);
    let delimiter = guess_delimiter(&text)?;
    let trimmed = |row: Vec<String>| row.iter().map(|c| c.trim().to_string()).collect::<Vec<_>>();
    let mut table = CsvRows::new(&text, delimiter).map(trimmed);
    let headers = table.next()?;
    Some(CsvPreview { delimiter, headers, rows: table.take(rows).collect() })
}

/// UTF-8 when the bytes are valid UTF-8, else windows-1252 (what Dutch banks write when it is not UTF-8).
fn decode_text(content: &[u8]) -> String {
    match encoding_rs::UTF_8.decode_without_bom_handling_and_without_replacement(content) {
        Some(text) => text.into_owned(),
        None => encoding_rs::WINDOWS_1252.decode_without_bom_handling(content).0.into_owned(),
    }
}

/// The delimiter that gives the first rows two or more cells and the same number most often; on a tie, the one with
/// more cells, then the first in [`DELIMITERS`].
fn guess_delimiter(text: &str) -> Option<char> {
    // `max_by_key` keeps the last of equals, so walk the candidates back to front.
    DELIMITERS
        .iter()
        .rev()
        .filter_map(|&d| {
            let counts: Vec<usize> = CsvRows::new(text, d).take(SAMPLE_ROWS).map(|r| r.len()).collect();
            let first = *counts.first()?;
            let same = counts.iter().filter(|&&n| n == first).count();
            (first >= 2).then_some((d, same, first))
        })
        .max_by_key(|&(_, same, cells)| (same, cells))
        .map(|(d, _, _)| d)
}
