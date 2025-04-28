use std::collections::HashMap;

use itertools::Itertools;
use reqwest::blocking::Client;
use serde::Deserialize;

type Result<T> = anyhow::Result<T>;

#[derive(Deserialize)]
struct PRList {
    total_count: u32,
    items: Vec<PRInfo>,
}

#[derive(Deserialize)]
struct PRInfo {
    repository_url: String,
}

fn prs_from_search(client: &Client, query: &str) -> Result<Vec<PRInfo>> {
    let mut all = Vec::new();
    let per_page = 100;
    let mut page = 1;
    loop {
        let query = query.replace(" ", "+");
        let url = format!(
            "https://api.github.com/search/issues?q={}&per_page={}&page={}",
            query, per_page, page
        );
        let resp: PRList = client.get(&url).send()?.json()?;
        let retrieved = resp.items.len();
        all.extend(resp.items);
        if all.len() >= resp.total_count as usize || retrieved == 0 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

fn count_by_pr(prs: &[PRInfo]) -> HashMap<String, u32> {
    let mut counts = std::collections::HashMap::new();
    for pr in prs.iter() {
        counts
            .entry(pr.repository_url.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
    counts
}

type StatsMap = HashMap<String, (u32, u32, f32)>;

fn pr_stats() -> Result<StatsMap> {
    let client = Client::builder().user_agent("rust-agent").build()?;
    let prs_approved = {
        let results = prs_from_search(&client, "is:pr is:merged review:approved org:ssec-jhu")?;
        count_by_pr(&results)
    };
    let prs_not_approved = {
        let results = prs_from_search(&client, "is:pr is:merged -review:approved org:ssec-jhu")?;
        count_by_pr(&results)
    };
    let mut combined = HashMap::new();
    for repo in prs_approved.keys().chain(prs_not_approved.keys()) {
        combined.entry(repo.clone()).or_insert_with(|| {
            let approved = prs_approved.get(repo).unwrap_or(&0);
            let not_approved = prs_not_approved.get(repo).unwrap_or(&0);
            let rate = *approved as f32 / (*approved as f32 + *not_approved as f32);
            (*approved, *not_approved, rate)
        });
    }
    Ok(combined)
}

pub fn report() -> Result<()> {
    let combined = pr_stats()?;
    let mut tot_approved = 0;
    let mut tot_not_approved = 0;
    for (repo, (approved, not_approved, rate)) in combined.iter().sorted_by_key(|a| a.0) {
        let total = approved + not_approved;
        println!(
            "{repo}: {total} total, {approved} approved, {not_approved} not approved, rate: {rate:.2}"
        );
        tot_approved += approved;
        tot_not_approved += not_approved;
    }
    let tot_rate = tot_approved as f32 / (tot_approved as f32 + tot_not_approved as f32);
    let tot_total = tot_approved + tot_not_approved;
    println!(
        "Total: {tot_total} total, {tot_approved} approved, {tot_not_approved} not approved, rate: {tot_rate:2}"
    );
    Ok(())
}
