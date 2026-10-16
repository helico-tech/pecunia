//! The Pots screen: every account with money you have, its pots with balances and goals, and moving money between them.

use egui::Ui;

use crate::theme::Tokens;
use crate::{PecuniaApp, lists};

pub fn show(_app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    lists::heading(ui, "Pots", "Coming soon.", t);
}
