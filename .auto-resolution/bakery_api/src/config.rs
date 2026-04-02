#[derive(clap::Parser)]
pub struct Config {
    #[clap(long, env, default_value = "3000")]
    pub port: String,
    #[clap(long, env, default_value = "127.0.0.1")]
    pub host: String,
}
