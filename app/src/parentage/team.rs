//! One button, a whole Champions Meeting team: three runners from the account, each with the
//! parent careers her lineage needs (the planner), a deck with a borrowed MLB friend card, skills
//! to buy, and a race in the simulator against her teammates; then one queue of parent careers
//! for all three, set against the careers there's time for before the CM.
//!
//! Runners come from the meta sheet's picks the player owns, one per style it ranks well, then
//! the best aptitude fit (`App::trainee_candidates`). A runner whose lineage can't be reached
//! under the 20-career cap gives way to the next candidate for her style.

use super::jobs::{Proposal, Run};
use super::{Goal, Inventory};
use crate::App;
use crate::knowledge::{EventPreset, Style};
use crate::tab_planner::Plan;

/// One runner of the team.
#[derive(Clone)]
pub struct Runner {
    pub trainee: u32,
    pub style: Style,
    /// Whether the meta sheet names her for this CM and style.
    pub meta_pick: bool,
    pub goal: Goal,
    pub parents: Proposal,
    /// Her career plan: deck (slot 6 a borrowed MLB friend card), target stats, skills.
    pub plan: Plan,
    /// Her share of wins in the simulator head to head against each teammate, averaged, with a
    /// 95% interval: who should be the ace.
    pub sim: Option<(f64, f64, f64)>,
    /// From the same races: the share she survived (didn't run dry) and reached full spurt.
    pub stamina: Option<(f64, f64)>,
    /// Each skill's share of races it fired in.
    pub skill_rates: Vec<(u32, f64)>,
    pub notes: Vec<String>,
}

/// The team for one Champions Meeting.
#[derive(Clone)]
pub struct CmTeam {
    pub event: EventPreset,
    pub runners: Vec<Runner>,
    /// Every parent step of the three, harder first: (runner index, step).
    pub queue: Vec<(usize, Run)>,
    /// Parent careers there's time for before the runners must start, and the careers the queue
    /// is expected to take.
    pub budget: f64,
    pub needed: f64,
    pub days_left: i64,
    pub notes: Vec<String>,
}

/// The pipeline's knobs.
#[derive(Clone, Debug)]
pub struct TeamOpts {
    pub careers_per_day: f64,
    /// Careers held back for each runner's own career.
    pub held_back: f64,
    pub races: usize,
    pub seed: u64,
}

impl Default for TeamOpts {
    fn default() -> Self {
        TeamOpts { careers_per_day: 4.8, held_back: 2.0, races: 300, seed: 12345 }
    }
}

/// Styles in the order the meta sheet ranks them for this CM ("Pace>End≥Front>Late"), the
/// others after; all four when the sheet has nothing.
fn ranked_styles(app: &App, preset: &EventPreset) -> Vec<Style> {
    let ranking = app.kb.pvp_meta.as_ref().and_then(|m| m.events.iter().find(|e| e.series == "CM" && e.number == preset.id)).map(|e| e.style_ranking.clone()).unwrap_or_default();
    let mut out: Vec<Style> = ranking
        .split(['>', '≥', '=', ',', ' '])
        .filter_map(|w| match w.trim() {
            "Front" => Some(Style::Front),
            "Pace" => Some(Style::Pace),
            "Late" => Some(Style::Late),
            "End" => Some(Style::End),
            _ => None,
        })
        .collect();
    out.dedup();
    for s in [Style::Front, Style::Pace, Style::Late, Style::End] {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn style_index(s: Style) -> usize {
    match s {
        Style::Front => 6,
        Style::Pace => 7,
        Style::Late => 8,
        Style::End => 9,
    }
}

impl App {
    /// The parent planner's view of the account: finished umas and owned trainees.
    pub(crate) fn parent_inventory(&self) -> Inventory {
        let owned = self.roster.umas.iter().map(|u| (u.id, u.stars)).collect();
        Inventory::new(&self.kb, &self.veterans(), owned)
    }

    /// A Plan for the CM with this style (and trainee, if any), as the planner would start it.
    fn cm_plan(&self, preset: &EventPreset, style: Style, trainee: Option<u32>) -> Plan {
        let mut plan = Plan::default();
        plan.apply_cm(&self.kb, preset);
        plan.style = style;
        plan.trainee = trainee;
        plan.friend_lb = 4;
        plan
    }

    /// The uma whose unique the sheet recommends inheriting for this CM and style, if any.
    /// Never one of the runner's own outfits: a legacy can't be her own character.
    fn meta_green(&self, preset: &EventPreset, style: Style, runner: u32) -> Option<u32> {
        let ev = self.kb.pvp_meta.as_ref()?.events.iter().find(|e| e.series == "CM" && e.number == preset.id)?;
        let st = ev.styles.get(crate::meta::style_key(style))?;
        st.parents.iter().find_map(|p| {
            let unique = 100_000 + p.skill % 100_000;
            self.kb.umas.values().filter(|u| u.unique_skill == Some(unique) && crate::affinity::chara(u.id) != crate::affinity::chara(runner)).map(|u| u.id).min()
        })
    }

    /// The team for a Champions Meeting. Runs the simulator, so it takes a few seconds.
    pub(crate) fn cm_team(&mut self, preset: &EventPreset, opts: &TeamOpts) -> Result<CmTeam, String> {
        let inv = self.parent_inventory();
        let saved = std::mem::take(&mut self.planner.plan);
        let mut out = self.cm_team_inner(preset, opts, &inv);
        // Without finished umas the team, decks and skills still stand; parents wait for an import.
        if let (Ok(t), true) = (&mut out, inv.parents.is_empty()) {
            t.notes.push("No finished umas imported yet, so parents aren't planned: import them (Settings → Profile → Import finished umas) and plan again.".into());
        }
        self.planner.plan = saved;
        out
    }

    fn cm_team_inner(&mut self, preset: &EventPreset, opts: &TeamOpts, inv: &Inventory) -> Result<CmTeam, String> {
        let mut notes = Vec::new();
        let mut runners: Vec<Runner> = Vec::new();
        let styles = ranked_styles(self, preset);
        // One runner per style the sheet ranks best, then fill from the top style again.
        let order: Vec<Style> = styles.iter().copied().take(3).chain(styles.iter().copied()).collect();
        for style in order {
            if runners.len() == 3 {
                break;
            }
            self.planner.plan = self.cm_plan(preset, style, None);
            let meta: Vec<u32> = self.plan_meta_trainees();
            let taken: Vec<u32> = runners.iter().map(|r| crate::affinity::chara(r.trainee)).collect();
            let candidates: Vec<u32> = self.trainee_candidates().into_iter().filter(|t| !taken.contains(&crate::affinity::chara(*t))).take(4).collect();
            let surface = if self.planner.plan.surface == crate::knowledge::Surface::Dirt { 1 } else { 0 };
            let distance = 2 + crate::knowledge::Distance::ALL.iter().position(|d| *d == self.planner.plan.distance).unwrap_or(2);
            let mut picked: Option<Runner> = None;
            for t in candidates {
                let mut goal = Goal { runner: t, targets: vec![surface, distance, style_index(style)], label: format!("CM {} ({}), {}", preset.id, preset.name, style.label()), ..Default::default() };
                let mut rnotes = Vec::new();
                let green = self.meta_green(preset, style, t);
                goal.green = green;
                let mut p = super::jobs::propose(&self.kb, &goal, inv);
                if green.is_some() && (!p.impossible.is_empty() || (p.steps.is_empty() && !p.now.as_ref().is_some_and(super::score::Lineage::meets)) || p.over_cap) {
                    rnotes.push(format!("The sheet's inherited unique ({}) is out of reach here, so her lineage is planned without it.", green.map(|u| super::uma_name(&self.kb, u)).unwrap_or_default()));
                    goal.green = None;
                    p = super::jobs::propose(&self.kb, &goal, inv);
                }
                let ready = p.now.as_ref().is_some_and(super::score::Lineage::meets);
                let reachable = ready || (!p.steps.is_empty() && !p.over_cap);
                let r = Runner { trainee: t, style, meta_pick: meta.contains(&t), goal, parents: p, plan: Plan::default(), sim: None, stamina: None, skill_rates: Vec::new(), notes: rnotes };
                if reachable {
                    picked = Some(r);
                    break;
                }
                picked.get_or_insert(r);
            }
            if let Some(r) = picked {
                runners.push(r);
            }
        }
        if runners.is_empty() {
            return Err("No owned uma can run this CM's course.".into());
        }
        // Each runner's career plan: deck with a borrowed MLB friend card, the sheet's stats, skills.
        let meta_stats = self.kb.pvp_meta.as_ref().and_then(|m| m.events.iter().find(|e| e.series == "CM" && e.number == preset.id)).map(|e| e.stats.clone()).unwrap_or_default();
        for r in &mut runners {
            let mut plan = self.cm_plan(preset, r.style, Some(r.trainee));
            let parsed: Vec<u32> = meta_stats.iter().filter_map(|s| s.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()).collect();
            plan.target = if parsed.len() == 5 { std::array::from_fn(|i| parsed[i]) } else { [1200, 1000, 1000, 600, 1000] };
            self.planner.plan = plan;
            self.recommend_deck(true);
            self.auto_pick_skills();
            r.plan = self.planner.plan.clone();
        }
        // Race each pair; a runner's share is her average win share against her teammates.
        if let Some(data) = self.sim_data.clone() {
            let mut builds = Vec::new();
            for r in &runners {
                self.planner.plan = r.plan.clone();
                let mut b = self.plan_sim_build(true).ok();
                if let Some(b) = b.as_mut() {
                    b.skills.retain(|s| !preset.disabled_skills.contains(s));
                }
                builds.push(b);
            }
            self.planner.plan = runners[0].plan.clone();
            let setup = self.plan_race_setup();
            if let Some(course) = setup.course.and_then(|c| data.courses.get(&c).cloned()) {
                let course = std::sync::Arc::new(course);
                let race = crate::sim::run::race_params(i32::from(setup.ground.unwrap_or(1)), i32::from(setup.weather.unwrap_or(1)), i32::from(setup.season.unwrap_or(1)), i32::from(setup.time.unwrap_or(2)), None);
                let sim_opts = crate::sim::run::SimOptions { samples: opts.races, seed: opts.seed as _, ..Default::default() };
                let n = runners.len();
                let mut wins = vec![Vec::new(); n];
                // Per runner: (survival, full spurt, runs) per race set, and skill fire rates.
                let mut stam: Vec<Vec<(f64, f64)>> = vec![Vec::new(); n];
                let mut rates: Vec<std::collections::BTreeMap<u32, Vec<f64>>> = vec![Default::default(); n];
                for i in 0..n {
                    for j in i + 1..n {
                        let (Some(a), Some(b)) = (&builds[i], &builds[j]) else { continue };
                        let res = crate::sim::run::run_comparison(&data, &course, &race, a, b, &sim_opts, true, false, None);
                        let runs = res.gaps.len().max(1) as f64;
                        let b_share = res.gaps.iter().filter(|g| **g > 0.0).count() as f64 / runs;
                        wins[i].push((1.0 - b_share, runs));
                        wins[j].push((b_share, runs));
                        for (k, u) in [(i, &res.umas[0]), (j, &res.umas[1])] {
                            // The simulator reports these as percentages.
                            stam[k].push((u.survival_rate / 100.0, u.full_spurt_rate / 100.0));
                            for s in &u.skills {
                                rates[k].entry(s.id).or_default().push(s.rate / 100.0);
                            }
                        }
                    }
                }
                for (k, r) in runners.iter_mut().enumerate() {
                    if !stam[k].is_empty() {
                        let m = stam[k].len() as f64;
                        r.stamina = Some((stam[k].iter().map(|x| x.0).sum::<f64>() / m, stam[k].iter().map(|x| x.1).sum::<f64>() / m));
                    }
                    r.skill_rates = rates[k].iter().map(|(id, v)| (*id, v.iter().sum::<f64>() / v.len() as f64)).collect();
                }
                for (r, w) in runners.iter_mut().zip(wins) {
                    if w.is_empty() {
                        continue;
                    }
                    let share = w.iter().map(|x| x.0).sum::<f64>() / w.len() as f64;
                    let runs: f64 = w.iter().map(|x| x.1).sum();
                    let half = 1.96 * (share * (1.0 - share) / runs).sqrt();
                    r.sim = Some((share, (share - half).max(0.0), (share + half).min(1.0)));
                }
            } else {
                notes.push("The simulator has no course for this CM yet, so the runners weren't raced.".into());
            }
        }
        // One queue of parent careers for the three, harder first: misses show up early.
        let mut queue: Vec<(usize, Run)> = runners.iter().enumerate().flat_map(|(i, r)| r.parents.steps.iter().map(move |s| (i, s.clone()))).collect();
        queue.sort_by(|a, b| b.1.careers().total_cmp(&a.1.careers()));
        let needed: f64 = queue.iter().map(|q| q.1.careers()).fold(0.0, |a, b| a + b);
        let days_left = crate::tab_planner::days_apart(&preset.date, &crate::ui_util::today()).max(0);
        let budget = (days_left as f64 * opts.careers_per_day - opts.held_back * runners.len() as f64).max(0.0);
        for r in &runners {
            if let Some((surv, spurt)) = r.stamina.filter(|s| s.0 < 0.9 || s.1 < 0.5) {
                notes.push(format!(
                    "{} ran dry in {:.0}% of her races and reached full spurt in {:.0}%: raise her Stamina target or add recovery skills.",
                    super::uma_name(&self.kb, r.trainee),
                    (1.0 - surv) * 100.0,
                    spurt * 100.0
                ));
            }
        }
        if days_left < 5 {
            notes.push(format!("{} is {days_left} days away: too soon to breed for. Race it with the lineages you have.", preset.name));
        } else if needed > budget {
            notes.push(format!("The parent careers ({needed:.0} expected) are more than there's time for ({budget:.0}). Drop the runner with the longest queue, or borrow a friend's legacy for her."));
        }
        Ok(CmTeam { event: preset.clone(), runners, queue, budget, needed, days_left, notes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The next three CMs on the real account, timed:
    /// `UMALORD_ROOT=<data folder with knowledge/> cargo test --release cm_team_real_account -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cm_team_real_account() {
        let root = std::path::PathBuf::from(std::env::var("UMALORD_ROOT").expect("UMALORD_ROOT"));
        let mut a = App::new(Some(root));
        let cms: Vec<EventPreset> = a.kb.upcoming_cms(&crate::ui_util::today()).into_iter().take(4).cloned().collect();
        for cm in cms {
            let t = std::time::Instant::now();
            match a.cm_team(&cm, &TeamOpts::default()) {
                Ok(team) => {
                    let inv = a.parent_inventory();
                    eprintln!("==== {} ({:.1} s)\n{}", cm.name, t.elapsed().as_secs_f64(), super::super::text::team(&a.kb, &team, &inv));
                    assert!(team.runners.len() <= 3 && !team.runners.is_empty());
                    let charas: std::collections::BTreeSet<u32> = team.runners.iter().map(|r| crate::affinity::chara(r.trainee)).collect();
                    assert_eq!(charas.len(), team.runners.len(), "three different characters");
                    for r in &team.runners {
                        assert!(r.plan.friend.is_some() && r.plan.friend_lb == 4, "a borrowed MLB friend card");
                        assert!(a.roster.umas.iter().any(|u| u.id == r.trainee), "an owned runner");
                    }
                }
                Err(e) => eprintln!("==== {}: {e}", cm.name),
            }
        }
    }

    #[test]
    fn ranks_styles_from_the_sheet() {
        let a = App::for_tests();
        let preset = a.kb.upcoming_cms("2026-10-01").first().map(|e| (*e).clone()).expect("a CM in the bundled schedule");
        let s = ranked_styles(&a, &preset);
        assert_eq!(s.len(), 4);
        let mut sorted = s.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "each style once");
    }
}
