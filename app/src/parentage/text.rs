//! The planner's answers as text for the assistant (and, until its page is designed, for the
//! player through it). Odds are labelled calculated: they come from the game's rules and the
//! published spark tables, not from a simulation.

use super::jobs::{CAP, Proposal, Run};
use super::outcome::within;
use super::score::Lineage;
use super::track::ParentPlan;
use super::{APTITUDES, Goal, Inventory, Parent, STATS, green_spark, parent_name, uma_name};
use crate::affinity::symbol;
use crate::knowledge::Knowledge;
use crate::veterans::Veteran;

const GRADES: [&str; 9] = ["?", "G", "F", "E", "D", "C", "B", "A", "S"];

fn pct(p: f64) -> String {
    if p >= 0.995 { "~100%".into() } else { format!("{:.1}%", p * 100.0) }
}

pub fn goal(kb: &Knowledge, g: &Goal) -> String {
    let mut out = format!("{}: {} at A", uma_name(kb, g.runner), g.targets.iter().map(|i| APTITUDES[*i]).collect::<Vec<_>>().join(", "));
    if let Some(u) = g.green {
        out += &format!(", inheriting {}", kb.sparks.get(&green_spark(u, 1)).map_or_else(|| uma_name(kb, u), |s| s.name.clone()));
    }
    let stats: Vec<String> = g.stats.iter().enumerate().filter(|(_, v)| **v > 0).map(|(i, v)| format!("{} +{v}", STATS[i])).collect();
    if !stats.is_empty() {
        out += &format!("; wanted at the start: {}", stats.join(", "));
    }
    if !g.whites.is_empty() {
        out += &format!("; whites worth inheriting: {}", g.whites.iter().map(|k| white_name(kb, *k)).collect::<Vec<_>>().join(", "));
    }
    if !g.label.is_empty() {
        out += &format!(" ({})", g.label);
    }
    out
}

fn white_name(kb: &Knowledge, key: u32) -> String {
    (1..=3).find_map(|s| kb.sparks.get(&(key * 100 + s))).map_or_else(|| format!("spark {key}"), |s| s.name.clone())
}

/// One lineage: its two legacies, affinity, start aptitudes for the targets, stats, uniques and
/// wanted whites.
pub fn lineage(kb: &Knowledge, parents: &[&Parent], l: &Lineage) -> String {
    let names: Vec<String> = parents.iter().map(|p| parent_name(kb, p)).collect();
    let mut out = format!("{} — lineage affinity {} {} (slots {}). Start: ", names.join(" + "), l.affinity, symbol(l.affinity), l.slots.map(|s| s.to_string()).join("/"));
    out += &l
        .targets
        .iter()
        .map(|(i, before, after, stars)| {
            let mark = if *after >= 7 { "✔" } else { "⚠ below A" };
            if before == after { format!("{} {} {mark}", APTITUDES[*i], GRADES[*before as usize]) } else { format!("{} {}→{} ({stars} pink stars) {mark}", APTITUDES[*i], GRADES[*before as usize], GRADES[*after as usize]) }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let stats: Vec<String> = l.blues.iter().enumerate().filter(|(_, v)| **v > 0).map(|(i, v)| format!("{} +{v}", STATS[i])).collect();
    if !stats.is_empty() {
        out += &format!(". Stats {}", stats.join(", "));
    }
    let greens: Vec<String> = parents.iter().filter_map(|p| p.green).filter_map(|(u, s)| kb.sparks.get(&green_spark(u, s)).map(|x| format!("{} {s}★", x.name))).collect();
    if !greens.is_empty() {
        out += &format!(". Uniques {}", greens.join(", "));
    }
    if !l.whites.is_empty() {
        out += &format!(". Whites (fire at least once) {}", l.whites.iter().map(|(k, p)| format!("{} {}", white_name(kb, *k), pct(*p))).collect::<Vec<_>>().join(", "));
    }
    if !l.green {
        out += ". ⚠ The wanted unique isn't a legacy's green";
    }
    out + "."
}

fn step(kb: &Knowledge, inv: &Inventory, n: usize, r: &Run) -> String {
    let legacies: Vec<String> = r.legacies.iter().map(|id| inv.by_id(*id).map_or(format!("#{id}"), |p| parent_name(kb, p))).collect();
    let labels = |ws: &[super::jobs::Want]| ws.iter().map(|w| w.label(kb)).collect::<Vec<_>>().join(", ");
    let mut out = format!("Step {n}: a career of {} with {} as her legacies.\n", uma_name(kb, r.trainee), legacies.join(" and "));
    out += &format!("  Must have: {}.", if r.must.is_empty() { "nothing beyond finishing it (her parents bring what's needed)".into() } else { labels(&r.must) });
    if !r.nice.is_empty() {
        let nice: Vec<String> = r.nice.iter().map(|w| w.odds(&inv.history).map_or_else(|| w.label(kb), |p| format!("{} ({} a career)", w.label(kb), pct(p)))).collect();
        out += &format!(" Nice to have: {}.", nice.join(", "));
    }
    out += "\n";
    if r.p < 1.0 {
        out += &format!(
            "  {} a career (best case {}), about {:.1} careers with the 30 TP spark reroll; {} within {CAP:.0} careers.{}\n",
            pct(r.p),
            pct(r.p_best),
            r.careers(),
            pct(within(r.p, CAP as u32)),
            if r.careers() > CAP { " ⚠ Past the cap." } else { "" }
        );
    }
    out
}

pub fn proposal(kb: &Knowledge, goal_: &Goal, inv: &Inventory, p: &Proposal, per_day: f64) -> String {
    let mut out = format!("Goal: {}.\n", goal(kb, goal_));
    if !p.impossible.is_empty() {
        return out + &format!("{} more than four grades below A for her: no lineage starts it at A (pink stars raise four grades at most). Pick a runner with a better grade there.", p.impossible.iter().map(|i| APTITUDES[*i]).collect::<Vec<_>>().join(" and "));
    }
    match &p.now {
        Some(l) => {
            let ps: Vec<&Parent> = l.legacies.iter().map(|i| &inv.parents[*i]).collect();
            out += &format!("Best lineage from your finished umas: {}\n", lineage(kb, &ps, l));
            if l.meets() {
                return out + "It already meets the goal: train the runner with these two.";
            }
        }
        None => out += "No two of your finished umas can be her legacies yet.\n",
    }
    // What the best lineage lacks, per target.
    let have = |i: usize| p.now.as_ref().and_then(|l| l.targets.iter().find(|t| t.0 == i)).map_or(0, |t| t.3);
    let short: Vec<String> = p.need.iter().filter(|(i, n)| have(*i) < *n).map(|(i, n)| format!("{} needs {n} pink stars in the lineage and the best has {}", APTITUDES[*i], have(*i))).collect();
    if !short.is_empty() {
        out += &format!("Short of A at the start: {}.\n", short.join("; "));
    }
    if p.steps.is_empty() {
        return out + "No route from the umas you own reaches it: borrow a friend's legacy with the missing pinks, pick another runner, or accept a lower grade.";
    }
    out += &format!("\nPlan (calculated from the game's spark rules; keep running each step until a finished uma has its must-haves; odds use the Wiki's grade-weighted pink rule, uma.guide's even rule as the best case; at most {CAP:.0} careers a step):\n");
    for (i, r) in p.steps.iter().enumerate() {
        out += &step(kb, inv, i + 1, r);
    }
    if let Some(l) = &p.after {
        let names: Vec<String> = match p.partner.and_then(|id| inv.by_id(id)) {
            Some(w) => vec![format!("step {}'s product", p.steps.len()), parent_name(kb, w)],
            None => vec!["the two steps' products".into()],
        };
        out += &format!("Then the runner's lineage: {} — affinity {} {}, every target at A at the start.\n", names.join(" + "), l.affinity, symbol(l.affinity));
    }
    let total = p.careers();
    out += &format!("About {total:.1} careers in all, {:.1} days at {per_day} parent careers a day.", total / per_day.max(0.1));
    if p.over_cap {
        out += &format!("\n⚠ Nothing fits under {CAP:.0} careers a step: this is the cheapest route anyway. Better options: borrow a friend's legacy with the missing pinks, pick another runner, or accept a lower grade.");
    }
    if !p.alternatives.is_empty() {
        out += "\nOther first steps: ";
        let must = |r: &Run| if r.must.is_empty() { "just finish it".to_string() } else { r.must.iter().map(|w| w.label(kb)).collect::<Vec<_>>().join(", ") };
        out += &p.alternatives.iter().map(|r| format!("{} ({}, about {:.1} careers)", uma_name(kb, r.trainee), must(r), r.careers())).collect::<Vec<_>>().join("; ");
        out += ".";
    }
    out
}

/// The saved plan: its steps, where it stands, and the finished umas that count for the
/// current step.
pub fn plan(kb: &Knowledge, plan: &ParentPlan, inv: &Inventory, vets: &[Veteran]) -> String {
    let mut out = format!("Parent plan (made {}): {}.\n", plan.made, goal(kb, &plan.goal));
    if let Some(why) = &plan.blocked {
        out += &format!("⚠ Waiting for your choice: {why}. Pick another route: another runner or goal, a friend's legacy, or the cheapest route past the cap if you accept it.\n");
    }
    for (i, s) in plan.steps.iter().enumerate() {
        match s.done {
            Some(id) => out += &format!("Step {}: done with #{id}{}.\n", i + 1, if s.settled { " (settled for, short of its must-haves)" } else { "" }),
            None => out += &step(kb, inv, i + 1, &s.run),
        }
    }
    if let Some(w) = plan.partner {
        out += &format!("Then the last step's product pairs with {} as the runner's legacies.\n", inv.by_id(w).map_or(format!("#{w}"), |p| parent_name(kb, p)));
    }
    if plan.blocked.is_some() {
        return out;
    }
    let Some(cur) = plan.current() else {
        return out + "Every step is done: train the runner with the lineage the plan ends in.";
    };
    let hits = plan.hits(vets);
    let run = &plan.steps[cur].run;
    if hits.is_empty() {
        out += &format!("No finished {} since step {} began. Import after each career (Settings → Profile → Import finished umas) and the step is checked.", uma_name(kb, run.trainee), cur + 1);
    } else {
        out += &format!("Step {} so far: {} career{} kept since it began (careers whose uma you released don't show).\n", cur + 1, hits.len(), if hits.len() == 1 { "" } else { "s" });
        for h in &hits {
            let nice: Vec<String> = h.nice.iter().map(|i| run.nice[*i].label(kb)).collect();
            out += &format!(
                "- #{}: {}{}{}\n",
                h.id,
                if h.must { "has the must-haves" } else { "short of the must-haves" },
                if nice.is_empty() { String::new() } else { format!(", plus {}", nice.join(", ")) },
                if h.same_parents { "" } else { " (ran with other legacies, so its parents bring different sparks)" }
            );
        }
        out += if hits.iter().any(|h| h.must) { "Keep the one that meets them and move on." } else { "Keep running, or settle for one and the plan is worked out again around it." };
    }
    out
}

/// The line an import adds when a plan is open.
pub fn import_note(kb: &Knowledge, plan: &ParentPlan, vets: &[Veteran]) -> Option<String> {
    let cur = plan.current()?;
    let hit = plan.hits(vets).into_iter().find(|h| h.must)?;
    let must = &plan.steps[cur].run.must;
    let what = if must.is_empty() { "finishes it".to_string() } else { format!("has its must-haves ({})", must.iter().map(|w| w.label(kb)).collect::<Vec<_>>().join(", ")) };
    Some(format!("Parent plan: #{} {what}, so step {} can end. Keep it and move on?", hit.id, cur + 1))
}
