//! The Accounts and categories screen: the whole account tree with balances, and adding or editing accounts, pots and categories.

use egui::Ui;

use crate::theme::Tokens;
use crate::{PecuniaApp, lists};

pub fn show(_app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    lists::heading(ui, "Accounts", "Coming soon.", t);
}
