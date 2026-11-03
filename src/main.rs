//! Run the local browser controller service. TLS terminates at a trusted proxy.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    web_controller::server::serve().await
}
