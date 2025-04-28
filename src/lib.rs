use std::{collections::HashMap, env};

use itertools::Itertools;
use reqwest::{header::{HeaderMap, HeaderValue}, Client};
use serde::Deserialize;
use tokio::{task::JoinSet, try_join};

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

fn search_url(query: &str, page: u32) -> String {
    format!(
        "https://api.github.com/search/issues?q={}&per_page=100&page={}",
        query, page
    )
}

async fn prs_from_search(client: &Client, query: &str) -> Result<Vec<PRInfo>> {
    let mut all = Vec::new();
    let per_page = 100;
    // get first response, which tells how many other requests to make
    let resp: PRList = client
        .get(search_url(query, 1))
        .send()
        .await?
        .json()
        .await?;
    all.extend(resp.items);

    // issue all other page requests in parallel
    let num_pages = (resp.total_count as f32 / per_page as f32).ceil() as u32;
    let mut paged_res: JoinSet<_> = (2..=num_pages)
        .map(|page| client.get(search_url(query, page)).send())
        .collect();

    while let Some(res) = paged_res.join_next().await {
        let resp: PRList = res??.json().await?;
        all.extend(resp.items);
    }
    Ok(all)
}

fn count_by_pr(prs: &[PRInfo]) -> HashMap<&str, usize> {
    prs.iter().map(|pr| pr.repository_url.as_str()).counts()
}

fn make_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    if let Ok(token) = env::var("GITHUB_TOKEN") {
        println!("Using token from GITHUB_TOKEN");
        let value = HeaderValue::from_str(&format!("Bearer {}", token))?;
        headers.insert("Authorization", value);
    }
    let client = Client::builder().user_agent("rust-agent").default_headers(headers).build()?;
    Ok(client)
}

type StatsMap = HashMap<String, (usize, usize, f32)>;

async fn pr_stats() -> Result<StatsMap> {
    let client = make_client()?;
    // Do both search queries in parallel
    let (res_approved, res_not) = try_join!(
        prs_from_search(&client, "is:pr is:merged review:approved org:ssec-jhu"),
        prs_from_search(&client, "is:pr is:merged -review:approved org:ssec-jhu"),
    )?;
    let prs_approved = count_by_pr(&res_approved);
    let prs_not_approved = count_by_pr(&res_not);
    // Combine the counts. Note that we don't necessarily know that the repos
    // will be fully in both sets, hence we chain the keys which may yield
    // repeats but covers all of them.
    let mut combined = HashMap::new();
    for repo in prs_approved.keys().chain(prs_not_approved.keys()) {
        combined.entry(repo.to_string()).or_insert_with(|| {
            let approved = prs_approved.get(repo).unwrap_or(&0);
            let not_approved = prs_not_approved.get(repo).unwrap_or(&0);
            let rate = *approved as f32 / (*approved as f32 + *not_approved as f32);
            (*approved, *not_approved, rate)
        });
    }
    Ok(combined)
}

pub async fn report() -> Result<()> {
    let combined = pr_stats().await?;
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
