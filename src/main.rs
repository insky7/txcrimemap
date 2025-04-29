use axum::{
    body::Body,
    http::header::{ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_TYPE, ORIGIN},
    routing::{get, post},
    Router,
};
use lambda_http::tracing;
use modules::{helper_functions, routes};
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::fmt::SubscriberBuilder;

mod constants;
mod modules;

#[tokio::main]
async fn main() {
    // init tracing
    SubscriberBuilder::default()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::INFO)
        .json()
        .init();

    // define trace layer for logging each request
    let trace_layer = TraceLayer::new_for_http().on_request(
        |_: &axum::http::Request<Body>, _: &tracing::Span| tracing::info!("begin request!"),
    );

    // set up CORS layer
    let cors_layer = CorsLayer::new()
        .allow_headers([ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_TYPE, ORIGIN])
        .allow_methods(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any);

    // build app
    let app = Router::new()
        .route("/", get(routes::landing_page))
        .route("/geocode", post(helper_functions::geocode))
        .layer(cors_layer)
        .layer(trace_layer)
        .layer(CompressionLayer::new().gzip(true).deflate(true));

    #[cfg(debug_assertions)]
    {
        // local development server (cargo run (no release))
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tracing::info!("listening on http://{}", addr);
        axum::serve(listener, app).await.unwrap();
    }

    #[cfg(not(debug_assertions))]
    {
        // Lambda runtime
        let app = tower::ServiceBuilder::new()
            .layer(axum_aws_lambda::LambdaLayer::default())
            .service(app);

        tracing::info!("Starting Lambda runtime");
        lambda_http::run(app).await.unwrap();
    }
}
