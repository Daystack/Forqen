//! The interactive rebase editor.
//!
//! A dialog rather than a page: a rebase is a modal operation with a beginning
//! and an end, and it rewrites the history every other view is showing.
//!
//! Reordering is done with buttons, not drag-and-drop. Dragging looks better in
//! a screenshot, but a rebase plan is a keyboard-shaped task — the same hands
//! that typed the commits reorder them — and a drag target that misses by four
//! pixels silently reorders the wrong pair.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use git::rebase::{self, Action, Outcome, Plan};

use crate::state::AppState;

pub struct RebaseDialog {
    window: adw::Window,
    state: AppState,
    list: gtk::ListBox,
    status: gtk::Label,
    start_btn: gtk::Button,
    continue_btn: gtk::Button,
    skip_btn: gtk::Button,
    abort_btn: gtk::Button,
    plan: Rc<RefCell<Plan>>,
    onto: RefCell<String>,
    on_change: Rc<dyn Fn()>,
}

impl RebaseDialog {
    /// Present the editor for the commits between `onto` and HEAD.
    pub fn present(
        parent: &impl IsA<gtk::Window>,
        state: AppState,
        onto: &str,
        on_change: Rc<dyn Fn()>,
    ) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Interactive rebase")
            .default_width(720)
            .default_height(560)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.add_css_class("dim-label");

        let start_btn = gtk::Button::with_label("Start rebase");
        start_btn.add_css_class("suggested-action");

        // Only meaningful once a rebase has stopped, so they begin hidden
        // rather than merely insensitive — three dead buttons on first open
        // would suggest the dialog is broken.
        let continue_btn = gtk::Button::with_label("Continue");
        continue_btn.add_css_class("suggested-action");
        continue_btn.set_visible(false);
        let skip_btn = gtk::Button::with_label("Skip commit");
        skip_btn.set_visible(false);
        let abort_btn = gtk::Button::with_label("Abort");
        abort_btn.add_css_class("destructive-action");
        abort_btn.set_visible(false);

        let dialog = Rc::new(Self {
            window: window.clone(),
            state,
            list: list.clone(),
            status: status.clone(),
            start_btn: start_btn.clone(),
            continue_btn: continue_btn.clone(),
            skip_btn: skip_btn.clone(),
            abort_btn: abort_btn.clone(),
            plan: Rc::new(RefCell::new(Plan::default())),
            onto: RefCell::new(onto.to_string()),
            on_change,
        });

        {
            let this = dialog.clone();
            start_btn.connect_clicked(move |_| this.start());
        }
        {
            let this = dialog.clone();
            continue_btn.connect_clicked(move |_| this.step(Step::Continue));
        }
        {
            let this = dialog.clone();
            skip_btn.connect_clicked(move |_| this.step(Step::Skip));
        }
        {
            let this = dialog.clone();
            abort_btn.connect_clicked(move |_| this.step(Step::Abort));
        }

        window.set_content(Some(&build_layout(
            &list,
            &status,
            &start_btn,
            &continue_btn,
            &skip_btn,
            &abort_btn,
            onto,
        )));

        dialog.load();
        window.present();
    }

    fn load(self: &Rc<Self>) {
        let onto = self.onto.borrow().clone();
        let plan = self
            .state
            .with(|s| Plan::from_range(&s.repo, &onto))
            .and_then(Result::ok);

        match plan {
            Some(plan) => {
                *self.plan.borrow_mut() = plan;
                self.rebuild();
            }
            None => {
                self.status
                    .set_text(&format!("Could not read the commits above {onto}"));
                self.start_btn.set_sensitive(false);
            }
        }

        // A rebase already in progress means the last one stopped; offer the
        // controls for it rather than a fresh plan the user cannot start.
        if self
            .state
            .with(|s| rebase::in_progress(&s.repo))
            .unwrap_or(false)
        {
            self.show_stopped("A rebase is already in progress");
        }
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let plan = self.plan.borrow().clone();
        for (i, step) in plan.steps.iter().enumerate() {
            self.list.append(&self.row(i, step, plan.steps.len()));
        }

        match plan.problem() {
            Some(problem) => {
                self.status.set_text(problem);
                self.status.add_css_class("error");
                self.start_btn.set_sensitive(false);
            }
            None => {
                let folds = plan.steps.iter().filter(|s| s.action.is_fold()).count();
                let drops = plan
                    .steps
                    .iter()
                    .filter(|s| s.action == Action::Drop)
                    .count();
                self.status
                    .set_text(&summary_line(plan.steps.len(), folds, drops));
                self.status.remove_css_class("error");
                self.start_btn.set_sensitive(true);
            }
        }
    }

    fn row(
        self: &Rc<Self>,
        index: usize,
        step: &git::rebase::Step,
        total: usize,
    ) -> gtk::ListBoxRow {
        let actions: Vec<&str> = Action::all().iter().map(|a| a.keyword()).collect();
        let chooser = gtk::DropDown::from_strings(&actions);
        chooser.set_selected(
            Action::all()
                .iter()
                .position(|a| *a == step.action)
                .unwrap_or(0) as u32,
        );
        {
            let this = self.clone();
            chooser.connect_selected_notify(move |c| {
                let picked = Action::all()[c.selected() as usize];
                if let Some(s) = this.plan.borrow_mut().steps.get_mut(index) {
                    s.action = picked;
                }
                // Rebuilding re-runs validation, which is what decides whether
                // Start is live — the reason this is not just a label update.
                this.rebuild();
            });
        }

        let sha = gtk::Label::new(Some(&step.id.short()));
        sha.add_css_class("monospace");
        sha.add_css_class("dim-label");

        let summary = gtk::Label::new(Some(&step.summary));
        summary.set_xalign(0.0);
        summary.set_hexpand(true);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if step.action == Action::Drop {
            // Struck through rather than hidden: the plan is a record of what
            // was decided, and a vanished row cannot be undone by eye.
            summary.add_css_class("dim-label");
        }

        let up = crate::commands::icon_button("go-up-symbolic", "Move earlier");
        up.add_css_class("flat");
        up.set_sensitive(index > 0);
        {
            let this = self.clone();
            up.connect_clicked(move |_| this.move_step(index, index.saturating_sub(1)));
        }

        let down = crate::commands::icon_button("go-down-symbolic", "Move later");
        down.add_css_class("flat");
        down.set_sensitive(index + 1 < total);
        {
            let this = self.clone();
            down.connect_clicked(move |_| this.move_step(index, index + 1));
        }

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row_box.set_margin_top(6);
        row_box.set_margin_bottom(6);
        row_box.append(&chooser);
        row_box.append(&sha);
        row_box.append(&summary);
        row_box.append(&up);
        row_box.append(&down);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        row
    }

    fn move_step(self: &Rc<Self>, from: usize, to: usize) {
        self.plan.borrow_mut().move_step(from, to);
        self.rebuild();
        // Keep the moved row selected so a second press continues moving the
        // same commit rather than whatever landed under the pointer.
        if let Some(row) = self.list.row_at_index(to as i32) {
            self.list.select_row(Some(&row));
        }
    }

    fn start(self: &Rc<Self>) {
        let onto = self.onto.borrow().clone();
        let plan = self.plan.borrow().clone();

        let result = self.state.with(|s| rebase::run(&s.repo, &onto, &plan));
        (self.on_change)();

        match result {
            Some(Ok(Outcome::Complete)) => {
                self.window.close();
            }
            Some(Ok(Outcome::Stopped { reason })) => self.show_stopped(&reason),
            Some(Err(e)) => {
                self.status.set_text(&e.to_string());
                self.status.add_css_class("error");
            }
            None => self.status.set_text("No repository open"),
        }
    }

    /// Switch the dialog into mid-rebase mode.
    fn show_stopped(self: &Rc<Self>, reason: &str) {
        self.status.set_text(reason);
        self.status.add_css_class("error");
        self.start_btn.set_visible(false);
        self.continue_btn.set_visible(true);
        self.skip_btn.set_visible(true);
        self.abort_btn.set_visible(true);
        // The plan is now git's to finish; editing it would have no effect.
        self.list.set_sensitive(false);
    }

    fn step(self: &Rc<Self>, step: Step) {
        let result = self.state.with(|s| match step {
            Step::Continue => rebase::cont(&s.repo),
            Step::Skip => rebase::skip(&s.repo),
            Step::Abort => rebase::abort(&s.repo).map(|()| Outcome::Complete),
        });
        (self.on_change)();

        match result {
            Some(Ok(Outcome::Complete)) => self.window.close(),
            Some(Ok(Outcome::Stopped { reason })) => self.status.set_text(&reason),
            Some(Err(e)) => self.status.set_text(&e.to_string()),
            None => self.status.set_text("No repository open"),
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    Continue,
    Skip,
    Abort,
}

/// One line describing what the plan will do.
pub fn summary_line(total: usize, folds: usize, drops: usize) -> String {
    // Saturating: a caller passing more drops than commits is nonsense, but it
    // must render a wrong number rather than panic — this runs on every
    // keystroke in the action dropdowns.
    let kept = total.saturating_sub(drops);
    let resulting = kept.saturating_sub(folds);
    match (folds, drops) {
        (0, 0) => format!("{total} commits, replayed unchanged"),
        _ => format!("{total} commits → {resulting}: {folds} folded, {drops} dropped"),
    }
}

fn build_layout(
    list: &gtk::ListBox,
    status: &gtk::Label,
    start_btn: &gtk::Button,
    continue_btn: &gtk::Button,
    skip_btn: &gtk::Button,
    abort_btn: &gtk::Button,
    onto: &str,
) -> gtk::Widget {
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(
        "Interactive rebase",
        &format!("onto {onto}"),
    )));

    let hint = gtk::Label::new(Some(
        "Commits are listed oldest first, the order git replays them. \
         Squash and fixup fold into the commit above.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");

    let scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    status.set_hexpand(true);
    actions.append(status);
    actions.append(abort_btn);
    actions.append(skip_btn);
    actions.append(continue_btn);
    actions.append(start_btn);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.append(&hint);
    content.append(&scroll);
    content.append(&actions);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    view.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_plan_says_so_plainly() {
        assert_eq!(summary_line(4, 0, 0), "4 commits, replayed unchanged");
    }

    #[test]
    fn folding_and_dropping_report_the_resulting_count() {
        // 5 commits, 1 folded into its predecessor, 1 dropped → 3 remain.
        assert_eq!(summary_line(5, 1, 1), "5 commits → 3: 1 folded, 1 dropped");
        assert_eq!(summary_line(3, 0, 1), "3 commits → 2: 0 folded, 1 dropped");
        assert_eq!(summary_line(3, 1, 0), "3 commits → 2: 1 folded, 0 dropped");
    }

    #[test]
    fn the_count_never_underflows() {
        // Nonsensical inputs must not panic in a release build's debug assert
        // or wrap around to a huge number in the label.
        assert!(summary_line(1, 5, 5).contains("→ 0"));
    }

    #[test]
    fn every_action_has_a_keyword_the_dropdown_can_show() {
        let words: Vec<&str> = Action::all().iter().map(|a| a.keyword()).collect();
        assert_eq!(words, ["pick", "reword", "edit", "squash", "fixup", "drop"]);
        // The dropdown maps by index, so order must match `all()` exactly.
        assert_eq!(Action::all()[0], Action::Pick);
        assert_eq!(Action::all()[5], Action::Drop);
    }
}
