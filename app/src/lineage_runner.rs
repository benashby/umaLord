//! The Lineage page's One runner view: one runner's parent plan (`plans/parents.json`), step by
//! step. The current step's Next career card and its progress against the odds come first; past
//! the step's bad-luck mark the Stuck panel opens (settle with the closest uma, try another
//! route, or keep going), each choice previewed before it changes anything (round 8, section 3).
//! The step list, the plan's choices (pins and steps set aside) and its history fill the
//! overview pane.

use crate::App;
use crate::parentage::brief::Brief;
use crate::parentage::jobs::{Avoid, Run};
use crate::parentage::track::{self, ParentPlan};
use crate::parentage::{Goal, uma_name};
use crate::theme::{pal, size};
use crate::ui_util::semibold;
use eframe::egui::{self, CornerRadius, RichText, Stroke, vec2};

/// What a Stuck panel preview would do, waiting for the player's go-ahead.
#[derive(Clone)]
pub(crate) enum Pending {
    Settle(u64),
    Route,
}

#[derive(Default)]
pub(crate) struct RunnerState {
    pub loaded: bool,
    pub plan: Option<ParentPlan>,
    /// The goal picker's choices: runner, surface (0 Turf, 1 Dirt), distance (0-3), style (0-3).
    pub pick: (Option<u32>, usize, usize, usize),
    /// The current step's brief, keyed by the step it was made for.
    pub brief: Option<(String, Brief)>,
    /// A previewed choice: what it is, the steps after, and the planner's words.
    pub preview: Option<(Pending, Vec<Run>, String)>,
    pub stuck_open: Option<bool>,
    pub message: Option<String>,
}

impl App {
    pub(crate) fn lineage_runner(&mut self, ui: &mut egui::Ui) {
        if !self.lineage.runner.loaded {
            self.lineage.runner.loaded = true;
            self.lineage.runner.plan = self.root.as_deref().and_then(track::load);
            // UMALORD_RUNNER=<outfit>[:<target>,…]: a plan in memory, for screenshots (targets are
            // APTITUDES indices; her best surface, distance and style when left out).
            if self.lineage.runner.plan.is_none()
                && let Ok(v) = std::env::var("UMALORD_RUNNER")
            {
                let (id, targets) = v.split_once(':').map_or((v.as_str(), None), |(a, b)| (a, Some(b.split(',').filter_map(|t| t.parse().ok()).collect())));
                if let Ok(id) = id.parse::<u32>() {
                    self.lineage.runner.plan = self.plan_for(id, targets).ok();
                }
            }
        }
        if self.lineage.runner.plan.is_none() {
            self.goal_picker(ui);
            return;
        }
        // UMALORD_PREVIEW=route: open the another-route preview, for screenshots.
        if std::env::var("UMALORD_PREVIEW").is_ok_and(|v| v == "route") && self.lineage.runner.preview.is_none() && self.lineage.runner.message.is_none() {
            let (inv, vets) = (self.parent_inventory(), self.veterans());
            if let Some(plan) = &self.lineage.runner.plan {
                match plan.preview_another_route(&self.kb, &inv, &vets, &crate::ui_util::today()) {
                    Ok((after, msg)) => self.lineage.runner.preview = Some((Pending::Route, after, msg)),
                    Err(e) => self.lineage.runner.message = Some(e),
                }
            }
        }
        let plan = self.lineage.runner.plan.clone().unwrap_or_else(|| unreachable!());
        let vets = self.veterans();
        let inv = self.parent_inventory();
        // The current step's brief, made once per step.
        let current = plan.current();
        if let Some(i) = current {
            let key = format!("{i}:{:?}", plan.steps[i].run);
            if self.lineage.runner.brief.as_ref().is_none_or(|(k, _)| *k != key) {
                let b = self.parent_brief(&plan.goal, &plan.steps[i].run, &plan.inventory(&self.kb, &inv), i);
                self.lineage.runner.brief = Some((key, b));
            }
        }
        let brief = self.lineage.runner.brief.as_ref().map(|(_, b)| b.clone()).filter(|_| current.is_some());
        let p = pal(ui);
        self.panes_with(
            ui,
            0.62,
            |this, ui| {
                ui.horizontal_wrapped(|ui| {
                    this.avatar(ui, plan.goal.runner, 32.0);
                    ui.label(semibold(format!("Parent plan for {}", uma_name(&this.kb, plan.goal.runner)), size::XL));
                    ui.label(RichText::new(format!("{} at A at the start", plan.goal.targets.iter().map(|t| crate::parentage::APTITUDES[*t]).collect::<Vec<_>>().join(", "))).size(13.0).color(p.weak));
                    if ui.small_button("Start a new plan").on_hover_text("The current plan moves to plans/past-parent-plans").clicked() {
                        if let Some(root) = this.root.clone() {
                            let _ = track::cancel(&root, &crate::ui_util::today());
                        }
                        this.lineage.runner = RunnerState { loaded: true, ..Default::default() };
                    }
                });
                this.constraints_strip(ui, &plan);
            },
            |this, ui| {
                if let Some(m) = this.lineage.runner.message.clone() {
                    crate::ui_util::card(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(m).size(13.0));
                            if this.lineage.runner.plan.as_ref().is_some_and(|p| p.before.is_some()) && ui.button("↶ Undo").clicked() {
                                this.runner_do(|plan, _, _, _, today| plan.undo(today));
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
                if let Some(why) = &plan.blocked {
                    crate::ui_util::card(ui, |ui| {
                        ui.label(semibold("The plan is waiting for your choice", 15.0));
                        ui.label(RichText::new(why).size(13.0).color(p.warn));
                    });
                    ui.add_space(8.0);
                }
                match (current, &brief) {
                    (Some(i), Some(b)) => {
                        this.step_card(ui, &plan, i, &vets);
                        ui.add_space(12.0);
                        this.next_card(ui, b);
                    }
                    _ if plan.blocked.is_none() => {
                        crate::ui_util::card(ui, |ui| {
                            ui.label(semibold("Every step is done: her lineage is ready.", 15.0));
                            ui.label(RichText::new(format!("Train {} with her planned parents.", uma_name(&this.kb, plan.goal.runner))).size(13.0).color(p.weak));
                        });
                    }
                    _ => {}
                }
            },
            |this, ui| {
                this.step_list(ui, &plan);
                ui.add_space(12.0);
                crate::ui_util::card(ui, |ui| {
                    ui.label(semibold("History", 15.0));
                    for l in plan.log.iter().rev().take(8) {
                        ui.add(egui::Label::new(RichText::new(l).size(12.5).color(p.weak)).wrap());
                    }
                });
            },
        );
    }

    /// A plan for a runner: her best surface, distance and style unless given.
    fn plan_for(&mut self, runner: u32, targets: Option<Vec<usize>>) -> Result<ParentPlan, String> {
        let info = self.kb.umas.get(&runner).ok_or("unknown uma")?;
        let a = &info.aptitude;
        let targets = targets.unwrap_or_else(|| {
            use crate::knowledge::{Distance, Surface, grade_rank};
            let s = Surface::ALL.iter().position(|s| *s == *Surface::ALL.iter().max_by_key(|s| grade_rank(a.surface(**s))).unwrap_or(&Surface::Turf)).unwrap_or(0);
            let d = Distance::ALL.iter().enumerate().max_by_key(|(_, d)| grade_rank(a.distance(**d))).map_or(2, |x| x.0);
            vec![s, 2 + d, 6 + a.best_style() as usize]
        });
        let goal = Goal { runner, targets, label: format!("One runner: {}", uma_name(&self.kb, runner)), ..Default::default() };
        let inv = self.parent_inventory();
        let p = crate::parentage::jobs::propose(&self.kb, &goal, &inv);
        if !p.impossible.is_empty() {
            return Err("Her grades are too far below A for parents to lift.".into());
        }
        Ok(ParentPlan::new(goal, &p, &self.veterans(), &crate::ui_util::today()))
    }

    fn goal_picker(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        crate::ui_util::card(ui, |ui| {
            ui.label(semibold("Plan one runner's parents", 17.0));
            ui.label(RichText::new("Pick a runner and what must start at A. The planner works out the parent careers, cheapest first, and tracks them as you import finished umas.").size(13.0).color(p.weak));
            ui.add_space(8.0);
            let (mut runner, mut surface, mut distance, mut style) = self.lineage.runner.pick;
            let picked_before = runner;
            let owned: Vec<u32> = self.roster.umas.iter().map(|u| u.id).collect();
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("runner_pick").width(260.0).selected_text(runner.map_or("Runner…".into(), |r| uma_name(&self.kb, r))).show_ui(ui, |ui| {
                    for id in &owned {
                        ui.selectable_value(&mut runner, Some(*id), uma_name(&self.kb, *id));
                    }
                });
                for (label, value, options) in [("Surface", &mut surface, &["Turf", "Dirt"][..]), ("Distance", &mut distance, &["Sprint", "Mile", "Medium", "Long"][..]), ("Style", &mut style, &["Front Runner", "Pace Chaser", "Late Surger", "End Closer"][..])] {
                    ui.label(RichText::new(label).size(12.5).color(p.weak));
                    egui::ComboBox::from_id_salt(label).selected_text(options[*value]).show_ui(ui, |ui| {
                        for (k, o) in options.iter().enumerate() {
                            ui.selectable_value(value, k, *o);
                        }
                    });
                }
            });
            // A newly chosen runner starts at her own best grades.
            if runner != picked_before
                && let Some(a) = runner.and_then(|r| self.kb.umas.get(&r)).map(|u| u.aptitude.clone())
            {
                use crate::knowledge::{Distance, Surface, grade_rank};
                surface = Surface::ALL.iter().enumerate().max_by_key(|(_, s)| grade_rank(a.surface(**s))).map_or(0, |x| x.0);
                distance = Distance::ALL.iter().enumerate().max_by_key(|(_, d)| grade_rank(a.distance(**d))).map_or(2, |x| x.0);
                style = a.best_style() as usize;
            }
            self.lineage.runner.pick = (runner, surface, distance, style);
            ui.add_space(8.0);
            let go = ui.add_enabled(runner.is_some(), crate::ui_util::primary_button(ui, "Plan her parents", 14.0)).on_disabled_hover_text("Pick a runner first");
            if let Some(r) = runner.filter(|_| go.clicked()) {
                match self.plan_for(r, Some(vec![surface, 2 + distance, 6 + style])) {
                    Ok(plan) => {
                        if let Some(root) = &self.root {
                            let _ = track::save(root, &plan);
                        }
                        self.lineage.runner.plan = Some(plan);
                    }
                    Err(e) => self.lineage.runner.message = Some(e),
                }
            }
            if let Some(m) = &self.lineage.runner.message {
                ui.label(RichText::new(m).color(p.warn));
            }
        });
    }

    /// Run an action on the plan, save it, and show what it said.
    fn runner_do(&mut self, f: impl FnOnce(&mut ParentPlan, &crate::knowledge::Knowledge, &crate::parentage::Inventory, &[crate::veterans::Veteran], &str) -> Result<String, String>) {
        let Some(mut plan) = self.lineage.runner.plan.take() else { return };
        let inv = self.parent_inventory();
        let vets = self.veterans();
        let msg = f(&mut plan, &self.kb, &inv, &vets, &crate::ui_util::today());
        if let Some(root) = &self.root {
            let _ = track::save(root, &plan);
        }
        self.lineage.runner.plan = Some(plan);
        self.lineage.runner.preview = None;
        self.lineage.runner.message = Some(match msg {
            Ok(m) | Err(m) => m,
        });
    }

    /// The pins and steps set aside, as removable chips.
    fn constraints_strip(&mut self, ui: &mut egui::Ui, plan: &ParentPlan) {
        if plan.pin.avoid.is_empty() && plan.pin.is_empty() {
            return;
        }
        let p = pal(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Your choices:").size(12.5).color(p.weak));
            let mut remove: Option<usize> = None;
            for (i, a) in plan.pin.avoid.iter().enumerate() {
                let text = format!("✕ {} with #{} and #{} set aside", uma_name(&self.kb, a.trainee), a.legacies[0], a.legacies[1]);
                if ui.add(egui::Button::new(RichText::new(text).size(12.0)).corner_radius(CornerRadius::same(12))).on_hover_text("Bring this step back").clicked() {
                    remove = Some(i);
                }
            }
            if let Some(t) = plan.pin.trainee {
                ui.label(RichText::new(format!("📌 {} as the next trainee", uma_name(&self.kb, t))).size(12.0));
            }
            if let Some(i) = remove {
                self.runner_do(|plan, kb, inv, vets, today| {
                    let mut pin = plan.pin.clone();
                    let gone: Avoid = pin.avoid.remove(i);
                    let back = format!("Brought back {}'s step. ", uma_name(kb, gone.trainee));
                    plan.pin.avoid.clear();
                    Ok(back + &plan.set_pin(kb, inv, vets, pin, today))
                });
            }
        });
        ui.add_space(4.0);
    }

    /// The current step: what it needs, careers run against the odds, and the Stuck panel.
    fn step_card(&mut self, ui: &mut egui::Ui, plan: &ParentPlan, i: usize, vets: &[crate::veterans::Veteran]) {
        let p = pal(ui);
        let step = &plan.steps[i];
        let run = &step.run;
        let hits = plan.hits(vets);
        // UMALORD_CAREERS=<n> shows the card n careers in, for screenshots of the later states.
        let n = std::env::var("UMALORD_CAREERS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(hits.len() as u32);
        let (typical, unlucky) = crate::parentage::verify::step_spread(run.p);
        let done_by_now = ((1.0 - crate::parentage::verify::still_going(run.p, n)) * 20.0).round() as u32;
        let past_mark = n >= unlucky;
        egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                crate::ui_util::pill(ui, RichText::new(format!("▶ Step {} of {}", i + 1, plan.steps.len())).size(12.0).color(p.on_accent), p.accent, Stroke::NONE);
                self.avatar(ui, run.trainee, 32.0);
                ui.label(semibold(format!("Train {}", uma_name(&self.kb, run.trainee)), 15.0));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Must have:").size(12.5).color(p.weak));
                if run.must.is_empty() {
                    ui.label(RichText::new("nothing: just finish her").size(12.5));
                }
                for w in &run.must {
                    let (kind, name, stars) = w.chip(&self.kb);
                    crate::lineage_kit::spark_chip(ui, crate::lineage_kit::Spark::of(kind), &name, 0, Some(stars), false, true);
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                crate::lineage_kit::odds_dots(ui, run.p, p.accent, p.border);
                ui.label(RichText::new(format!("{} a career ({:.1}%); usually {typical}.", crate::parentage::verify::one_in(run.p), run.p * 100.0)).size(13.0));
            });
            // The careers run so far as dots, a ring at the 9-in-10 mark.
            ui.horizontal(|ui| {
                let total = n.max(unlucky) + 1;
                let d = 10.0;
                let (rect, _) = ui.allocate_exact_size(vec2(total as f32 * (d + 3.0), d + 4.0), egui::Sense::hover());
                for k in 0..total {
                    let c = egui::pos2(rect.left() + d / 2.0 + k as f32 * (d + 3.0), rect.center().y);
                    if k < n {
                        ui.painter().circle_filled(c, d / 2.0, if k + 1 >= unlucky { crate::lineage_kit::ink(p.warn, p.surface) } else { p.weak });
                    } else {
                        ui.painter().circle_stroke(c, d / 2.0 - 0.5, Stroke::new(1.0, p.border));
                    }
                    if k + 1 == unlucky {
                        ui.painter().circle_stroke(c, d / 2.0 + 2.5, Stroke::new(1.5, p.text));
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                let mark = if past_mark { format!("Career {} · past the 9-in-10 mark", n + 1) } else { format!("Career {}", n + 1) };
                ui.label(semibold(mark, 13.0).color(if past_mark { crate::lineage_kit::ink(p.warn, p.surface) } else { p.text }));
                ui.label(RichText::new(format!("{done_by_now} of 20 players are done by now{}", if past_mark { "; you're one of the few still going" } else { "" })).size(13.0).color(p.weak));
                let open = self.lineage.runner.stuck_open.unwrap_or(past_mark);
                if ui.link(if open { "Hide" } else { "Stuck?" }).clicked() {
                    self.lineage.runner.stuck_open = Some(!open);
                }
            });
            if self.lineage.runner.stuck_open.unwrap_or(past_mark) {
                ui.add_space(8.0);
                self.stuck_panel(ui, plan, n, unlucky, vets);
            }
        });
    }

    /// Settle with the closest, try another route, or keep going: each previewed first.
    fn stuck_panel(&mut self, ui: &mut egui::Ui, plan: &ParentPlan, n: u32, unlucky: u32, vets: &[crate::veterans::Veteran]) {
        let p = pal(ui);
        let i = plan.current().unwrap_or(0);
        let run = plan.steps[i].run.clone();
        egui::Frame::new().fill(crate::lineage_kit::tint(p.warn, p.surface, 0.08)).stroke(Stroke::new(1.0, p.warn.gamma_multiply(0.5))).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(semibold(format!("Stuck on step {}?", i + 1), 14.5));
            let panel_bg = crate::lineage_kit::tint(p.warn, p.surface, 0.08);
            ui.label(RichText::new(format!("You've run {n} careers. About 9 in 10 players are done by career {unlucky}; the other 1 in 10, like you, are still going.")).size(12.5).color(crate::lineage_kit::ink(p.weak, panel_bg)));
            ui.add_space(6.0);
            // A preview waiting for the go-ahead replaces the three choices.
            if let Some((what, after, msg)) = self.lineage.runner.preview.clone() {
                let before: f64 = plan.remaining().iter().map(Run::careers).sum();
                let now: f64 = after.iter().map(Run::careers).sum();
                ui.label(semibold(match &what { Pending::Settle(id) => format!("Settle step {} with #{id}", i + 1), Pending::Route => "Try another route".into() }, 13.5));
                // The step that would come next, as the planner has it.
                if let Some(first) = after.first() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("New next step:").size(12.5).color(p.weak));
                        self.avatar(ui, first.trainee, 24.0);
                        ui.label(semibold(uma_name(&self.kb, first.trainee), 13.0));
                        if first.legacies != [0, 0] {
                            ui.label(RichText::new("with").size(12.5).color(p.weak));
                            for (id, uma) in first.legacies.iter().zip(first.legacy_umas) {
                                self.avatar(ui, uma, 20.0);
                                ui.label(RichText::new(format!("#{id}")).size(12.5));
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for w in &first.must {
                            let (kind, name, stars) = w.chip(&self.kb);
                            crate::lineage_kit::spark_chip(ui, crate::lineage_kit::Spark::of(kind), &name, 0, Some(stars), false, true);
                        }
                        crate::lineage_kit::odds_dots(ui, first.p, p.accent, p.border);
                        ui.label(RichText::new(crate::parentage::verify::one_in(first.p)).size(12.5));
                    });
                } else {
                    ui.label(RichText::new("No steps left after this: her lineage would be ready.").size(12.5));
                }
                ui.add(egui::Label::new(RichText::new(&msg).size(12.5).color(p.weak)).wrap());
                let saving = before - now;
                ui.label(RichText::new(if saving.abs() < 0.5 { format!("No faster: about {before:.0} careers left either way, with a fresh chance of luck.") } else if saving > 0.0 { format!("Careers left: about {before:.0} → about {now:.0} ({saving:.0} fewer).") } else { format!("Careers left: about {before:.0} → about {now:.0} ({:.0} more).", -saving) }).size(13.0));
                ui.label(RichText::new("Nothing changes until you press the button.").size(12.0).color(p.weak));
                ui.horizontal(|ui| {
                    let label = match &what { Pending::Settle(_) => "Settle with her", Pending::Route => "Use this route" };
                    if ui.add(crate::ui_util::primary_button(ui, label, 13.0)).clicked() {
                        match what {
                            Pending::Settle(id) => self.runner_do(move |plan, kb, inv, vets, today| plan.settle(kb, inv, vets, id, today)),
                            Pending::Route => self.runner_do(|plan, kb, inv, vets, today| plan.another_route(kb, inv, vets, today)),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.lineage.runner.preview = None;
                    }
                });
                return;
            }
            let cols = if ui.available_width() >= 720.0 { 3 } else { 1 };
            let settle_ui = |this: &mut Self, ui: &mut egui::Ui| {
                ui.label(semibold("1 · Settle with your closest", 13.5));
                prose(ui, "Keep the one that came closest. The steps before stay done; the rest is planned again from her actual sparks.");
                let closest = plan.closest(vets);
                if closest.is_empty() {
                    ui.label(RichText::new(format!("No {} finished since this step began.", uma_name(&this.kb, run.trainee))).size(12.5).color(crate::lineage_kit::ink(p.weak, crate::lineage_kit::tint(p.warn, p.surface, 0.08))));
                }
                for h in closest.iter().take(3) {
                    ui.horizontal_wrapped(|ui| {
                        this.avatar(ui, run.trainee, 24.0);
                        ui.label(RichText::new(format!("#{}", h.id)).size(13.0));
                        let note = if h.must { "has the must-haves".to_string() } else { format!("{} star{} short", h.short, if h.short == 1 { "" } else { "s" }) };
                        ui.label(RichText::new(note).size(12.5).color(if h.must { p.good } else { crate::lineage_kit::ink(p.warn, p.surface) }));
                        if !h.same_parents {
                            ui.label(RichText::new("other legacies").size(12.0).color(p.weak));
                        }
                        if ui.small_button("Preview").clicked() {
                            let inv = this.parent_inventory();
                            match plan.preview_settle(&this.kb, &inv, vets, h.id, &crate::ui_util::today()) {
                                Ok((after, msg)) => this.lineage.runner.preview = Some((Pending::Settle(h.id), after, msg)),
                                Err(e) => this.lineage.runner.message = Some(e),
                            }
                        }
                    });
                }
            };
            let route_ui = |this: &mut Self, ui: &mut egui::Ui| {
                ui.label(semibold("2 · Try another route", 13.5));
                prose(ui, &format!("Set this step aside: the same goal, without {} and these two legacies. The steps before stay done.", uma_name(&this.kb, run.trainee)));
                if ui.button("Preview the route").clicked() {
                    let inv = this.parent_inventory();
                    match plan.preview_another_route(&this.kb, &inv, vets, &crate::ui_util::today()) {
                        Ok((after, msg)) => this.lineage.runner.preview = Some((Pending::Route, after, msg)),
                        Err(e) => this.lineage.runner.message = Some(e),
                    }
                }
            };
            let keep_ui = |this: &mut Self, ui: &mut egui::Ui| {
                ui.label(semibold("3 · Keep going", 13.5));
                let (_, tail) = crate::parentage::verify::step_spread(run.p);
                prose(ui, &format!("Each career still hits {}: bad luck doesn't change the odds. About 1 in 10 players need {tail} or more careers; that's luck, not a mistake.", crate::parentage::verify::one_in(run.p)));
                prose(ui, "Costs nothing to decide; this closes until you open it again.");
                if ui.add(crate::ui_util::primary_button(ui, "Keep going", 13.0)).clicked() {
                    this.lineage.runner.stuck_open = Some(false);
                }
            };
            if cols == 3 {
                ui.columns(3, |c| {
                    c[0].with_layout(egui::Layout::top_down(egui::Align::Min), |ui| settle_ui(self, ui));
                    c[1].with_layout(egui::Layout::top_down(egui::Align::Min), |ui| route_ui(self, ui));
                    c[2].with_layout(egui::Layout::top_down(egui::Align::Min), |ui| keep_ui(self, ui));
                });
            } else {
                settle_ui(self, ui);
                ui.separator();
                route_ui(self, ui);
                ui.separator();
                keep_ui(self, ui);
            }
        });
    }

    /// Every step: done, settled, current or next.
    fn step_list(&mut self, ui: &mut egui::Ui, plan: &ParentPlan) {
        let p = pal(ui);
        let current = plan.current();
        crate::ui_util::card(ui, |ui| {
            ui.label(semibold(format!("Steps · {}", plan.steps.len()), 15.0));
            for (k, s) in plan.steps.iter().enumerate() {
                let (status, color) = match (s.finished(), s.settled, Some(k) == current) {
                    (true, true, _) => ("settled", crate::lineage_kit::ink(p.warn, p.raised)),
                    (true, false, _) => ("✔ done", crate::lineage_kit::ink(p.good, p.raised)),
                    (false, _, true) => ("▶ current", crate::lineage_kit::ink(p.accent, p.raised)),
                    _ => ("next", p.weak),
                };
                egui::Frame::new().fill(p.raised).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(semibold(format!("Step {}", k + 1), 13.0));
                        ui.label(RichText::new(status).size(12.5).color(color));
                        self.avatar(ui, s.run.trainee, 24.0);
                        ui.label(RichText::new(uma_name(&self.kb, s.run.trainee)).size(13.0));
                        for w in &s.run.must {
                            let (kind, name, stars) = w.chip(&self.kb);
                            crate::lineage_kit::spark_chip(ui, crate::lineage_kit::Spark::of(kind), &name, 0, Some(stars), false, true);
                        }
                        let (typical, _) = crate::parentage::verify::step_spread(s.run.p);
                        if !s.finished() {
                            ui.label(RichText::new(format!("usually {typical}")).size(12.5).color(p.weak));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    }
}

/// A paragraph, ragged right.
fn prose(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.5)).wrap());
}
