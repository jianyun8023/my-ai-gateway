#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    my_ai_gateway::run().await
}
