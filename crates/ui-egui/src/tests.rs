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
fn reports_switch_step_and_open_a_category() {
    use pecunia_engine::model::NaiveDate;
    use pecunia_engine::view::ReportKind;
    let mut h = harness(sample());
    h.run_steps(3);
    h.state_mut().run("view.report", serde_json::json!({ "report": "incomeExpenses", "from": "2026-09-01", "to": "2026-09-30" }));
    h.run_steps(2);
    h.get_by_label("Previous period").click();
    h.run_steps(2);
    assert_eq!(h.state().session.view.from, NaiveDate::from_ymd_opt(2026, 8, 1));
    assert_eq!(h.state().session.view.to, NaiveDate::from_ymd_opt(2026, 8, 31));
    h.get_by_label_contains("Groceries").click();
    h.run_steps(2);
    let s = &h.state().session;
    assert_eq!(s.view.screen, Screen::Transactions);
    assert_eq!(s.view.filter.account, s.book().resolve_account("Groceries"));
    assert_eq!(s.view.filter.from, NaiveDate::from_ymd_opt(2026, 8, 1));
    h.state_mut().go(Screen::Reports);
    h.run_steps(2);
    let tabs = [
        ("Net worth", ReportKind::NetWorth),
        ("Cash flow", ReportKind::CashFlow),
        ("Fixed costs", ReportKind::FixedCosts),
        ("Bank check", ReportKind::BankCheck),
    ];
    for (label, kind) in tabs {
        h.get_by_label(label).click();
        h.run_steps(2);
        assert_eq!(h.state().session.view.report, kind);
    }
}
