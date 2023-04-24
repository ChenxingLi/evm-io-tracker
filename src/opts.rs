extern crate structopt;

pub use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "EVM IO Tracker.", rename_all = "kebab-case")]
pub enum Options {
    Fetch(FetchOptions),
    Combine(CombineOptions),
    Seal(SealOptions),
}

#[derive(Debug, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct FetchOptions {
    #[structopt(long, default_value = "http://127.0.0.1:8545/")]
    pub node_url: String,

    #[structopt(long)]
    pub start_block: usize,

    #[structopt(long, default_value = "50")]
    pub batch_size: usize,
}

#[derive(Debug, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct CombineOptions {
    #[structopt(long)]
    pub start_block: Option<usize>,

    #[structopt(long)]
    pub end_block: Option<usize>,

    #[structopt(long, default_value = "data")]
    pub path: String,
}

#[derive(Debug, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct SealOptions {
    #[structopt(long)]
    pub input: String,

    #[structopt(long, default_value = "data")]
    pub output: String,
}
