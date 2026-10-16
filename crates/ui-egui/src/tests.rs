//! UI tests with egui_kittest: they drive the app like a person would, by accessibility labels.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pecunia_engine::{Screen, Session};

use crate::PecuniaApp;

fn harness(app: PecuniaApp) -> Harness<'static, PecuniaApp> {
    Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut PecuniaApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    )
}

fn sample() -> PecuniaApp {
    let mut s = Session::sample();
    s.now = || 1_791_936_000_000;
    PecuniaApp::new(s)
}

#[test]
fn every_screen_renders() {
    let mut h = harness(sample());
    h.run_steps(3);
    for screen in Screen::ALL {
        h.state_mut().go(screen);
        h.run_steps(2);
    }
}

#[test]
fn sidebar_navigates_and_shows_an_account() {
    let mut h = harness(sample());
    h.run_steps(3);
    h.get_by_label("Review").click();
    h.run_steps(2);
    assert_eq!(h.state().session.view.screen, Screen::Review);
    h.get_by_label("ING Betaalrekening").click();
    h.run_steps(2);
    let s = &h.state().session;
    assert_eq!(s.view.screen, Screen::Transactions);
    assert_eq!(s.view.filter.account, s.book().resolve_account("ING Betaalrekening"));
}

#[test]
fn accounts_screen_edits_adds_and_shows_a_register() {
    let mut h = harness(sample());
    h.run_steps(3);
    h.state_mut().go(Screen::Accounts);
    h.run_steps(2);
    h.get_by_label("Edit ING Oranje Spaar").click();
    h.run_steps(2);
    assert!(matches!(h.state().ui.dialog, Some(crate::dialogs::Dialog::EditAccount(_))));
    h.state_mut().ui.dialog = None;
    h.run_steps(2);

    h.get_by_label("New category group in Where money goes").click();
    h.run_steps(2);
    assert!(matches!(h.state().ui.dialog, Some(crate::dialogs::Dialog::NewAccount(_))));
    h.state_mut().ui.dialog = None;
    h.run_steps(2);

    let missing =
        pecunia_engine::model::catalogue::ENTRIES.iter().map(|e| e.name).find(|n| h.state().session.book().resolve_account(n).is_none()).unwrap();
    h.get_by_label("Add a usual category").click();
    h.run_steps(2);
    h.get_by_label(&format!("Add {missing}")).click();
    h.run_steps(2);
    assert!(h.state().session.book().resolve_account(missing).is_some());

    let label = {
        let s = &h.state().session;
        let b = s.book();
        let wallet = b.resolve_account("Wallet").unwrap();
        format!("Wallet, Cash, {}", b.balances_at(s.today()).shown(&b.tree(), wallet).format())
    };
    h.get_by_label(&label).click();
    h.run_steps(2);
    let s = &h.state().session;
    assert_eq!(s.view.screen, Screen::Transactions);
    assert_eq!(s.view.filter.account, s.book().resolve_account("Wallet"));
}
