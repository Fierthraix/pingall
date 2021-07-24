use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub struct Opt {
    /// Interface to search.
    #[structopt[short = "i", long = "interface"]]
    pub interface: Option<String>,

    /// Don't attempt to resolve hostnames.
    #[structopt(short = "d", long = "dont-resolve")]
    pub dont_resolve: bool,
}
