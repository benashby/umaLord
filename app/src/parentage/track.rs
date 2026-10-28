//! The saved parent plan (plans/parents.json, one goal at a time) and its progress.
//!
//! The player runs the current step until a finished uma has its must-haves. Each import is
//! checked against that step: the umas of its trainee finished since the step began, with what
//! they have. Keeping one ends the step: with its must-haves met, or settled for without them.
//! Either way the plan is worked out again from everything the account now has; if the goal still
//! fits under the cap the new steps replace the old ones, otherwise the plan waits for the player
//! to pick another route.

use super::jobs::{Proposal, Run};
use super::{Goal, Inventory};
use crate::knowledge::Knowledge;
use crate::veterans::Veteran;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "plans/parents.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ParentPlan {
    pub goal: Goal,
    /// The day it was made (local).
    pub made: String,
    /// Steps finished, then the ones to run.
    pub steps: Vec<Step>,
    /// The finished uma the last step's product pairs with, for a one-step route.
    pub partner: Option<u64>,
    /// Why the plan is waiting for the player's choice, when the goal no longer fits the cap.
    pub blocked: Option<String>,
    /// The player accepted a route with a step past the cap.
    pub over_cap: bool,
    /// What happened, oldest first.
    pub log: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Step {
    pub run: Run,
    /// When the newest finished uma was made as the step began (the game's create_time): only
    /// umas finished after it count for the step.
    pub after: String,
    /// The finished uma kept for it.
    pub done: Option<u64>,
    /// Kept without all its must-haves.
    pub settled: bool,
}

/// A finished uma that counts for the current step.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: u64,
    pub must: bool,
    /// The nice-to-haves it has.
    pub nice: Vec<usize>,
    /// Whether it ran with the step's legacies.
    pub same_parents: bool,
}

impl ParentPlan {
    /// A plan from a proposal, its steps starting after the newest finished uma.
    pub fn new(goal: Goal, p: &Proposal, vets: &[Veteran], today: &str) -> Self {
        let after = newest(vets);
        let steps = p.steps.iter().map(|r| Step { run: r.clone(), after: after.clone(), ..Default::default() }).collect();
        ParentPlan { goal, made: today.into(), steps, partner: p.partner, blocked: None, over_cap: p.over_cap, log: vec![format!("{today}: planned.")] }
    }

    pub fn current(&self) -> Option<usize> {
        self.steps.iter().position(|s| s.done.is_none())
    }

    /// The current step's finished umas, newest first.
    pub fn hits(&self, vets: &[Veteran]) -> Vec<Hit> {
        let Some(step) = self.current().map(|i| &self.steps[i]) else { return Vec::new() };
        let mut out: Vec<(String, Hit)> = vets
            .iter()
            .filter(|v| v.card_id == step.run.trainee && created(v) > step.after)
            .map(|v| {
                let own = v.factors();
                let same = same_parents(step, v, vets);
                // The route counts the step's legacies' own pinks too, so other legacies don't meet it.
                let hit = Hit { id: v.trained_chara_id, must: same && step.run.must.iter().all(|w| w.met(&own)), nice: step.run.nice.iter().enumerate().filter(|(_, w)| w.met(&own)).map(|(i, _)| i).collect(), same_parents: same };
                (created(v), hit)
            })
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out.into_iter().map(|h| h.1).collect()
    }

    /// Whether finished uma `id` can be kept for the current step: the step, whether it ran
    /// with the step's legacies, and whether it meets the must-haves.
    pub fn judge(&self, kb: &Knowledge, vets: &[Veteran], id: u64) -> Result<(usize, bool, bool), String> {
        if let Some(why) = &self.blocked {
            return Err(format!("The plan is waiting for your choice ({why}): plan another route first."));
        }
        let i = self.current().ok_or("Every step is done: the runner's lineage is ready.")?;
        let v = vets.iter().find(|v| v.trained_chara_id == id).ok_or(format!("No finished uma #{id}; import it first."))?;
        let step = &self.steps[i];
        if v.card_id != step.run.trainee {
            return Err(format!("#{id} is {}, but step {} is a career of {}.", super::uma_name(kb, v.card_id), i + 1, super::uma_name(kb, step.run.trainee)));
        }
        if created(v) <= step.after {
            return Err(format!("#{id} finished before step {} began; keep one finished since.", i + 1));
        }
        let same = same_parents(step, v, vets);
        Ok((i, same, same && step.run.must.iter().all(|w| w.met(&v.factors()))))
    }

    /// Keep finished uma `id` for the current step, then work the plan out again from the
    /// account as it is now. Returns what changed.
    pub fn settle(&mut self, kb: &Knowledge, inv: &Inventory, vets: &[Veteran], id: u64, today: &str) -> Result<String, String> {
        let (i, same, met) = self.judge(kb, vets, id)?;
        let own = vets.iter().find(|v| v.trained_chara_id == id).map(|v| v.factors()).unwrap_or_default();
        let step = &mut self.steps[i];
        step.done = Some(id);
        step.settled = !met;
        let mut msg = if met {
            format!("Step {} done with #{id}.", i + 1)
        } else if !same {
            format!("Step {} settled with #{id}, which ran with other legacies than the step's.", i + 1)
        } else {
            format!("Step {} settled with #{id}, short of {}.", i + 1, missing(kb, &step.run, &own))
        };
        self.log.push(format!("{today}: {msg}"));
        msg += " ";
        msg += &self.revalidate(kb, inv, vets, today);
        Ok(msg)
    }

    /// Work the remaining steps out again from what the account has now.
    pub fn revalidate(&mut self, kb: &Knowledge, inv: &Inventory, vets: &[Veteran], today: &str) -> String {
        let p = super::jobs::propose(kb, &self.goal, inv);
        let done: Vec<Step> = self.steps.iter().filter(|s| s.done.is_some()).cloned().collect();
        let after = newest(vets);
        let msg = if p.now.as_ref().is_some_and(super::score::Lineage::meets) {
            self.blocked = None;
            self.steps = done;
            self.partner = None;
            "The account's finished umas now meet the goal: the runner can train.".to_string()
        } else if !p.impossible.is_empty() || p.steps.is_empty() || (p.over_cap && !self.over_cap) {
            let why = if p.steps.is_empty() { "no route reaches the goal from what the account has" } else { "every route left has a step past the cap" };
            self.blocked = Some(why.to_string());
            self.steps = done;
            self.partner = None;
            format!("The goal no longer fits: {why}. Pick another route: another runner or goal, a friend's legacy, or the cheapest route past the cap if you accept it.")
        } else {
            let same = p.steps.iter().map(|r| (r.trainee, &r.must)).eq(self.steps.iter().filter(|s| s.done.is_none()).map(|s| (s.run.trainee, &s.run.must)));
            self.blocked = None;
            self.partner = p.partner;
            self.steps = done.into_iter().chain(p.steps.iter().map(|r| Step { run: r.clone(), after: after.clone(), ..Default::default() })).collect();
            if same { "The rest of the plan stands.".into() } else { "The plan's next steps changed to fit what you have now.".into() }
        };
        self.log.push(format!("{today}: {msg}"));
        msg
    }
}

/// Whether a finished uma ran with the step's two legacies. The game keeps a parent's outfit,
/// not its id, so outfits are compared.
fn same_parents(step: &Step, v: &Veteran, vets: &[Veteran]) -> bool {
    v.ancestor(10).zip(v.ancestor(20)).is_some_and(|(a, b)| {
        // The outfits the step recorded, else (an older plan) the legacies still imported.
        let mut want: Vec<u32> = if step.run.legacy_umas.iter().all(|u| *u > 0) {
            step.run.legacy_umas.to_vec()
        } else {
            step.run.legacies.iter().filter_map(|id| vets.iter().find(|x| x.trained_chara_id == *id)).map(|x| x.card_id).collect()
        };
        let mut got = vec![a.card_id, b.card_id];
        got.sort_unstable();
        want.sort_unstable();
        got == want
    })
}

/// The must-haves a finished uma lacks.
fn missing(kb: &Knowledge, run: &Run, own: &[u32]) -> String {
    run.must.iter().filter(|w| !w.met(own)).map(|w| w.label(kb)).collect::<Vec<_>>().join(", ")
}

fn created(v: &Veteran) -> String {
    v.create_time.as_str().unwrap_or_default().to_string()
}

fn newest(vets: &[Veteran]) -> String {
    vets.iter().map(created).max().unwrap_or_default()
}

pub fn load(root: &Path) -> Option<ParentPlan> {
    serde_json::from_str(&std::fs::read_to_string(root.join(FILE)).ok()?).ok()
}

pub fn save(root: &Path, plan: &ParentPlan) -> Result<(), String> {
    let path = root.join(FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(plan).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kb, spark};
    use super::*;
    use serde_json::json;

    fn vet(id: u64, card: u32, time: &str, factors: &[u32], parents: &[(u32, u32)]) -> Veteran {
        serde_json::from_value(json!({
            "trained_chara_id": id, "card_id": card, "create_time": time,
            "factor_info_array": factors.iter().map(|f| json!({"factor_id": f})).collect::<Vec<_>>(),
            "succession_chara_array": parents.iter().map(|(pos, c)| json!({"position_id": pos, "card_id": c})).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    /// Oguri Cap needs one Dirt star; Haru Urara runs for it with the two finished umas there are.
    fn start() -> (Knowledge, Goal, Vec<Veteran>, ParentPlan) {
        let kb = kb();
        let goal = Goal { runner: 100601, targets: vec![1], ..Default::default() };
        let vets = vec![vet(1, 106801, "2026-10-01 10:00:00", &[spark(&kb, "pink", "Turf", 2)], &[(10, 100101), (20, 100201)]), vet(2, 100201, "2026-10-02 10:00:00", &[], &[(10, 100101), (20, 106801)])];
        let inv = Inventory::new(&kb, &vets, vec![(105201, 3)]);
        let p = super::super::jobs::propose(&kb, &goal, &inv);
        let plan = ParentPlan::new(goal.clone(), &p, &vets, "2026-10-27");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].run.trainee, 105201);
        assert_eq!(plan.steps[0].after, "2026-10-02 10:00:00");
        (kb, goal, vets, plan)
    }

    #[test]
    fn import_meets_the_step() {
        let (kb, _, mut vets, mut plan) = start();
        // An older Haru Urara doesn't count; the new one has Dirt 2★.
        vets.push(vet(3, 105201, "2026-09-01 10:00:00", &[spark(&kb, "pink", "Dirt", 3)], &[]));
        vets.push(vet(4, 105201, "2026-10-20 10:00:00", &[spark(&kb, "pink", "Dirt", 2)], &[(10, 106801), (20, 100201)]));
        let hits = plan.hits(&vets);
        assert_eq!(hits, vec![Hit { id: 4, must: true, nice: vec![0], same_parents: true }]);
        assert!(super::super::text::import_note(&kb, &plan, &vets).is_some_and(|n| n.contains("#4")));
        let inv = Inventory::new(&kb, &vets, vec![(105201, 3)]);
        let msg = plan.settle(&kb, &inv, &vets, 4, "2026-10-21").unwrap();
        assert!(msg.contains("Step 1 done") && msg.contains("meet the goal"), "{msg}");
        assert_eq!(plan.current(), None);
        assert!(plan.settle(&kb, &inv, &vets, 4, "2026-10-21").is_err(), "nothing left to settle");
    }

    #[test]
    fn settling_short_replans() {
        let (kb, _, mut vets, mut plan) = start();
        vets.push(vet(4, 105201, "2026-10-20 10:00:00", &[spark(&kb, "pink", "Sprint", 2)], &[(10, 106801), (20, 100201)]));
        assert!(!plan.hits(&vets)[0].must);
        assert!(plan.settle(&kb, &Inventory::new(&kb, &vets, vec![]), &vets, 1, "x").is_err(), "#1 isn't a Haru Urara");
        let inv = Inventory::new(&kb, &vets, vec![(105201, 3)]);
        let msg = plan.settle(&kb, &inv, &vets, 4, "2026-10-21").unwrap();
        assert!(msg.contains("settled") && msg.contains("Dirt pink 1★+"), "{msg}");
        assert!(plan.steps[0].settled && plan.blocked.is_none());
        // The goal still fits, so a new step follows, counting from the settled uma on.
        assert_eq!(plan.current(), Some(1));
        assert_eq!(plan.steps[1].after, "2026-10-20 10:00:00");
        // With nothing left to train, the plan waits for a choice, and says so rather than done.
        let none = Inventory::new(&kb, &vets, vec![]);
        let msg = plan.revalidate(&kb, &none, &vets, "2026-10-22");
        assert!(plan.blocked.is_some() && msg.contains("no longer fits"), "{msg}");
        assert_eq!(plan.steps.len(), 1, "only the step kept stays");
        let text = super::super::text::plan(&kb, &plan, &none, &vets);
        assert!(text.contains("Waiting for your choice") && !text.contains("Every step is done"), "{text}");
    }

    #[test]
    fn other_legacies_or_older_umas_dont_count() {
        let (kb, _, mut vets, mut plan) = start();
        // Dirt 2★, but run with other legacies: the route counted the step's legacies' pinks.
        vets.push(vet(4, 105201, "2026-10-20 10:00:00", &[spark(&kb, "pink", "Dirt", 2)], &[(10, 100101), (20, 100201)]));
        vets.push(vet(5, 105201, "2026-09-20 10:00:00", &[spark(&kb, "pink", "Dirt", 2)], &[(10, 106801), (20, 100201)]));
        let hits = plan.hits(&vets);
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].must && !hits[0].same_parents);
        assert!(super::super::text::import_note(&kb, &plan, &vets).is_none());
        let inv = Inventory::new(&kb, &vets, vec![(105201, 3)]);
        assert!(plan.settle(&kb, &inv, &vets, 5, "x").is_err(), "#5 finished before the step began");
        let msg = plan.settle(&kb, &inv, &vets, 4, "x").unwrap();
        assert!(msg.contains("other legacies") && plan.steps[0].settled, "{msg}");
    }
}

/// A player following plans on the real account with the dice rolled for them: run the current
/// step, reroll the sparks once, keep the first uma with the must-haves, and let the plan work
/// itself out again. Every plan must finish (or say it no longer fits), and the careers it takes
/// should match what it predicted.
/// `cargo test --release parentage_play_through -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn parentage_play_through() {
    use super::outcome::PINK_STARS;
    use super::{after_start, ranks};
    use serde_json::json;
    let kb = super::tests::kb();
    let root = std::env::var_os("UMALORD_ROOT").map(std::path::PathBuf::from).or_else(|| crate::data::find_root(None)).unwrap();
    let base_vets = crate::veterans::load(&root);
    let (roster, _) = crate::data::Roster::load(Some(&root));
    let owned: Vec<(u32, u8)> = roster.umas.iter().map(|u| (u.id, u.stars)).collect();
    let inv0 = Inventory::new(&kb, &base_vets, owned.clone());
    let mut goals = Vec::new();
    for &(runner, _) in &owned {
        for surface in [0, 1] {
            for distance in 2..6 {
                for style in 6..10 {
                    let goal = Goal { runner, targets: vec![surface, distance, style], ..Default::default() };
                    let p = super::jobs::propose(&kb, &goal, &inv0);
                    if !p.steps.is_empty() && !p.over_cap {
                        goals.push((goal, p.careers()));
                    }
                }
            }
        }
    }
    let filter = std::env::var("GOALS").unwrap_or_default();
    let goals: Vec<_> = if filter.is_empty() {
        goals.iter().step_by((goals.len() / 20).max(1)).cloned().collect()
    } else {
        goals.into_iter().filter(|(g, _)| filter.split(';').any(|f| super::text::goal(&kb, g).contains(f))).collect()
    };
    let mut rng = crate::sim::rng::Rng::new(12345.0);
    let mut pick = |weights: &[f64]| {
        let r = rng.random() * weights.iter().sum::<f64>();
        let mut acc = 0.0;
        weights.iter().position(|w| {
            acc += w;
            r < acc
        }).unwrap_or(weights.len() - 1)
    };
    let (mut total_pred, mut total_real, mut blocked) = (0.0, 0.0, 0);
    for (goal, predicted) in &goals {
        let trials = 12;
        let mut careers_sum = 0usize;
        for _ in 0..trials {
            let mut vets = base_vets.clone();
            let inv = Inventory::new(&kb, &vets, owned.clone());
            let mut plan = ParentPlan::new(goal.clone(), &super::jobs::propose(&kb, goal, &inv), &vets, "t");
            let mut careers = 0;
            let mut next_id = 900_000u64;
            while let Some(cur) = plan.current() {
                assert!(careers < 300, "{}: never finishes", super::text::goal(&kb, goal));
                careers += 1;
                let run = plan.steps[cur].run.clone();
                let legs: Vec<&crate::veterans::Veteran> = run.legacies.iter().filter_map(|id| vets.iter().find(|v| v.trained_chara_id == *id)).collect();
                let lp: Vec<super::Parent> = legs.iter().map(|v| super::Parent::from_veteran(&kb, v)).collect();
                let base = ranks(&kb.umas[&run.trainee].aptitude);
                let fin: Vec<u8> = (0..10).map(|i| after_start(base[i], lp.iter().map(|p| u32::from(p.pink_line[i])).sum())).collect();
                // The Wiki's grade weights, the rule the plan's odds use.
                let weights: Vec<f64> = fin.iter().map(|r| [0.0, 0.10, 0.25, 0.45, 0.65, 0.775, 0.875, 1.0, 1.075][*r as usize]).collect();
                // Two draws (the reroll); keep the one with the must-haves, else the first.
                let mut draw = || {
                    let apt = pick(&weights);
                    let stars = 1 + pick(&PINK_STARS) as u8;
                    let stat = pick(&[1.0; 5]);
                    let bstars = 1 + pick(&[0.45, 0.50, 0.05]) as u8;
                    let gstars = 1 + pick(&[0.45, 0.50, 0.05]) as u8;
                    let mut f = vec![super::pink_spark(apt, stars), (stat as u32 + 1) * 100 + u32::from(bstars)];
                    if owned.iter().any(|(u, s)| *u == run.trainee && *s >= 3) {
                        f.push(super::green_spark(run.trainee, gstars));
                    }
                    f
                };
                let a = draw();
                let b = draw();
                let own = if run.must.iter().all(|w| w.met(&a)) { a } else { b };
                if !run.must.iter().all(|w| w.met(&own)) {
                    continue;
                }
                next_id += 1;
                let anc: Vec<serde_json::Value> = legs.iter().zip([10, 20]).map(|(v, pos)| json!({"position_id": pos, "card_id": v.card_id, "factor_info_array": v.factors().iter().map(|f| json!({"factor_id": f})).collect::<Vec<_>>()})).collect();
                let v: crate::veterans::Veteran = serde_json::from_value(json!({"trained_chara_id": next_id, "card_id": run.trainee, "create_time": format!("2027-01-01 00:{:02}:{:02}", careers / 60, careers % 60), "factor_info_array": own.iter().map(|f| json!({"factor_id": f})).collect::<Vec<_>>(), "succession_chara_array": anc, "speed": 900, "stamina": 800, "power": 800, "guts": 400, "wiz": 600, "rank": 14})).unwrap();
                vets.push(v);
                let inv = Inventory::new(&kb, &vets, owned.clone());
                plan.settle(&kb, &inv, &vets, next_id, "t").unwrap();
                if plan.blocked.is_some() {
                    blocked += 1;
                    break;
                }
            }
            if !filter.is_empty() {
                eprintln!("  {careers} careers: {}", plan.log.join(" | "));
            }
            careers_sum += careers;
        }
        let real = careers_sum as f64 / f64::from(trials);
        total_pred += predicted;
        total_real += real;
        eprintln!("{:>5.1} predicted {:>5.1} played  {}", predicted, real, super::text::goal(&kb, goal));
    }
    eprintln!("all: {total_pred:.1} predicted, {total_real:.1} played, {blocked} blocked");
}
