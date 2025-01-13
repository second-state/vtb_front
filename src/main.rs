use clap::{command, Parser};

mod live2d;

#[derive(Debug, Parser, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:8000")]
    listen: String,
    #[arg(short, long, default_value = "./dist")]
    dist: String,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let listener = tokio::net::TcpListener::bind(&args.listen).await.unwrap();

    log::info!("Listening on http://{}", &args.listen);

    let state = live2d::ServiceState::new();

    let app = live2d::router(state, &args.dist);
    axum::serve(listener, app).await.unwrap();
}
