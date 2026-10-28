//! One button, a whole Team Trials team: five races (Sprint, Mile, Medium, Long, Dirt) with up
//! to three runners each, from the account's umas, each scored by an estimate of her Team
//! Trials points, the top slot of each race its Ace, and the career that would raise the team
//! most.
//!
//! The score follows `knowledge/mechanics.md` (Team Trials): placement points, estimated from
//! head-to-head races against the other candidates for that race in the simulator (labelled an
//! estimate: the real race has twelve runners); skill points, each skill's points (gold 1,200;
//! white or green 500; an inherited unique 500; her own unique by her stars) times the share of
//! races it fired in; and the positioning bonus for each running style on the race's team.

use super::jobs::Proposal;
use super::{Goal, Inventory};
use crate::App;
use crate::knowledge::{Distance, Style, Surface, grade_rank};
use crate::tab_planner::Plan;
use std::collections::BTreeMap;

/// The five races, with the stamina the playbook's guide asks for on random tracks.
pub const ROWS: [(&str, Surface, Distance, u32); 5] = [
    ("Sprint", Surface::Turf, Distance::Sprint, 450),
    ("Mile", Surface::Turf, Distance::Mile, 650),
    ("Medium", Surface::Turf, Distance::Medium, 750),
    ("Long", Surface::Turf, Distance::Long, 900),
    ("Dirt", Surface::Dirt, Distance::Mile, 650),
];

/// Positioning: 1,000 in each of the early, mid and late race per running style on the team.
const POSITIONING: f64 = 3000.0;

/// One runner in a race's team.
#[derive(Clone)]
pub struct Slot {
    pub trainee: u32,
    pub style: Style,
    pub ace: bool,
    /// Her estimated points: placement, skills, positioning (shared by style), and the total
    /// with the Ace's +10%.
    pub placement: f64,
    pub skills: f64,
    pub total: f64,
    /// Her surface and distance grades are A or better without help.
    pub natural: bool,
    pub parents: Proposal,
    pub plan: Plan,
}

#[derive(Clone)]
pub struct Row {
    pub name: &'static str,
    pub slots: Vec<Slot>,
    /// Styles on the team (each adds the positioning bonus).
    pub styles: usize,
    pub notes: Vec<String>,
}

#[derive(Clone)]
pub struct TtTeam {
    pub rows: Vec<Row>,
    pub total: f64,
    /// The career that raises the team most: the weakest slot's next step, in words.
    pub next: String,
    pub notes: Vec<String>,
}

/// Points for one skill she fires.
fn skill_points(kb: &crate::knowledge::Knowledge, id: u32, own_unique: Option<u32>, stars: u8) -> f64 {
    if Some(id) == own_unique {
        return if stars >= 3 { 2500.0 } else { 2000.0 };
    }
    if id >= 900_000 && id < 1_000_000 {
        return 500.0;
    }
    match kb.skills.get(&id).map(|s| s.rarity) {
        Some(2) | Some(0) => 1200.0,
        Some(3..=5) => 500.0,
        Some(_) => 500.0,
        None => 0.0,
    }
}

impl App {
    /// A Team Trials plan for one race and trainee, ready for the deck and skill pickers.
    fn tt_plan(&self, row: usize, trainee: u32, style: Style, course: Option<u32>) -> Plan {
        let (_, surface, distance, stamina) = ROWS[row];
        let mut plan = Plan { purpose: crate::tab_planner::Purpose::TeamTrials, ..Plan::default() };
        plan.surface = surface;
        plan.distance = distance;
        plan.umalator_course = course;
        plan.style = style;
        plan.trainee = Some(trainee);
        plan.friend_lb = 4;
        plan.target = [1200, stamina, 1000, 600, 1000];
        plan.pick_objective = crate::skill_opt::PickObjective::Consistency;
        plan
    }

    /// A representative course for each race: one an event uses, else the first that fits.
    fn tt_course(&self, row: usize) -> Option<u32> {
        let (_, surface, distance, _) = ROWS[row];
        let fits: Vec<&crate::knowledge::UmalatorCourse> = self.kb.umalator_courses.iter().filter(|c| c.surface == surface.label() && Distance::of_metres(c.distance) == distance).collect();
        fits.iter().find(|c| self.kb.umalator_presets.iter().any(|e| e.course == c.id)).or(fits.first()).map(|c| c.id)
    }

    /// The Team Trials team. Builds and races up to six candidates per race, so it takes a few
    /// seconds.
    pub(crate) fn tt_team(&mut self, races: usize) -> Result<TtTeam, String> {
        let inv = self.parent_inventory();
        let saved = std::mem::take(&mut self.planner.plan);
        let out = self.tt_team_inner(&inv, races);
        self.planner.plan = saved;
        out
    }

    fn tt_team_inner(&mut self, inv: &Inventory, races: usize) -> Result<TtTeam, String> {
        let data = self.sim_data.clone().ok_or("The simulator's data isn't loaded.")?;
        let mut notes = Vec::new();
        // Candidates per race: A in its surface and distance first, then B (parents can lift a B).
        let mut cands: Vec<Vec<(u32, Style, bool)>> = Vec::new();
        for &(_, surface, distance, _) in &ROWS {
            let mut list: Vec<(u32, Style, bool, (u8, u8, u8))> = self
                .roster
                .umas
                .iter()
                .filter_map(|u| {
                    let a = &self.kb.umas.get(&u.id)?.aptitude;
                    let (gs, gd) = (grade_rank(a.surface(surface)), grade_rank(a.distance(distance)));
                    (gs >= 6 && gd >= 6).then(|| (u.id, a.best_style(), gs >= 7 && gd >= 7, (gs.min(7) + gd.min(7), u.stars, u.potential)))
                })
                .collect();
            list.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)));
            cands.push(list.into_iter().take(6).map(|c| (c.0, c.1, c.2)).collect());
        }
        // Build and race each race's candidates head to head; score each.
        let mut scored: Vec<Vec<Slot>> = Vec::new();
        for (row, list) in cands.iter().enumerate() {
            let course_id = self.tt_course(row);
            let mut builds = Vec::new();
            let mut slots = Vec::new();
            for &(t, style, natural) in list {
                self.planner.plan = self.tt_plan(row, t, style, course_id);
                self.recommend_deck(true);
                self.auto_pick_skills();
                builds.push(self.plan_sim_build(true).ok());
                let (_, surface, distance, _) = ROWS[row];
                let goal = Goal { runner: t, targets: vec![usize::from(surface == Surface::Dirt), 2 + Distance::ALL.iter().position(|d| *d == distance).unwrap_or(1), 6 + style as usize], label: format!("Team Trials {}", ROWS[row].0), ..Default::default() };
                let parents = super::jobs::propose(&self.kb, &goal, inv);
                slots.push(Slot { trainee: t, style, ace: false, placement: 0.0, skills: 0.0, total: 0.0, natural, parents, plan: self.planner.plan.clone() });
            }
            let Some(course) = course_id.and_then(|c| data.courses.get(&c).cloned()) else {
                notes.push(format!("No simulator course for {}: its runners weren't raced.", ROWS[row].0));
                scored.push(slots);
                continue;
            };
            let course = std::sync::Arc::new(course);
            let race = crate::sim::run::race_params(1, 1, 1, 2, None);
            let opts = crate::sim::run::SimOptions { samples: races, seed: 12345.0, ..Default::default() };
            let n = slots.len();
            let mut share = vec![Vec::new(); n];
            let mut rates: Vec<BTreeMap<u32, Vec<f64>>> = vec![Default::default(); n];
            for i in 0..n {
                for j in i + 1..n {
                    let (Some(a), Some(b)) = (&builds[i], &builds[j]) else { continue };
                    let res = crate::sim::run::run_comparison(&data, &course, &race, a, b, &opts, true, false, None);
                    let runs = res.gaps.len().max(1) as f64;
                    let b_share = res.gaps.iter().filter(|g| **g > 0.0).count() as f64 / runs;
                    share[i].push(1.0 - b_share);
                    share[j].push(b_share);
                    for (k, u) in [(i, &res.umas[0]), (j, &res.umas[1])] {
                        for s in &u.skills {
                            rates[k].entry(s.id).or_default().push(s.rate / 100.0);
                        }
                    }
                }
            }
            for (k, s) in slots.iter_mut().enumerate() {
                let w = if share[k].is_empty() { 0.5 } else { share[k].iter().sum::<f64>() / share[k].len() as f64 };
                // Between about 6th (4,000) and 1st (10,000) in a twelve-runner field: an estimate.
                s.placement = 4000.0 + 6000.0 * w;
                let info = self.kb.umas.get(&s.trainee);
                let stars = self.roster.umas.iter().find(|u| u.id == s.trainee).map_or(3, |u| u.stars);
                s.skills = rates[k].iter().map(|(id, v)| skill_points(&self.kb, *id, info.and_then(|i| i.unique_skill), stars) * v.iter().sum::<f64>() / v.len() as f64).sum();
            }
            scored.push(slots);
        }
        // Fill the races with the fewest candidates first; each character once in the team.
        let mut order: Vec<usize> = (0..ROWS.len()).collect();
        order.sort_by_key(|r| scored[*r].len());
        let mut used: Vec<u32> = Vec::new();
        let mut rows: Vec<Option<Row>> = vec![None; ROWS.len()];
        for r in order {
            let mut pool: Vec<Slot> = scored[r].iter().filter(|s| !used.contains(&crate::affinity::chara(s.trainee))).cloned().collect();
            let mut team: Vec<Slot> = Vec::new();
            // Greedy: each pick maximises her points plus the positioning bonus for a new style.
            while team.len() < 3 && !pool.is_empty() {
                let gain = |s: &Slot, team: &[Slot]| s.placement + s.skills + if team.iter().any(|t| t.style == s.style) { 0.0 } else { POSITIONING };
                let (i, _) = pool.iter().enumerate().max_by(|a, b| gain(a.1, &team).total_cmp(&gain(b.1, &team))).ok_or("no candidate")?;
                let s = pool.remove(i);
                pool.retain(|p| crate::affinity::chara(p.trainee) != crate::affinity::chara(s.trainee));
                team.push(s);
            }
            for s in &team {
                used.push(crate::affinity::chara(s.trainee));
            }
            team.sort_by(|a, b| (b.placement + b.skills).total_cmp(&(a.placement + a.skills)));
            let styles = {
                let mut v: Vec<Style> = team.iter().map(|s| s.style).collect();
                v.sort_by_key(|s| *s as u8);
                v.dedup();
                v.len()
            };
            let mut rnotes = Vec::new();
            if team.len() < 3 {
                rnotes.push(format!("Only {} of your umas can run {} (A or B in it): an empty slot scores nothing.", team.len(), ROWS[r].0));
            }
            for (k, s) in team.iter_mut().enumerate() {
                s.ace = k == 0;
                let base = s.placement + s.skills + POSITIONING * styles as f64 / 3.0;
                s.total = if s.ace { base * 1.1 } else { base };
            }
            rows[r] = Some(Row { name: ROWS[r].0, slots: team, styles, notes: rnotes });
        }
        let rows: Vec<Row> = rows.into_iter().flatten().collect();
        let total: f64 = rows.iter().flat_map(|r| r.slots.iter().map(|s| s.total)).sum();
        // The career that raises the team most: fill an empty slot, else lift the weakest one.
        let next = if let Some(row) = rows.iter().find(|r| r.slots.len() < 3) {
            format!("Fill {}: no more of your umas can run it. Look at a uma with B in it whose parents can lift her to A.", row.name)
        } else if let Some((row, s)) = rows.iter().flat_map(|r| r.slots.iter().map(move |s| (r, s))).min_by(|a, b| a.1.total.total_cmp(&b.1.total)) {
            let name = super::uma_name(&self.kb, s.trainee);
            match s.parents.steps.first() {
                Some(step) if !s.natural => format!("{name} ({}): her lineage needs {} parent step(s) first; start with a career of {}.", row.name, s.parents.steps.len(), super::uma_name(&self.kb, step.trainee)),
                _ => format!("{name} ({}) scores lowest: train her career with the deck and skills shown; it raises the team most.", row.name),
            }
        } else {
            "Nothing to train.".into()
        };
        notes.push("Scores are estimates: placement from head-to-head races in the simulator (the real races have twelve runners), skills from how often each fired.".into());
        Ok(TtTeam { rows, total, next, notes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Team Trials team on the real account:
    /// `UMALORD_ROOT=<data folder with knowledge/> cargo test --release tt_team_real_account -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn tt_team_real_account() {
        let root = std::path::PathBuf::from(std::env::var("UMALORD_ROOT").expect("UMALORD_ROOT"));
        let mut a = App::new(Some(root));
        let t = std::time::Instant::now();
        let team = a.tt_team(200).unwrap();
        eprintln!("({:.1} s)\n{}", t.elapsed().as_secs_f64(), super::super::text::tt(&a.kb, &team));
        let mut charas = std::collections::BTreeSet::new();
        for r in &team.rows {
            assert!(r.slots.len() <= 3);
            assert_eq!(r.slots.iter().filter(|s| s.ace).count(), usize::from(!r.slots.is_empty()), "one Ace per race");
            for s in &r.slots {
                assert!(charas.insert(crate::affinity::chara(s.trainee)), "each character once");
                assert!(s.plan.friend.is_some() && s.plan.friend_lb == 4);
            }
        }
    }
}
