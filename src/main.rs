#[tokio::main]
async fn main() -> anyhow::Result<()> {
    xmas::repl().await
}
