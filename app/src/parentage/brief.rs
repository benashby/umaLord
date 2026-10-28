//! "What's my next career?": everything the player needs to run it, in one place. For a parent
//! step: the trainee, her two parents, the scenario, a deck with a borrowed MLB friend card, the
//! skills to buy (for rank, which sets spark stars), the stats to aim for, the G1s to win, and
//! what makes her a keeper. For a runner (a CM or Team Trials team's): her deck and skills, the
//! stats a viable run reaches (stamina is a hard floor), and when racing helps or hurts.
//!
//! Race advice follows `knowledge/mechanics.md` and `knowledge/modes.md`: a G1 her parents also
//! won adds 3 affinity per parent; a unique levels up at Senior-year fan counts (Turf 60,000 by
//! Valentine's, 70,000 by Fan Appreciation, 120,000 by Christmas; Dirt 40,000 / 60,000 / 80,000);
//! a third race in a row risks her mood and a fourth costs stats (umareference, Optional Races,
//! 2026-08-26; Game8, Unique Skill Level, read 2026-10-28).

use super::jobs::Run;
use super::{Goal, Inventory, STATS, uma_name};
use crate::App;
use crate::knowledge::{Knowledge, Style, grade_rank};
use crate::tab_planner::{Plan, Purpose};

/// One career, ready to run.
#[derive(Clone)]
pub struct Brief {
    /// What it's for, in a line.
    pub why: String,
    pub trainee: u32,
    pub style: Style,
    pub scenario: String,
    /// Her two parents, in words.
    pub parents: String,
    pub deck: Vec<u32>,
    pub friend: Option<u32>,
    pub skills: Vec<u32>,
    /// The stats to train toward.
    pub targets: [u32; 5],
    /// What a viable run must reach (stamina for her race), or what only shapes the odds.
    pub stat_notes: Vec<String>,
    pub races: Vec<String>,
    /// Keep her if…
    pub keep: Vec<String>,
}

/// The unique's fan counts for her, by whether she's a Dirt uma (higher Dirt grade than Turf).
fn fan_line(kb: &Knowledge, trainee: u32) -> String {
    let dirt = kb.umas.get(&trainee).is_some_and(|u| grade_rank(&u.aptitude.dirt) > grade_rank(&u.aptitude.turf));
    let (a, b, c) = if dirt { ("40,000", "60,000", "80,000") } else { ("60,000", "70,000", "120,000") };
    format!("Her unique levels up in the Senior year at {a} fans by Valentine's, {b} by Fan Appreciation (with the Director's bond green) and {c} by Christmas: run extra races only to reach those, on turns without a strong friendship training.")
}

const RACING: &str = "Don't run three races in a row if you can help it (a third risks her mood; a fourth costs stats).";

impl App {
    /// The brief for a parent step of a goal's plan.
    pub(crate) fn parent_brief(&mut self, goal: &Goal, run: &Run, inv: &Inventory, step: usize) -> Brief {
        let kb = &self.kb;
        let style = kb.umas.get(&run.trainee).map(|u| u.aptitude.best_style()).unwrap_or_default();
        let parents = if run.legacies == [0, 0] {
            "any two legacies (yours or a friend's), best without pinks that raise her other aptitudes to A".to_string()
        } else {
            run.legacies.iter().map(|id| inv.by_id(*id).map_or(format!("#{id}"), |p| super::parent_name(kb, p))).collect::<Vec<_>>().join(" and ")
        };
        // Parents: rank decides green and white stars, so skills are bought for rating; a
        // wanted blue stat should finish at 1100 or more.
        let mut targets = [1200, 900, 1000, 500, 900];
        for i in &run.stat_aims {
            targets[*i] = targets[*i].max(1100);
        }
        let saved = std::mem::take(&mut self.planner.plan);
        self.planner.plan = Plan {
            purpose: Purpose::Parent,
            trainee: Some(run.trainee),
            style,
            target: targets,
            friend_lb: 4,
            pick_objective: crate::skill_opt::PickObjective::Rating,
            scenario: "Trackblazer".into(),
            ..Plan::default()
        };
        self.recommend_deck(true);
        self.rating_auto_pick();
        let plan = std::mem::replace(&mut self.planner.plan, saved);
        let kb = &self.kb;
        let mut races: Vec<String> = run.g1s.iter().map(|(n, c)| format!("Win {n} (+{} affinity)", 3 * u32::from(*c))).collect();
        races.push("Trackblazer is built on racing (30 to 40 races a career): pick the G1s above among them.".into());
        races.push(RACING.into());
        let mut stat_notes = vec!["Stats don't pass or fail a parent run: they set her rank (SS or better makes green and white sparks 2★ or better 80% of the time) and her blue spark's stars.".into()];
        for i in &run.stat_aims {
            stat_notes.push(format!("{} at 1100 or more: her blue spark, if it's {}, comes 2★ or better 80% of the time.", STATS[*i], STATS[*i]));
        }
        let mut keep: Vec<String> = run.must.iter().map(|w| format!("Must have: {}", w.label(kb))).collect();
        if keep.is_empty() {
            keep.push("Any finished career counts: her parents bring what the plan needs.".into());
        }
        keep.extend(run.nice.iter().map(|w| format!("Nice to have: {}", w.label(kb))));
        keep.push("Use the 30 TP spark reroll on a run that's close.".into());
        Brief {
            why: format!("Step {} of the parent plan for {}.", step + 1, uma_name(kb, goal.runner)),
            trainee: run.trainee,
            style,
            scenario: plan.scenario.clone(),
            parents,
            deck: plan.deck.iter().flatten().copied().collect(),
            friend: plan.friend,
            skills: plan.skills.clone(),
            targets: plan.target,
            stat_notes,
            races,
            keep,
        }
    }

    /// The brief for a runner's own career (a CM or Team Trials team's), from her plan.
    pub(crate) fn runner_brief(&self, why: String, plan: &Plan, stamina_floor: Option<(u32, String)>, parents: String) -> Brief {
        let kb = &self.kb;
        let trainee = plan.trainee.unwrap_or_default();
        let mut stat_notes = Vec::new();
        if let Some((floor, race)) = &stamina_floor {
            stat_notes.push(format!("Stamina {floor} or more is the floor for {race}: below it she runs dry on longer tracks and loses placement and skill points."));
        }
        stat_notes.push(format!(
            "Aim for {}; Speed first, then the stamina floor, then Power and Wit (Wit makes skills fire and keeps her calm).",
            plan.target.iter().enumerate().map(|(i, v)| format!("{} {v}", STATS[i])).collect::<Vec<_>>().join(", ")
        ));
        let mut races = vec![fan_line(kb, trainee)];
        if plan.scenario == "Grand Concert" {
            races.push("Grand Concert's races give no Performance Points: run her objective races and the fan races above, and train the rest.".into());
        }
        races.push(RACING.into());
        let keep = vec![
            format!("Keep her if she reaches the stats above{} and learns the skills listed.", stamina_floor.as_ref().map_or(String::new(), |(f, _)| format!(" (Stamina {f} at the least)"))),
            "Skills you can't afford by the end: the ones listed first matter most.".into(),
        ];
        Brief {
            why,
            trainee,
            style: plan.style,
            scenario: if plan.scenario.is_empty() { "the scenario of your choice".into() } else { plan.scenario.clone() },
            parents,
            deck: plan.deck.iter().flatten().copied().collect(),
            friend: plan.friend,
            skills: plan.skills.clone(),
            targets: plan.target,
            stat_notes,
            races,
            keep,
        }
    }
}

/// The brief as text: "What to do next".
pub fn text(kb: &Knowledge, b: &Brief) -> String {
    let deck: Vec<String> = b.deck.iter().filter_map(|c| kb.cards.get(c).map(|c| format!("{} [{}]", c.uma, c.title))).collect();
    let friend = b.friend.and_then(|c| kb.cards.get(&c)).map_or("none".into(), |c| format!("{} [{}], fully limit broken (MLB)", c.uma, c.title));
    let skills: Vec<String> = b.skills.iter().filter_map(|s| kb.skills.get(s).map(|k| k.name.clone())).collect();
    let article = if b.style == Style::End { "an" } else { "a" };
    let mut out = format!("Next career: {} as {article} {}. {}\n", uma_name(kb, b.trainee), b.style.label(), b.why);
    out += &format!("- Parents: {}.\n- Scenario: {}.\n", b.parents, b.scenario);
    out += &format!("- Deck: {}; borrow {friend}.\n", deck.join(", "));
    out += &format!("- Skills to target: {}.\n", if skills.is_empty() { "none picked yet".to_string() } else { skills.join(", ") });
    out += &format!("- Stats: {}\n", b.stat_notes.join(" "));
    out += &format!("- Races: {}\n", b.races.join(" "));
    out += &format!("- Keep: {}\n", b.keep.join(" "));
    out
}

impl App {
    /// A CM team's next career: the queue's first parent step, else the ace's own career (the
    /// runner who wins most head to head).
    pub(crate) fn cm_next(&mut self, team: &super::team::CmTeam, inv: &Inventory) -> Option<Brief> {
        if let Some((i, run)) = team.queue.first() {
            let r = &team.runners[*i];
            let step = r.parents.steps.iter().position(|s| s == run).unwrap_or(0);
            return Some(self.parent_brief(&r.goal, run, inv, step));
        }
        let ace = team.runners.iter().max_by(|a, b| a.sim.map_or(0.0, |s| s.0).total_cmp(&b.sim.map_or(0.0, |s| s.0)))?;
        let parents = ace.parents.now.as_ref().map_or("her best two finished umas".into(), |l| l.legacies.iter().map(|i| super::parent_name(&self.kb, &inv.parents[*i])).collect::<Vec<_>>().join(" and "));
        Some(self.runner_brief(format!("Her own career for CM {} ({}): she wins most head to head, so she's the ace.", team.event.id, team.event.name), &ace.plan, Some((ace.plan.target[1], team.event.course_name.clone())), parents))
    }

    /// A Team Trials team's next career: the weakest runner's first parent step, else her own
    /// career.
    pub(crate) fn tt_next(&mut self, team: &super::tt::TtTeam, inv: &Inventory) -> Option<Brief> {
        let (row, s) = team.rows.iter().flat_map(|r| r.slots.iter().map(move |s| (r, s))).min_by(|a, b| a.1.total.total_cmp(&b.1.total))?;
        // An unreliable unique at the bottom: brief the community's steadier runner for that race
        // that the player owns and the team doesn't use yet.
        let meta = self.tt_meta();
        if meta.role(s.trainee).is_some_and(|r| r.unique.as_deref() == Some("inconsistent")) {
            let on_team: Vec<u32> = team.rows.iter().flat_map(|r| r.slots.iter().map(|x| crate::affinity::chara(x.trainee))).collect();
            let alt = meta.roles.list.iter().find(|r| r.races.iter().any(|x| x == row.name) && r.unique.is_none() && !on_team.contains(&crate::affinity::chara(r.id)) && self.roster.umas.iter().any(|u| u.id == r.id));
            if let Some(r) = alt {
                let a = self.kb.umas.get(&r.id).map(|u| u.aptitude.clone()).unwrap_or_default();
                let style = r.styles.iter().filter_map(|x| super::tt::parse_style(x)).find(|st| grade_rank(a.style(*st)) >= 6).unwrap_or_else(|| a.best_style());
                let plan = self.tt_runner_plan(&meta, row.name, r.id, style);
                let replaced = uma_name(&self.kb, s.trainee);
                let goal = super::tt::Slot { trainee: r.id, style, ..s.clone() }.parents_goal(row.name);
                let parents = super::score::rank(&self.kb, &goal, &inv.parents, 1)
                    .first()
                    .map(|l| {
                        let names = l.legacies.iter().map(|i| super::parent_name(&self.kb, &inv.parents[*i])).collect::<Vec<_>>().join(" and ");
                        if l.meets() { names } else { format!("{names} (her best pair, though short of A somewhere: see her lineage plan)") }
                    })
                    .unwrap_or_else(|| "any two legacies (yours or a friend's)".into());
                return Some(self.runner_brief(format!("Her own career for Team Trials ({}), to replace {replaced}, whose unique fires unreliably.", row.name), &plan, Some((plan.target[1], row.name.to_string())), parents));
            }
        }
        if let Some(run) = s.parents.steps.first() {
            return Some(self.parent_brief(&s.parents_goal(row.name), run, inv, 0));
        }
        let parents = s.parents.now.as_ref().map_or("her best two finished umas".into(), |l| l.legacies.iter().map(|i| super::parent_name(&self.kb, &inv.parents[*i])).collect::<Vec<_>>().join(" and "));
        Some(self.runner_brief(format!("Her own career for Team Trials ({}): she scores lowest on the team, so a better run of her raises it most.", row.name), &s.plan, Some((s.plan.target[1], row.name.to_string())), parents))
    }
}
