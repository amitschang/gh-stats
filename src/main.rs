use gh_stats::report;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();
    let org = std::env::args().nth(1).expect("positional argument required: org name");
    log::info!("reporting for org: {org}");
    report(&org).await.unwrap();
}
