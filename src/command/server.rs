use std::sync::Arc;

use actix_web::{middleware, web, App, HttpServer};
use clap::Clap;

use crate::{
    config::Config,
    engine::{Evaluator, UpfrontImporter, WatchingImporter},
    server::routes,
    util::error_printing::PrintableError,
};

use super::{Command, Opts};

/// run in server mode
#[derive(Clap, Clone)]
pub struct Server {
    /// directory use for server
    directory: String,

    #[clap(short, long, default_value = "2332")]
    port: usize,

    #[clap(short, long, default_value = "10")]
    max_connections: u32,

    #[clap(short, long, default_value = "sql")]
    extension: String,

    #[clap(short, long)]
    watch: bool,
}

impl Command for Server {
    fn run_command(&self, _opt: &Opts) -> anyhow::Result<()> {
        let clone = self.clone();
        actix_rt::System::new().block_on(run_server(clone))?;
        Ok(())
    }
}

fn create_evaluator(directory: &str, extension: &str, watch: bool) -> anyhow::Result<Evaluator> {
    if watch {
        let importer = WatchingImporter::new(directory, extension)?;
        Ok(Evaluator::with_importer(importer))
    } else {
        match UpfrontImporter::new(directory, extension) {
            Err(errors) => {
                let mut buffer = String::new();
                for error in errors {
                    error.print_error(&mut buffer)?;
                    eprint!("{}\n", buffer);
                    buffer.clear();
                }
                return Err(anyhow!("failed to import some sql files"));
            }
            Ok(importer) => Ok(Evaluator::with_importer(importer)),
        }
    }
}

pub async fn run_server(cmd: Server) -> anyhow::Result<()> {
    // import all files
    let evaluator = create_evaluator(cmd.directory.as_str(), cmd.extension.as_str(), cmd.watch)?;

    let config = Config::read_config()?;
    let pool = crate::server::init::connect_to_db(&config, None).await?;
    let config = Arc::new(config);

    for endpoint in evaluator.importer.get_all_endpoints()? {
        info!("using endpoint {}", endpoint)
    }

    let listen_loc = format!("0.0.0.0:{}", cmd.port);
    info!("server listening on {}", listen_loc);
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .data(config.clone())
            .data(pool.clone())
            .data(evaluator.clone())
            .route("/api/v1/auth", web::post().to(routes::auth_query))
            .route("/api/v1/query", web::post().to(routes::run_queries))
    })
    .bind(listen_loc)?
    .run()
    .await?;

    Ok(())
}
