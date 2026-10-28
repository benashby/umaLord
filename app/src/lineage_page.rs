//! The Lineage page: "what's the next career I should run, and how?" for a Champions Meeting
//! team, a Team Trials team, or one runner. The planner (`parentage/`) does the work on a
//! worker thread with its own copy of the app's data (`agent::mcp::headless_app`), since a team
//! takes seconds to race; the page shows the Next career card first, then today's team, the
//! planned runners and the parent queue. The design is Claude Design rounds 3 to 9
//! (`DESIGN-ROUND*.md` in the umaLord notes); the art and spark chips follow round 8.

use crate::App;
use crate::parentage::brief::Brief;
use crate::parentage::team::{CmTeam, TeamOpts};
use crate::parentage::tt::{TtPins, TtTeam};
use crate::theme::{pal, size};
use crate::ui_util::semibold;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) enum LineageView {
    #[default]
    Cm,
    Tt,
    Runner,
}

impl LineageView {
    fn info(self) -> (&'static str, &'static str, &'static str) {
        match self {
            LineageView::Cm => ("🏆", "Champions Meeting", "Three runners for the next Champions Meeting, and the parent careers they need."),
            LineageView::Tt => ("🏟", "Team Trials", "Fifteen runners across the five races, an Ace for each, and the career that raises the team most."),
            LineageView::Runner => ("🌱", "One runner", "One runner's parent plan, step by step."),
        }
    }
}

/// A team the worker built, with everything the page shows already worked out.
pub(crate) enum Built {
    Cm { team: CmTeam, next: Option<Brief>, queue: Vec<QueueRow> },
    Tt { team: TtTeam, next: Option<Brief> },
}

/// One row of the parent queue, in words.
#[derive(Clone, Debug)]
pub(crate) struct QueueRow {
    pub trainee: u32,
    pub for_runner: u32,
    pub what: String,
    pub legacies: Vec<(u64, u32)>,
    pub careers: String,
    pub style: String,
}

#[derive(Default)]
pub(crate) struct LineageState {
    pub view: LineageView,
    /// Which upcoming CM, by position in the schedule.
    pub cm: usize,
    pub job: Option<(LineageView, Receiver<Result<Built, String>>)>,
    pub cm_built: Option<Result<Built, String>>,
    pub tt_built: Option<Result<Built, String>>,
}

impl App {
    pub(crate) fn lineage_page(&mut self, ui: &mut egui::Ui) {
        self.lineage_poll(ui.ctx());
        egui::Panel::top("lineage_bar")
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 18, top: 12, bottom: 10 }))
            .show(ui, |ui| self.lineage_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 20, top: 4, bottom: 12 })).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("lineage_scroll").auto_shrink([false, false]).show(ui, |ui| match self.lineage.view {
                LineageView::Cm => self.lineage_cm(ui),
                LineageView::Tt => self.lineage_tt(ui),
                LineageView::Runner => {
                    ui.label(RichText::new("One runner's plan comes next.").color(pal(ui).weak));
                }
            });
        });
    }

    fn lineage_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            ui.label(semibold("🌱 Lineage", 22.0));
            ui.add_space(14.0);
            for v in [LineageView::Cm, LineageView::Tt, LineageView::Runner] {
                let (icon, label, what) = v.info();
                if ui.add(egui::Button::selectable(self.lineage.view == v, RichText::new(format!("{icon} {label}")).size(14.0)).min_size(vec2(0.0, 30.0))).on_hover_text(what).clicked() {
                    self.lineage.view = v;
                }
            }
        });
    }

    /// Start building the view's team on a worker thread, unless one is built or building.
    fn lineage_start(&mut self, ctx: &egui::Context, view: LineageView) {
        if self.lineage.job.is_some() {
            return;
        }
        let root = self.root.clone();
        let today = crate::ui_util::today();
        let cm = self.kb.upcoming_cms(&today).get(self.lineage.cm).map(|e| (*e).clone());
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut app = crate::agent::mcp::headless_app(root);
            let out = match view {
                LineageView::Cm => cm.ok_or_else(|| "No upcoming Champions Meeting in the schedule.".to_string()).and_then(|preset| {
                    let team = app.cm_team(&preset, &TeamOpts::default())?;
                    let inv = app.parent_inventory();
                    let next = app.cm_next(&team, &inv);
                    let queue = team.queue.iter().map(|(i, run)| queue_row(&app, &inv, team.runners[*i].trainee, run)).collect();
                    Ok(Built::Cm { team, next, queue })
                }),
                LineageView::Tt => app.tt_team(200, &TtPins::default()).map(|team| {
                    let inv = app.parent_inventory();
                    let next = app.tt_next(&team, &inv);
                    Built::Tt { team, next }
                }),
                LineageView::Runner => Err("not built here".into()),
            };
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        self.lineage.job = Some((view, rx));
    }

    fn lineage_poll(&mut self, ctx: &egui::Context) {
        let Some((view, rx)) = &self.lineage.job else { return };
        if let Ok(r) = rx.try_recv() {
            match view {
                LineageView::Cm => self.lineage.cm_built = Some(r),
                _ => self.lineage.tt_built = Some(r),
            }
            self.lineage.job = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Whether the page is still building (screenshots wait for it).
    pub(crate) fn lineage_busy(&self) -> bool {
        self.lineage.job.is_some()
    }

    fn building(&self, ui: &mut egui::Ui, what: &str) {
        crate::ui_util::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(what).color(pal(ui).weak));
            });
        });
    }

    fn lineage_cm(&mut self, ui: &mut egui::Ui) {
        if self.lineage.cm_built.is_none() {
            let ctx = ui.ctx().clone();
            self.lineage_start(&ctx, LineageView::Cm);
            self.building(ui, "Planning the team: three runners, their parents, decks and skills, raced against each other and against your finished umas…");
            return;
        }
        let Some(Ok(Built::Cm { team, next, queue })) = &self.lineage.cm_built else {
            if let Some(Err(e)) = &self.lineage.cm_built {
                ui.label(RichText::new(format!("⚠ {e}")).color(pal(ui).warn));
            }
            return;
        };
        let (team, next, queue) = (team.clone(), next.clone(), queue.clone());
        let e = &team.event;
        ui.horizontal(|ui| {
            ui.label(semibold(format!("CM {} {} team", e.id, e.name), size::XL));
            pill(ui, &e.course_name, pal(ui).accent);
            ui.label(RichText::new(format!("{} · {} days away{}", e.date, team.days_left, if e.announced { "" } else { " (estimated)" })).size(13.0).color(pal(ui).weak));
        });
        ui.add_space(8.0);
        if let Some(b) = &next {
            self.next_card(ui, b);
            ui.add_space(12.0);
        }
        self.today_row(ui, &team);
        ui.add_space(12.0);
        self.runner_cards(ui, &team);
        ui.add_space(12.0);
        self.queue_card(ui, &queue, &team);
        for n in &team.notes {
            ui.label(RichText::new(format!("⚠ {n}")).size(12.5).color(pal(ui).warn));
        }
    }

    fn lineage_tt(&mut self, ui: &mut egui::Ui) {
        if self.lineage.tt_built.is_none() {
            let ctx = ui.ctx().clone();
            self.lineage_start(&ctx, LineageView::Tt);
            self.building(ui, "Planning the Team Trials team: racing each race's candidates head to head (about 20 seconds)…");
            return;
        }
        let Some(Ok(Built::Tt { team, next })) = &self.lineage.tt_built else {
            if let Some(Err(e)) = &self.lineage.tt_built {
                ui.label(RichText::new(format!("⚠ {e}")).color(pal(ui).warn));
            }
            return;
        };
        let (team, next) = (team.clone(), next.clone());
        ui.horizontal(|ui| {
            ui.label(semibold("Team Trials team", size::XL));
            ui.label(RichText::new(format!("about {} points a round (estimate)", thousands(team.total))).size(13.0).color(pal(ui).weak));
        });
        ui.add_space(8.0);
        if let Some(b) = &next {
            self.next_card(ui, b);
            ui.add_space(12.0);
        }
        if team.today_total > 0.0 {
            crate::ui_util::card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("TODAY").size(11.5).strong().color(pal(ui).weak));
                    ui.label(semibold(format!("What you can field today scores about {}", thousands(team.today_total)), 14.0));
                    ui.label(RichText::new(format!("raced together, the plan beats it by about +{}", thousands(team.gain))).color(pal(ui).good));
                });
            });
            ui.add_space(12.0);
        }
        for row in &team.rows {
            crate::ui_util::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(semibold(row.name, 15.5));
                    ui.label(RichText::new(format!("{} style{}", row.styles, if row.styles == 1 { "" } else { "s" })).size(12.5).color(pal(ui).weak));
                    if let Some(g) = row.gain {
                        ui.label(RichText::new(format!("the plan gains {:+}", g.round() as i64)).size(12.5).color(pal(ui).good));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for s in &row.slots {
                        let mut frame = egui::Frame::new().fill(pal(ui).raised).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(8));
                        if s.ace {
                            frame = frame.stroke(Stroke::new(1.5, pal(ui).gold));
                        }
                        frame.show(ui, |ui| {
                            ui.set_width(250.0);
                            ui.horizontal(|ui| {
                                self.avatar(ui, s.trainee, 32.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(semibold(crate::parentage::uma_name(&self.kb, s.trainee), 13.5));
                                        if s.ace {
                                            ui.label(RichText::new("★ Ace").size(11.5).color(pal(ui).gold));
                                        }
                                    });
                                    ui.label(RichText::new(format!("{} · ~{}", s.style.label(), thousands(s.total))).size(12.0).color(pal(ui).weak));
                                });
                            });
                        });
                    }
                });
                if !row.today.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Today:").size(12.0).color(pal(ui).weak));
                        for (t, pts) in &row.today {
                            self.avatar(ui, t.outfit, 18.0);
                            ui.label(RichText::new(format!("#{} {} ~{}", t.vet, crate::parentage::uma_name(&self.kb, t.outfit), thousands(*pts))).size(12.0));
                        }
                    });
                }
            });
            ui.add_space(8.0);
        }
    }

    /// "What you can field today": the best of the player's finished umas as they are.
    fn today_row(&self, ui: &mut egui::Ui, team: &CmTeam) {
        crate::ui_util::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("TODAY").size(11.5).strong().color(pal(ui).weak));
                ui.label(semibold("What you can field today:", 14.0));
                if team.today.is_empty() {
                    ui.label(RichText::new("none of your finished umas can run this course yet").color(pal(ui).weak));
                }
                for t in &team.today {
                    egui::Frame::new().fill(pal(ui).raised).corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            self.avatar(ui, t.outfit, 24.0);
                            ui.label(RichText::new(format!("#{} {}", t.vet, crate::parentage::uma_name(&self.kb, t.outfit))).size(13.0));
                            if let Some(s) = t.share {
                                ui.label(RichText::new(format!("wins {:.0}%", s * 100.0)).size(12.0).color(pal(ui).weak));
                            }
                        });
                    });
                }
            });
            ui.label(RichText::new("The best team from your finished umas as they are: their own stats, skills and aptitudes, raced in the simulator. The plan's gains are measured against it.").size(12.0).color(pal(ui).weak));
        });
    }

    fn runner_cards(&self, ui: &mut egui::Ui, team: &CmTeam) {
        let n = team.runners.len().max(1);
        let narrow = ui.available_width() < 900.0;
        let draw = |ui: &mut egui::Ui, r: &crate::parentage::team::Runner, ace: bool| {
            crate::ui_util::card(ui, |ui| {
                ui.horizontal(|ui| {
                    self.avatar(ui, r.trainee, 48.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(semibold(crate::parentage::uma_name(&self.kb, r.trainee), 15.0));
                            if ace {
                                pill(ui, "★ Ace", pal(ui).gold);
                            }
                        });
                        ui.label(RichText::new(format!("{}{}", r.style.label(), if r.meta_pick { " · the sheet's pick" } else { "" })).size(12.5).color(pal(ui).weak));
                    });
                });
                if let Some(v) = r.vs_today {
                    ui.label(RichText::new(format!("Against today's best she wins {:.0}%", v * 100.0)).size(13.5));
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for c in r.plan.deck.iter().flatten() {
                        self.card_art(ui, *c, 40.0, false);
                    }
                    if let Some(f) = r.plan.friend {
                        self.card_art(ui, f, 40.0, true);
                    }
                });
                let lineage = if r.parents.now.as_ref().is_some_and(crate::parentage::score::Lineage::meets) {
                    "Her lineage is ready.".to_string()
                } else {
                    format!("Waiting on {} parent step{}.", r.parents.steps.len(), if r.parents.steps.len() == 1 { "" } else { "s" })
                };
                ui.label(RichText::new(lineage).size(12.5).color(pal(ui).weak));
            });
        };
        let ace = team.runners.iter().enumerate().filter_map(|(i, r)| r.sim.map(|s| (i, s.0))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
        if narrow {
            for (i, r) in team.runners.iter().enumerate() {
                draw(ui, r, ace == Some(i));
                ui.add_space(8.0);
            }
        } else {
            ui.columns(n, |cols| {
                for (i, (col, r)) in cols.iter_mut().zip(&team.runners).enumerate() {
                    draw(col, r, ace == Some(i));
                }
            });
        }
    }

    fn queue_card(&self, ui: &mut egui::Ui, queue: &[QueueRow], team: &CmTeam) {
        crate::ui_util::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(semibold("Parent queue", 15.5));
                ui.label(RichText::new(format!("{} step{} · about {:.0} careers of the {:.0} there's time for", queue.len(), if queue.len() == 1 { "" } else { "s" }, team.needed, team.budget)).size(12.5).color(pal(ui).weak));
            });
            if queue.is_empty() {
                ui.label(RichText::new("No parent careers needed: train the three.").color(pal(ui).weak));
            }
            for (k, q) in queue.iter().enumerate() {
                egui::Frame::new().fill(pal(ui).raised).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, &format!("Step {}", k + 1), pal(ui).accent);
                        ui.label(RichText::new("⏱").size(13.0)).on_hover_text("Independent Training: a parent career runs in the background");
                        self.avatar(ui, q.trainee, 32.0);
                        ui.vertical(|ui| {
                            ui.label(semibold(format!("{} as {}", crate::parentage::uma_name(&self.kb, q.trainee), q.style), 13.5));
                            ui.label(RichText::new(format!("For {}: {}", crate::parentage::uma_name(&self.kb, q.for_runner), q.what)).size(12.0).color(pal(ui).weak));
                        });
                        for (id, uma) in &q.legacies {
                            self.avatar(ui, *uma, 24.0);
                            ui.label(RichText::new(if *id == 0 { "any".to_string() } else { format!("#{id}") }).size(12.0));
                        }
                        ui.label(RichText::new(&q.careers).size(12.5));
                    });
                });
                ui.add_space(4.0);
            }
        });
    }

    /// The Next career card: who and why, how, parents, scenario, deck, skills, stats, races,
    /// keep-if, set up.
    fn next_card(&self, ui: &mut egui::Ui, b: &Brief) {
        let p = pal(ui);
        egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.5, p.accent)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                self.avatar(ui, b.trainee, 64.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("NEXT CAREER").size(11.5).strong().color(p.accent));
                        pill(ui, &b.mode, if b.mode == "Manual career" { p.accent } else { p.weak });
                    });
                    ui.label(semibold(format!("Next: {} as {}", crate::parentage::uma_name(&self.kb, b.trainee), b.style.label()), 19.0));
                    ui.label(RichText::new(&b.why).size(13.0).color(p.weak));
                });
            });
            ui.add_space(8.0);
            let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                ui.horizontal_top(|ui| {
                    ui.add_sized(vec2(110.0, 18.0), egui::Label::new(RichText::new(label).size(11.5).strong().color(p.weak)));
                    ui.vertical(|ui| add(ui));
                });
                ui.add_space(6.0);
            };
            row(ui, "PARENTS", &mut |ui| _ = ui.label(&b.parents));
            row(ui, "SCENARIO", &mut |ui| _ = ui.label(&b.scenario));
            row(ui, "DECK", &mut |ui| {
                ui.horizontal_wrapped(|ui| {
                    for c in &b.deck {
                        self.card_art(ui, *c, 54.0, false);
                    }
                    if let Some(f) = b.friend {
                        self.card_art(ui, f, 54.0, true);
                    }
                });
            });
            row(ui, "SKILLS", &mut |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in b.skills.iter().take(10).enumerate() {
                        let name = self.kb.skills.get(s).map_or(s.to_string(), |k| k.name.clone());
                        pill(ui, &format!("{} {name}", i + 1), p.weak);
                    }
                });
            });
            row(ui, "STATS", &mut |ui| {
                for n in &b.stat_notes {
                    ui.add(egui::Label::new(RichText::new(n).size(12.5)).wrap());
                }
            });
            row(ui, "RACES", &mut |ui| {
                for r in &b.races {
                    ui.add(egui::Label::new(RichText::new(r).size(12.5)).wrap());
                }
                if !b.agenda.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for (name, grades, chance) in &b.agenda {
                            pill(ui, &format!("{name} · {grades} · ~{chance}%"), p.good);
                        }
                    });
                }
            });
            row(ui, "KEEP HER IF", &mut |ui| {
                for k in &b.keep {
                    ui.add(egui::Label::new(RichText::new(k).size(12.5)).wrap());
                }
            });
            if !b.setup.is_empty() {
                row(ui, "SET UP", &mut |ui| {
                    for s in &b.setup {
                        ui.add(egui::Label::new(RichText::new(s).size(12.5)).wrap());
                    }
                });
            }
        });
    }

    /// An uma's round avatar, `size` across, with a thin ring; initials if the art is missing.
    pub(crate) fn avatar(&self, ui: &mut egui::Ui, outfit: u32, size: f32) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
        if ui.is_rect_visible(rect) {
            let p = pal(ui);
            match self.art_uri("umas", outfit) {
                Some(uri) => egui::Image::new(uri).corner_radius(CornerRadius::same((size / 2.0) as u8)).paint_at(ui, rect),
                None => {
                    ui.painter().circle_filled(rect.center(), size / 2.0, p.hover);
                    let name = crate::parentage::uma_name(&self.kb, outfit);
                    let init: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, init, FontId::proportional(size * 0.38), p.weak);
                }
            }
            ui.painter().circle_stroke(rect.center(), size / 2.0, Stroke::new(1.0, p.border));
        }
        resp.on_hover_text(crate::parentage::uma_name(&self.kb, outfit))
    }

    /// A support card's art, `w` wide at 3:4; a borrowed card gets a dashed-look ring and an MLB tag.
    pub(crate) fn card_art(&self, ui: &mut egui::Ui, id: u32, w: f32, borrowed: bool) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 4.0 / 3.0), Sense::hover());
        if ui.is_rect_visible(rect) {
            let p = pal(ui);
            match self.art_uri("cards", id) {
                Some(uri) => egui::Image::new(uri).corner_radius(CornerRadius::same(4)).paint_at(ui, rect),
                None => _ = ui.painter().rect_filled(rect, 4.0, p.hover),
            }
            if borrowed {
                ui.painter().rect_stroke(rect.expand(1.5), 5.0, Stroke::new(1.5, p.accent), StrokeKind::Outside);
                let tag = Rect::from_min_size(rect.right_top() + vec2(-26.0f32.min(w), 0.0), vec2(26.0f32.min(w), 13.0));
                ui.painter().rect_filled(tag, CornerRadius { nw: 0, ne: 4, sw: 4, se: 0 }, p.accent);
                ui.painter().text(tag.center(), Align2::CENTER_CENTER, "MLB", FontId::proportional(9.0), p.on_accent);
            }
        }
        let name = self.kb.cards.get(&id).map_or(id.to_string(), |c| format!("{} [{}]", c.uma, c.title));
        resp.on_hover_text(if borrowed { format!("Borrow: {name}, fully limit broken (MLB)") } else { name })
    }
}

/// A queue row from a step.
fn queue_row(app: &App, inv: &crate::parentage::Inventory, runner: u32, run: &crate::parentage::jobs::Run) -> QueueRow {
    let legacies = run.legacies.iter().zip(run.legacy_umas).map(|(id, uma)| (*id, if uma == 0 { inv.by_id(*id).map_or(0, |p| p.uma) } else { uma })).collect();
    let what = if run.must.is_empty() { "just finishing her".to_string() } else { run.must.iter().map(|w| w.label(&app.kb)).collect::<Vec<_>>().join(", ") };
    let style = app.kb.umas.get(&run.trainee).map(|u| u.aptitude.best_style().label()).unwrap_or("").to_string();
    QueueRow { trainee: run.trainee, for_runner: runner, what, legacies, careers: format!("about {:.1} careers", run.careers()), style }
}

/// A small rounded label.
fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new().fill(color.gamma_multiply(0.16)).stroke(Stroke::new(1.0, color.gamma_multiply(0.55))).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).size(12.0).color(color));
    });
}

fn thousands(v: f64) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}
