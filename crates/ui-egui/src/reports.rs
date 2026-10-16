//! The Reports screen: income and spending, net worth, cash flow, fixed costs and the bank check, for a chosen period.

use egui::Ui;

use crate::theme::Tokens;
use crate::{PecuniaApp, lists};

pub fn show(_app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    lists::heading(ui, "Reports", "Coming soon.", t);
}
