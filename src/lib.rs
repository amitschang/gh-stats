use std::{collections::HashMap, env, fmt::Display};

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

async fn prs_from_search(client: &Client, query: impl Into<String>) -> Result<Vec<PRInfo>> {
    let mut all = Vec::new();
    let per_page = 100;
    let query = query.into();
    // get first response, which tells how many other requests to make
    let resp: PRList = client
        .get(search_url(&query, 1))
        .send()
        .await?
        .json()
        .await?;
    all.extend(resp.items);

    // issue all other page requests in parallel
    let num_pages = (resp.total_count as f32 / per_page as f32).ceil() as u32;
    log::debug!("first page returned from query {query}, total count is: {}, num pages: {num_pages}", resp.total_count);

    let mut paged_res: JoinSet<_> = (2..=num_pages)
        .map(|page| client.get(search_url(&query, page)).send())
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
        log::info!("Using token from GITHUB_TOKEN");
        let value = HeaderValue::from_str(&format!("Bearer {}", token))?;
        headers.insert("Authorization", value);
    }
    let client = Client::builder().user_agent("rust-agent").default_headers(headers).build()?;
    Ok(client)
}

struct PRStats {
    approved: usize,
    not_approved: usize,
}

impl PRStats {
    fn new() -> Self {
        PRStats { approved: 0, not_approved: 0 }
    }

    fn new_with(approved: usize, not_approved: usize) -> Self {
        PRStats { approved, not_approved }
    }

    fn update_from(&mut self, other: &PRStats) {
        self.approved += other.approved;
        self.not_approved += other.not_approved;
    }
}

impl Display for PRStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total: {}, approved: {}, not approved: {}, rate: {:.2}",
            self.approved + self.not_approved,
            self.approved,
            self.not_approved,
            self.approved as f32 / (self.approved + self.not_approved) as f32
        )
    }
}

type StatsMap = HashMap<String, PRStats>;

async fn pr_stats(org: &str) -> Result<StatsMap> {
    let client = make_client()?;
    // Do both search queries in parallel
    let (res_approved, res_not) = try_join!(
        prs_from_search(&client, format!("is:pr is:merged review:approved org:{org}")),
        prs_from_search(&client, format!("is:pr is:merged -review:approved org:{org}")),
    )?;
    let prs_approved = count_by_pr(&res_approved);
    let prs_not_approved = count_by_pr(&res_not);
    // Combine the counts. Note that we don't necessarily know that the repos
    // will be fully in both sets, hence we chain the keys which may yield
    // repeats but covers all of them.
    let mut combined = HashMap::new();
    for repo in prs_approved.keys().chain(prs_not_approved.keys()) {
        combined.entry(repo.to_string()).or_insert_with(|| {
            PRStats::new_with(
                *prs_approved.get(repo).unwrap_or(&0),
                *prs_not_approved.get(repo).unwrap_or(&0))
        });
    }
    Ok(combined)
}

pub async fn report(org: &str) -> Result<()> {
    let combined = pr_stats(org).await?;
    let mut tot_stats = PRStats::new();
    for (repo, stats) in combined.iter().sorted_by_key(|a| a.0) {
        println!(
            "{repo}: {stats}"
        );
        tot_stats.update_from(stats);
    }
    println!("Total: {tot_stats}");
    Ok(())
}
