use gh_stats::report;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    report().await.unwrap();
}
