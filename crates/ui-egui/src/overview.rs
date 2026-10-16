//! The Overview screen: net worth, free to spend, this month and last, balances over the last year, and what needs attention.

use egui::Ui;

use crate::theme::Tokens;
use crate::{PecuniaApp, lists};

pub fn show(_app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    lists::heading(ui, "Overview", "Coming soon.", t);
}
