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

/// The community Team Trials meta (`knowledge/data/tt_meta.json`, hand-curated and sourced; how
/// to refresh it is in `knowledge/UPDATING.md`): each outfit's usual races and styles, track
/// lengths, stamina targets, the scenario, the inherit and the borrows.
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TtMeta {
    pub read: String,
    pub tracks: BTreeMap<String, serde_json::Value>,
    pub stamina: BTreeMap<String, serde_json::Value>,
    pub scenario: MetaScenario,
    pub inherit: MetaInherit,
    pub borrows: BTreeMap<String, serde_json::Value>,
    pub roles: MetaRoles,
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MetaScenario {
    pub name: String,
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MetaInherit {
    pub uma: u32,
    pub skill: String,
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MetaRoles {
    pub list: Vec<Role>,
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Role {
    pub id: u32,
    pub races: Vec<String>,
    pub styles: Vec<String>,
    pub unique: Option<String>,
    pub note: Option<String>,
}

impl TtMeta {
    pub fn role(&self, uma: u32) -> Option<&Role> {
        self.roles.list.iter().find(|r| r.id == uma)
    }

    /// The race's most common track length.
    fn length(&self, race: &str) -> Option<u32> {
        self.tracks.get(race)?.as_object()?.iter().filter_map(|(k, v)| Some((k.parse::<u32>().ok()?, v.as_f64()?))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0)
    }

    fn stamina(&self, race: &str) -> Option<u32> {
        self.stamina.get(race)?.as_u64().map(|v| v as u32)
    }

    fn borrows(&self, key: &str) -> Vec<u32> {
        self.borrows.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default()
    }
}

fn style_name(s: Style) -> &'static str {
    match s {
        Style::Front => "Front",
        Style::Pace => "Pace",
        Style::Late => "Late",
        Style::End => "End",
    }
}

pub(crate) fn parse_style(s: &str) -> Option<Style> {
    Some(match s {
        "Front" => Style::Front,
        "Pace" => Style::Pace,
        "Late" => Style::Late,
        "End" => Style::End,
        _ => return None,
    })
}

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
    /// Why she's here, and what to watch.
    pub notes: Vec<String>,
}

impl Slot {
    /// The goal her lineage plan was made for (her race's surface and distance, her style).
    pub fn parents_goal(&self, race: &str) -> Goal {
        let i = ROWS.iter().position(|r| r.0 == race).unwrap_or(0);
        let (_, surface, distance, _) = ROWS[i];
        Goal { runner: self.trainee, targets: vec![usize::from(surface == Surface::Dirt), 2 + Distance::ALL.iter().position(|d| *d == distance).unwrap_or(1), 6 + self.style as usize], label: format!("Team Trials {race}"), ..Default::default() }
    }
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
    fn tt_plan(&self, meta: &TtMeta, row: usize, trainee: u32, style: Style, course: Option<u32>) -> Plan {
        let (name, surface, distance, stamina) = ROWS[row];
        let stamina = meta.stamina(name).unwrap_or(stamina);
        let mut plan = Plan { purpose: crate::tab_planner::Purpose::TeamTrials, ..Plan::default() };
        plan.surface = surface;
        plan.distance = distance;
        plan.umalator_course = course;
        plan.style = style;
        plan.trainee = Some(trainee);
        plan.friend_lb = 4;
        plan.target = [1200, stamina, 1000, 600, 1000];
        plan.pick_objective = crate::skill_opt::PickObjective::Consistency;
        if !meta.scenario.name.is_empty() {
            plan.scenario = meta.scenario.name.clone();
        }
        plan
    }

    /// A runner's Team Trials plan for a race, with the meta's borrow and skills: what a brief needs.
    pub(crate) fn tt_runner_plan(&mut self, meta: &TtMeta, race: &str, trainee: u32, style: Style) -> Plan {
        let row = ROWS.iter().position(|r| r.0 == race).unwrap_or(0);
        let course = self.tt_course(meta, row);
        let saved = std::mem::take(&mut self.planner.plan);
        self.planner.plan = self.tt_plan(meta, row, trainee, style, course);
        self.recommend_deck(true);
        if let Some(f) = self.tt_friend(meta, row, style, &self.planner.plan.clone()) {
            self.planner.plan.friend = Some(f);
            self.planner.plan.friend_lb = 4;
        }
        self.auto_pick_skills();
        std::mem::replace(&mut self.planner.plan, saved)
    }

    /// The meta file from the data folder, or the copy built in.
    pub(crate) fn tt_meta(&self) -> TtMeta {
        let path = self.root.as_ref().map(|r| r.join("knowledge/data/tt_meta.json"));
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| include_str!("../../../knowledge/data/tt_meta.json").to_string());
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// The friend card to borrow for a runner: the meta's general pick, then the race's, then the
    /// style's, skipping her own character and characters already in the deck.
    fn tt_friend(&self, meta: &TtMeta, row: usize, style: Style, plan: &Plan) -> Option<u32> {
        // A deck holds one card per character, and never the trainee's own.
        let mut taken: Vec<&str> = plan.deck.iter().flatten().filter_map(|id| self.kb.cards.get(id).map(|c| c.uma.as_str())).collect();
        taken.extend(plan.trainee.and_then(|t| self.kb.umas.get(&t)).map(|u| u.name.as_str()));
        let today = crate::ui_util::today();
        ["general", ROWS[row].0, style_name(style)]
            .iter()
            .flat_map(|k| meta.borrows(k))
            .find(|id| self.kb.cards.get(id).is_some_and(|c| c.released(&today) && !taken.contains(&c.uma.as_str())))
    }

    /// A representative course for each race: one an event uses, else the first that fits.
    fn tt_course(&self, meta: &TtMeta, row: usize) -> Option<u32> {
        let (name, surface, distance, _) = ROWS[row];
        let length = meta.length(name);
        let mut fits: Vec<&crate::knowledge::UmalatorCourse> = self.kb.umalator_courses.iter().filter(|c| c.surface == surface.label() && length.map_or(Distance::of_metres(c.distance) == distance, |l| c.distance == l)).collect();
        if fits.is_empty() {
            fits = self.kb.umalator_courses.iter().filter(|c| c.surface == surface.label() && Distance::of_metres(c.distance) == distance).collect();
        }
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
        let meta = self.tt_meta();
        // Candidates per race: the meta's usual runners for it first (in its order), then A in
        // its surface and distance, then B (parents can lift a B). Her style is the meta's
        // preferred one where her grade there is B or better, else her best.
        let mut cands: Vec<Vec<(u32, Style, bool)>> = Vec::new();
        for &(name, surface, distance, _) in &ROWS {
            let mut list: Vec<(u32, Style, bool, (u8, bool, u8, u8, u8))> = self
                .roster
                .umas
                .iter()
                .filter_map(|u| {
                    let a = &self.kb.umas.get(&u.id)?.aptitude;
                    let (gs, gd) = (grade_rank(a.surface(surface)), grade_rank(a.distance(distance)));
                    if gs < 6 || gd < 6 {
                        return None;
                    }
                    let role = meta.role(u.id);
                    let fit = role.and_then(|r| r.races.iter().position(|x| x == name)).map_or(9, |i| i as u8);
                    let style = role.and_then(|r| r.styles.iter().filter_map(|s| parse_style(s)).find(|s| grade_rank(a.style(*s)) >= 6)).unwrap_or_else(|| a.best_style());
                    let steady = role.is_none_or(|r| r.unique.as_deref() != Some("inconsistent"));
                    Some((u.id, style, gs >= 7 && gd >= 7, (fit, steady, gs.min(7) + gd.min(7), u.stars, u.potential)))
                })
                .collect();
            list.sort_by(|a, b| a.3.0.cmp(&b.3.0).then(b.3.1.cmp(&a.3.1)).then(b.2.cmp(&a.2)).then((b.3.2, b.3.3, b.3.4).cmp(&(a.3.2, a.3.3, a.3.4))));
            cands.push(list.into_iter().take(6).map(|c| (c.0, c.1, c.2)).collect());
        }
        // Build and race each race's candidates head to head; score each.
        let mut scored: Vec<Vec<Slot>> = Vec::new();
        for (row, list) in cands.iter().enumerate() {
            let course_id = self.tt_course(&meta, row);
            let mut builds = Vec::new();
            let mut slots = Vec::new();
            for &(t, style, natural) in list {
                self.planner.plan = self.tt_plan(&meta, row, t, style, course_id);
                self.recommend_deck(true);
                // The meta's borrow for this race and style, when one fits the deck.
                if let Some(f) = self.tt_friend(&meta, row, style, &self.planner.plan.clone()) {
                    self.planner.plan.friend = Some(f);
                    self.planner.plan.friend_lb = 4;
                }
                self.auto_pick_skills();
                builds.push(self.plan_sim_build(true).ok());
                let mut snotes = Vec::new();
                if let Some(r) = meta.role(t) {
                    if r.races.iter().any(|x| x == ROWS[row].0) {
                        let article = if style == Style::End { "an" } else { "a" };
                        snotes.push(format!("Because: the community runs her in {} as {article} {} (umareference, read {}).", ROWS[row].0, style.label(), meta.read));
                    }
                    if let Some(n) = &r.note {
                        snotes.push(format!("Note: {n}."));
                    }
                }
                slots.push(Slot { trainee: t, style, ace: false, placement: 0.0, skills: 0.0, total: 0.0, natural, parents: Proposal::default(), plan: self.planner.plan.clone(), notes: snotes });
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
                let unique = info.and_then(|i| i.unique_skill);
                // A unique the community finds unreliable fires far less in a full field than in
                // head-to-head races; count 40% of it.
                let shaky = meta.role(s.trainee).is_some_and(|r| r.unique.as_deref() == Some("inconsistent"));
                s.skills = rates[k].iter().map(|(id, v)| skill_points(&self.kb, *id, unique, stars) * v.iter().sum::<f64>() / v.len() as f64 * if shaky && Some(*id) == unique { 0.4 } else { 1.0 }).sum();
                if shaky {
                    s.notes.push("Her unique fires unreliably in Team Trials (community), so it's counted at 40%.".into());
                }
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
        let mut rows: Vec<Row> = rows.into_iter().flatten().collect();
        // Each runner's lineage: her race's surface and distance and her style at A, inheriting
        // the meta's unique (564 Escapades) when a route reaches it, else without.
        let inherit = (meta.inherit.uma > 0 && self.kb.umas.contains_key(&meta.inherit.uma)).then_some(meta.inherit.uma);
        for row in &mut rows {
            let i = ROWS.iter().position(|r| r.0 == row.name).unwrap_or(0);
            let (_, surface, distance, _) = ROWS[i];
            for s in &mut row.slots {
                let mut goal = Goal { runner: s.trainee, targets: vec![usize::from(surface == Surface::Dirt), 2 + Distance::ALL.iter().position(|d| *d == distance).unwrap_or(1), 6 + s.style as usize], label: format!("Team Trials {}", row.name), ..Default::default() };
                let mut p = None;
                if let Some(g) = inherit.filter(|g| crate::affinity::chara(*g) != crate::affinity::chara(s.trainee)) {
                    goal.green = Some(g);
                    let with = super::jobs::propose(&self.kb, &goal, inv);
                    let reach = with.now.as_ref().is_some_and(super::score::Lineage::meets) || (!with.steps.is_empty() && !with.over_cap);
                    if reach {
                        s.notes.push(format!("Her lineage carries {} (the community's inherit for every Team Trials runner).", meta.inherit.skill));
                        p = Some(with);
                    } else {
                        // The community's way: borrow a friend's finished Summer Gold Ship as one
                        // parent; her green comes with her. Does one of the player's own finished
                        // umas still give the rest of the start?
                        let mut inv2 = inv.clone();
                        let borrowed = inv2.borrow(&self.kb, &super::Borrow { uma: g, own: vec![super::green_spark(g, 1)], parents: [(0, vec![]), (0, vec![])] });
                        let with_friend = super::jobs::propose(&self.kb, &goal, &inv2);
                        let partner = with_friend.now.as_ref().filter(|l| l.meets()).and_then(|l| l.legacies.iter().map(|i| &inv2.parents[*i]).find(|x| x.id != borrowed).map(|x| super::parent_name(&self.kb, x)));
                        goal.green = None;
                        match partner {
                            Some(w) => s.notes.push(format!("Borrow a friend's Summer Gold Ship as one parent ({} comes with her) and pair her with {w}: every target still starts at A.", meta.inherit.skill)),
                            None => s.notes.push(format!("{} needs Summer Gold Ship as a parent: borrow a friend's, and plan the other parent so her targets still start at A.", meta.inherit.skill)),
                        }
                    }
                }
                s.parents = p.unwrap_or_else(|| super::jobs::propose(&self.kb, &goal, inv));
            }
        }
        let total: f64 = rows.iter().flat_map(|r| r.slots.iter().map(|s| s.total)).sum();
        // The career that raises the team most: fill an empty slot, else lift the weakest one.
        let next = if let Some(row) = rows.iter().find(|r| r.slots.len() < 3) {
            format!("Fill {}: no more of your umas can run it. Look at a uma with B in it whose parents can lift her to A.", row.name)
        } else if let Some((row, s)) = rows.iter().flat_map(|r| r.slots.iter().map(move |s| (r, s))).min_by(|a, b| a.1.total.total_cmp(&b.1.total)) {
            let name = super::uma_name(&self.kb, s.trainee);
            let shaky = meta.role(s.trainee).is_some_and(|r| r.unique.as_deref() == Some("inconsistent"));
            let on_team: Vec<u32> = rows.iter().flat_map(|r| r.slots.iter().map(|x| crate::affinity::chara(x.trainee))).collect();
            let others: Vec<String> = meta.roles.list.iter().filter(|r| r.races.iter().any(|x| x == row.name) && r.unique.is_none() && !on_team.contains(&crate::affinity::chara(r.id))).map(|r| {
                let owned = self.roster.umas.iter().any(|u| u.id == r.id);
                format!("{}{}", super::uma_name(&self.kb, r.id), if owned { " (yours)" } else { "" })
            }).take(4).collect();
            if shaky && !others.is_empty() {
                format!("{name} ({}) scores lowest and her unique fires unreliably: a steadier {} runner would score more, such as {}.", row.name, row.name, others.join(", "))
            } else {
            match s.parents.steps.first() {
                Some(step) if !s.natural => format!("{name} ({}): her lineage needs {} parent step(s) first; start with a career of {}.", row.name, s.parents.steps.len(), super::uma_name(&self.kb, step.trainee)),
                _ => format!("{name} ({}) scores lowest: train her career with the deck and skills shown; it raises the team most.", row.name),
            }
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
        let inv = a.parent_inventory();
        let next = a.tt_next(&team, &inv).expect("a next career");
        eprintln!("{}", super::super::brief::text(&a.kb, &next));
        assert!(next.friend.is_some(), "the next career borrows a friend card");
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
